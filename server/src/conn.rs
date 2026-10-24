//! One task per WebSocket. Checks the origin and connection caps before upgrading,
//! enforces message size and rate limits, and routes messages to the hub or the
//! player's current match.

use crate::hub::{ConnCtl, HubCmd};
use crate::limits::{ConnGuard, TokenBucket};
use crate::matchroom::{MatchCmd, MatchTx};
use crate::protocol::{self, ClientMsg, MAX_MESSAGE_BYTES, PROTOCOL_VERSION};
use crate::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(75);
const PING_EVERY: Duration = Duration::from_secs(20);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_STRIKES: u32 = 30;

fn origin_allowed(headers: &HeaderMap, allowed: &[String]) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return false };
    let origin = origin.trim_end_matches('/');
    if !allowed.is_empty() {
        return allowed.iter().any(|a| a == origin || a == "*");
    }
    // Default: same origin only (scheme-agnostic so it works behind a TLS proxy).
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else { return false };
    if origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://")) == Some(host) {
        return true;
    }
    if let Some(fwd_host) = headers.get("x-forwarded-host").and_then(|v| v.to_str().ok()) {
        if origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://")) == Some(fwd_host) {
            return true;
        }
    }
    false
}

fn client_ip(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        let forwarded = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .filter_map(|s| s.trim().parse::<IpAddr>().ok())
            .next_back();
        if let Some(ip) = forwarded {
            return ip;
        }
    }
    peer.ip()
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !origin_allowed(&headers, &app.cfg.allowed_origins) {
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    let ip = client_ip(&headers, peer, app.cfg.trust_proxy);
    let Some(guard) = app.limiter.try_acquire(ip) else {
        return (StatusCode::TOO_MANY_REQUESTS, "too many connections").into_response();
    };
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| run(socket, app, guard))
}

