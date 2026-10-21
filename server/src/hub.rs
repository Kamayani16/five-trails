//! The hub task: who is online, the quick-match queue, private rooms and resume
//! tokens. Everything lives in memory and disappears on restart by design.

use crate::matchroom::{MatchCmd, MatchId, MatchKind, MatchRoom, MatchTx, Player};
use crate::protocol::{self, Profile, CLOCK_CHOICES, ROOM_ALPHABET, ROOM_CODE_LEN};
use rand::{rngs::OsRng, Rng};
use serde_json::json;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub type ConnId = u64;
pub type ConnTx = mpsc::Sender<ConnCtl>;
pub type HubTx = mpsc::Sender<HubCmd>;

const MAX_ROOMS: usize = 5000;
const ROOM_TTL: Duration = Duration::from_secs(15 * 60);
const JOIN_FAILS_BEFORE_BLOCK: u32 = 8;
const JOIN_BLOCK: Duration = Duration::from_secs(60);
const STATS_EVERY: Duration = Duration::from_secs(10);
/// After this long in the queue, anyone with the same clock setting is a fair match.
const QUEUE_OPEN_AFTER: Duration = Duration::from_secs(20);

/// Messages from the hub or a match to one connection task.
#[derive(Debug)]
pub enum ConnCtl {
    Send(String),
    Joined { tx: MatchTx, seat: usize },
    Left,
}

#[derive(Debug)]
pub enum HubCmd {
    Connect { id: ConnId, tx: ConnTx },
    Hello { id: ConnId, profile: Profile, resume: Option<String> },
    UpdateProfile { id: ConnId, name: String, trophies: u32, level: u32 },
    Queue { id: ConnId, clock: u32 },
    Unqueue { id: ConnId },
    Create { id: ConnId, clock: u32 },
    CancelRoom { id: ConnId },
    Join { id: ConnId, code: String },
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
    pub fn new() -> (Self, HubTx) {
        let (tx, rx) = mpsc::channel(4096);
        let hub = Self {
            rx,
            tx: tx.clone(),
            conns: HashMap::new(),
            queue: Vec::new(),
            rooms: HashMap::new(),
            resumes: HashMap::new(),
            next_match: 1,
            last_stats: Instant::now(),
        };
        (hub, tx)
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

    fn handle(&mut self, cmd: HubCmd) {
        match cmd {
            HubCmd::Connect { id, tx } => {
                self.conns.insert(id, ConnInfo { tx, profile: None, activity: Activity::Idle, join_fails: 0, join_blocked_until: None });
            }
            HubCmd::Hello { id, profile, resume } => {
                let Some(c) = self.conns.get_mut(&id) else { return };
                if c.profile.is_some() {
                    return;
                }
                c.profile = Some(profile);
                let welcome = protocol::msg(json!({ "t": "welcome", "stats": self.stats_json() }));
                self.send(id, welcome);
                if let Some(token) = resume {
                    self.resume(id, &token);
                }
            }
            HubCmd::UpdateProfile { id, name, trophies, level } => {
                if let Some(p) = self.conns.get_mut(&id).and_then(|c| c.profile.as_mut()) {
                    p.name = name;
                    p.trophies = trophies.min(1_000_000);
                    p.level = level.clamp(1, 10_000);
                }
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
                self.queue.push(QueueEntry { id, clock, trophies, since: Instant::now() });
                self.send(id, protocol::msg(json!({ "t": "queued", "clock": clock })));
                self.matchmake();
            }
            HubCmd::Unqueue { id } => {
                if self.leave_queue(id) {
                    self.send(id, protocol::msg(json!({ "t": "unqueued" })));
                }
            }
            HubCmd::Create { id, clock } => {
                if !CLOCK_CHOICES.contains(&clock) {
                    return self.send(id, protocol::error("bad_clock"));
                }
                let Some(c) = self.conns.get(&id) else { return };
                if c.profile.is_none() || c.activity != Activity::Idle {
                    return self.send(id, protocol::error("busy"));
                }
                if self.rooms.len() >= MAX_ROOMS {
                    return self.send(id, protocol::error("server_full"));
                }
                let code = loop {
                    let code = random_code();
                    if !self.rooms.contains_key(&code) {
                        break code;
                    }
                };
                self.rooms.insert(code.clone(), Room { host: id, clock, created: Instant::now() });
                if let Some(c) = self.conns.get_mut(&id) {
                    c.activity = Activity::Hosting(code.clone());
                }
                self.send(id, protocol::msg(json!({ "t": "room", "code": code })));
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
                if let Some(c) = self.conns.get_mut(&id) {
                    c.join_fails += 1;
                    if c.join_fails >= JOIN_FAILS_BEFORE_BLOCK {
                        c.join_fails = 0;
                        c.join_blocked_until = Some(now + JOIN_BLOCK);
                    }
                }
                self.send(id, protocol::error("room_not_found"));
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

    fn tick(&mut self) {
        self.matchmake();
        let now = Instant::now();
        let expired: Vec<String> = self.rooms.iter().filter(|(_, r)| now.duration_since(r.created) >= ROOM_TTL).map(|(k, _)| k.clone()).collect();
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
    fn tokens_and_codes_are_well_formed() {
        let t = random_token();
        assert!(protocol::clean_token(&t).is_some());
        let c = random_code();
        assert_eq!(protocol::clean_code(&c), Some(c.clone()));
        assert_ne!(random_token(), t);
    }
}
