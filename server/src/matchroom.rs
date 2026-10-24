//! One task per live match. The task owns the game state outright (no locks):
//! connections send it commands, it validates and applies them, and it pushes each
//! player a view containing only what that player may see.

use crate::bot::{Brain, Step};
use crate::economy::{PlayEvent, PlayKind, MIN_SEQ_FOR_QUESTS};
use crate::game::{value, EndReason, Game, RuleError, START_DELAY};
use crate::hub::{ConnCtl, ConnId, ConnTx, HubCmd, HubTx};
use crate::protocol::{self, Action, Profile, EMOTE_COUNT};
use crate::store::Outcome;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// How long a dropped player has to reconnect before the match is forfeited.
pub const RECONNECT_GRACE: Duration = Duration::from_secs(45);
/// How long a finished match stays open for rematches.
const AFTER_GAME: Duration = Duration::from_secs(180);
/// Hard cap on a match's lifetime, rematches included.
const MAX_LIFETIME: Duration = Duration::from_secs(3 * 60 * 60);
const EMOTE_GAP: Duration = Duration::from_millis(2000);

/// Player id of the computer opponent. It is not a valid player id, so it never matches an account.
pub const BOT_PID: &str = "";
/// Connection id standing in for the computer: real connections are numbered from 1.
const BOT_CONN: ConnId = 0;

pub type MatchId = u64;
pub type MatchTx = mpsc::Sender<MatchCmd>;

#[derive(Debug)]
pub enum MatchCmd {
    Move { seat: usize, conn: ConnId, seq: u32, action: Action, card: Option<u8>, pile: Option<i8> },
    Emote { seat: usize, conn: ConnId, k: u8 },
    Forfeit { seat: usize, conn: ConnId },
    Rematch { seat: usize, conn: ConnId },
    Leave { seat: usize, conn: ConnId },
    Detach { seat: usize, conn: ConnId },
    Attach { seat: usize, conn: ConnId, tx: ConnTx },
    /// A spectator starts or stops watching.
    Watch { conn: ConnId, tx: ConnTx },
    Unwatch { conn: ConnId },
}

/// Most spectators one match will take.
const MAX_SPECTATORS: usize = 20;

#[derive(Clone, Debug)]
pub enum MatchKind {
    Quick,
    Room(String),
    /// Against the computer at the given strength tier (see `bot::tier_for`). Unranked.
    Bot(usize),
    /// Match `.1` of cup `.0`. One game, no rematch; the result goes to the cup.
    Cup(String, usize),
}

pub struct Player {
    pub profile: Profile,
    pub token: String,
    pub conn: Option<ConnId>,
    pub tx: Option<ConnTx>,
    gone_since: Option<Instant>,
    left: bool,
    rematch: bool,
    last_emote: Option<Instant>,
    bot: bool,
}

impl Player {
    pub fn new(profile: Profile, token: String, conn: ConnId, tx: ConnTx) -> Self {
        Self { profile, token, conn: Some(conn), tx: Some(tx), gone_since: None, left: false, rematch: false, last_emote: None, bot: false }
    }

    /// The computer. It is always "connected", never "leaves" and has nothing to send to.
    pub fn bot(name: &str, trophies: u32) -> Self {
        let profile = Profile { name: name.to_owned(), pid: BOT_PID.to_owned(), trophies, level: 1, back: crate::economy::DEFAULT_BACK.to_owned() };
        Self { profile, token: String::new(), conn: Some(BOT_CONN), tx: None, gone_since: None, left: true, rematch: false, last_emote: None, bot: true }
    }
}

