# Five Trails

A two-player expedition card battler you can play in the browser, against the computer or against other people online.

```
five-trails/
├── server/          Rust server: names, leaderboard, matchmaking and authoritative matches
│   └── src/
│       ├── main.rs        HTTP + WebSocket entry point, security headers, static files
│       ├── conn.rs        One task per WebSocket: origin check, limits, message routing
│       ├── hub.rs         Who is online, quick-match queue, private rooms, resume tokens
│       ├── store.rs       Unique player names and the ranked leaderboard (saved to disk)
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

- **Most player data stays in the browser.** Trophies, XP, match history, head-to-head records, card style and settings are kept in IndexedDB (`five-trails` database).
- **Evil Eye:** once per battle (separate from Sabotage), a player can expose one random card in the rival's hand (they see it until it is played; the rival is told which card was seen). It plays a take-over on both screens: a 3-second build-up, then the reveal holds until the player taps (closing on its own after 15 s) on both screens, is locked when fewer than 10 cards remain in the deck and on a rushed turn, and the server only ever sends the one revealed card.
- **Accounts, rewards and seasons (protocol v2):** each device holds a secret 20-character recovery code (Settings → Account). Only its SHA-256 hash is sent on to the game; the opponent sees a public id only. Entering the code on another device restores the account, coins and a backup of progress. Coins come from finished online battles (win 15, draw 8, loss 5; half against the computer; at most 150 a day, and only battles long enough to count for quests), a free chest (every 6 h, bank of 2), daily quests (3) and weekly quests (2), and monthly season rewards (leaderboard resets monthly; all-time board is kept). Coins are spent in the Shop (tap the coins in the lobby): 11 table themes that restyle your own screen, and 10 card backs your rival sees in battle. Three of each are free, one card back (Summit Crown) is earned at 1400 trophies and cannot be bought, and purchases are kept on the server with the account. After 20 s in the quick-match queue the player may take an unranked computer battle (counts for quests, free to quit). Sign in with Apple and push notifications need the iOS app; until then reminders use local notifications through Capacitor's LocalNotifications plugin when present.
- **Cups:** private tournaments for 3 to 6 players (Cups tab). The host creates one, shares the 6-letter code, and starts it once at least 3 have joined. Round robin: everyone plays everyone once (win 3, draw 1, then head-to-head, then score difference), with three days to play. Knockout: a bracket seeded by trophies with byes for the top seeds, a day per match, and drawn ties replayed. A match starts when both players tap Play; at the deadline a player who showed up beats one who did not (otherwise a pointless draw in round robin, or the higher seed goes through in knockout). Prizes are paid by the game: champion 100 coins and the Champion's Laurel card back, runner-up 40, everyone else who played 15. Cup games never change trophies and do not count for quests or battle coins. Cups are saved with player data (`cups` in `players.json`). **Spectating:** while a cup match is being played, every other member of that cup sees it as live and can watch. Spectators get their own view from the server: the board, discards, scores, clocks, emotes and which powers were used, but no cards in either hand, no Evil Eye card, no pending Rush and no card drawn from the deck. Players see how many people are watching (up to 20 per match).
- **The server keeps two small things on disk** (`players.json` in `FT_DATA_DIR`): each player's unique name, and their ranked record from online Quick matches, which feeds the leaderboard. Everything else it holds is live and in memory: who is connected, the quick-match queue, open rooms and running matches. Restarting it ends any live matches but keeps names and rankings.
- **Names are unique.** A new player gets a generated name such as `SwiftOtter482` and can change it in Settings. The server compares names ignoring case and refuses one that is taken (`name_taken`). If a browser arrives with a name someone else already owns, the server gives it a numbered variant.
- **The server runs every online match.** It shuffles with a cryptographically secure RNG, deals, keeps both hands and the deck order hidden, checks every move, and runs the clocks and Rush timers. Each player only receives what they are allowed to see. A modified browser cannot peek at cards or make illegal moves.
- **Playing the computer happens entirely in the browser** and works without the server.

### Player discovery and matchmaking

- **Live counts:** the lobby shows how many players are online, searching and in battle, refreshed every 10 seconds.
- **Quick match:** pairs players who chose the same clock setting and have similar trophy counts. The allowed gap starts at 100 trophies and widens by 25 per second of waiting. After 20 seconds anyone with the same clock is a fair match. The same browser profile is never paired with itself.
- **Private rooms:** a room has a 5-letter code. Leave the box empty for a random one (23 unambiguous letters, about 6.4 million combinations), or type your own, such as `TIGER`. A room closes by itself after 2 minutes if nobody joins (`FT_ROOM_TTL_SECS`), and the host sees a countdown. The host can send the invite through WhatsApp, the phone's share sheet, or copy it (the invite includes a link that opens the join box with the code filled in). Private rooms are never ranked.
- **Leaderboard:** the Ranks tab shows the top 50 and your own place. Only online Quick matches count (+30 for a win, −20 for a loss, never below 0), and the server works out every result itself, so nobody can edit their way up. Games against the computer and friend rooms don't count, and leaving a battle against the computer is free: it records no win or loss.
- **Reconnects:** each player gets a random 256-bit resume token per match, kept in `sessionStorage`. If the connection drops, both clocks pause and the player has 45 seconds to come back, even after a page reload. If they don't, the match is forfeited.
- **Rematch:** both players tap Rematch on the results screen. A finished match stays open for 3 minutes.

### Card styles

Each arena unlocks a fancier card style (Classic, Sandstone, Pearl, Jade, Ember, Frost, Legend at 0, 100, 250, 450, 700, 1000 and 1400 trophies). Reaching a new arena equips its style; you can switch among unlocked styles in the Shop (Cards tab). This is purely cosmetic and lives in the browser.


## In-app purchases (coin packs, iPhone app only)

Coin packs are sold through Apple in the iPhone app (repository `five-trails-ios`), using the free
[cordova-plugin-purchase](https://github.com/j3k0/cordova-plugin-purchase) (MIT). The web version never shows them.

| Product id (App Store Connect, type Consumable) | Coins | Suggested price |
|---|---|---|
| `coins_300` | 300 | ₹99 / $0.99 |
| `coins_1000` | 1,000 (+11%) | ₹299 / $2.99 |
| `coins_1800` | 1,800 (+20%) | ₹499 / $4.99 |
| `coins_4000` | 4,000 (+33%) | ₹999 / $9.99 |

The app sends the server only a transaction id. The server looks it up with Apple's App Store Server API,
checks bundle id, product and that it was not refunded, pays each transaction once (to one account), and
takes coins back when Apple sends a refund notification. To turn it on, set:

- `FT_APPLE_ISSUER_ID`, `FT_APPLE_KEY_ID`: from App Store Connect → Users and Access → Integrations → In-App Purchase keys
- `FT_APPLE_KEY_FILE`: path to that `.p8` key (keep it secret; mount it into the container)
- `FT_APPLE_BUNDLE_ID`: the app's bundle id, e.g. `com.fivetrails.app`
- `FT_APPLE_SANDBOX=0` to refuse sandbox purchases (default: accepted, as App Review and TestFlight use them)
- `FT_ALLOWED_ORIGINS`: must include `capacitor://localhost` (the iPhone app) as well as your site's own origin

