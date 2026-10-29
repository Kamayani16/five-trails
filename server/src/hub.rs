//! The hub task: who is online, the quick-match queue, private rooms and resume tokens,
//! plus the persistent player registry and leaderboard (see `store`). Live state is in
//! memory and disappears on restart; the registry is saved to disk.

use crate::bot;
use crate::cup::{CupError, Format};
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
/// How often cup deadlines are checked.
const CUP_CHECK_EVERY: Duration = Duration::from_secs(30);
/// Cups listed for one player.
const CUPS_LISTED: usize = 12;
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
    MatchResult { pids: [String; 2], outcome: Outcome, kind: PlayKind, events: [PlayEvent; 2], counted: bool, cup: Option<(String, usize)>, scores: [i32; 2] },
    CupCreate { id: ConnId, name: String, format: String, clock: u32 },
    CupJoin { id: ConnId, code: String },
    CupLeave { id: ConnId, code: String },
    CupStart { id: ConnId, code: String },
    CupList { id: ConnId },
    CupGet { id: ConnId, code: String },
    CupPlay { id: ConnId, code: String, m: usize },
    Watch { id: ConnId, code: String, m: usize },
    Unwatch { id: ConnId },
    IapVerify { id: ConnId, tx: String },
    FriendAdd { id: ConnId, to: FriendTo },
    FriendAccept { id: ConnId, pid: String },
    FriendDrop { id: ConnId, pid: String },
    Friends { id: ConnId },
    FriendInvite { id: ConnId, pid: String, clock: u32 },
    InviteAccept { id: ConnId, pid: String },
    InviteDrop { id: ConnId, pid: String },
    /// Apple's answer about a purchase (from a background lookup).
    IapChecked { id: ConnId, pid: String, tx: String, result: Result<(String, u32), crate::iap::IapError> },
    /// An App Store Server Notification named this transaction (maybe a refund).
    IapNotice { tx: String },
    /// Apple says this transaction was revoked.
    IapRevoked { tx: String },
    /// Write player data to disk now and confirm (used on shutdown).
    Flush { done: oneshot::Sender<()> },
    LeftMatch { id: ConnId },
    Disconnect { id: ConnId },
    MatchClosed { id: MatchId, tokens: Vec<String>, conns: Vec<ConnId> },
}

/// Who a friend request is for.
#[derive(Debug)]
pub enum FriendTo {
    Name(String),
    Pid(String),
}

#[derive(Debug, PartialEq, Eq)]
enum Activity {
    Idle,
    Queued,
    Hosting(String),
    InMatch(MatchId),
    /// Waiting for the opponent of cup match `.1` in cup `.0`.
    CupWait(String, usize),
    /// Watching a match as a spectator.
    Watching(MatchId),
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
    last_cup_check: Instant,
    /// Cup matches being played now: (cup, match) -> the match task.
    cup_live: HashMap<(String, usize), (MatchId, MatchTx)>,
    /// Spectators: connection -> the match they watch.
    watching: HashMap<ConnId, (MatchId, MatchTx)>,
    /// App Store purchases (None when not configured).
    apple: Option<Arc<crate::iap::AppleConfig>>,
    http: reqwest::Client,
    /// Purchase lookups in flight per connection (a few at most).
    iap_busy: HashMap<ConnId, u8>,
}