pub struct MatchRoom {
    id: MatchId,
    kind: MatchKind,
    base: Option<Duration>,
    game: Game,
    players: [Player; 2],
    rng: StdRng,
    rx: mpsc::Receiver<MatchCmd>,
    hub: HubTx,
    created: Instant,
    over_at: Option<Instant>,
    /// The current game's result has been sent to the hub for the leaderboard.
    reported: bool,
    done: bool,
    /// Seat of the computer, if this is a computer match.
    bot_seat: Option<usize>,
    tier: usize,
    brain: Brain,
    /// When the computer acts next.
    bot_due: Option<Instant>,
    /// How long the computer pauses after the step it just took.
    bot_pause: Option<u64>,
    /// When the current game began.
    started: Instant,
    /// Connections watching this match (they see public information only).
    spectators: Vec<(ConnId, ConnTx)>,
}

fn send(tx: &Option<ConnTx>, text: String) {
    if let Some(tx) = tx {
        // A full queue means the client is not reading; it will be dropped by its own task.
        let _ = tx.try_send(ConnCtl::Send(text));
    }
}

impl MatchRoom {
    pub fn new(id: MatchId, kind: MatchKind, clock_secs: u32, players: [Player; 2], rx: mpsc::Receiver<MatchCmd>, hub: HubTx) -> Self {
        let mut rng = StdRng::from_entropy();
        let base = (clock_secs > 0).then(|| Duration::from_secs(u64::from(clock_secs)));
        let first = rng.gen_range(0..2);
        let now = Instant::now();
        let game = Game::new(&mut rng, first, base, now);
        let bot_seat = players.iter().position(|p| p.bot);
        let tier = if let MatchKind::Bot(t) = kind { t } else { 0 };
        Self {
            id,
            kind,
            base,
            game,
            players,
            rng,
            rx,
            hub,
            created: now,
            over_at: None,
            reported: false,
            done: false,
            bot_seat,
            tier,
            brain: Brain::new(),
            bot_due: None,
            bot_pause: None,
            started: now,
            spectators: Vec::new(),
        }
    }

    pub async fn run(mut self) {
        tracing::info!(match_id = self.id, "match started");
        for seat in 0..2 {
            self.send_start(seat);
        }
        self.plan_bot(Instant::now());
        while !self.done {
            let deadline = self.next_deadline(Instant::now());
            tokio::select! {
                cmd = self.rx.recv() => match cmd {
                    Some(cmd) => self.handle(cmd),
                    None => break,
                },
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => self.on_timer(Instant::now()),
            }
        }
        for (_, tx) in &self.spectators {
            let _ = tx.try_send(ConnCtl::Send(protocol::msg(json!({ "t": "watch_end" }))));
        }
        for p in &self.players {
            send(&p.tx, protocol::msg(json!({ "t": "closed" })));
            if let Some(tx) = &p.tx {
                let _ = tx.try_send(ConnCtl::Left);
            }
        }
        let tokens = self.players.iter().map(|p| p.token.clone()).collect();
        let conns = self.players.iter().filter_map(|p| p.conn).collect();
        let _ = self.hub.send(HubCmd::MatchClosed { id: self.id, tokens, conns }).await;
        tracing::info!(match_id = self.id, "match closed");
    }

    fn next_deadline(&self, now: Instant) -> Instant {
        let mut d = (self.created + MAX_LIFETIME).min(now + Duration::from_secs(600));
        let mut consider = |t: Option<Instant>| {
            if let Some(t) = t {
                d = d.min(t);
            }
        };
        consider(self.bot_due);
        if self.game.over {
            consider(self.over_at.map(|t| t + AFTER_GAME));
        } else {
            consider(self.game.clock_deadline());
            consider(self.game.rush_deadline());
            for p in &self.players {
                if !p.left {
                    consider(p.gone_since.map(|g| g + RECONNECT_GRACE));
                }
            }
        }
        d
    }