async fn run(socket: WebSocket, app: AppState, _guard: ConnGuard) {
    let id = app.next_conn.fetch_add(1, Ordering::Relaxed);
    let (ctl_tx, mut ctl_rx) = mpsc::channel::<ConnCtl>(128);
    if app.hub.send(HubCmd::Connect { id, tx: ctl_tx }).await.is_err() {
        return;
    }
    tracing::debug!(conn = id, "connected");
    let (mut sink, mut stream) = socket.split();
    let mut link: Option<(MatchTx, usize)> = None;
    let mut said_hello = false;
    let opened = Instant::now();
    let mut last_heard = Instant::now();
    let mut bucket = TokenBucket::new(20.0, 8.0);
    let mut strikes = 0u32;
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;

    macro_rules! reply {
        ($text:expr) => {
            match tokio::time::timeout(WRITE_TIMEOUT, sink.send(Message::Text($text.into()))).await {
                Ok(Ok(())) => {}
                _ => break,
            }
        };
    }

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let text = match incoming {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => { last_heard = Instant::now(); continue; }
                    // Binary frames, close frames, errors and end of stream all end the session.
                    _ => break,
                };
                last_heard = Instant::now();
                if !bucket.take() {
                    strikes += 1;
                    if strikes > MAX_STRIKES { break; }
                    reply!(protocol::error("rate_limited"));
                    continue;
                }
                let msg = match serde_json::from_str::<ClientMsg>(text.as_str()) {
                    Ok(m) => m,
                    Err(_) => {
                        strikes += 1;
                        if strikes > MAX_STRIKES || !said_hello { break; }
                        reply!(protocol::error("bad_message"));
                        continue;
                    }
                };
                if !said_hello {
                    let ClientMsg::Hello { v, name, pid, key, restore, tr, lv, resume } = msg else { break };
                    if v != PROTOCOL_VERSION {
                        reply!(protocol::error("version"));
                        break;
                    }
                    let Some(profile) = protocol::profile_from_hello(&name, &pid, tr, lv) else {
                        reply!(protocol::error("bad_hello"));
                        break;
                    };
                    let Some(key) = protocol::clean_key(&key) else {
                        reply!(protocol::error("bad_hello"));
                        break;
                    };
                    let resume = resume.as_deref().and_then(protocol::clean_token).map(str::to_owned);
                    said_hello = true;
                    // Only the hash of the key goes any further than this task.
                    let key_hash = protocol::hash_key(&key);
                    if app.hub.send(HubCmd::Hello { id, profile, key_hash, create: !restore, resume }).await.is_err() { break; }
                    continue;
                }
                let to_match = |link: &Option<(MatchTx, usize)>, make: &dyn Fn(usize) -> MatchCmd| -> Option<bool> {
                    link.as_ref().map(|(tx, seat)| tx.try_send(make(*seat)).is_ok())
                };
                let hub_cmd = match msg {
                    ClientMsg::Hello { .. } => None,
                    ClientMsg::Ping => { reply!(protocol::msg(json!({ "t": "pong" }))); None }
                    ClientMsg::Leaderboard { season } => Some(HubCmd::Leaderboard { id, season }),
                    ClientMsg::Me => Some(HubCmd::Me { id }),
                    ClientMsg::Sync { tr, best, xp, ai, pvp } => Some(HubCmd::Sync { id, prog: crate::store::Prog { tr, best, xp, ai, pvp } }),
                    ClientMsg::ClaimChest => Some(HubCmd::ClaimChest { id }),
                    ClientMsg::Buy { item } => Some(HubCmd::Buy { id, item }),
                    ClientMsg::Equip { kind, item } => Some(HubCmd::Equip { id, kind, item }),
                    ClientMsg::ClaimQuest { slot } => Some(HubCmd::ClaimQuest { id, slot }),
                    ClientMsg::AckSeason => Some(HubCmd::AckSeason { id }),
                    ClientMsg::CupCreate { name, format, clock } => Some(HubCmd::CupCreate { id, name, format, clock }),
                    ClientMsg::CupJoin { code } => Some(HubCmd::CupJoin { id, code }),
                    ClientMsg::CupLeave { code } => Some(HubCmd::CupLeave { id, code }),
                    ClientMsg::CupStart { code } => Some(HubCmd::CupStart { id, code }),
                    ClientMsg::CupList => Some(HubCmd::CupList { id }),
                    ClientMsg::CupGet { code } => Some(HubCmd::CupGet { id, code }),
                    ClientMsg::CupPlay { code, m } => Some(HubCmd::CupPlay { id, code, m }),
                    ClientMsg::Bot => Some(HubCmd::Bot { id }),
                    ClientMsg::Profile { name, tr, lv } => match protocol::clean_name(&name) {
                        Some(name) => Some(HubCmd::UpdateProfile { id, name, trophies: tr, level: lv }),
                        None => { reply!(protocol::error("bad_hello")); None }
                    },
                    ClientMsg::Queue { clock } => Some(HubCmd::Queue { id, clock }),
                    ClientMsg::Unqueue => Some(HubCmd::Unqueue { id }),
                    ClientMsg::Create { clock, code } => Some(HubCmd::Create { id, clock, code }),
                    ClientMsg::CancelRoom => Some(HubCmd::CancelRoom { id }),
                    ClientMsg::Join { code } => Some(HubCmd::Join { id, code }),
                    ClientMsg::LeaveMatch => {
                        if let Some((tx, seat)) = link.take() {
                            let _ = tx.try_send(MatchCmd::Leave { seat, conn: id });
                            Some(HubCmd::LeftMatch { id })
                        } else { None }
                    }
                    ClientMsg::Move { seq, a, id: card, pile } => {
                        match to_match(&link, &|seat| MatchCmd::Move { seat, conn: id, seq, action: a, card, pile }) {
                            Some(true) => {}
                            Some(false) => reply!(protocol::error("busy")),
                            None => reply!(protocol::error("not_in_match")),
                        }
                        None
                    }
                    ClientMsg::Emote { k } => { to_match(&link, &|seat| MatchCmd::Emote { seat, conn: id, k }); None }
                    ClientMsg::Forfeit => { to_match(&link, &|seat| MatchCmd::Forfeit { seat, conn: id }); None }
                    ClientMsg::Rematch => { to_match(&link, &|seat| MatchCmd::Rematch { seat, conn: id }); None }
                };
                if let Some(cmd) = hub_cmd {
                    if app.hub.send(cmd).await.is_err() { break; }
                }
            }
            ctl = ctl_rx.recv() => match ctl {
                Some(ConnCtl::Send(text)) => reply!(text),
                Some(ConnCtl::Joined { tx, seat }) => link = Some((tx, seat)),
                Some(ConnCtl::Left) => link = None,
                Some(ConnCtl::Close) | None => break,
            },
            _ = ping.tick() => {
                let now = Instant::now();
                if (!said_hello && now.duration_since(opened) > HELLO_TIMEOUT) || now.duration_since(last_heard) > IDLE_TIMEOUT {
                    break;
                }
                match tokio::time::timeout(WRITE_TIMEOUT, sink.send(Message::Ping(Default::default()))).await {
                    Ok(Ok(())) => {}
                    _ => break,
                }
            }
        }
    }

    if let Some((tx, seat)) = link {
        let _ = tx.send(MatchCmd::Detach { seat, conn: id }).await;
    }
    let _ = app.hub.send(HubCmd::Disconnect { id }).await;
    let _ = tokio::time::timeout(WRITE_TIMEOUT, sink.close()).await;
    tracing::debug!(conn = id, "disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(origin: Option<&str>, host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_str(host).unwrap());
        if let Some(o) = origin {
            h.insert(header::ORIGIN, HeaderValue::from_str(o).unwrap());
        }
        h
    }

    #[test]
    fn same_origin_by_default() {
        assert!(origin_allowed(&headers(Some("https://play.example.com"), "play.example.com"), &[]));
        assert!(!origin_allowed(&headers(Some("https://evil.example"), "play.example.com"), &[]));
        assert!(!origin_allowed(&headers(None, "play.example.com"), &[]));
    }

    #[test]
    fn explicit_allow_list() {
        let allow = vec!["https://play.example.com".to_owned()];
        assert!(origin_allowed(&headers(Some("https://play.example.com"), "api.example.com"), &allow));
        assert!(!origin_allowed(&headers(Some("http://play.example.com"), "api.example.com"), &allow));
    }

    #[test]
    fn wildcard_and_forwarded_host_allowed() {
        let allow = vec!["*".to_owned()];
        assert!(origin_allowed(&headers(Some("https://anywhere.example"), "127.0.0.1:8080"), &allow));

        let mut h = headers(Some("https://preview.example.com"), "127.0.0.1:8080");
        h.insert("x-forwarded-host", HeaderValue::from_static("preview.example.com"));
        assert!(origin_allowed(&h, &[]));
    }

    #[test]
    fn proxy_header_only_when_trusted() {
        let peer: SocketAddr = "10.0.0.2:5000".parse().unwrap();
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("1.1.1.1, 203.0.113.9"));
        assert_eq!(client_ip(&h, peer, false), peer.ip());
        assert_eq!(client_ip(&h, peer, true), "203.0.113.9".parse::<IpAddr>().unwrap());
    }
}
