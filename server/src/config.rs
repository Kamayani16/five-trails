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
    /// FT_PREVIEW=1: relax X-Frame-Options and CSP frame-ancestors for Heimdall preview iframe.
    pub preview: bool,
    /// FT_HSTS=1: send Strict-Transport-Security (only when served over HTTPS).
    pub hsts: bool,
    /// FT_MAX_CONNS, default 5000.
    pub max_conns: usize,
    /// FT_MAX_CONNS_PER_IP, default 8 (IPv6 counted per /64).
    pub max_conns_per_ip: usize,
    /// FT_DATA_DIR, default `data`: folder for `players.json` (names and the leaderboard).
    /// Set to `memory` to keep everything in memory only (it is lost on restart).
    pub data_dir: Option<PathBuf>,
    /// FT_ROOM_TTL_SECS, default 120: how long a private room waits for a friend before it
    /// closes by itself (10 to 3600).
    pub room_ttl_secs: u64,
    /// FT_DAY_OFFSET_MINUTES, default 330 (India, UTC+5:30): minutes added to UTC to find
    /// "midnight" for daily quests, weekly quests and the monthly season. Between -720 and 840.
    pub day_offset_minutes: i64,
    /// FT_CHEST_SECS, default 21600 (6 hours): time for the next free chest to arrive (10 to 86400).
    pub chest_secs: u64,
    /// FT_BOT_AFTER_SECS, default 20: how long a quick-match player waits before the computer
    /// is offered (2 to 300).
    pub bot_after_secs: u64,
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
            preview: flag("FT_PREVIEW"),
            hsts: flag("FT_HSTS"),
            max_conns: num("FT_MAX_CONNS", 5000)?,
            max_conns_per_ip: num("FT_MAX_CONNS_PER_IP", 8)?,
            room_ttl_secs: num("FT_ROOM_TTL_SECS", 120)?.clamp(10, 3600) as u64,
            chest_secs: num("FT_CHEST_SECS", 6 * 3600)?.clamp(10, 86_400) as u64,
            bot_after_secs: num("FT_BOT_AFTER_SECS", 20)?.clamp(2, 300) as u64,
            day_offset_minutes: env("FT_DAY_OFFSET_MINUTES").map_or(Ok(330), |v| v.parse::<i64>().map_err(|e| format!("FT_DAY_OFFSET_MINUTES: {e}")))?.clamp(-720, 840),
            data_dir: match env("FT_DATA_DIR").as_deref() {
                Some("memory") => None,
                Some(dir) => Some(PathBuf::from(dir)),
                None => Some(PathBuf::from("data")),
            },
        })
    }
}