    fn on_timer(&mut self, now: Instant) {
        if now >= self.created + MAX_LIFETIME {
            self.done = true;
            return;
        }
        if self.game.over {
            if self.over_at.is_some_and(|t| now >= t + AFTER_GAME) {
                self.done = true;
            }
            return;
        }
        if self.bot_due.is_some_and(|t| now >= t) {
            self.bot_act(now);
            return;
        }
        let before = self.game.seq;
        for seat in 0..2 {
            if !self.game.over && self.players[seat].gone_since.is_some_and(|g| now >= g + RECONNECT_GRACE) {
                self.game.flag(seat, EndReason::Forfeit, now);
            }
        }
        if !self.game.over && self.game.clock_deadline().is_some_and(|d| now >= d) {
            let turn = self.game.turn;
            self.game.flag(turn, EndReason::Time, now);
        }
        if !self.game.over && self.game.rush_deadline().is_some_and(|d| now >= d) {
            self.game.auto_move(now, &mut self.rng);
        }
        if self.game.seq != before {
            self.after_change(now);
        }
    }

    /// Who won the finished game: a clock or forfeit loss decides it, otherwise the higher score.
    fn outcome(&self) -> Outcome {
        match self.game.loser_by_flag {
            Some(loser) => Outcome::Win(1 - loser),
            None => match self.game.total(0).cmp(&self.game.total(1)) {
                std::cmp::Ordering::Greater => Outcome::Win(0),
                std::cmp::Ordering::Less => Outcome::Win(1),
                std::cmp::Ordering::Equal => Outcome::Draw,
            },
        }
    }

    /// What one seat did in the finished game, for quests.
    fn event(&self, seat: usize, kind: PlayKind, outcome: Outcome) -> PlayEvent {
        let cols = &self.game.columns[seat];
        PlayEvent {
            kind,
            won: outcome == Outcome::Win(seat),
            sab: 1u8.saturating_sub(self.game.sabotage_left[seat]),
            eye: u8::from(self.game.eye_used[seat]),
            rush: 1u8.saturating_sub(self.game.rush_left[seat]),
            wagers: u8::try_from(cols.iter().flatten().filter(|&&c| value(c) == 0).count()).unwrap_or(u8::MAX),
            score: self.game.total(seat),
            longest: u8::try_from(cols.iter().map(Vec::len).max().unwrap_or(0)).unwrap_or(u8::MAX),
        }
    }

    /// Tell the hub how a finished game went, once per game. Only Quick matches are ranked
    /// (friend rooms are easy to farm); all three kinds can move quests. Walking away from a
    /// game against the computer is free: it is reported as nothing at all.
    fn report_result(&mut self) {
        if !self.game.over || self.reported {
            return;
        }
        self.reported = true;
        let kind = match self.kind {
            MatchKind::Quick => PlayKind::Ranked,
            MatchKind::Room(_) => PlayKind::Friend,
            MatchKind::Bot(_) => PlayKind::Bot,
            MatchKind::Cup(..) => PlayKind::Cup,
        };
        if kind == PlayKind::Bot && self.game.end_reason == Some(EndReason::Forfeit) {
            return;
        }
        let outcome = self.outcome();
        let cmd = HubCmd::MatchResult {
            pids: [self.players[0].profile.pid.clone(), self.players[1].profile.pid.clone()],
            outcome,
            kind,
            events: [self.event(0, kind, outcome), self.event(1, kind, outcome)],
            counted: self.game.seq >= MIN_SEQ_FOR_QUESTS && kind != PlayKind::Cup,
            cup: if let MatchKind::Cup(code, m) = &self.kind { Some((code.clone(), *m)) } else { None },
            scores: [self.game.total(0), self.game.total(1)],
        };
        let hub = self.hub.clone();
        // Never drop a result because the hub's queue is momentarily full.
        tokio::spawn(async move {
            let _ = hub.send(cmd).await;
        });
    }

    /// Schedule the computer's next step, if it is its turn and a person is here to play it.
    fn plan_bot(&mut self, now: Instant) {
        self.bot_due = None;
        let pause = self.bot_pause.take();
        let Some(b) = self.bot_seat else { return };
        if self.game.over || self.game.turn != b || self.players.iter().any(|p| !p.bot && p.conn.is_none()) {
            return;
        }
        let pause = pause.unwrap_or_else(|| self.rng.gen_range(900..1600));
        // Never act before the VS splash has finished.
        let earliest = self.started + START_DELAY + Duration::from_millis(1300);
        self.bot_due = Some((now + Duration::from_millis(pause)).max(earliest));
    }

