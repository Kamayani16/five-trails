//! Settings from environment variables. Defaults are safe for local development.

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    /// FT_BIND, default 127.0.0.1:8080. Use 0.0.0.0:8080 inside a container.
    pub bind: SocketAddr,
    /// FT_WEB_DIR: folder with index.html, app.js, app.css. Empty to serve only the API.
    pub web_dir: Option<PathBuf>,
    /// FT_ALLOWED_ORIGINS: comma-separated exact origins allowed to open a WebSocket,
    /// e.g. "https://play.example.com". When empty, only same-origin pages may connect.
    pub allowed_origins: Vec<String>,
    /// FT_TRUST_PROXY=1: take the client address from the last X-Forwarded-For entry.
    /// Only enable behind a reverse proxy that sets this header.
    pub trust_proxy: bool,
    /// FT_HSTS=1: send Strict-Transport-Security (only when served over HTTPS).
    pub hsts: bool,
    /// FT_MAX_CONNS, default 5000.
    pub max_conns: usize,
    /// FT_MAX_CONNS_PER_IP, default 8 (IPv6 counted per /64).
    pub max_conns_per_ip: usize,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn flag(key: &str) -> bool {
    matches!(env(key).as_deref(), Some("1" | "true" | "yes"))
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bind = env("FT_BIND")
            .unwrap_or_else(|| "127.0.0.1:8080".into())
            .parse()
            .map_err(|e| format!("FT_BIND: {e}"))?;
        let web_dir = match env("FT_WEB_DIR") {
            Some(d) => Some(PathBuf::from(d)),
            None => ["web", "../web"].iter().map(PathBuf::from).find(|p| p.join("index.html").is_file()),
        };
        let allowed_origins = env("FT_ALLOWED_ORIGINS")
            .map(|s| s.split(',').map(|o| o.trim().trim_end_matches('/').to_owned()).filter(|o| !o.is_empty()).collect())
            .unwrap_or_default();
        let num = |key: &str, default: usize| -> Result<usize, String> {
            env(key).map_or(Ok(default), |v| v.parse().map_err(|e| format!("{key}: {e}")))
        };
        Ok(Self {
            bind,
            web_dir,
            allowed_origins,
            trust_proxy: flag("FT_TRUST_PROXY"),
            hsts: flag("FT_HSTS"),
            max_conns: num("FT_MAX_CONNS", 5000)?,
            max_conns_per_ip: num("FT_MAX_CONNS_PER_IP", 8)?,
        })
    }
}
