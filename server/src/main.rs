//! Five Trails server: matchmaking, server-authoritative matches and the leaderboard.
//!
//! Live queues, rooms and matches are held in memory only. The one thing written to disk is
//! `players.json` in `FT_DATA_DIR`: each player's account (unique name, hashed secret key),
//! ranked record, season record, coins, free chests and quest progress, plus a backup of the
//! lobby progress the browser reports (for restoring on a new device). Settings and the
//! detailed battle history stay in the player's own browser (IndexedDB).

#![forbid(unsafe_code)]

mod bot;
mod config;
mod conn;
mod economy;
mod game;
mod hub;
mod limits;
mod matchroom;
mod protocol;
mod store;

use axum::http::{header, HeaderName, HeaderValue};
use axum::{routing::get, Router};
use config::Config;
use hub::{Hub, HubCmd, HubTx};
use limits::ConnLimiter;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub hub: HubTx,
    pub limiter: Arc<ConnLimiter>,
    pub next_conn: Arc<AtomicU64>,
}

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; \
style-src-attr 'unsafe-inline'; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; \
base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'";

const CSP_PREVIEW: &str = "default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; \
style-src-attr 'unsafe-inline'; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self' ws: wss:; \
base-uri 'none'; form-action 'none'; object-src 'none'";

fn app(state: AppState) -> Router {
    let cfg = Arc::clone(&state.cfg);
    let limiter = Arc::clone(&state.limiter);
    let mut router = Router::new()
        .route("/ws", get(conn::ws_handler))
        .route("/healthz", get(move || async move { format!("ok {}", limiter.active()) }))
        .with_state(state);
    if let Some(dir) = &cfg.web_dir {
        router = router.fallback_service(ServeDir::new(dir));
    }
    let set = |name: HeaderName, value: &'static str| SetResponseHeaderLayer::if_not_present(name, HeaderValue::from_static(value));
    if cfg.preview {
        router = router
            .layer(set(header::CONTENT_SECURITY_POLICY, CSP_PREVIEW))
            .layer(set(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
            .layer(set(header::REFERRER_POLICY, "no-referrer"))
            .layer(set(HeaderName::from_static("permissions-policy"), "camera=(), microphone=(), geolocation=(), payment=()"))
            .layer(set(header::CACHE_CONTROL, "no-cache"));
    } else {
        router = router
            .layer(set(header::CONTENT_SECURITY_POLICY, CSP))
            .layer(set(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
            .layer(set(header::X_FRAME_OPTIONS, "DENY"))
            .layer(set(header::REFERRER_POLICY, "no-referrer"))
            .layer(set(HeaderName::from_static("permissions-policy"), "camera=(), microphone=(), geolocation=(), payment=()"))
            .layer(set(HeaderName::from_static("cross-origin-opener-policy"), "same-origin"))
            .layer(set(HeaderName::from_static("cross-origin-resource-policy"), "same-origin"))
            .layer(set(header::CACHE_CONTROL, "no-cache"));
    }
    if cfg.hsts {
        router = router.layer(set(header::STRICT_TRANSPORT_SECURITY, "max-age=31536000; includeSubDomains"));
    }
    router
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cfg = Arc::new(Config::from_env()?);
    let data_path = cfg.data_dir.as_ref().map(|d| d.join("players.json"));
    let tuning = economy::Tuning { day_offset: cfg.day_offset_minutes * 60, chest_every: cfg.chest_secs };
    let store = match &data_path {
        Some(path) => store::Store::load(path).map_err(|e| format!("{}: {e}", path.display()))?,
        None => store::Store::new(),
    }
    .with_tuning(tuning);
    tracing::info!(data = ?data_path, players = store.len(), "player data loaded");
    let (hub, hub_tx) = Hub::new(store, data_path, std::time::Duration::from_secs(cfg.room_ttl_secs));
    let hub = hub.with_bot_wait(std::time::Duration::from_secs(cfg.bot_after_secs));
    tokio::spawn(hub.run());
    let state = AppState {
        cfg: Arc::clone(&cfg),
        hub: hub_tx,
        limiter: ConnLimiter::new(cfg.max_conns, cfg.max_conns_per_ip),
        next_conn: Arc::new(AtomicU64::new(1)),
    };
    let flush_tx = state.hub.clone();
    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    tracing::info!(
        bind = %cfg.bind,
        web_dir = ?cfg.web_dir,
        origins = ?cfg.allowed_origins,
        "Five Trails server listening"
    );
    axum::serve(listener, app(state).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    // Save names and rankings before exiting.
    let (done, saved) = tokio::sync::oneshot::channel();
    if flush_tx.send(HubCmd::Flush { done }).await.is_ok() {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), saved).await;
    }
    Ok(())
}