    fn bot_act(&mut self, now: Instant) {
        self.bot_due = None;
        let Some(b) = self.bot_seat else { return };
        if self.game.over || self.game.turn != b {
            return;
        }
        let step = self.brain.next(&self.game, b, self.tier, &mut self.rng);
        let result = match step {
            Step::Rush => self.game.rush(b),
            Step::Sabotage => self.game.sabotage(b, &mut self.rng),
            Step::Eye => self.game.eye(b, &mut self.rng),
            Step::Play(card, to_column) => self.game.play(b, card, to_column),
            Step::Draw(pile) => self.game.draw(b, pile, now),
        };
        match result {
            Ok(()) => self.bot_pause = Some(step.pause_after_ms()),
            Err(e) => {
                // Should not happen. A random legal move keeps the game going.
                tracing::warn!(match_id = self.id, ?step, error = ?e, "computer move refused");
                self.brain.reset();
                self.game.auto_move(now, &mut self.rng);
            }
        }
        self.after_change(now);
    }

    fn after_change(&mut self, now: Instant) {
        if self.game.over && self.over_at.is_none() {
            self.over_at = Some(now);
        }
        self.report_result();
        self.broadcast_state(now);
        if self.game.over && self.players.iter().all(|p| p.left || p.conn.is_none()) {
            self.done = true;
        }
        self.plan_bot(now);
    }

    fn is_current(&self, seat: usize, conn: ConnId) -> bool {
        seat < 2 && self.players[seat].conn == Some(conn)
    }

