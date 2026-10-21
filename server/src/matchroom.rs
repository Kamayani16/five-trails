//! One task per live match. The task owns the game state outright (no locks):
//! connections send it commands, it validates and applies them, and it pushes each
//! player a view containing only what that player may see.

use crate::game::{EndReason, Game, RuleError};
use crate::hub::{ConnCtl, ConnId, ConnTx, HubCmd, HubTx};
use crate::protocol::{self, Action, Profile, EMOTE_COUNT};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// How long a dropped player has to reconnect before the match is forfeited.
pub const RECONNECT_GRACE: Duration = Duration::from_secs(45);
/// How long a finished match stays open for rematches.
const AFTER_GAME: Duration = Duration::from_secs(180);
/// Hard cap on a match's lifetime, rematches included.
const MAX_LIFETIME: Duration = Duration::from_secs(3 * 60 * 60);
const EMOTE_GAP: Duration = Duration::from_millis(2000);

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
}

#[derive(Clone, Debug)]
pub enum MatchKind {
    Quick,
    Room(String),
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
}

impl Player {
    pub fn new(profile: Profile, token: String, conn: ConnId, tx: ConnTx) -> Self {
        Self { profile, token, conn: Some(conn), tx: Some(tx), gone_since: None, left: false, rematch: false, last_emote: None }
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
    done: bool,
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
        Self { id, kind, base, game, players, rng, rx, hub, created: now, over_at: None, done: false }
    }

    pub async fn run(mut self) {
        tracing::info!(match_id = self.id, "match started");
        for seat in 0..2 {
            self.send_start(seat);
        }
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

    fn after_change(&mut self, now: Instant) {
        if self.game.over && self.over_at.is_none() {
            self.over_at = Some(now);
        }
        self.broadcast_state(now);
        if self.game.over && self.players.iter().all(|p| p.left || p.conn.is_none()) {
            self.done = true;
        }
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
                    Ok(()) => self.after_change(now),
                    Err(code) => {
                        send(&self.players[seat].tx, protocol::error(code));
                        self.send_state(seat, now);
                    }
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
                let other = &self.players[1 - seat];
                if other.left || other.conn.is_none() {
                    send(&self.players[seat].tx, protocol::error("opponent_left"));
                    return;
                }
                self.players[seat].rematch = true;
                if self.players.iter().all(|p| p.rematch) {
                    let first = self.rng.gen_range(0..2);
                    self.game = Game::new(&mut self.rng, first, self.base, now);
                    self.over_at = None;
                    for p in &mut self.players {
                        p.rematch = false;
                    }
                    for s in 0..2 {
                        self.send_start(s);
                    }
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
            Action::Rush => self.game.rush(seat),
        }
    }

    fn send_start(&self, seat: usize) {
        let now = Instant::now();
        let opp = &self.players[1 - seat];
        let (kind, code) = match &self.kind {
            MatchKind::Quick => ("quick", None),
            MatchKind::Room(c) => ("room", Some(c.as_str())),
        };
        let m = json!({
            "t": "match",
            "view": self.game.view(seat, now),
            "opp": opp.profile.to_json(),
            "oppConnected": opp.conn.is_some(),
            "token": self.players[seat].token,
            "kind": kind,
            "code": code,
            "clock": self.base.map_or(0, |b| b.as_secs()),
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
    }

    fn send_opp_status(&self, seat: usize) {
        let o = &self.players[1 - seat];
        send(&self.players[seat].tx, protocol::msg(json!({ "t": "opp", "connected": o.conn.is_some(), "left": o.left })));
    }

    fn send_rematch_status(&self) {
        for seat in 0..2 {
            let m = json!({ "t": "rematch", "me": self.players[seat].rematch, "opp": self.players[1 - seat].rematch });
            send(&self.players[seat].tx, protocol::msg(m));
        }
    }
}
