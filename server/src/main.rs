//! Five Trails server: stateless matchmaking and server-authoritative matches.
//!
//! Nothing is written to disk. Player profiles and records live in each player's
//! browser (IndexedDB); this process only holds live queues, rooms and matches in memory.

#![forbid(unsafe_code)]

mod config;
mod conn;
mod game;
mod hub;
mod limits;
mod matchroom;
mod protocol;

use axum::http::{header, HeaderName, HeaderValue};
use axum::{routing::get, Router};
use config::Config;
use hub::{Hub, HubTx};
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
    router = router
        .layer(set(header::CONTENT_SECURITY_POLICY, CSP))
        .layer(set(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(set(header::X_FRAME_OPTIONS, "DENY"))
        .layer(set(header::REFERRER_POLICY, "no-referrer"))
        .layer(set(HeaderName::from_static("permissions-policy"), "camera=(), microphone=(), geolocation=(), payment=()"))
        .layer(set(HeaderName::from_static("cross-origin-opener-policy"), "same-origin"))
        .layer(set(HeaderName::from_static("cross-origin-resource-policy"), "same-origin"))
        .layer(set(header::CACHE_CONTROL, "no-cache"));
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
    let (hub, hub_tx) = Hub::new();
    tokio::spawn(hub.run());
    let state = AppState {
        cfg: Arc::clone(&cfg),
        hub: hub_tx,
        limiter: ConnLimiter::new(cfg.max_conns, cfg.max_conns_per_ip),
        next_conn: Arc::new(AtomicU64::new(1)),
    };
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
    Ok(())
}