    fn handle(&mut self, cmd: MatchCmd) {
        let now = Instant::now();
        match cmd {
            MatchCmd::Move { seat, conn, seq, action, card, pile } => {
                if !self.is_current(seat, conn) {
                    return;
                }
                let result = if seq != self.game.seq {
                    Err("stale")
                } else if !self.game.over && self.players.iter().any(|p| p.conn.is_none()) {
                    Err("opponent_away")
                } else {
                    self.apply(seat, action, card, pile, now).map_err(RuleError::code)
                };
                match result {
                    Ok(()) => {
                        self.brain.reset();
                        self.after_change(now)
                    }
                    Err(code) => {
                        send(&self.players[seat].tx, protocol::error(code));
                        self.send_state(seat, now);
                    }
                }
            }
            MatchCmd::Watch { conn, tx } => {
                if !self.spectators.iter().any(|(c, _)| *c == conn) {
                    if self.spectators.len() >= MAX_SPECTATORS {
                        let _ = tx.try_send(ConnCtl::Send(protocol::error("watch_full")));
                        let _ = tx.try_send(ConnCtl::Send(protocol::msg(json!({ "t": "watch_end" }))));
                        return;
                    }
                    self.spectators.push((conn, tx.clone()));
                }
                self.send_watch_start(&tx, now);
                self.send_watchers();
            }
            MatchCmd::Unwatch { conn } => {
                let before = self.spectators.len();
                self.spectators.retain(|(c, _)| *c != conn);
                if self.spectators.len() != before {
                    self.send_watchers();
                }
            }
            MatchCmd::Emote { seat, conn, k } => {
                if !self.is_current(seat, conn) || k >= EMOTE_COUNT {
                    return;
                }
                let p = &mut self.players[seat];
                if p.last_emote.is_some_and(|t| now < t + EMOTE_GAP) {
                    return;
                }
                p.last_emote = Some(now);
                send(&self.players[1 - seat].tx, protocol::msg(json!({ "t": "emote", "k": k })));
                self.to_spectators(&protocol::msg(json!({ "t": "emote", "side": seat, "k": k })));
            }
            MatchCmd::Forfeit { seat, conn } => {
                if self.is_current(seat, conn) && !self.game.over {
                    self.game.flag(seat, EndReason::Forfeit, now);
                    self.after_change(now);
                }
            }
            MatchCmd::Rematch { seat, conn } => {
                if !self.is_current(seat, conn) || !self.game.over {
                    return;
                }
                if matches!(self.kind, MatchKind::Cup(..)) {
                    send(&self.players[seat].tx, protocol::error("cup_no_rematch"));
                    return;
                }
                let other = &self.players[1 - seat];
                if (other.left && !other.bot) || other.conn.is_none() {
                    send(&self.players[seat].tx, protocol::error("opponent_left"));
                    return;
                }
                self.players[seat].rematch = true;
                if let Some(b) = self.bot_seat {
                    self.players[b].rematch = true; // the computer always accepts
                }
                if self.players.iter().all(|p| p.rematch) {
                    let first = self.rng.gen_range(0..2);
                    self.game = Game::new(&mut self.rng, first, self.base, now);
                    self.over_at = None;
                    self.reported = false;
                    self.started = now;
                    self.brain.reset();
                    self.bot_pause = None;
                    for p in &mut self.players {
                        p.rematch = false;
                    }
                    for s in 0..2 {
                        self.send_start(s);
                    }
                    self.plan_bot(now);
                } else {
                    self.send_rematch_status();
                }
            }
            MatchCmd::Leave { seat, conn } => {
                if !self.is_current(seat, conn) {
                    return;
                }
                if !self.game.over {
                    self.game.flag(seat, EndReason::Forfeit, now);
                    self.over_at = Some(now);
                }
                self.report_result();
                let p = &mut self.players[seat];
                p.left = true;
                p.conn = None;
                p.tx = None;
                p.rematch = false;
                self.broadcast_state(now);
                self.send_opp_status(1 - seat);
                self.send_rematch_status();
                if self.players.iter().all(|p| p.left || p.conn.is_none()) {
                    self.done = true;
                }
            }
            MatchCmd::Detach { seat, conn } => {
                if !self.is_current(seat, conn) {
                    return;
                }
                let p = &mut self.players[seat];
                p.conn = None;
                p.tx = None;
                p.rematch = false;
                if self.game.over {
                    p.left = true;
                } else {
                    p.gone_since = Some(now);
                    self.game.pause(now);
                    self.broadcast_state(now);
                }
                self.send_opp_status(1 - seat);
                if self.players.iter().all(|p| p.left || p.conn.is_none()) && self.game.over {
                    self.done = true;
                }
            }
            MatchCmd::Attach { seat, conn, tx } => {
                if seat >= 2 {
                    return;
                }
                if self.players[seat].left {
                    let _ = tx.try_send(ConnCtl::Send(protocol::msg(json!({ "t": "resume_failed" }))));
                    let _ = tx.try_send(ConnCtl::Left);
                    return;
                }
                if let (Some(old), Some(old_tx)) = (self.players[seat].conn, self.players[seat].tx.take()) {
                    if old != conn {
                        let _ = old_tx.try_send(ConnCtl::Send(protocol::error("replaced")));
                        let _ = old_tx.try_send(ConnCtl::Left);
                    }
                }
                let p = &mut self.players[seat];
                p.conn = Some(conn);
                p.tx = Some(tx);
                p.gone_since = None;
                if !self.game.over && self.players.iter().all(|p| p.conn.is_some()) {
                    self.game.resume(now);
                }
                self.send_start(seat);
                self.send_state(1 - seat, now);
                self.send_opp_status(1 - seat);
                self.plan_bot(now);
            }
        }
    }

    fn apply(&mut self, seat: usize, action: Action, card: Option<u8>, pile: Option<i8>, now: Instant) -> Result<(), RuleError> {
        match action {
            Action::Play | Action::Disc => {
                let card = card.filter(|&c| c < crate::game::DECK_SIZE).ok_or(RuleError::NotInHand)?;
                self.game.play(seat, card, action == Action::Play)
            }
            Action::Draw => {
                let pile = match pile {
                    None | Some(-1) => None,
                    Some(p @ 0..=4) => Some(p as usize),
                    Some(_) => return Err(RuleError::BadPile),
                };
                self.game.draw(seat, pile, now)
            }
            Action::Sab => self.game.sabotage(seat, &mut self.rng),
            Action::Eye => self.game.eye(seat, &mut self.rng),
            Action::Rush => self.game.rush(seat),
        }
    }

