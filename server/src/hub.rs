//! The hub task: who is online, the quick-match queue, private rooms and resume tokens,
//! plus the persistent player registry and leaderboard (see `store`). Live state is in
//! memory and disappears on restart; the registry is saved to disk.

use crate::bot;
use crate::economy::{self, PlayEvent, PlayKind};
use crate::matchroom::{MatchCmd, MatchId, MatchKind, MatchRoom, MatchTx, Player};
use crate::protocol::{self, Profile, CLOCK_CHOICES, ROOM_ALPHABET, ROOM_CODE_LEN};
use crate::store::{self, LoginError, Outcome, Prog, RankScope, Ranking, RenameError, Store};
use rand::{rngs::OsRng, Rng};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

pub type ConnId = u64;
pub type ConnTx = mpsc::Sender<ConnCtl>;
pub type HubTx = mpsc::Sender<HubCmd>;

const MAX_ROOMS: usize = 5000;
const JOIN_FAILS_BEFORE_BLOCK: u32 = 8;
const JOIN_BLOCK: Duration = Duration::from_secs(60);
const STATS_EVERY: Duration = Duration::from_secs(10);
/// How often changed player data is written to disk.
const SAVE_EVERY: Duration = Duration::from_secs(10);
/// The leaderboard snapshot is rebuilt at most this often, however many people ask.
const RANKING_MIN_AGE: Duration = Duration::from_secs(3);
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);
/// Rows sent to a client; its own place is sent separately.
const LEADERBOARD_ROWS: usize = 50;
/// After this long in the queue, anyone with the same clock setting is a fair match.
const QUEUE_OPEN_AFTER: Duration = Duration::from_secs(20);
/// After this long in the queue the player is offered a game against the computer.
pub const BOT_OFFER_AFTER: Duration = Duration::from_secs(20);

/// Messages from the hub or a match to one connection task.
#[derive(Debug)]
pub enum ConnCtl {
    Send(String),
    Joined { tx: MatchTx, seat: usize },
    Left,
    /// Close the connection after what was already sent.
    Close,
}

#[derive(Debug)]
pub enum HubCmd {
    Connect { id: ConnId, tx: ConnTx },
    /// `key_hash` is the SHA-256 of the player's secret account key; the key itself never gets here.
    /// `create` is false when the player is restoring an account: an unknown key is then refused.
    Hello { id: ConnId, profile: Profile, key_hash: String, create: bool, resume: Option<String> },
    UpdateProfile { id: ConnId, name: String, trophies: u32, level: u32 },
    Queue { id: ConnId, clock: u32 },
    Unqueue { id: ConnId },
    Create { id: ConnId, clock: u32, code: Option<String> },
    CancelRoom { id: ConnId },
    Join { id: ConnId, code: String },
    Leaderboard { id: ConnId, season: bool },
    Me { id: ConnId },
    Sync { id: ConnId, prog: Prog },
    ClaimChest { id: ConnId },
    ClaimQuest { id: ConnId, slot: String },
    Buy { id: ConnId, item: String },
    Equip { id: ConnId, kind: String, item: String },
    AckSeason { id: ConnId },
    /// Play the computer instead of waiting for an opponent.
    Bot { id: ConnId },
    /// An online match finished. `pids` and `events` are in seat order; the computer's pid is empty.
    MatchResult { pids: [String; 2], outcome: Outcome, kind: PlayKind, events: [PlayEvent; 2], counted: bool },
    /// Write player data to disk now and confirm (used on shutdown).
    Flush { done: oneshot::Sender<()> },
    LeftMatch { id: ConnId },
    Disconnect { id: ConnId },
    MatchClosed { id: MatchId, tokens: Vec<String>, conns: Vec<ConnId> },
}

#[derive(Debug, PartialEq, Eq)]
enum Activity {
    Idle,
    Queued,
    Hosting(String),
    InMatch(MatchId),
}

struct ConnInfo {
    tx: ConnTx,
    profile: Option<Profile>,
    activity: Activity,
    join_fails: u32,
    join_blocked_until: Option<Instant>,
}

struct QueueEntry {
    id: ConnId,
    clock: u32,
    trophies: u32,
    since: Instant,
    /// The computer has been offered.
    offered: bool,
}

struct Room {
    host: ConnId,
    clock: u32,
    created: Instant,
}

pub struct Hub {
    rx: mpsc::Receiver<HubCmd>,
    tx: HubTx,
    conns: HashMap<ConnId, ConnInfo>,
    queue: Vec<QueueEntry>,
    rooms: HashMap<String, Room>,
    resumes: HashMap<String, (MatchId, MatchTx, usize)>,
    next_match: MatchId,
    last_stats: Instant,
    store: Store,
    data_path: Option<PathBuf>,
    last_save: Instant,
    last_prune: Instant,
    save_running: Arc<AtomicBool>,
    save_failed: Arc<AtomicBool>,
    ranking: Ranking,
    season_ranking: Ranking,
    ranking_at: Instant,
    ranking_dirty: bool,
    /// How long a private room waits for a friend before closing.
    room_ttl: Duration,
    /// How long a queued player waits before the computer is offered.
    bot_offer_after: Duration,
}

