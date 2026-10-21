# Five Trails

A two-player expedition card battler you can play in the browser, against the computer or against other people online.

```
five-trails/
├── server/          Rust server: player discovery, matchmaking and authoritative matches
│   └── src/
│       ├── main.rs        HTTP + WebSocket entry point, security headers, static files
│       ├── conn.rs        One task per WebSocket: origin check, limits, message routing
│       ├── hub.rs         Who is online, quick-match queue, private rooms, resume tokens
│       ├── matchroom.rs   One task per match: applies moves, clocks, reconnects, rematches
│       ├── game.rs        Rules engine and per-player views (unit tested)
│       ├── protocol.rs    Wire messages and input validation
│       ├── limits.rs      Connection caps and message rate limiting
│       └── config.rs      Environment variables
├── web/             Browser client (index.html, app.css, app.js)
├── Dockerfile
└── Caddyfile.example
```

## How it fits together

- **Player data stays in the browser.** Name, player id, trophies, XP, match history, head-to-head records and settings are kept in IndexedDB (`five-trails` database). The server never stores any of it.
- **The server is stateless.** It writes nothing to disk and has no database. It only holds live things in memory: who is connected, the quick-match queue, open rooms and running matches. Restarting it ends any live matches.
- **The server runs every online match.** It shuffles with a cryptographically secure RNG, deals, keeps both hands and the deck order hidden, checks every move, and runs the clocks and Rush timers. Each player only receives what they are allowed to see. A modified browser cannot peek at cards or make illegal moves.
- **Playing the computer happens entirely in the browser** and works without the server.

### Player discovery and matchmaking

- **Live counts:** the lobby shows how many players are online, searching and in battle, refreshed every 10 seconds.
- **Quick match:** pairs players who chose the same clock setting and have similar trophy counts. The allowed gap starts at 100 trophies and widens by 25 per second of waiting. After 20 seconds anyone with the same clock is a fair match. The same browser profile is never paired with itself.
- **Private rooms:** you get a 6-letter code (23 unambiguous letters, about 148 million combinations) that expires after 15 minutes. Join attempts are rate limited.
- **Reconnects:** each player gets a random 256-bit resume token per match, kept in `sessionStorage`. If the connection drops, both clocks pause and the player has 45 seconds to come back, even after a page reload. If they don't, the match is forfeited.
- **Rematch:** both players tap Rematch on the results screen. A finished match stays open for 3 minutes.

## Security measures

| Area | What the server does |
| --- | --- |
| Cheating | Server-authoritative rules; opponents' hands, deck order and hidden draws are never sent; stale or out-of-turn moves are rejected |
| Randomness | Deck shuffles use `StdRng` (ChaCha) seeded from the OS; room codes and resume tokens come straight from `OsRng` |
| Cross-site WebSocket hijacking | `Origin` must match the page's own host, or an explicit allow list; missing `Origin` is refused |
| Flooding | 2 KB max message and frame size; 20-message burst then 8 per second per connection; persistent abusers are disconnected |
| Connection exhaustion | Global cap (default 5000) and per-address cap (default 8; IPv6 counted per /64); hello required within 10 s; idle connections closed after 75 s; 5 s write timeout for slow readers; bounded queues everywhere |
| Input | Strict JSON schema with unknown fields rejected; names limited to 1–16 letters, digits, spaces and `-_.'`; ids, codes, tokens and clock values all validated |
| Brute force | Room-code guessing blocked for 60 s after 8 wrong codes |
| Browser hardening | Strict Content-Security-Policy (no inline or eval'd script), `X-Frame-Options: DENY`, `nosniff`, `no-referrer`, COOP/CORP, optional HSTS |
| Code safety | `#![forbid(unsafe_code)]`, overflow checks in release builds, Clippy clean with `-D warnings` |
| Privacy | Logs contain connection and match ids only, never names or addresses |

**Known limit:** trophies and levels are self-reported by the browser, because records live only in IndexedDB. Someone who edits their own browser storage can change their own numbers. The server only uses them for display and to pair similar players, so this can't affect anyone else's match or record. Truly tamper-proof rankings would need server-side storage, which this design deliberately avoids.

## Run locally

Requires Rust 1.82 or newer.

```bash
cd server
cargo run --release
# open http://127.0.0.1:8080 in two browser windows (use a private window for the second
# player so it gets its own IndexedDB profile), then tap Play online → Quick match in both
```

Run the tests:

```bash
cd server
cargo test
cargo clippy --all-targets -- -D warnings
```

## Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `FT_BIND` | `127.0.0.1:8080` | Address to listen on (`0.0.0.0:8080` in a container) |
| `FT_WEB_DIR` | `web` or `../web` if found | Folder served as the website; leave unset and remove the folder to run the API only |
| `FT_ALLOWED_ORIGINS` | empty (same origin only) | Comma-separated origins allowed to connect, e.g. `https://play.example.com` |
| `FT_TRUST_PROXY` | `0` | `1` to read the client address from the last `X-Forwarded-For` entry. Only enable behind your own reverse proxy |
| `FT_HSTS` | `0` | `1` to send `Strict-Transport-Security` (when served over HTTPS) |
| `FT_MAX_CONNS` | `5000` | Total simultaneous connections |
| `FT_MAX_CONNS_PER_IP` | `8` | Simultaneous connections per address |
| `RUST_LOG` | `info` | Log level |

If the website is hosted somewhere other than the server, set the server address in `web/index.html`:

```html
<meta name="five-trails-server" content="wss://play-api.example.com/ws">
```

Then add that page's origin to `FT_ALLOWED_ORIGINS`, and allow the server in the page's own CSP `connect-src`.

## Deploy

Always serve the game over HTTPS so the WebSocket uses `wss://`.

**Docker** (from the project root):

```bash
docker build -t five-trails .
docker run -d --name five-trails -p 127.0.0.1:8080:8080 \
  -e FT_TRUST_PROXY=1 -e FT_HSTS=1 five-trails
```

The image builds a static release binary and runs it as a non-root user on a minimal distroless base.

**TLS with Caddy** in front (automatic HTTPS certificates): copy `Caddyfile.example` to `Caddyfile`, put in your domain, and run `caddy run`. Caddy keeps the `Host` header and appends the client address to `X-Forwarded-For`, which is why `FT_TRUST_PROXY=1` is safe in that setup.

**Scaling:** one instance comfortably handles thousands of players, since each match is a lightweight task. All players who should be able to find each other must reach the same instance. To run several instances, route by sticky sessions and accept that the matchmaking pools are separate.

## Wire protocol (version 1)

JSON text frames, each tagged by `t`.

- **Client → server:** `hello {v, name, pid, tr, lv, resume?}` (must come first), `profile {name, tr, lv}`, `ping`, `queue {clock}`, `unqueue`, `create {clock}`, `cancel_room`, `join {code}`, `move {seq, a: play|disc|draw|sab|rush, id?, pile?}`, `emote {k}`, `forfeit`, `rematch`, `leave_match`
- **Server → client:** `welcome {stats}`, `stats {stats}`, `queued`, `unqueued`, `room {code}`, `room_closed`, `room_expired`, `match {view, opp, token, kind, code, clock, oppConnected}`, `state {view}`, `opp {connected, left}`, `emote {k}`, `rematch {me, opp}`, `closed`, `resume_failed`, `pong`, `error {code}`

`clock` is one of `0, 180, 300, 600` seconds per player. `view` contains only what the receiving player may see.