fn random_token() -> String {
    let bytes: [u8; 32] = OsRng.gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_code() -> String {
    (0..ROOM_CODE_LEN).map(|_| ROOM_ALPHABET[OsRng.gen_range(0..ROOM_ALPHABET.len())] as char).collect()
}

fn random_cup_code() -> String {
    (0..crate::cup::CUP_CODE_LEN).map(|_| ROOM_ALPHABET[OsRng.gen_range(0..ROOM_ALPHABET.len())] as char).collect()
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
            last_cup_check: Instant::now(),
            cup_live: HashMap::new(),
            watching: HashMap::new(),
            apple: None,
            http: reqwest::Client::new(),
            iap_busy: HashMap::new(),
        };
        (hub, tx)
    }

    /// Turn on App Store purchases.
    pub fn with_apple(mut self, apple: Option<crate::iap::AppleConfig>) -> Self {
        self.apple = apple.map(Arc::new);
        self
    }

    fn iap_packs(&self) -> Value {
        if self.apple.is_none() {
            return Value::Null;
        }
        json!(crate::iap::PACKS.iter().map(|(id, coins)| json!({ "id": id, "coins": coins })).collect::<Vec<_>>())
    }

    fn iap_verify(&mut self, id: ConnId, tx: String) {
        let Some(pid) = self.pid_of(id) else { return };
        let Some(apple) = self.apple.clone() else { return self.send(id, protocol::msg(json!({ "t": "iap_fail", "tx": tx, "code": "iap_off" }))) };
        // Replays of a purchase already paid are answered at once, so the app can finish it.
        match self.store.iap_owner(&tx) {
            Some(owner) if owner == pid => {
                let coins = self.store.coins_of(&pid);
                return self.send(id, protocol::msg(json!({ "t": "iap_ok", "tx": tx, "added": 0, "coins": coins, "again": true })));
            }
            Some(_) => return self.send(id, protocol::msg(json!({ "t": "iap_fail", "tx": tx, "code": "iap_other_account" }))),
            None => {}
        }
        let busy = self.iap_busy.entry(id).or_insert(0);
        if *busy >= 3 {
            return self.send(id, protocol::msg(json!({ "t": "iap_fail", "tx": tx, "code": "iap_retry" })));
        }
        *busy += 1;
        let (http, hub) = (self.http.clone(), self.tx.clone());
        tokio::spawn(async move {
            let result = match crate::iap::fetch(&apple, &http, &tx).await {
                Ok(t) => crate::iap::worth(&apple, &t),
                Err(e) => Err(e),
            };
            let _ = hub.send(HubCmd::IapChecked { id, pid, tx, result }).await;
        });
    }

    fn iap_checked(&mut self, id: ConnId, pid: &str, tx: &str, result: Result<(String, u32), crate::iap::IapError>) {
        if let Some(b) = self.iap_busy.get_mut(&id) {
            *b = b.saturating_sub(1);
        }
        let reply = match result {
            Ok((product, coins)) => match self.store.iap_credit(pid, tx, &product, coins, store::now_secs()) {
                store::Credit::Paid(bal) => {
                    tracing::info!(tx, product, coins, "App Store purchase paid");
                    json!({ "t": "iap_ok", "tx": tx, "product": product, "added": coins, "coins": bal })
                }
                store::Credit::Already(bal) => json!({ "t": "iap_ok", "tx": tx, "product": product, "added": 0, "coins": bal, "again": true }),
                store::Credit::OtherAccount => json!({ "t": "iap_fail", "tx": tx, "code": "iap_other_account" }),
                store::Credit::Unknown => return,
            },
            Err(e) => json!({ "t": "iap_fail", "tx": tx, "code": e.code() }),
        };
        // Answer every connection of this player (the app may have reconnected meanwhile).
        let text = protocol::msg(reply);
        self.send_to_pid(pid, &text);
        self.send_me_to_pid(pid);
    }

    fn iap_notice(&mut self, tx: String) {
        let Some(apple) = self.apple.clone() else { return };
        if self.store.iap_owner(&tx).is_none() {
            return;
        }
        let (http, hub) = (self.http.clone(), self.tx.clone());
        tokio::spawn(async move {
            if let Ok(t) = crate::iap::fetch(&apple, &http, &tx).await {
                if t.revocation_date.is_some() {
                    let _ = hub.send(HubCmd::IapRevoked { tx }).await;
                }
            }
        });
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

    fn conns_of(&self, pid: &str) -> Vec<ConnId> {
        self.conns.iter().filter(|(_, c)| c.profile.as_ref().is_some_and(|p| p.pid == pid)).map(|(&id, _)| id).collect()
    }

    /// "battle" while in a match, "online" while connected, otherwise "off" with seconds since
    /// the player was last active. Only ever shown to accepted friends.
    fn presence(&self, pid: &str, seen: u64, now: u64) -> (&'static str, Option<u64>) {
        let mut online = false;
        for c in self.conns.values() {
            if c.profile.as_ref().is_some_and(|p| p.pid == pid) {
                if matches!(c.activity, Activity::InMatch(_)) {
                    return ("battle", None);
                }
                online = true;
            }
        }
        if online { ("online", None) } else { ("off", Some(now.saturating_sub(seen))) }
    }

    fn friends_json(&self, pid: &str) -> Option<String> {
        let rec = self.store.get(pid)?;
        let now = store::now_secs();
        let list: Vec<Value> = rec
            .friends
            .iter()
            .filter_map(|f| {
                let r = self.store.get(f)?;
                let (st, ago) = self.presence(f, r.seen, now);
                Some(json!({ "pid": f, "n": r.name, "st": st, "ago": ago }))
            })
            .collect();
        let names = |v: &Vec<String>| -> Vec<Value> { v.iter().filter_map(|f| self.store.get(f).map(|r| json!({ "pid": f, "n": r.name }))).collect() };
        Some(protocol::msg(json!({ "t": "friends", "list": list, "inc": names(&rec.freq_in), "out": names(&rec.freq_out) })))
    }

    fn name_of(&self, pid: &str) -> String {
        self.store.get(pid).map(|r| r.name.clone()).unwrap_or_default()
    }

    fn send_invites(&mut self, pid: &str) {
        let ids = self.conns_of(pid);
        if ids.is_empty() {
            return;
        }
        let now = store::now_secs();
        let list: Vec<Value> = self
            .store
            .invites_of(pid, now)
            .iter()
            .filter_map(|i| self.store.get(&i.from).map(|r| json!({ "pid": i.from, "n": r.name, "clock": i.clock, "age": now.saturating_sub(i.at) })))
            .collect();
        let text = protocol::msg(json!({ "t": "invites", "list": list }));
        for id in ids {
            self.send(id, text.clone());
        }
    }

    /// `pid` just came online: show their waiting invites, and tell each inviter who is online.
    fn deliver_invites(&mut self, pid: &str) {
        let invites = self.store.invites_of(pid, store::now_secs());
        if invites.is_empty() {
            return;
        }
        self.send_invites(pid);
        let text = protocol::msg(json!({ "t": "invite_seen", "n": self.name_of(pid) }));
        for i in invites {
            self.send_to_pid(&i.from, &text);
        }
    }

    fn send_friends(&self, pid: &str) {
        let ids = self.conns_of(pid);
        if ids.is_empty() {
            return;
        }
        if let Some(text) = self.friends_json(pid) {
            for id in ids {
                self.send(id, text.clone());
            }
        }
    }

    /// Tell `pid`'s friends who are online that `pid` came online, went offline or started a battle.
    fn notify_friends(&self, pid: &str) {
        let Some(friends) = self.store.get(pid).map(|r| r.friends.clone()) else { return };
        for f in friends {
            self.send_friends(&f);
        }
    }

    fn handle_friend(&mut self, cmd: HubCmd) {
        match cmd {
            HubCmd::FriendAdd { id, to } => {
                let Some(me) = self.pid_of(id) else { return };
                let target = match to {
                    FriendTo::Name(n) => self.store.pid_by_name(&n).map(str::to_owned),
                    FriendTo::Pid(p) => self.store.get(&p).map(|_| p),
                };
                let Some(target) = target else { return self.send(id, protocol::error("friend_not_found")) };
                match self.store.friend_request(&me, &target) {
                    Ok(reply) => {
                        let what = match reply {
                            store::FriendReply::Sent => "sent",
                            store::FriendReply::Accepted => "accepted",
                            store::FriendReply::Already => "already",
                        };
                        let name = self.store.get(&target).map(|r| r.name.clone()).unwrap_or_default();
                        self.send(id, protocol::msg(json!({ "t": "friend_ok", "what": what, "n": name })));
                        self.send_friends(&me);
                        self.send_friends(&target);
                    }
                    Err(e) => self.send(id, protocol::error(e.code())),
                }
            }
            HubCmd::FriendAccept { id, pid } => {
                let Some(me) = self.pid_of(id) else { return };
                match self.store.friend_accept(&me, &pid) {
                    Ok(()) => {
                        let name = self.store.get(&pid).map(|r| r.name.clone()).unwrap_or_default();
                        self.send(id, protocol::msg(json!({ "t": "friend_ok", "what": "accepted", "n": name })));
                        self.send_friends(&me);
                        self.send_friends(&pid);
                    }
                    Err(e) => self.send(id, protocol::error(e.code())),
                }
            }
            HubCmd::FriendDrop { id, pid } => {
                let Some(me) = self.pid_of(id) else { return };
                self.store.friend_drop(&me, &pid);
                self.send_friends(&me);
                self.send_friends(&pid);
            }
            HubCmd::Friends { id } => {
                if let Some(text) = self.pid_of(id).and_then(|p| self.friends_json(&p)) {
                    self.send(id, text);
                }
            }
            HubCmd::FriendInvite { id, pid, clock } => {
                let Some(me) = self.pid_of(id) else { return };
                if !CLOCK_CHOICES.contains(&clock) {
                    return self.send(id, protocol::error("bad_clock"));
                }
                match self.store.invite(&me, &pid, clock, store::now_secs()) {
                    Ok(()) => {
                        let online = !self.conns_of(&pid).is_empty();
                        let name = self.name_of(&pid);
                        self.send(id, protocol::msg(json!({ "t": "invite_sent", "n": name, "online": online })));
                        self.send_invites(&pid);
                    }
                    Err(e) => self.send(id, protocol::error(e.code())),
                }
            }
            HubCmd::InviteAccept { id, pid } => {
                let Some(me) = self.pid_of(id) else { return };
                if self.conns.get(&id).is_none_or(|c| c.activity != Activity::Idle) {
                    return self.send(id, protocol::error("busy"));
                }
                let now = store::now_secs();
                let Some(inv) = self.store.invite_take(&me, &pid, now) else {
                    self.send(id, protocol::error("invite_gone"));
                    return self.send_invites(&me);
                };
                self.send_invites(&me);
                let host = self.conns.iter().find(|(_, c)| c.activity == Activity::Idle && c.profile.as_ref().is_some_and(|p| p.pid == pid)).map(|(&cid, _)| cid);
                match host {
                    Some(host) => {
                        let code = loop {
                            let code = random_code();
                            if !self.rooms.contains_key(&code) {
                                break code;
                            }
                        };
                        self.start_match(host, id, inv.clock, MatchKind::Room(code));
                    }
                    None => {
                        // They are offline or busy: turn it around, so they see "<me> wants to play".
                        let _ = self.store.invite(&me, &pid, inv.clock, now);
                        let name = self.name_of(&pid);
                        self.send(id, protocol::msg(json!({ "t": "invite_away", "n": name })));
                        self.send_invites(&pid);
                    }
                }
            }
            HubCmd::InviteDrop { id, pid } => {
                let Some(me) = self.pid_of(id) else { return };
                if self.store.invite_take(&me, &pid, store::now_secs()).is_some() {
                    let text = protocol::msg(json!({ "t": "invite_declined", "n": self.name_of(&me) }));
                    self.send_to_pid(&pid, &text);
                }
                self.send_invites(&me);
            }
            _ => {}
        }
    }

    fn send_to_pid(&self, pid: &str, text: &str) {
        for id in self.conns_of(pid) {
            self.send(id, text.to_owned());
        }
    }

    /// Send every member their own view of a cup.
    fn push_cup(&self, code: &str) {
        let Some(c) = self.store.cup(code) else { return };
        let now = store::now_secs();
        for (i, p) in c.players.iter().enumerate() {
            let text = protocol::msg(json!({ "t": "cup", "cup": self.cup_view(c, Some(i), now) }));
            self.send_to_pid(&p.pid, &text);
        }
    }

    /// A cup as one member sees it, with the matches being played right now marked live.
    fn cup_view(&self, c: &crate::cup::Cup, me: Option<usize>, now: u64) -> Value {
        let mut v = c.view(me, now);
        for (code, m) in self.cup_live.keys() {
            if *code == c.code {
                if let Some(x) = v["matches"].get_mut(*m) {
                    x["live"] = json!(true);
                }
            }
        }
        v
    }

    /// A cup member asks to watch a match of that cup that is being played.
    fn watch(&mut self, id: ConnId, code: &str, m: usize) {
        let Some(pid) = self.pid_of(id) else { return };
        if self.conns.get(&id).is_none_or(|c| c.activity != Activity::Idle) {
            return self.send(id, protocol::error("busy"));
        }
        if self.store.cup(code).is_none_or(|c| c.index_of(&pid).is_none()) {
            return self.cup_error(id, CupError::NotFound);
        }
        let Some((mid, mtx)) = self.cup_live.get(&(code.to_owned(), m)).cloned() else {
            return self.send(id, protocol::error("not_live"));
        };
        let Some(c) = self.conns.get_mut(&id) else { return };
        if mtx.try_send(MatchCmd::Watch { conn: id, tx: c.tx.clone() }).is_err() {
            return self.send(id, protocol::error("not_live"));
        }
        c.activity = Activity::Watching(mid);
        self.watching.insert(id, (mid, mtx));
    }

    fn unwatch(&mut self, id: ConnId) -> bool {
        let Some((_, mtx)) = self.watching.remove(&id) else { return false };
        let _ = mtx.try_send(MatchCmd::Unwatch { conn: id });
        if let Some(c) = self.conns.get_mut(&id) {
            if matches!(c.activity, Activity::Watching(_)) {
                c.activity = Activity::Idle;
            }
        }
        true
    }

    fn cup_error(&self, id: ConnId, e: CupError) {
        self.send(id, protocol::error(e.code()));
    }

    /// Tell each winner what a finished cup paid, and refresh their coins.
    fn announce_prizes(&mut self, code: &str, paid: crate::cup::Paid) {
        let name = self.store.cup(code).map(|c| c.name.clone()).unwrap_or_default();
        for (pid, place, coins) in paid {
            let text = protocol::msg(json!({ "t": "cup_prize", "code": code, "name": name, "place": place, "coins": coins }));
            self.send_to_pid(&pid, &text);
            self.send_me_to_pid(&pid);
        }
    }

    fn handle_cup(&mut self, cmd: HubCmd) {
        let now = store::now_secs();
        match cmd {
            HubCmd::CupCreate { id, name, format, clock } => {
                let Some(pid) = self.pid_of(id) else { return };
                let Some(format) = Format::parse(&format) else { return self.send(id, protocol::error("bad_format")) };
                if !CLOCK_CHOICES.contains(&clock) {
                    return self.send(id, protocol::error("bad_clock"));
                }
                let code = loop {
                    let c = random_cup_code();
                    if !self.store.cup_code_taken(&c) {
                        break c;
                    }
                };
                match self.store.cup_create(&pid, code.clone(), &name, format, clock, now) {
                    Ok(_) => self.push_cup(&code),
                    Err(e) => self.cup_error(id, e),
                }
            }
            HubCmd::CupJoin { id, code } => {
                let Some(pid) = self.pid_of(id) else { return };
                let t = Instant::now();
                if self.conns.get(&id).is_some_and(|c| c.join_blocked_until.is_some_and(|b| t < b)) {
                    return self.send(id, protocol::error("too_many_attempts"));
                }
                let Some(code) = protocol::clean_cup_code(&code) else {
                    self.note_failed_attempt(id, t);
                    return self.cup_error(id, CupError::NotFound);
                };
                match self.store.cup_join(&pid, &code) {
                    Ok(_) => self.push_cup(&code),
                    Err(e) => {
                        if e == CupError::NotFound {
                            self.note_failed_attempt(id, t);
                        }
                        self.cup_error(id, e);
                    }
                }
            }
            HubCmd::CupLeave { id, code } => {
                let Some(pid) = self.pid_of(id) else { return };
                match self.store.cup_leave(&pid, &code) {
                    Ok(gone) if !gone.is_empty() => {
                        let text = protocol::msg(json!({ "t": "cup_gone", "code": code }));
                        for p in gone {
                            self.send_to_pid(&p, &text);
                        }
                    }
                    Ok(_) => {
                        self.send_to_pid(&pid, &protocol::msg(json!({ "t": "cup_gone", "code": code })));
                        self.push_cup(&code);
                    }
                    Err(e) => self.cup_error(id, e),
                }
            }
            HubCmd::CupStart { id, code } => {
                let Some(pid) = self.pid_of(id) else { return };
                match self.store.cup_start(&pid, &code, now) {
                    Ok(()) => self.push_cup(&code),
                    Err(e) => self.cup_error(id, e),
                }
            }
            HubCmd::CupList { id } => {
                let Some(pid) = self.pid_of(id) else { return };
                let cups: Vec<Value> = self.store.cups_of(&pid).into_iter().take(CUPS_LISTED).map(|c| self.cup_view(c, c.index_of(&pid), now)).collect();
                self.send(id, protocol::msg(json!({ "t": "cups", "cups": cups })));
            }
            HubCmd::CupGet { id, code } => {
                let Some(pid) = self.pid_of(id) else { return };
                match self.store.cup(&code).filter(|c| c.index_of(&pid).is_some()) {
                    Some(c) => {
                        let v = self.cup_view(c, c.index_of(&pid), now);
                        self.send(id, protocol::msg(json!({ "t": "cup", "cup": v })));
                    }
                    None => self.cup_error(id, CupError::NotFound),
                }
            }
            HubCmd::CupPlay { id, code, m } => self.cup_play(id, &code, m),
            HubCmd::Watch { id, code, m } => self.watch(id, &code, m),
            HubCmd::Unwatch { id } if self.unwatch(id) => self.send(id, protocol::msg(json!({ "t": "watch_end" }))),
            _ => {}
        }
    }

    /// A player is ready for a cup match: start it if the opponent is already waiting.
    fn cup_play(&mut self, id: ConnId, code: &str, m: usize) {
        let Some(pid) = self.pid_of(id) else { return };
        if self.conns.get(&id).is_none_or(|c| c.activity != Activity::Idle) {
            return self.send(id, protocol::error("busy"));
        }
        let Some(cup) = self.store.cup(code) else { return self.cup_error(id, CupError::NotFound) };
        let opp = match cup.playable(m, &pid) {
            Ok(o) => o,
            Err(e) => return self.cup_error(id, e),
        };
        let (opp_pid, opp_name, clock) = (cup.players[opp].pid.clone(), cup.players[opp].name.clone(), cup.clock);
        let my_name = cup.index_of(&pid).map(|i| cup.players[i].name.clone()).unwrap_or_default();
        self.store.cup_mark_ready(code, m, &pid);
        let waiting = self.conns_of(&opp_pid).into_iter().find(|cid| self.conns.get(cid).is_some_and(|c| c.activity == Activity::CupWait(code.to_owned(), m)));
        match waiting {
            Some(other) => {
                if let Some(c) = self.conns.get_mut(&other) {
                    c.activity = Activity::Idle;
                }
                self.start_match(other, id, clock, MatchKind::Cup(code.to_owned(), m));
            }
            None => {
                if let Some(c) = self.conns.get_mut(&id) {
                    c.activity = Activity::CupWait(code.to_owned(), m);
                }
                self.send(id, protocol::msg(json!({ "t": "cup_wait", "code": code, "m": m, "opp": opp_name })));
                let ping = protocol::msg(json!({ "t": "cup_ping", "code": code, "m": m, "from": my_name }));
                self.send_to_pid(&opp_pid, &ping);
            }
        }
        self.push_cup(code);
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
            c @ (HubCmd::CupCreate { .. } | HubCmd::CupJoin { .. } | HubCmd::CupLeave { .. } | HubCmd::CupStart { .. } | HubCmd::CupList { .. } | HubCmd::CupGet { .. } | HubCmd::CupPlay { .. } | HubCmd::Watch { .. } | HubCmd::Unwatch { .. }) => self.handle_cup(c),
            HubCmd::IapVerify { id, tx } => self.iap_verify(id, tx),
            c @ (HubCmd::FriendAdd { .. } | HubCmd::FriendAccept { .. } | HubCmd::FriendDrop { .. } | HubCmd::Friends { .. } | HubCmd::FriendInvite { .. } | HubCmd::InviteAccept { .. } | HubCmd::InviteDrop { .. }) => self.handle_friend(c),
            HubCmd::IapChecked { id, pid, tx, result } => self.iap_checked(id, &pid, &tx, result),
            HubCmd::IapNotice { tx } => self.iap_notice(tx),
            HubCmd::IapRevoked { tx } => {
                if let Some(pid) = self.store.iap_revoke(&tx) {
                    tracing::info!(tx, "App Store purchase refunded: coins taken back");
                    self.send_me_to_pid(&pid);
                }
            }
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
                    "created": login.created, "roomTtl": self.room_ttl.as_secs(), "prog": prog, "iap": self.iap_packs(),
                }));
                self.send(id, welcome);
                if let Some(me) = self.store.me_json(&login.pid, store::now_secs()) {
                    self.send(id, protocol::msg(me));
                }
                if let Some(text) = self.friends_json(&login.pid) {
                    self.send(id, text);
                }
                self.notify_friends(&login.pid);
                if self.conns_of(&login.pid).len() == 1 {
                    self.deliver_invites(&login.pid);
                } else {
                    self.send_invites(&login.pid);
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
            HubCmd::MatchResult { pids, outcome, kind, events, counted, cup, scores } => {
                let now = store::now_secs();
                if let Some((code, idx)) = cup {
                    let winner = match outcome {
                        Outcome::Win(s) => Some(pids[s].as_str()),
                        Outcome::Draw => None,
                    };
                    let paid = self.store.cup_record(&code, idx, [&pids[0], &pids[1]], scores, winner, now);
                    for (seat, pid) in pids.iter().enumerate() {
                        self.store.record_play(pid, &events[seat], false, false, now);
                        self.send_to_pid(pid, &protocol::msg(json!({ "t": "earned", "coins": 0, "counted": true, "cup": code })));
                    }
                    self.push_cup(&code);
                    if let Some(paid) = paid {
                        self.announce_prizes(&code, paid);
                    }
                    return;
                }
                if kind == PlayKind::Ranked {
                    self.store.record_match([&pids[0], &pids[1]], outcome, now);
                    self.ranking_dirty = true;
                }
                for seat in 0..2 {
                    let coins = self.store.record_play(&pids[seat], &events[seat], outcome == Outcome::Draw, counted, now);
                    let earned = protocol::msg(json!({ "t": "earned", "coins": coins, "counted": counted }));
                    let ids: Vec<ConnId> = self.conns.iter().filter(|(_, c)| c.profile.as_ref().is_some_and(|p| p.pid == pids[seat])).map(|(&id, _)| id).collect();
                    for id in ids {
                        self.send(id, earned.clone());
                    }
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
                if let Some(c) = self.conns.get_mut(&id) {
                    if matches!(c.activity, Activity::CupWait(..)) {
                        c.activity = Activity::Idle;
                        self.send(id, protocol::msg(json!({ "t": "cup_unwait" })));
                    }
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
                self.unwatch(id);
                self.iap_busy.remove(&id);
                self.leave_queue(id);
                self.close_room_of(id);
                let pid = self.pid_of(id);
                self.conns.remove(&id);
                if let Some(pid) = pid {
                    if self.conns_of(&pid).is_empty() {
                        self.store.touch(&pid, store::now_secs());
                        self.notify_friends(&pid);
                    }
                }
            }
            HubCmd::MatchClosed { id, tokens, conns } => {
                let ended: Vec<(String, usize)> = self.cup_live.iter().filter(|(_, (mid, _))| *mid == id).map(|(k, _)| k.clone()).collect();
                for k in &ended {
                    self.cup_live.remove(k);
                }
                let watchers: Vec<ConnId> = self.watching.iter().filter(|(_, (mid, _))| *mid == id).map(|(&c, _)| c).collect();
                for c in watchers {
                    self.watching.remove(&c);
                    if let Some(ci) = self.conns.get_mut(&c) {
                        ci.activity = Activity::Idle;
                    }
                }
                for (code, _) in ended {
                    self.push_cup(&code);
                }
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
        self.resumes.insert(tb, (id, mtx.clone(), 1));
        for cid in [a, b] {
            if let Some(c) = self.conns.get_mut(&cid) {
                c.activity = Activity::InMatch(id);
            }
        }
        let live_cup = if let MatchKind::Cup(code, m) = &kind { Some((code.clone(), *m)) } else { None };
        if let Some(k) = &live_cup {
            self.cup_live.insert(k.clone(), (id, mtx.clone()));
        }
        let pids = [players[0].profile.pid.clone(), players[1].profile.pid.clone()];
        let room = MatchRoom::new(id, kind, clock, players, mrx, self.tx.clone());
        tokio::spawn(room.run());
        for p in pids {
            self.notify_friends(&p);
        }
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
        if now.duration_since(self.last_cup_check) >= CUP_CHECK_EVERY {
            self.last_cup_check = now;
            for (code, paid) in self.store.cup_expire(store::now_secs()) {
                self.push_cup(&code);
                self.announce_prizes(&code, paid);
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

    fn find(msgs: &[Value], t: &str) -> Option<Value> {
        msgs.iter().rev().find(|m| m["t"] == t).cloned()
    }

    #[tokio::test]
    async fn a_round_robin_cup_runs_from_lobby_to_prizes() {
        let (mut hub, mut rx) = hub_with_players(4, Duration::from_secs(120));
        for r in &mut rx {
            all_msgs(r);
        }
        hub.handle(HubCmd::CupCreate { id: 1, name: "  Saturday   Cup ".into(), format: "rr".into(), clock: 300 });
        let c = find(&all_msgs(&mut rx[0]), "cup").expect("host sees the new cup")["cup"].clone();
        let code = c["code"].as_str().unwrap().to_owned();
        assert_eq!((c["name"].as_str(), c["state"].as_str(), c["host"].as_bool()), (Some("Saturday Cup"), Some("lobby"), Some(true)));
        hub.handle(HubCmd::CupJoin { id: 2, code: code.to_lowercase() });
        hub.handle(HubCmd::CupJoin { id: 3, code: "ZZZZZZ".into() });
        assert_eq!(last_msg(&mut rx[2])["code"], "cup_not_found");
        hub.handle(HubCmd::CupStart { id: 1, code: code.clone() });
        assert_eq!(last_msg(&mut rx[0])["code"], "cup_too_few");
        hub.handle(HubCmd::CupJoin { id: 3, code: code.clone() });
        hub.handle(HubCmd::CupStart { id: 2, code: code.clone() });
        assert_eq!(last_msg(&mut rx[1])["code"], "cup_not_host");
        hub.handle(HubCmd::CupStart { id: 1, code: code.clone() });
        let v = find(&all_msgs(&mut rx[2]), "cup").unwrap()["cup"].clone();
        assert_eq!((v["state"].as_str(), v["me"].as_u64(), v["matches"].as_array().unwrap().len()), (Some("live"), Some(2), 3));
        assert!(!v.to_string().contains("pid-"), "no pids in what players see");
        hub.handle(HubCmd::CupJoin { id: 4, code: code.clone() });
        assert_eq!(last_msg(&mut rx[3])["code"], "cup_started");
        let ms = v["matches"].as_array().unwrap().clone();
        let idx = |x: u64, y: u64| ms.iter().position(|m| (m["a"] == x && m["b"] == y) || (m["a"] == y && m["b"] == x)).unwrap();
        // P1 (player 0) shows up for the match with P2 (player 1) and waits; P2 is told.
        let m01 = idx(0, 1);
        hub.handle(HubCmd::CupPlay { id: 3, code: code.clone(), m: m01 });
        assert_eq!(last_msg(&mut rx[2])["code"], "cup_not_your_match");
        hub.handle(HubCmd::CupPlay { id: 1, code: code.clone(), m: m01 });
        let a = all_msgs(&mut rx[0]);
        assert_eq!(find(&a, "cup_wait").unwrap()["opp"], "P2");
        let b = all_msgs(&mut rx[1]);
        assert_eq!(find(&b, "cup_ping").unwrap()["from"], "P1");
        hub.handle(HubCmd::Queue { id: 1, clock: 300 });
        assert_eq!(last_msg(&mut rx[0])["code"], "busy", "waiting for a cup match is being busy");
        // P2 taps Play: the match starts for both.
        hub.handle(HubCmd::CupPlay { id: 2, code: code.clone(), m: m01 });
        let joined = |r: &mut mpsc::Receiver<ConnCtl>| {
            let mut j = false;
            while let Ok(c) = r.try_recv() {
                j |= matches!(c, ConnCtl::Joined { .. });
            }
            j
        };
        assert!(joined(&mut rx[0]) && joined(&mut rx[1]), "both players are put in the match");
        // Report the three results directly (the match task would do this).
        let pid = |i: u64| format!("pid-{i:08}");
        let report = |hub: &mut Hub, x: u64, y: u64, m: usize, sx: i32, sy: i32| {
            let outcome = if sx > sy { Outcome::Win(0) } else if sx < sy { Outcome::Win(1) } else { Outcome::Draw };
            hub.handle(HubCmd::MatchResult {
                pids: [pid(x), pid(y)],
                outcome,
                kind: PlayKind::Cup,
                events: [event(PlayKind::Cup, sx > sy), event(PlayKind::Cup, sy > sx)],
                counted: false,
                cup: Some((code.clone(), m)),
                scores: [sx, sy],
            });
        };
        report(&mut hub, 1, 2, m01, 30, 12);
        let after = all_msgs(&mut rx[1]);
        assert_eq!(find(&after, "earned").unwrap()["coins"], 0, "cup matches pay no battle coins");
        report(&mut hub, 3, 1, idx(0, 2), 5, 25);
        report(&mut hub, 2, 3, idx(1, 2), 20, 20);
        let m = all_msgs(&mut rx[0]);
        let done = find(&m, "cup").unwrap()["cup"].clone();
        assert_eq!((done["state"].as_str(), done["champion"].as_u64()), (Some("done"), Some(0)));
        assert_eq!(done["table"][0]["pts"], 6);
        let prize = find(&m, "cup_prize").expect("the champion is told");
        assert_eq!((prize["place"].as_u64(), prize["coins"].as_u64()), (Some(1), Some(100)));
        assert_eq!(find(&m, "me").unwrap()["coins"], 100);
        assert_eq!(hub.store.get(&pid(1)).unwrap().tr, 0, "cup games never move trophies");
        let p3 = all_msgs(&mut rx[2]);
        assert_eq!(find(&p3, "cup_prize").unwrap()["coins"], 15);
        // The champion can now wear the Laurel.
        hub.handle(HubCmd::Equip { id: 1, kind: "back".into(), item: "laurel".into() });
        assert_eq!(last_msg(&mut rx[0])["shop"]["back"], "laurel");
        hub.handle(HubCmd::Equip { id: 3, kind: "back".into(), item: "laurel".into() });
        assert_eq!(last_msg(&mut rx[2])["code"], "item_locked");
        hub.handle(HubCmd::CupList { id: 2 });
        let l = last_msg(&mut rx[1]);
        assert_eq!(l["cups"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_verified_purchase_pays_once_and_replays_are_answered_at_once() {
        let (mut hub, mut rx) = hub_with_players(2, Duration::from_secs(120));
        for r in &mut rx {
            all_msgs(r);
        }
        hub.handle(HubCmd::IapVerify { id: 1, tx: "2000001".into() });
        assert_eq!((last_msg(&mut rx[0])["code"].as_str()), Some("iap_off"), "off until configured");
        let pid = hub.pid_of(1).unwrap();
        hub.handle(HubCmd::IapChecked { id: 1, pid: pid.clone(), tx: "2000001".into(), result: Ok(("coins_1000".into(), 1000)) });
        let m = all_msgs(&mut rx[0]);
        let ok = find(&m, "iap_ok").unwrap();
        assert_eq!((ok["added"].as_u64(), ok["coins"].as_u64()), (Some(1000), Some(1000)));
        assert_eq!(find(&m, "me").unwrap()["coins"], 1000);
        hub.handle(HubCmd::IapChecked { id: 1, pid: pid.clone(), tx: "2000001".into(), result: Ok(("coins_1000".into(), 1000)) });
        assert_eq!(find(&all_msgs(&mut rx[0]), "iap_ok").unwrap()["added"], 0, "never twice");
        let other = hub.pid_of(2).unwrap();
        hub.handle(HubCmd::IapChecked { id: 2, pid: other, tx: "2000001".into(), result: Ok(("coins_1000".into(), 1000)) });
        assert_eq!(find(&all_msgs(&mut rx[1]), "iap_fail").unwrap()["code"], "iap_other_account");
        hub.handle(HubCmd::IapChecked { id: 1, pid: pid.clone(), tx: "2000002".into(), result: Err(crate::iap::IapError::Retry) });
        assert_eq!(find(&all_msgs(&mut rx[0]), "iap_fail").unwrap()["code"], "iap_retry");
        hub.handle(HubCmd::IapRevoked { tx: "2000001".into() });
        assert_eq!(find(&all_msgs(&mut rx[0]), "me").unwrap()["coins"], 0, "a refund takes the coins back");
    }

    #[test]
    fn friends_see_online_battle_and_last_active_but_strangers_do_not() {
        let (mut hub, mut rx) = hub_with_players(3, Duration::from_secs(120));
        let hello = all_msgs(&mut rx[0]);
        let f = find(&hello, "friends").expect("the friend list comes with the welcome");
        assert_eq!((f["list"].as_array().unwrap().len(), f["inc"].as_array().unwrap().len()), (0, 0));
        all_msgs(&mut rx[1]);
        all_msgs(&mut rx[2]);
        // P1 asks P2 by name; P2 is told at once.
        hub.handle(HubCmd::FriendAdd { id: 1, to: FriendTo::Name("p2".into()) });
        let m = all_msgs(&mut rx[0]);
        assert_eq!((m[0]["t"].as_str(), m[0]["what"].as_str(), m[0]["n"].as_str()), (Some("friend_ok"), Some("sent"), Some("P2")));
        let f = find(&m, "friends").unwrap();
        assert_eq!(f["out"][0]["n"], "P2");
        let f2 = find(&all_msgs(&mut rx[1]), "friends").unwrap();
        assert_eq!(f2["inc"][0]["n"], "P1");
        assert_eq!(f2["list"].as_array().unwrap().len(), 0, "no presence before accepting");
        hub.handle(HubCmd::FriendAdd { id: 1, to: FriendTo::Name("Nobody".into()) });
        assert_eq!(last_msg(&mut rx[0])["code"], "friend_not_found");
        // P2 accepts: both now see each other online.
        hub.handle(HubCmd::FriendAccept { id: 2, pid: "pid-00000001".into() });
        let f2 = find(&all_msgs(&mut rx[1]), "friends").unwrap();
        assert_eq!((f2["list"][0]["n"].as_str(), f2["list"][0]["st"].as_str()), (Some("P1"), Some("online")));
        let f1 = find(&all_msgs(&mut rx[0]), "friends").unwrap();
        assert_eq!(f1["list"][0]["st"], "online");
        // P2 goes offline: P1 is told, with how long ago P2 was active.
        hub.handle(HubCmd::Disconnect { id: 2 });
        let f1 = find(&all_msgs(&mut rx[0]), "friends").unwrap();
        assert_eq!(f1["list"][0]["st"], "off");
        assert!(f1["list"][0]["ago"].as_u64().unwrap() < 5);
        // P3 is not a friend and hears nothing about either.
        assert!(find(&all_msgs(&mut rx[2]), "friends").is_none());
        // Removing a friend clears it for both.
        hub.handle(HubCmd::FriendDrop { id: 1, pid: "pid-00000002".into() });
        assert_eq!(find(&all_msgs(&mut rx[0]), "friends").unwrap()["list"].as_array().unwrap().len(), 0);
        assert!(hub.store.get("pid-00000002").unwrap().friends.is_empty());
    }

    #[tokio::test]
    async fn an_invite_waits_for_an_offline_friend_and_starts_a_battle() {
        let (mut hub, mut rx) = hub_with_players(2, Duration::from_secs(120));
        hub.handle(HubCmd::FriendAdd { id: 1, to: FriendTo::Name("P2".into()) });
        hub.handle(HubCmd::FriendAccept { id: 2, pid: "pid-00000001".into() });
        hub.handle(HubCmd::FriendInvite { id: 2, pid: "pid-00000003".into(), clock: 300 });
        assert_eq!(last_msg(&mut rx[1])["code"], "invite_not_friend");
        // P2 goes offline; P1 invites them.
        hub.handle(HubCmd::Disconnect { id: 2 });
        all_msgs(&mut rx[0]);
        hub.handle(HubCmd::FriendInvite { id: 1, pid: "pid-00000002".into(), clock: 180 });
        let m = all_msgs(&mut rx[0]);
        assert_eq!((m[0]["t"].as_str(), m[0]["online"].as_bool()), (Some("invite_sent"), Some(false)));
        // P2 comes back: the invite is waiting and P1 is told P2 saw it.
        let (tx, mut rx2) = mpsc::channel(64);
        hub.handle(HubCmd::Connect { id: 5, tx });
        let profile = protocol::profile_from_hello("P2", "pid-00000002", 0, 1).unwrap();
        hub.handle(HubCmd::Hello { id: 5, key_hash: store::test_auth("pid-00000002"), profile, create: true, resume: None });
        let got = all_msgs(&mut rx2);
        let inv = find(&got, "invites").expect("waiting invites are delivered on login");
        assert_eq!((inv["list"][0]["n"].as_str(), inv["list"][0]["clock"].as_u64()), (Some("P1"), Some(180)));
        assert!(find(&all_msgs(&mut rx[0]), "invite_seen").is_some());
        // P2 says yes: both are put into a friendly battle.
        hub.handle(HubCmd::InviteAccept { id: 5, pid: "pid-00000001".into() });
        let joined = |rx: &mut mpsc::Receiver<ConnCtl>| {
            let mut j = false;
            while let Ok(c) = rx.try_recv() {
                j |= matches!(c, ConnCtl::Joined { .. });
            }
            j
        };
        assert!(joined(&mut rx[0]) && joined(&mut rx2), "the battle starts for both");
        assert!(hub.store.invites_of("pid-00000002", store::now_secs()).is_empty());
    }

    #[test]
    fn saying_yes_to_a_friend_who_left_invites_them_back() {
        let (mut hub, mut rx) = hub_with_players(2, Duration::from_secs(120));
        hub.handle(HubCmd::FriendAdd { id: 1, to: FriendTo::Name("P2".into()) });
        hub.handle(HubCmd::FriendAccept { id: 2, pid: "pid-00000001".into() });
        hub.handle(HubCmd::FriendInvite { id: 1, pid: "pid-00000002".into(), clock: 300 });
        hub.handle(HubCmd::Disconnect { id: 1 });
        all_msgs(&mut rx[1]);
        hub.handle(HubCmd::InviteAccept { id: 2, pid: "pid-00000001".into() });
        let m = all_msgs(&mut rx[1]);
        assert_eq!(find(&m, "invite_away").unwrap()["n"], "P1");
        let back = hub.store.invites_of("pid-00000001", store::now_secs());
        assert_eq!(back[0].from, "pid-00000002", "P1 will find P2's invite when they return");
        hub.handle(HubCmd::InviteAccept { id: 2, pid: "pid-00000001".into() });
        assert_eq!(find(&all_msgs(&mut rx[1]), "error").unwrap()["code"], "invite_gone");
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
            cup: None,
            scores: [0, 0],
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
            cup: None,
            scores: [0, 0],
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
            cup: None,
            scores: [0, 0],
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
            cup: None,
            scores: [0, 0],
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