fn random_token() -> String {
    let bytes: [u8; 32] = OsRng.gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_code() -> String {
    (0..ROOM_CODE_LEN).map(|_| ROOM_ALPHABET[OsRng.gen_range(0..ROOM_ALPHABET.len())] as char).collect()
}

/// Trophy gap allowed for a pairing, widening the longer someone waits.
fn trophy_window(waited: Duration) -> u32 {
    if waited >= QUEUE_OPEN_AFTER {
        u32::MAX
    } else {
        100 + 25 * u32::try_from(waited.as_secs()).unwrap_or(u32::MAX / 25)
    }
}

impl Hub {
    pub fn new(mut store: Store, data_path: Option<PathBuf>, room_ttl: Duration) -> (Self, HubTx) {
        let (tx, rx) = mpsc::channel(4096);
        // A month may have ended while the server was down: pay out and start the new season.
        let paid = store.rollover(store::now_secs());
        if paid > 0 {
            tracing::info!(paid, "season closed, rewards paid");
        }
        // Start with the saved standings so the leaderboards are right from the first request.
        let ranking = store.ranking(RankScope::AllTime);
        let season_ranking = store.ranking(RankScope::Season);
        let hub = Self {
            rx,
            tx: tx.clone(),
            conns: HashMap::new(),
            queue: Vec::new(),
            rooms: HashMap::new(),
            resumes: HashMap::new(),
            next_match: 1,
            last_stats: Instant::now(),
            store,
            data_path,
            last_save: Instant::now(),
            last_prune: Instant::now(),
            save_running: Arc::new(AtomicBool::new(false)),
            save_failed: Arc::new(AtomicBool::new(false)),
            ranking,
            season_ranking,
            ranking_at: Instant::now(),
            ranking_dirty: false,
            room_ttl,
            bot_offer_after: BOT_OFFER_AFTER,
        };
        (hub, tx)
    }

    /// Change how long a queued player waits before the computer is offered.
    pub fn with_bot_wait(mut self, wait: Duration) -> Self {
        self.bot_offer_after = wait;
        self
    }

    pub async fn run(mut self) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                cmd = self.rx.recv() => match cmd {
                    Some(cmd) => self.handle(cmd),
                    None => break,
                },
                _ = tick.tick() => self.tick(),
            }
        }
    }

    fn send(&self, id: ConnId, text: String) {
        if let Some(c) = self.conns.get(&id) {
            let _ = c.tx.try_send(ConnCtl::Send(text));
        }
    }

    fn stats_json(&self) -> serde_json::Value {
        let online = self.conns.values().filter(|c| c.profile.is_some()).count();
        let playing = self.conns.values().filter(|c| matches!(c.activity, Activity::InMatch(_))).count();
        json!({ "online": online, "searching": self.queue.len(), "playing": playing })
    }

    /// Rebuild the leaderboard snapshot if something changed and it is not too fresh.
    fn refresh_ranking(&mut self) {
        if self.ranking_dirty && self.ranking_at.elapsed() >= RANKING_MIN_AGE {
            self.ranking = self.store.ranking(RankScope::AllTime);
            self.season_ranking = self.store.ranking(RankScope::Season);
            self.ranking_at = Instant::now();
            self.ranking_dirty = false;
        }
    }

    fn leaderboard_json(&self, pid: &str, season: bool) -> String {
        let ranking = if season { &self.season_ranking } else { &self.ranking };
        let top: Vec<Value> = ranking
            .rows
            .iter()
            .take(LEADERBOARD_ROWS)
            .map(|p| json!({ "n": p.name, "tr": p.tr, "w": p.w, "l": p.l, "d": p.d, "me": p.pid == pid }))
            .collect();
        let me = match ranking.rank_of(pid) {
            Some((rank, p)) => json!({ "rank": rank, "n": p.name, "tr": p.tr, "w": p.w, "l": p.l, "d": p.d }),
            None => Value::Null,
        };
        let mut m = json!({ "t": "leaderboard", "scope": if season { "season" } else { "all" }, "top": top, "me": me, "total": ranking.rows.len() });
        if season {
            let now = store::now_secs();
            m["season"] = json!({ "name": economy::season_name(self.store.season_id()), "endsIn": economy::season_ends_in(now, &self.store.tuning()) });
        }
        protocol::msg(m)
    }

    /// Send each connection of player `pid` their current rewards screen data.
    fn send_me_to_pid(&mut self, pid: &str) {
        let ids: Vec<ConnId> = self.conns.iter().filter(|(_, c)| c.profile.as_ref().is_some_and(|p| p.pid == pid)).map(|(id, _)| *id).collect();
        if ids.is_empty() {
            return;
        }
        if let Some(me) = self.store.me_json(pid, store::now_secs()) {
            let text = protocol::msg(me);
            for id in ids {
                self.send(id, text.clone());
            }
        }
    }

    fn pid_of(&self, id: ConnId) -> Option<String> {
        self.conns.get(&id).and_then(|c| c.profile.as_ref()).map(|p| p.pid.clone())
    }

    /// Write the player data to disk on this thread (shutdown).
    fn save_now(&mut self) {
        let Some(path) = &self.data_path else { return };
        match store::write_atomic(path, &self.store.to_json()) {
            Ok(()) => self.store.mark_clean(),
            Err(e) => tracing::error!(error = %e, "could not save player data"),
        }
    }

    /// Write changed player data to disk in the background, at most every `SAVE_EVERY`.
    fn save_soon(&mut self) {
        if self.save_failed.swap(false, Ordering::Relaxed) {
            self.store.mark_dirty();
        }
        let Some(path) = self.data_path.clone() else { return };
        if !self.store.is_dirty() || self.save_running.load(Ordering::Relaxed) || self.last_save.elapsed() < SAVE_EVERY {
            return;
        }
        self.last_save = Instant::now();
        self.store.mark_clean();
        let json = self.store.to_json();
        let (running, failed) = (Arc::clone(&self.save_running), Arc::clone(&self.save_failed));
        running.store(true, Ordering::Relaxed);
        tokio::task::spawn_blocking(move || {
            if let Err(e) = store::write_atomic(&path, &json) {
                tracing::error!(error = %e, "could not save player data");
                failed.store(true, Ordering::Relaxed);
            }
            running.store(false, Ordering::Relaxed);
        });
    }

    fn handle(&mut self, cmd: HubCmd) {
        match cmd {
            HubCmd::Connect { id, tx } => {
                self.conns.insert(id, ConnInfo { tx, profile: None, activity: Activity::Idle, join_fails: 0, join_blocked_until: None });
            }
            HubCmd::Hello { id, mut profile, key_hash, create, resume } => {
                let Some(c) = self.conns.get(&id) else { return };
                if c.profile.is_some() {
                    return;
                }
                // The key decides which account this is; the id and name the browser sent only
                // matter for a brand-new account. Names are unique.
                let login = match self.store.login(&profile.pid, &key_hash, &profile.name, create, store::now_secs()) {
                    Ok(l) => l,
                    Err(LoginError::Unknown) => {
                        self.send(id, protocol::error("unknown_key"));
                        if let Some(c) = self.conns.get(&id) {
                            let _ = c.tx.try_send(ConnCtl::Close);
                        }
                        return;
                    }
                    Err(LoginError::Full) => {
                        self.send(id, protocol::error("server_full"));
                        if let Some(c) = self.conns.get(&id) {
                            let _ = c.tx.try_send(ConnCtl::Close);
                        }
                        return;
                    }
                };
                self.ranking_dirty = true;
                profile.name = login.name.clone();
                profile.pid = login.pid.clone();
                profile.back = self.store.back_of(&login.pid);
                let prog = self.store.prog(&login.pid);
                let Some(c) = self.conns.get_mut(&id) else { return };
                c.profile = Some(profile);
                let welcome = protocol::msg(json!({
                    "t": "welcome", "stats": self.stats_json(), "name": login.name, "pid": login.pid,
                    "created": login.created, "roomTtl": self.room_ttl.as_secs(), "prog": prog,
                }));
                self.send(id, welcome);
                if let Some(me) = self.store.me_json(&login.pid, store::now_secs()) {
                    self.send(id, protocol::msg(me));
                }
                if let Some(token) = resume {
                    self.resume(id, &token);
                }
            }
            HubCmd::UpdateProfile { id, name, trophies, level } => {
                let Some(pid) = self.conns.get(&id).and_then(|c| c.profile.as_ref()).map(|p| p.pid.clone()) else { return };
                let renamed = self.store.rename(&pid, &name, store::now_secs());
                if let Some(p) = self.conns.get_mut(&id).and_then(|c| c.profile.as_mut()) {
                    p.trophies = trophies.min(1_000_000);
                    p.level = level.clamp(1, 10_000);
                    if let Ok(n) = &renamed {
                        p.name = n.clone();
                    }
                }
                let reply = match renamed {
                    Ok(n) => {
                        self.ranking_dirty = true;
                        protocol::msg(json!({ "t": "name", "name": n }))
                    }
                    Err(RenameError::Taken) => protocol::error("name_taken"),
                    Err(RenameError::Unknown) => protocol::error("bad_hello"),
                };
                self.send(id, reply);
            }
            HubCmd::Leaderboard { id, season } => {
                let Some(pid) = self.pid_of(id) else { return };
                self.refresh_ranking();
                let text = self.leaderboard_json(&pid, season);
                self.send(id, text);
            }
            HubCmd::Me { id } => {
                if let Some(pid) = self.pid_of(id) {
                    if let Some(me) = self.store.me_json(&pid, store::now_secs()) {
                        self.send(id, protocol::msg(me));
                    }
                }
            }
            HubCmd::Sync { id, prog } => {
                if let Some(pid) = self.pid_of(id) {
                    self.store.set_prog(&pid, prog);
                }
            }
            HubCmd::ClaimChest { id } => {
                let Some(pid) = self.pid_of(id) else { return };
                match self.store.claim_chest(&pid, store::now_secs()) {
                    Ok(coins) => {
                        self.send(id, protocol::msg(json!({ "t": "claimed", "what": "chest", "coins": coins })));
                        self.send_me_to_pid(&pid);
                    }
                    Err(e) => self.send(id, protocol::error(e.code())),
                }
            }
            HubCmd::ClaimQuest { id, slot } => {
                let Some(pid) = self.pid_of(id) else { return };
                match self.store.claim_quest(&pid, &slot, store::now_secs()) {
                    Ok(coins) => {
                        self.send(id, protocol::msg(json!({ "t": "claimed", "what": "quest", "slot": slot, "coins": coins })));
                        self.send_me_to_pid(&pid);
                    }
                    Err(e) => self.send(id, protocol::error(e.code())),
                }
            }
            HubCmd::Buy { id, item } => {
                let Some(pid) = self.pid_of(id) else { return };
                match self.store.buy(&pid, &item) {
                    Ok(coins) => {
                        self.send(id, protocol::msg(json!({ "t": "bought", "item": item, "coins": coins })));
                        self.send_me_to_pid(&pid);
                    }
                    Err(e) => self.send(id, protocol::error(e.code())),
                }
            }
            HubCmd::Equip { id, kind, item } => {
                let Some(pid) = self.pid_of(id) else { return };
                match self.store.equip(&pid, &kind, &item) {
                    Ok(back) => {
                        if let Some(p) = self.conns.get_mut(&id).and_then(|c| c.profile.as_mut()) {
                            p.back = back;
                        }
                        self.send_me_to_pid(&pid);
                    }
                    Err(e) => self.send(id, protocol::error(e.code())),
                }
            }
            HubCmd::AckSeason { id } => {
                if let Some(pid) = self.pid_of(id) {
                    self.store.ack_season(&pid);
                }
            }
            HubCmd::Bot { id } => self.start_bot_for(id),
            HubCmd::MatchResult { pids, outcome, kind, events, counted } => {
                let now = store::now_secs();
                if kind == PlayKind::Ranked {
                    self.store.record_match([&pids[0], &pids[1]], outcome, now);
                    self.ranking_dirty = true;
                }
                for seat in 0..2 {
                    self.store.record_play(&pids[seat], &events[seat], counted, now);
                    self.send_me_to_pid(&pids[seat]);
                }
            }
            HubCmd::Flush { done } => {
                self.save_now();
                let _ = done.send(());
            }
            HubCmd::Queue { id, clock } => {
                if !CLOCK_CHOICES.contains(&clock) {
                    return self.send(id, protocol::error("bad_clock"));
                }
                let Some(c) = self.conns.get_mut(&id) else { return };
                let Some(trophies) = c.profile.as_ref().map(|p| p.trophies) else { return };
                if c.activity != Activity::Idle {
                    return self.send(id, protocol::error("busy"));
                }
                c.activity = Activity::Queued;
                self.queue.push(QueueEntry { id, clock, trophies, since: Instant::now(), offered: false });
                self.send(id, protocol::msg(json!({ "t": "queued", "clock": clock })));
                self.matchmake();
            }
            HubCmd::Unqueue { id } => {
                if self.leave_queue(id) {
                    self.send(id, protocol::msg(json!({ "t": "unqueued" })));
                }
            }
            HubCmd::Create { id, clock, code } => {
                if !CLOCK_CHOICES.contains(&clock) {
                    return self.send(id, protocol::error("bad_clock"));
                }
                let now = Instant::now();
                let Some(c) = self.conns.get(&id) else { return };
                if c.profile.is_none() || c.activity != Activity::Idle {
                    return self.send(id, protocol::error("busy"));
                }
                if c.join_blocked_until.is_some_and(|t| now < t) {
                    return self.send(id, protocol::error("too_many_attempts"));
                }
                if self.rooms.len() >= MAX_ROOMS {
                    return self.send(id, protocol::error("server_full"));
                }
                let code = match code {
                    Some(raw) => {
                        let Some(code) = protocol::clean_code(&raw) else {
                            return self.send(id, protocol::error("bad_code"));
                        };
                        if self.rooms.contains_key(&code) {
                            // Telling someone a code is taken would let them probe for open rooms,
                            // so these count against the same limit as wrong guesses when joining.
                            self.note_failed_attempt(id, now);
                            return self.send(id, protocol::error("code_taken"));
                        }
                        code
                    }
                    None => loop {
                        let code = random_code();
                        if !self.rooms.contains_key(&code) {
                            break code;
                        }
                    },
                };
                self.rooms.insert(code.clone(), Room { host: id, clock, created: now });
                if let Some(c) = self.conns.get_mut(&id) {
                    c.activity = Activity::Hosting(code.clone());
                }
                let ttl = self.room_ttl.as_secs();
                self.send(id, protocol::msg(json!({ "t": "room", "code": code, "ttl": ttl })));
            }
            HubCmd::CancelRoom { id } => {
                if self.close_room_of(id) {
                    self.send(id, protocol::msg(json!({ "t": "room_closed" })));
                }
            }
            HubCmd::Join { id, code } => self.join(id, &code),
            HubCmd::LeftMatch { id } => {
                if let Some(c) = self.conns.get_mut(&id) {
                    if matches!(c.activity, Activity::InMatch(_)) {
                        c.activity = Activity::Idle;
                    }
                }
            }
            HubCmd::Disconnect { id } => {
                self.leave_queue(id);
                self.close_room_of(id);
                self.conns.remove(&id);
            }
            HubCmd::MatchClosed { id, tokens, conns } => {
                for t in tokens {
                    self.resumes.remove(&t);
                }
                for cid in conns {
                    if let Some(c) = self.conns.get_mut(&cid) {
                        if c.activity == Activity::InMatch(id) {
                            c.activity = Activity::Idle;
                        }
                    }
                }
                // A player may have moved on from this match on another connection.
                for c in self.conns.values_mut() {
                    if c.activity == Activity::InMatch(id) {
                        c.activity = Activity::Idle;
                    }
                }
            }
        }
    }

    fn resume(&mut self, id: ConnId, token: &str) {
        let Some((mid, mtx, seat)) = self.resumes.get(token).cloned() else {
            return self.send(id, protocol::msg(json!({ "t": "resume_failed" })));
        };
        let Some(c) = self.conns.get_mut(&id) else { return };
        if c.activity != Activity::Idle {
            return;
        }
        c.activity = Activity::InMatch(mid);
        let tx = c.tx.clone();
        let _ = tx.try_send(ConnCtl::Joined { tx: mtx.clone(), seat });
        if mtx.try_send(MatchCmd::Attach { seat, conn: id, tx }).is_err() {
            c.activity = Activity::Idle;
            self.send(id, protocol::msg(json!({ "t": "resume_failed" })));
        }
    }

    fn join(&mut self, id: ConnId, code: &str) {
        let now = Instant::now();
        let Some(c) = self.conns.get_mut(&id) else { return };
        let Some(me) = c.profile.clone() else { return };
        if c.activity != Activity::Idle {
            return self.send(id, protocol::error("busy"));
        }
        if c.join_blocked_until.is_some_and(|t| now < t) {
            return self.send(id, protocol::error("too_many_attempts"));
        }
        let found = protocol::clean_code(code).and_then(|code| {
            let room = self.rooms.get(&code)?;
            let host = self.conns.get(&room.host)?;
            let same_player = host.profile.as_ref().is_some_and(|h| h.pid == me.pid);
            (room.host != id && !same_player).then_some(code)
        });
        match found {
            Some(code) => {
                let room = self.rooms.remove(&code).expect("room exists");
                self.start_match(room.host, id, room.clock, MatchKind::Room(code));
            }
            None => {
                self.note_failed_attempt(id, now);
                self.send(id, protocol::error("room_not_found"));
            }
        }
    }

    /// Count a wrong room code (or an already-used one when creating) and block the
    /// connection for a while after too many.
    fn note_failed_attempt(&mut self, id: ConnId, now: Instant) {
        if let Some(c) = self.conns.get_mut(&id) {
            c.join_fails += 1;
            if c.join_fails >= JOIN_FAILS_BEFORE_BLOCK {
                c.join_fails = 0;
                c.join_blocked_until = Some(now + JOIN_BLOCK);
            }
        }
    }

    fn leave_queue(&mut self, id: ConnId) -> bool {
        let before = self.queue.len();
        self.queue.retain(|q| q.id != id);
        if let Some(c) = self.conns.get_mut(&id) {
            if c.activity == Activity::Queued {
                c.activity = Activity::Idle;
            }
        }
        self.queue.len() != before
    }

    fn close_room_of(&mut self, id: ConnId) -> bool {
        let Some(c) = self.conns.get_mut(&id) else { return false };
        if let Activity::Hosting(code) = &c.activity {
            self.rooms.remove(code);
            c.activity = Activity::Idle;
            return true;
        }
        false
    }

    fn matchmake(&mut self) {
        let now = Instant::now();
        loop {
            let mut best: Option<(usize, usize, u32)> = None;
            for i in 0..self.queue.len() {
                for j in (i + 1)..self.queue.len() {
                    let (a, b) = (&self.queue[i], &self.queue[j]);
                    if a.clock != b.clock {
                        continue;
                    }
                    let pid = |id| self.conns.get(&id).and_then(|c| c.profile.as_ref()).map(|p| p.pid.clone());
                    if pid(a.id) == pid(b.id) {
                        continue;
                    }
                    let gap = a.trophies.abs_diff(b.trophies);
                    let waited = now.duration_since(a.since.min(b.since));
                    if gap <= trophy_window(waited) && best.is_none_or(|(_, _, g)| gap < g) {
                        best = Some((i, j, gap));
                    }
                }
            }
            let Some((i, j, _)) = best else { break };
            let b = self.queue.remove(j);
            let a = self.queue.remove(i);
            for id in [a.id, b.id] {
                if let Some(c) = self.conns.get_mut(&id) {
                    c.activity = Activity::Idle;
                }
            }
            self.start_match(a.id, b.id, a.clock, MatchKind::Quick);
        }
    }

    fn start_match(&mut self, a: ConnId, b: ConnId, clock: u32, kind: MatchKind) {
        let (Some(ca), Some(cb)) = (self.conns.get(&a), self.conns.get(&b)) else { return };
        let (Some(pa), Some(pb)) = (ca.profile.clone(), cb.profile.clone()) else { return };
        let id = self.next_match;
        self.next_match += 1;
        let (mtx, mrx) = mpsc::channel(64);
        let (ta, tb) = (random_token(), random_token());
        // Tell each connection which match and seat it now belongs to before the match speaks.
        let _ = ca.tx.try_send(ConnCtl::Joined { tx: mtx.clone(), seat: 0 });
        let _ = cb.tx.try_send(ConnCtl::Joined { tx: mtx.clone(), seat: 1 });
        let players = [Player::new(pa, ta.clone(), a, ca.tx.clone()), Player::new(pb, tb.clone(), b, cb.tx.clone())];
        self.resumes.insert(ta, (id, mtx.clone(), 0));
        self.resumes.insert(tb, (id, mtx, 1));
        for cid in [a, b] {
            if let Some(c) = self.conns.get_mut(&cid) {
                c.activity = Activity::InMatch(id);
            }
        }
        let room = MatchRoom::new(id, kind, clock, players, mrx, self.tx.clone());
        tokio::spawn(room.run());
    }

    /// A queued player asked to play the computer instead of waiting.
    fn start_bot_for(&mut self, id: ConnId) {
        let waited_enough = self.queue.iter().find(|q| q.id == id).is_some_and(|q| q.since.elapsed() >= self.bot_offer_after * 3 / 4);
        if !waited_enough {
            return self.send(id, protocol::error("bot_unavailable"));
        }
        let Some(entry) = self.queue.iter().position(|q| q.id == id).map(|i| self.queue.remove(i)) else { return };
        let Some(c) = self.conns.get_mut(&id) else { return };
        c.activity = Activity::Idle;
        let Some(profile) = c.profile.clone() else { return };
        let match_id = self.next_match;
        self.next_match += 1;
        let (mtx, mrx) = mpsc::channel(64);
        let token = random_token();
        let _ = c.tx.try_send(ConnCtl::Joined { tx: mtx.clone(), seat: 0 });
        let tier = bot::tier_for(profile.trophies);
        let players = [Player::new(profile.clone(), token.clone(), id, c.tx.clone()), Player::bot("Computer", profile.trophies)];
        c.activity = Activity::InMatch(match_id);
        self.resumes.insert(token, (match_id, mtx, 0));
        let room = MatchRoom::new(match_id, MatchKind::Bot(tier), entry.clock, players, mrx, self.tx.clone());
        tokio::spawn(room.run());
    }

    fn tick(&mut self) {
        self.matchmake();
        let now = Instant::now();
        // Offer the computer to anyone who has waited a while.
        let mut offers = Vec::new();
        let offer_after = self.bot_offer_after;
        for q in &mut self.queue {
            if !q.offered && now.duration_since(q.since) >= offer_after {
                q.offered = true;
                offers.push(q.id);
            }
        }
        for id in offers {
            self.send(id, protocol::msg(json!({ "t": "bot_offer" })));
        }
        // A new month: pay out last season's rewards and tell everyone online.
        let before = self.store.season_id();
        if self.store.rollover(store::now_secs()) > 0 || self.store.season_id() != before {
            self.ranking_dirty = true;
            let pids: Vec<String> = self.conns.values().filter_map(|c| c.profile.as_ref().map(|p| p.pid.clone())).collect();
            for pid in pids {
                self.send_me_to_pid(&pid);
            }
        }
        if now.duration_since(self.last_prune) >= PRUNE_EVERY {
            self.last_prune = now;
            self.store.prune(store::now_secs());
        }
        self.save_soon();
        let expired: Vec<String> = self.rooms.iter().filter(|(_, r)| now.duration_since(r.created) >= self.room_ttl).map(|(k, _)| k.clone()).collect();
        for code in expired {
            if let Some(room) = self.rooms.remove(&code) {
                if let Some(c) = self.conns.get_mut(&room.host) {
                    c.activity = Activity::Idle;
                }
                self.send(room.host, protocol::msg(json!({ "t": "room_expired" })));
            }
        }
        if now.duration_since(self.last_stats) >= STATS_EVERY {
            self.last_stats = now;
            let text = protocol::msg(json!({ "t": "stats", "stats": self.stats_json() }));
            for c in self.conns.values() {
                if c.profile.is_some() && !matches!(c.activity, Activity::InMatch(_)) {
                    let _ = c.tx.try_send(ConnCtl::Send(text.clone()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_widens_then_opens() {
        assert_eq!(trophy_window(Duration::ZERO), 100);
        assert_eq!(trophy_window(Duration::from_secs(10)), 350);
        assert_eq!(trophy_window(QUEUE_OPEN_AFTER), u32::MAX);
    }

    #[test]
    fn saved_standings_are_on_the_leaderboard_straight_after_startup() {
        let mut saved = Store::new();
        saved.register("pid-aaaaaaaa", "Ann", 1);
        saved.register("pid-bbbbbbbb", "Bob", 1);
        saved.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Win(0), 2);
        let (hub, _tx) = Hub::new(Store::from_json(&saved.to_json()).unwrap(), None, Duration::from_secs(120));
        let v: Value = serde_json::from_str(&hub.leaderboard_json("pid-aaaaaaaa", false)).unwrap();
        assert_eq!(v["total"], 2);
        assert_eq!(v["me"]["rank"], 1);
    }

    #[test]
    fn leaderboard_lists_ranked_players_and_flags_the_asker() {
        let (mut hub, _tx) = Hub::new(Store::new(), None, Duration::from_secs(120));
        hub.store.register("pid-aaaaaaaa", "Ann", 1);
        hub.store.register("pid-bbbbbbbb", "Bob", 1);
        hub.store.register("pid-cccccccc", "Cy", 1); // never plays a ranked match
        hub.store.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Win(0), 2);
        hub.ranking = hub.store.ranking(RankScope::AllTime);
        let v: Value = serde_json::from_str(&hub.leaderboard_json("pid-bbbbbbbb", false)).unwrap();
        assert_eq!(v["t"], "leaderboard");
        assert_eq!(v["total"], 2);
        assert_eq!(v["top"][0]["n"], "Ann");
        assert_eq!(v["top"][0]["tr"], 30);
        assert_eq!(v["top"][0]["me"], false);
        assert_eq!(v["top"][1]["me"], true);
        assert_eq!(v["me"]["rank"], 2);
        let unranked: Value = serde_json::from_str(&hub.leaderboard_json("pid-cccccccc", false)).unwrap();
        assert!(unranked["me"].is_null());
        // The payload never carries anyone's player id.
        assert!(!hub.leaderboard_json("pid-cccccccc", false).contains("pid-"));
        assert!(!hub.leaderboard_json("pid-cccccccc", true).contains("pid-"));
    }

    /// A hub with `n` connected, greeted players and the receiving end of each one's channel.
    fn hub_with_players(n: u64, ttl: Duration) -> (Hub, Vec<mpsc::Receiver<ConnCtl>>) {
        let (mut hub, _tx) = Hub::new(Store::new(), None, ttl);
        let mut rxs = Vec::new();
        for id in 1..=n {
            let (tx, rx) = mpsc::channel(64);
            hub.handle(HubCmd::Connect { id, tx });
            let profile = protocol::profile_from_hello(&format!("P{id}"), &format!("pid-{id:08}"), 0, 1).unwrap();
            hub.handle(HubCmd::Hello { id, key_hash: store::test_auth(&profile.pid), profile, create: true, resume: None });
            rxs.push(rx);
        }
        (hub, rxs)
    }

    /// The last JSON message a connection was sent.
    fn last_msg(rx: &mut mpsc::Receiver<ConnCtl>) -> Value {
        let mut last = Value::Null;
        while let Ok(ctl) = rx.try_recv() {
            if let ConnCtl::Send(text) = ctl {
                last = serde_json::from_str(&text).unwrap();
            }
        }
        last
    }

    #[tokio::test]
    async fn custom_room_codes_are_accepted_once_and_validated() {
        let (mut hub, mut rx) = hub_with_players(3, Duration::from_secs(120));
        for r in rx.iter_mut() {
            last_msg(r); // drop the welcome
        }
        hub.handle(HubCmd::Create { id: 1, clock: 300, code: Some("tiger".into()) });
        let m = last_msg(&mut rx[0]);
        assert_eq!((m["t"].as_str(), m["code"].as_str(), m["ttl"].as_u64()), (Some("room"), Some("TIGER"), Some(120)));
        // The same code cannot be hosted twice.
        hub.handle(HubCmd::Create { id: 2, clock: 300, code: Some("TIGER".into()) });
        assert_eq!(last_msg(&mut rx[1])["code"], "code_taken");
        // Wrong length or characters are refused.
        for bad in ["TIG", "TIGERS", "TIG3R", ""] {
            hub.handle(HubCmd::Create { id: 2, clock: 300, code: Some(bad.into()) });
            assert_eq!(last_msg(&mut rx[1])["code"], "bad_code", "{bad:?}");
        }
        // A random code is five unambiguous letters.
        hub.handle(HubCmd::Create { id: 3, clock: 300, code: None });
        let code = last_msg(&mut rx[2])["code"].as_str().unwrap().to_owned();
        assert_eq!(code.len(), 5);
        assert!(code.bytes().all(|b| ROOM_ALPHABET.contains(&b)));
        // A friend can join the custom code in either case.
        hub.handle(HubCmd::Join { id: 2, code: "Tiger".into() });
        tokio::time::sleep(Duration::from_millis(50)).await; // let the match task start
        assert_eq!(last_msg(&mut rx[1])["t"], "match");
    }

    #[test]
    fn probing_for_open_rooms_by_creating_is_rate_limited() {
        let (mut hub, mut rx) = hub_with_players(2, Duration::from_secs(120));
        hub.handle(HubCmd::Create { id: 1, clock: 300, code: Some("ABCDE".into()) });
        last_msg(&mut rx[0]);
        last_msg(&mut rx[1]);
        for _ in 0..JOIN_FAILS_BEFORE_BLOCK {
            hub.handle(HubCmd::Create { id: 2, clock: 300, code: Some("ABCDE".into()) });
            assert_eq!(last_msg(&mut rx[1])["code"], "code_taken");
        }
        hub.handle(HubCmd::Create { id: 2, clock: 300, code: Some("ZZZZZ".into()) });
        assert_eq!(last_msg(&mut rx[1])["code"], "too_many_attempts");
    }

    #[test]
    fn an_unjoined_room_closes_after_its_timeout() {
        let (mut hub, mut rx) = hub_with_players(1, Duration::ZERO);
        hub.handle(HubCmd::Create { id: 1, clock: 0, code: Some("QUICK".into()) });
        assert_eq!(last_msg(&mut rx[0])["t"], "room");
        hub.tick();
        assert_eq!(last_msg(&mut rx[0])["t"], "room_expired");
        assert!(hub.rooms.is_empty());
        // The host is free to create another room straight away.
        hub.handle(HubCmd::Create { id: 1, clock: 0, code: None });
        assert_eq!(last_msg(&mut rx[0])["t"], "room");
    }

    /// Every JSON message a connection was sent, oldest first.
    fn all_msgs(rx: &mut mpsc::Receiver<ConnCtl>) -> Vec<Value> {
        let mut out = Vec::new();
        while let Ok(ctl) = rx.try_recv() {
            if let ConnCtl::Send(text) = ctl {
                out.push(serde_json::from_str(&text).unwrap());
            }
        }
        out
    }

    fn event(kind: PlayKind, won: bool) -> PlayEvent {
        PlayEvent { kind, won, sab: 1, eye: 1, rush: 1, wagers: 3, score: 40, longest: 8 }
    }

    #[test]
    fn the_same_key_on_another_device_is_the_same_account() {
        let (mut hub, mut rx) = hub_with_players(1, Duration::from_secs(120));
        let w = all_msgs(&mut rx[0]);
        assert_eq!(w[0]["t"], "welcome");
        assert_eq!((w[0]["created"].as_bool(), w[0]["name"].as_str(), w[0]["pid"].as_str()), (Some(true), Some("P1"), Some("pid-00000001")));
        assert_eq!(w[1]["t"], "me", "the rewards screen data follows the welcome");
        // A second device shows the same key but a different id and name.
        let (tx, mut rx2) = mpsc::channel(64);
        hub.handle(HubCmd::Connect { id: 9, tx });
        let profile = protocol::profile_from_hello("NewPhone", "pid-device-two", 0, 1).unwrap();
        hub.handle(HubCmd::Hello { id: 9, key_hash: store::test_auth("pid-00000001"), profile, create: true, resume: None });
        let w2 = all_msgs(&mut rx2);
        assert_eq!((w2[0]["created"].as_bool(), w2[0]["name"].as_str(), w2[0]["pid"].as_str()), (Some(false), Some("P1"), Some("pid-00000001")));
        // A device with another key gets its own account, even if it claims the same id.
        let (tx, mut rx3) = mpsc::channel(64);
        hub.handle(HubCmd::Connect { id: 10, tx });
        let profile = protocol::profile_from_hello("Mallory", "pid-00000001", 0, 1).unwrap();
        hub.handle(HubCmd::Hello { id: 10, key_hash: "someone-elses-key".into(), profile, create: true, resume: None });
        let w3 = all_msgs(&mut rx3);
        assert_eq!(w3[0]["created"], true);
        assert_ne!(w3[0]["pid"], "pid-00000001");
    }

    #[test]
    fn progress_backups_come_back_in_the_welcome_on_the_next_device() {
        let (mut hub, mut rx) = hub_with_players(1, Duration::from_secs(120));
        all_msgs(&mut rx[0]);
        hub.handle(HubCmd::Sync { id: 1, prog: Prog { tr: 90, best: 120, xp: 800, ai: [3, 1, 0], pvp: [1, 0, 0] } });
        let (tx, mut rx2) = mpsc::channel(64);
        hub.handle(HubCmd::Connect { id: 2, tx });
        let profile = protocol::profile_from_hello("NewPhone", "pid-device-two", 0, 1).unwrap();
        hub.handle(HubCmd::Hello { id: 2, key_hash: store::test_auth("pid-00000001"), profile, create: true, resume: None });
        let w = all_msgs(&mut rx2);
        assert_eq!((w[0]["prog"]["xp"].as_u64(), w[0]["prog"]["best"].as_u64(), w[0]["prog"]["ai"][0].as_u64()), (Some(800), Some(120), Some(3)));
    }

    #[test]
    fn buying_and_equipping_a_card_back_updates_what_rivals_will_see() {
        let (mut hub, mut rx) = hub_with_players(1, Duration::from_secs(120));
        all_msgs(&mut rx[0]);
        let pid = hub.pid_of(1).unwrap();
        hub.handle(HubCmd::Buy { id: 1, item: "paws".into() });
        assert_eq!(last_msg(&mut rx[0])["code"], "not_enough_coins");
        hub.store.get_mut(&pid).unwrap().coins = 250;
        hub.handle(HubCmd::Buy { id: 1, item: "paws".into() });
        let m = all_msgs(&mut rx[0]);
        assert_eq!((m[0]["t"].as_str(), m[0]["coins"].as_u64()), (Some("bought"), Some(50)));
        assert_eq!(m[1]["coins"], 50);
        hub.handle(HubCmd::Equip { id: 1, kind: "back".into(), item: "paws".into() });
        assert_eq!(last_msg(&mut rx[0])["shop"]["back"], "paws");
        assert_eq!(hub.conns[&1].profile.as_ref().unwrap().back, "paws");
        assert_eq!(hub.conns[&1].profile.as_ref().unwrap().to_json()["back"], "paws");
    }

    #[test]
    fn the_free_chest_pays_once_and_the_screen_updates() {
        let (mut hub, mut rx) = hub_with_players(1, Duration::from_secs(120));
        let first = all_msgs(&mut rx[0]);
        assert_eq!(first[1]["chest"]["n"], 1);
        hub.handle(HubCmd::ClaimChest { id: 1 });
        let m = all_msgs(&mut rx[0]);
        assert_eq!((m[0]["t"].as_str(), m[0]["what"].as_str(), m[0]["coins"].as_u64()), (Some("claimed"), Some("chest"), Some(40)));
        assert_eq!((m[1]["t"].as_str(), m[1]["coins"].as_u64(), m[1]["chest"]["n"].as_u64()), (Some("me"), Some(40), Some(0)));
        hub.handle(HubCmd::ClaimChest { id: 1 });
        assert_eq!(last_msg(&mut rx[0])["code"], "no_chest");
        hub.handle(HubCmd::ClaimQuest { id: 1, slot: "d0".into() });
        assert_eq!(last_msg(&mut rx[0])["code"], "quest_not_done");
        hub.handle(HubCmd::ClaimQuest { id: 1, slot: "zz".into() });
        assert_eq!(last_msg(&mut rx[0])["code"], "bad_quest");
    }

    #[test]
    fn only_quick_matches_rank_but_every_online_kind_moves_quests() {
        let (mut hub, mut rx) = hub_with_players(2, Duration::from_secs(120));
        for r in rx.iter_mut() {
            all_msgs(r);
        }
        let pids = ["pid-00000001".to_owned(), "pid-00000002".to_owned()];
        hub.handle(HubCmd::MatchResult {
            pids: pids.clone(),
            outcome: Outcome::Win(0),
            kind: PlayKind::Friend,
            events: [event(PlayKind::Friend, true), event(PlayKind::Friend, false)],
            counted: true,
        });
        assert_eq!(hub.store.get(&pids[0]).unwrap().games(), 0, "friend rooms never rank");
        assert_eq!(hub.store.get(&pids[0]).unwrap().plays, 1);
        let me = last_msg(&mut rx[0]);
        assert_eq!(me["t"], "me");
        assert!(me["daily"].as_array().unwrap().iter().any(|q| q["have"].as_u64() > Some(0)));
        hub.handle(HubCmd::MatchResult {
            pids: pids.clone(),
            outcome: Outcome::Win(0),
            kind: PlayKind::Ranked,
            events: [event(PlayKind::Ranked, true), event(PlayKind::Ranked, false)],
            counted: true,
        });
        assert_eq!((hub.store.get(&pids[0]).unwrap().tr, hub.store.get(&pids[0]).unwrap().w), (30, 1));
        assert_eq!(hub.store.get(&pids[1]).unwrap().l, 1);
        // Against the computer: the human's play counts, nothing ranks, the empty pid is ignored.
        hub.handle(HubCmd::MatchResult {
            pids: ["pid-00000001".into(), String::new()],
            outcome: Outcome::Win(0),
            kind: PlayKind::Bot,
            events: [event(PlayKind::Bot, true), event(PlayKind::Bot, false)],
            counted: true,
        });
        assert_eq!(hub.store.get(&pids[0]).unwrap().plays, 3);
        assert_eq!(hub.store.get(&pids[0]).unwrap().games(), 1, "the computer game did not rank");
        // A battle that was too short counts as played but moves no quests.
        let before = hub.store.me_json(&pids[1], store::now_secs()).unwrap()["daily"].clone();
        hub.handle(HubCmd::MatchResult {
            pids: pids.clone(),
            outcome: Outcome::Draw,
            kind: PlayKind::Friend,
            events: [event(PlayKind::Friend, false), event(PlayKind::Friend, false)],
            counted: false,
        });
        assert_eq!(hub.store.me_json(&pids[1], store::now_secs()).unwrap()["daily"], before);
    }

    #[tokio::test]
    async fn waiting_players_are_offered_the_computer_and_can_take_it() {
        let (mut hub, mut rx) = hub_with_players(1, Duration::from_secs(120));
        all_msgs(&mut rx[0]);
        hub.handle(HubCmd::Queue { id: 1, clock: 0 });
        assert_eq!(last_msg(&mut rx[0])["t"], "queued");
        // Too early: refused, and still waiting.
        hub.handle(HubCmd::Bot { id: 1 });
        assert_eq!(last_msg(&mut rx[0])["code"], "bot_unavailable");
        assert_eq!(hub.queue.len(), 1);
        // After a wait the offer arrives, once.
        hub.queue[0].since = Instant::now().checked_sub(Duration::from_secs(30)).unwrap();
        hub.tick();
        assert_eq!(last_msg(&mut rx[0])["t"], "bot_offer");
        hub.tick();
        assert!(all_msgs(&mut rx[0]).iter().all(|m| m["t"] != "bot_offer"), "offered only once");
        hub.handle(HubCmd::Bot { id: 1 });
        tokio::time::sleep(Duration::from_millis(50)).await; // let the match task start
        let m = last_msg(&mut rx[0]);
        assert_eq!((m["t"].as_str(), m["kind"].as_str(), m["opp"]["bot"].as_bool()), (Some("match"), Some("bot"), Some(true)));
        assert!(hub.queue.is_empty());
        // Asking again when not queued does nothing harmful.
        hub.handle(HubCmd::Bot { id: 1 });
        assert_eq!(last_msg(&mut rx[0])["code"], "bot_unavailable");
    }

    #[test]
    fn tokens_and_codes_are_well_formed() {
        let t = random_token();
        assert!(protocol::clean_token(&t).is_some());
        let c = random_code();
        assert_eq!(protocol::clean_code(&c), Some(c.clone()));
        assert_ne!(random_token(), t);
    }
}