In App Store Connect → App Information, set the **App Store Server Notifications** URL (Version 2) to
`https://YOUR-SERVER/apple/notifications`.

## Security measures

| Area | What the server does |
| --- | --- |
| Cheating | Server-authoritative rules; opponents' hands, deck order and hidden draws are never sent; stale or out-of-turn moves are rejected |
| Randomness | Deck shuffles use `StdRng` (ChaCha) seeded from the OS; room codes and resume tokens come straight from `OsRng` |
| Cross-site WebSocket hijacking | `Origin` must match the page's own host, or an explicit allow list; missing `Origin` is refused |
| Flooding | 2 KB max message and frame size; 20-message burst then 8 per second per connection; persistent abusers are disconnected |
| Connection exhaustion | Global cap (default 5000) and per-address cap (default 8; IPv6 counted per /64); hello required within 10 s; idle connections closed after 75 s; 5 s write timeout for slow readers; bounded queues everywhere |
| Input | Strict JSON schema with unknown fields rejected; names limited to 1–16 letters, digits, spaces and `-_.'`; ids, codes, tokens and clock values all validated |
| Brute force | Room-code guessing blocked for 60 s after 8 wrong codes; trying to create a code that is already in use counts as a wrong guess, so open rooms can't be probed that way either. Custom codes are easier to guess than random ones, which the app says |
| Leaderboard | Results are computed by the server from matches it ran; only Quick matches count; the file is written atomically; a corrupt file stops startup instead of being overwritten; players who never finish a ranked match are forgotten after 14 days; the registry holds at most 200,000 players |
| Browser hardening | Strict Content-Security-Policy (no inline or eval'd script), `X-Frame-Options: DENY`, `nosniff`, `no-referrer`, COOP/CORP, optional HSTS |
| Code safety | `#![forbid(unsafe_code)]`, overflow checks in release builds, Clippy clean with `-D warnings` |
| Privacy | Logs contain connection and match ids only, never names or addresses |

**Known limits:** the trophy count shown in the lobby, XP and levels are kept by the browser (they include games against the computer), so someone who edits their own browser storage can change those. The server only uses them to pair similar players and for display; the leaderboard does not use them. Two friends can still trade wins in Quick matches to climb the leaderboard, and a name can be squatted by registering it first. Both would need accounts or reporting to solve properly.

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
| `FT_DATA_DIR` | `data` | Folder for `players.json` (names and leaderboard). Use `memory` to keep nothing on disk. Back this folder up and keep it on a persistent disk |
| `FT_DAY_OFFSET_MINUTES` | `330` | Minutes added to UTC to decide when a day, week (Monday) and season (calendar month) rolls over; 330 = India |
| `FT_CHEST_SECS` | `21600` | Seconds between free chests (10 to 86400). Up to 2 are banked |
| `FT_BOT_AFTER_SECS` | `20` | Seconds in the quick-match queue before a computer battle is offered (2 to 300) |
| `FT_ROOM_TTL_SECS` | `120` | How long a private room waits for a friend before closing (10 to 3600) |
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
  -v five-trails-data:/data -e FT_TRUST_PROXY=1 -e FT_HSTS=1 five-trails
```

The image builds a release binary and runs it as a non-root user on a minimal distroless base. The `-v five-trails-data:/data` volume holds `players.json`: without it, names and the leaderboard are lost whenever the container is replaced. On a hosting service, attach a persistent disk and point `FT_DATA_DIR` at it.

**TLS with Caddy** in front (automatic HTTPS certificates): copy `Caddyfile.example` to `Caddyfile`, put in your domain, and run `caddy run`. Caddy keeps the `Host` header and appends the client address to `X-Forwarded-For`, which is why `FT_TRUST_PROXY=1` is safe in that setup.

**Scaling:** one instance comfortably handles thousands of players, since each match is a lightweight task. All players who should be able to find each other must reach the same instance, and only one instance may write a given `players.json`. Running several instances would need a shared database for names and rankings.

## Wire protocol (version 1)

JSON text frames, each tagged by `t`.

- **Client → server:** `hello {v, name, pid, tr, lv, resume?}` (must come first), `profile {name, tr, lv}` (also how a name is changed), `ping`, `leaderboard`, `queue {clock}`, `unqueue`, `create {clock, code?}` (`code` is an optional 5-letter code of the host's choice), `cancel_room`, `join {code}`, `move {seq, a: play|disc|draw|sab|rush, id?, pile?}`, `emote {k}`, `forfeit`, `rematch`, `leave_match`
- **Server → client:** `welcome {stats, name, roomTtl}` (`name` is the name the server registered for you), `name {name}`, `leaderboard {top: [{n, tr, w, l, d, me}], me: {rank, n, tr, w, l, d} | null, total}`, `stats {stats}`, `queued`, `unqueued`, `room {code, ttl}`, `room_closed`, `room_expired`, `match {view, opp, token, kind, code, clock, oppConnected}`, `state {view}`, `opp {connected, left}`, `emote {k}`, `rematch {me, opp}`, `closed`, `resume_failed`, `pong`, `error {code}`

Error codes include `name_taken`, `code_taken`, `bad_code`, `room_not_found`, `too_many_attempts` and `server_full`. `clock` is one of `0, 180, 300, 600` seconds per player. `view` contains only what the receiving player may see.