    fn send_start(&self, seat: usize) {
        let now = Instant::now();
        let opp = &self.players[1 - seat];
        let (kind, code) = match &self.kind {
            MatchKind::Quick => ("quick", None),
            MatchKind::Room(c) => ("room", Some(c.as_str())),
            MatchKind::Bot(_) => ("bot", None),
            MatchKind::Cup(c, _) => ("cup", Some(c.as_str())),
        };
        let mut opp_json = opp.profile.to_json();
        if opp.bot {
            opp_json["bot"] = json!(true);
            opp_json["tier"] = json!(self.tier);
            // The computer shows a card back that matches how tough it is.
            const BOT_BACKS: [&str; 7] = ["stripes", "dots", "chevron", "paws", "flames", "circuit", "crest"];
            opp_json["back"] = json!(BOT_BACKS[self.tier.min(BOT_BACKS.len() - 1)]);
        }
        let m = json!({
            "t": "match",
            "view": self.game.view(seat, now),
            "opp": opp_json,
            "oppConnected": opp.conn.is_some(),
            "token": self.players[seat].token,
            "kind": kind,
            "code": code,
            "clock": self.base.map_or(0, |b| b.as_secs()),
            "cupMatch": if let MatchKind::Cup(_, i) = &self.kind { json!(i) } else { Value::Null },
        });
        send(&self.players[seat].tx, protocol::msg(m));
    }

    fn send_state(&self, seat: usize, now: Instant) {
        let m = json!({ "t": "state", "view": self.game.view(seat, now) });
        send(&self.players[seat].tx, protocol::msg(m));
    }

    fn broadcast_state(&self, now: Instant) {
        for seat in 0..2 {
            self.send_state(seat, now);
        }
        if !self.spectators.is_empty() {
            self.to_spectators(&protocol::msg(json!({ "t": "watch_state", "view": self.game.spectator_view(now) })));
        }
    }

    fn to_spectators(&self, text: &str) {
        for (_, tx) in &self.spectators {
            let _ = tx.try_send(ConnCtl::Send(text.to_owned()));
        }
    }

    fn send_watch_start(&self, tx: &ConnTx, now: Instant) {
        let (kind, code, m) = match &self.kind {
            MatchKind::Cup(c, i) => ("cup", Some(c.as_str()), Some(*i)),
            MatchKind::Room(c) => ("room", Some(c.as_str()), None),
            MatchKind::Quick => ("quick", None, None),
            MatchKind::Bot(_) => ("bot", None, None),
        };
        let m = json!({
            "t": "watch_start", "view": self.game.spectator_view(now),
            "players": [self.players[0].profile.to_json(), self.players[1].profile.to_json()],
            "kind": kind, "code": code, "cupMatch": m, "clock": self.base.map_or(0, |b| b.as_secs()),
        });
        let _ = tx.try_send(ConnCtl::Send(protocol::msg(m)));
    }

    /// Tell both players how many people are watching.
    fn send_watchers(&self) {
        let text = protocol::msg(json!({ "t": "watchers", "n": self.spectators.len() }));
        for p in &self.players {
            send(&p.tx, text.clone());
        }
    }

    fn send_opp_status(&self, seat: usize) {
        let o = &self.players[1 - seat];
        send(&self.players[seat].tx, protocol::msg(json!({ "t": "opp", "connected": o.conn.is_some(), "left": o.left && !o.bot })));
    }

    fn send_rematch_status(&self) {
        for seat in 0..2 {
            let m = json!({ "t": "rematch", "me": self.players[seat].rematch, "opp": self.players[1 - seat].rematch });
            send(&self.players[seat].tx, protocol::msg(m));
        }
    }
}
