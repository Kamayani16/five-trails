//! Cups: small private tournaments for a closed group of 3 to 6 players.
//!
//! Two formats. **Round robin**: everyone plays everyone once; win 3, draw 1; the table
//! decides. **Knockout**: a bracket seeded by trophies, with byes for the top seeds when the
//! group is not a power of two; a drawn tie is replayed.
//!
//! Matches are played whenever both players are online (a "play anytime" window), so every
//! match has a deadline. When it passes, a player who showed up and waited beats one who did
//! not; otherwise round robin scores a 0-point draw and knockout sends the higher seed through.
//!
//! Everything here is pure: callers pass the current Unix time. Pids never leave the server;
//! the client sees players by their place in the cup.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const CUP_MIN: usize = 3;
pub const CUP_MAX: usize = 6;
pub const CUP_CODE_LEN: usize = 6;
/// Longest cup name.
pub const CUP_NAME_MAX: usize = 24;
/// Round robin: every match must be played within this long of the start.
pub const RR_WINDOW_SECS: u64 = 3 * 86_400;
/// Knockout: each match must be played within this long of both players being known.
pub const KO_WINDOW_SECS: u64 = 86_400;
/// Cups a player can be in at once (not counting finished ones).
pub const ACTIVE_CUPS_PER_PLAYER: usize = 5;
/// Coins paid by the game when a cup ends: champion, runner-up, everyone else who played.
pub const PRIZE: [u32; 3] = [100, 40, 15];
/// Feed lines kept per cup.
const FEED_MAX: usize = 30;
/// A lobby nobody started is dropped after a week; a finished cup after 30 days.
pub const LOBBY_TTL_SECS: u64 = 7 * 86_400;
pub const DONE_TTL_SECS: u64 = 30 * 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[serde(rename = "rr")]
    RoundRobin,
    #[serde(rename = "ko")]
    Knockout,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rr" => Some(Self::RoundRobin),
            "ko" => Some(Self::Knockout),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RoundRobin => "rr",
            Self::Knockout => "ko",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Lobby,
    Live,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CupPlayer {
    pub pid: String,
    pub name: String,
    /// Trophies when the cup started (for seeding).
    pub tr: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Res {
    /// Winning player (index into `players`), or None for a draw.
    pub win: Option<usize>,
    /// Scores of `a` and `b`.
    pub sa: i32,
    pub sb: i32,
    /// Decided by the deadline, not by playing.
    pub wo: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CupMatch {
    pub a: Option<usize>,
    pub b: Option<usize>,
    pub round: u8,
    pub res: Option<Res>,
    /// Each side has shown up to play (tapped Play and waited).
    pub ready: [bool; 2],
    /// Unix time the match must be played by (0 until both players are known).
    pub due: u64,
    /// Knockout ties that ended level and were replayed.
    pub draws: u8,
    /// Knockout: the match and slot (0 = a, 1 = b) the winner moves on to.
    pub next: Option<(usize, usize)>,
}

impl CupMatch {
    fn new(a: Option<usize>, b: Option<usize>, round: u8, next: Option<(usize, usize)>) -> Self {
        Self { a, b, round, res: None, ready: [false; 2], due: 0, draws: 0, next }
    }
    pub fn is_open(&self) -> bool {
        self.a.is_some() && self.b.is_some() && self.res.is_none()
    }
    /// Which side (0 = a, 1 = b) player `p` is on.
    pub fn side_of(&self, p: usize) -> Option<usize> {
        if self.a == Some(p) {
            Some(0)
        } else if self.b == Some(p) {
            Some(1)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cup {
    pub code: String,
    pub name: String,
    pub format: Format,
    pub clock: u32,
    pub created: u64,
    pub started: Option<u64>,
    pub ended: Option<u64>,
    pub state: State,
    /// players[0] is the host.
    pub players: Vec<CupPlayer>,
    /// Knockout seeding: seed order (best first) as player indexes.
    #[serde(default)]
    pub seeds: Vec<usize>,
    pub matches: Vec<CupMatch>,
    #[serde(default)]
    pub feed: Vec<String>,
    pub champion: Option<usize>,
    pub runner: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CupError {
    NotFound,
    Full,
    Started,
    NotHost,
    TooFew,
    NotYourMatch,
    NotOpen,
    TooMany,
    BadName,
}

impl CupError {
    pub fn code(self) -> &'static str {
        match self {
            CupError::NotFound => "cup_not_found",
            CupError::Full => "cup_full",
            CupError::Started => "cup_started",
            CupError::NotHost => "cup_not_host",
            CupError::TooFew => "cup_too_few",
            CupError::NotYourMatch => "cup_not_your_match",
            CupError::NotOpen => "cup_match_closed",
            CupError::TooMany => "cup_too_many",
            CupError::BadName => "cup_bad_name",
        }
    }
}

/// One row of the round-robin table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub p: usize,
    pub played: u32,
    pub w: u32,
    pub d: u32,
    pub l: u32,
    pub pts: u32,
    pub diff: i32,
}

/// What a finished cup pays each player: (player index, place 1..=3 meaning champion,
/// runner-up, took part; coins).
pub type Payout = Vec<(usize, u8, u32)>;
/// Prizes paid to real players: (pid, place, coins).
pub type Paid = Vec<(String, u8, u32)>;

pub fn clean_cup_name(raw: &str) -> Option<String> {
    let s: String = raw.chars().filter(|c| !c.is_control()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let n = s.chars().count();
    (1..=CUP_NAME_MAX).contains(&n).then_some(s)
}

impl Cup {
    pub fn new(code: String, name: String, format: Format, clock: u32, host: CupPlayer, now: u64) -> Self {
        Self {
            code,
            name,
            format,
            clock,
            created: now,
            started: None,
            ended: None,
            state: State::Lobby,
            players: vec![host],
            seeds: Vec::new(),
            matches: Vec::new(),
            feed: Vec::new(),
            champion: None,
            runner: None,
        }
    }

    pub fn index_of(&self, pid: &str) -> Option<usize> {
        self.players.iter().position(|p| p.pid == pid)
    }

    fn say(&mut self, line: String) {
        self.feed.push(line);
        if self.feed.len() > FEED_MAX {
            let cut = self.feed.len() - FEED_MAX;
            self.feed.drain(..cut);
        }
    }

    fn nm(&self, p: usize) -> String {
        self.players.get(p).map(|x| x.name.clone()).unwrap_or_default()
    }

    pub fn join(&mut self, p: CupPlayer) -> Result<(), CupError> {
        if self.index_of(&p.pid).is_some() {
            return Ok(());
        }
        if self.state != State::Lobby {
            return Err(CupError::Started);
        }
        if self.players.len() >= CUP_MAX {
            return Err(CupError::Full);
        }
        let line = format!("{} joined", p.name);
        self.players.push(p);
        self.say(line);
        Ok(())
    }

    /// Leave before the start. Returns true if the cup is now empty (or the host left) and
    /// should be deleted.
    pub fn leave(&mut self, pid: &str) -> Result<bool, CupError> {
        let i = self.index_of(pid).ok_or(CupError::NotFound)?;
        if self.state != State::Lobby {
            return Err(CupError::Started);
        }
        if i == 0 {
            return Ok(true);
        }
        let p = self.players.remove(i);
        self.say(format!("{} left", p.name));
        Ok(self.players.is_empty())
    }

    /// The host starts the cup: the schedule or bracket is drawn and the clock starts.
    pub fn start(&mut self, pid: &str, trophies: &[u32], now: u64) -> Result<(), CupError> {
        if self.index_of(pid) != Some(0) {
            return Err(CupError::NotHost);
        }
        if self.state != State::Lobby {
            return Err(CupError::Started);
        }
        let n = self.players.len();
        if n < CUP_MIN {
            return Err(CupError::TooFew);
        }
        for (p, &t) in self.players.iter_mut().zip(trophies) {
            p.tr = t;
        }
        self.started = Some(now);
        self.state = State::Live;
        match self.format {
            Format::RoundRobin => self.draw_round_robin(now),
            Format::Knockout => self.draw_bracket(now),
        }
        self.say(format!("The cup has started: {} players, {} matches", n, self.matches.len()));
        Ok(())
    }

    /// Circle method: every pair once, spread over rounds so nobody plays twice in a round.
    fn draw_round_robin(&mut self, now: u64) {
        let n = self.players.len();
        let mut ring: Vec<Option<usize>> = (0..n).map(Some).collect();
        if n % 2 == 1 {
            ring.push(None);
        }
        let m = ring.len();
        let due = now + RR_WINDOW_SECS;
        for r in 0..m - 1 {
            for i in 0..m / 2 {
                if let (Some(a), Some(b)) = (ring[i], ring[m - 1 - i]) {
                    let mut x = CupMatch::new(Some(a.min(b)), Some(a.max(b)), u8::try_from(r + 1).unwrap_or(u8::MAX), None);
                    x.due = due;
                    self.matches.push(x);
                }
            }
            // Keep the first seat fixed and rotate the rest.
            let last = ring.pop().expect("ring has players");
            ring.insert(1, last);
        }
    }

    /// Seeds by trophies (ties by join order); byes go to the top seeds.
    fn draw_bracket(&mut self, now: u64) {
        let mut seeds: Vec<usize> = (0..self.players.len()).collect();
        seeds.sort_by(|&x, &y| self.players[y].tr.cmp(&self.players[x].tr).then(x.cmp(&y)));
        let s = |k: usize| Some(seeds[k]);
        let ms = match seeds.len() {
            3 => vec![CupMatch::new(s(1), s(2), 1, Some((1, 1))), CupMatch::new(s(0), None, 2, None)],
            4 => vec![CupMatch::new(s(0), s(3), 1, Some((2, 0))), CupMatch::new(s(1), s(2), 1, Some((2, 1))), CupMatch::new(None, None, 2, None)],
            5 => vec![
                CupMatch::new(s(3), s(4), 1, Some((1, 1))),
                CupMatch::new(s(0), None, 2, Some((3, 0))),
                CupMatch::new(s(1), s(2), 2, Some((3, 1))),
                CupMatch::new(None, None, 3, None),
            ],
            _ => vec![
                CupMatch::new(s(2), s(5), 1, Some((3, 1))),
                CupMatch::new(s(3), s(4), 1, Some((2, 1))),
                CupMatch::new(s(0), None, 2, Some((4, 0))),
                CupMatch::new(s(1), None, 2, Some((4, 1))),
                CupMatch::new(None, None, 3, None),
            ],
        };
        self.seeds = seeds;
        self.matches = ms;
        for m in &mut self.matches {
            if m.a.is_some() && m.b.is_some() {
                m.due = now + KO_WINDOW_SECS;
            }
        }
    }

    /// The match `idx` this player may play now, and their opponent.
    pub fn playable(&self, idx: usize, pid: &str) -> Result<usize, CupError> {
        if self.state != State::Live {
            return Err(CupError::NotOpen);
        }
        let p = self.index_of(pid).ok_or(CupError::NotYourMatch)?;
        let m = self.matches.get(idx).ok_or(CupError::NotYourMatch)?;
        let side = m.side_of(p).ok_or(CupError::NotYourMatch)?;
        if !m.is_open() {
            return Err(CupError::NotOpen);
        }
        Ok(if side == 0 { m.b.expect("open") } else { m.a.expect("open") })
    }

    /// Note that a player showed up for a match (this decides walkovers at the deadline).
    pub fn mark_ready(&mut self, idx: usize, pid: &str) {
        let Some(p) = self.index_of(pid) else { return };
        if let Some(m) = self.matches.get_mut(idx) {
            if let Some(side) = m.side_of(p) {
                m.ready[side] = true;
            }
        }
    }

    /// A played match finished: `scores` and `winner` are by pid. Returns true if it counted.
    pub fn record(&mut self, idx: usize, pids: [&str; 2], scores: [i32; 2], winner: Option<&str>, now: u64) -> bool {
        let (Some(p0), Some(p1)) = (self.index_of(pids[0]), self.index_of(pids[1])) else { return false };
        let Some(m) = self.matches.get(idx) else { return false };
        if !m.is_open() || self.state != State::Live {
            return false;
        }
        let (sa, sb) = match (m.side_of(p0), m.side_of(p1)) {
            (Some(0), Some(1)) => (scores[0], scores[1]),
            (Some(1), Some(0)) => (scores[1], scores[0]),
            _ => return false,
        };
        let win = match winner {
            Some(w) => match self.index_of(w) {
                Some(i) if i == p0 || i == p1 => Some(i),
                _ => return false,
            },
            None => None,
        };
        let (a, b) = (m.a.expect("open"), m.b.expect("open"));
        if win.is_none() && self.format == Format::Knockout {
            let m = &mut self.matches[idx];
            m.draws = m.draws.saturating_add(1);
            m.ready = [false; 2];
            m.due = m.due.max(now + KO_WINDOW_SECS / 2);
            self.say(format!("{} and {} drew {}-{}: they play again", self.nm(a), self.nm(b), sa, sb));
            return true;
        }
        self.finish_match(idx, Res { win, sa, sb, wo: false }, now);
        true
    }

    fn finish_match(&mut self, idx: usize, res: Res, now: u64) {
        let (a, b, next) = {
            let m = &mut self.matches[idx];
            m.res = Some(res);
            (m.a.expect("open"), m.b.expect("open"), m.next)
        };
        let line = match (res.win, res.wo) {
            (Some(w), false) => {
                let l = if w == a { b } else { a };
                let (ws, ls) = if w == a { (res.sa, res.sb) } else { (res.sb, res.sa) };
                format!("{} beat {} {}-{}", self.nm(w), self.nm(l), ws, ls)
            }
            (Some(w), true) => {
                let l = if w == a { b } else { a };
                format!("{} wins: {} did not show up in time", self.nm(w), self.nm(l))
            }
            (None, true) => format!("{} vs {} was not played: a draw, no points", self.nm(a), self.nm(b)),
            (None, false) => format!("{} and {} drew {}-{}", self.nm(a), self.nm(b), res.sa, res.sb),
        };
        self.say(line);
        match self.format {
            Format::Knockout => {
                let w = res.win.expect("knockout ties have a winner");
                match next {
                    Some((ni, slot)) => {
                        let n = &mut self.matches[ni];
                        if slot == 0 {
                            n.a = Some(w);
                        } else {
                            n.b = Some(w);
                        }
                        if n.a.is_some() && n.b.is_some() {
                            n.due = now + KO_WINDOW_SECS;
                        }
                    }
                    None => {
                        let l = if w == a { b } else { a };
                        self.end(Some(w), Some(l), now);
                    }
                }
            }
            Format::RoundRobin => {
                if self.matches.iter().all(|m| m.res.is_some()) {
                    let t = self.table();
                    self.end(t.first().map(|r| r.p), t.get(1).map(|r| r.p), now);
                }
            }
        }
    }

    fn end(&mut self, champ: Option<usize>, runner: Option<usize>, now: u64) {
        self.state = State::Done;
        self.ended = Some(now);
        self.champion = champ;
        self.runner = runner;
        if let Some(c) = champ {
            self.say(format!("{} wins the cup!", self.nm(c)));
        }
    }

    /// Settle every match whose deadline has passed. Returns true if anything changed.
    pub fn expire(&mut self, now: u64) -> bool {
        if self.state != State::Live {
            return false;
        }
        let mut changed = false;
        // A knockout result can open the next round, so repeat until nothing is due.
        while let Some(idx) = self.matches.iter().position(|m| m.is_open() && m.due > 0 && now >= m.due) {
            let m = &self.matches[idx];
            let (a, b) = (m.a.expect("open"), m.b.expect("open"));
            let win = match m.ready {
                [true, false] => Some(a),
                [false, true] => Some(b),
                _ => match self.format {
                    Format::RoundRobin => None,
                    // Nobody (or both) showed: the higher seed goes through.
                    Format::Knockout => Some(if self.seed_rank(a) <= self.seed_rank(b) { a } else { b }),
                },
            };
            self.finish_match(idx, Res { win, sa: 0, sb: 0, wo: true }, now);
            changed = true;
            if self.state != State::Live {
                break;
            }
        }
        changed
    }

    fn seed_rank(&self, p: usize) -> usize {
        self.seeds.iter().position(|&s| s == p).unwrap_or(p)
    }

    /// Round-robin standings: points, then head-to-head, then score difference.
    pub fn table(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = (0..self.players.len()).map(|p| Row { p, played: 0, w: 0, d: 0, l: 0, pts: 0, diff: 0 }).collect();
        for m in &self.matches {
            let (Some(a), Some(b), Some(r)) = (m.a, m.b, m.res) else { continue };
            for (me, them, mine, theirs) in [(a, b, r.sa, r.sb), (b, a, r.sb, r.sa)] {
                let row = &mut rows[me];
                if !r.wo {
                    row.played += 1;
                    row.diff += mine - theirs;
                }
                match r.win {
                    Some(w) if w == me => {
                        row.w += 1;
                        row.pts += 3;
                    }
                    Some(_) => row.l += 1,
                    None if !r.wo => {
                        row.d += 1;
                        row.pts += 1;
                    }
                    None => {}
                }
                let _ = them;
            }
        }
        let h2h = |x: usize, y: usize| -> std::cmp::Ordering {
            for m in &self.matches {
                if let (Some(a), Some(b), Some(r)) = (m.a, m.b, m.res) {
                    if (a == x && b == y) || (a == y && b == x) {
                        return match r.win {
                            Some(w) if w == x => std::cmp::Ordering::Less,
                            Some(w) if w == y => std::cmp::Ordering::Greater,
                            _ => std::cmp::Ordering::Equal,
                        };
                    }
                }
            }
            std::cmp::Ordering::Equal
        };
        rows.sort_by(|x, y| y.pts.cmp(&x.pts).then(y.diff.cmp(&x.diff)).then(x.p.cmp(&y.p)));
        // Head-to-head settles a tie between exactly two players; bigger ties go on score.
        let mut i = 0;
        while i < rows.len() {
            let j = (i..rows.len()).take_while(|&k| rows[k].pts == rows[i].pts).count() + i;
            if j - i == 2 && h2h(rows[i].p, rows[i + 1].p) == std::cmp::Ordering::Greater {
                rows.swap(i, i + 1);
            }
            i = j;
        }
        rows
    }

    /// Who gets what when the cup is over.
    pub fn payout(&self) -> Payout {
        if self.state != State::Done {
            return Vec::new();
        }
        let played = |p: usize| self.matches.iter().any(|m| m.res.is_some_and(|r| !r.wo) && m.side_of(p).is_some());
        (0..self.players.len())
            .filter_map(|p| {
                if Some(p) == self.champion {
                    Some((p, 1, PRIZE[0]))
                } else if Some(p) == self.runner {
                    Some((p, 2, PRIZE[1]))
                } else if played(p) {
                    Some((p, 3, PRIZE[2]))
                } else {
                    None
                }
            })
            .collect()
    }

    /// How the cup looks to player `me` (an index), without anyone's pid.
    pub fn view(&self, me: Option<usize>, now: u64) -> Value {
        let players: Vec<Value> = self.players.iter().map(|p| json!({ "n": p.name, "tr": p.tr })).collect();
        let matches: Vec<Value> = self
            .matches
            .iter()
            .map(|m| {
                json!({
                    "a": m.a, "b": m.b, "r": m.round, "draws": m.draws,
                    "res": m.res.map(|r| json!({ "w": r.win, "sa": r.sa, "sb": r.sb, "wo": r.wo })),
                    "ready": m.ready, "dueIn": (m.due > 0 && m.res.is_none()).then(|| m.due.saturating_sub(now)),
                })
            })
            .collect();
        let table: Vec<Value> = if self.format == Format::RoundRobin && self.state != State::Lobby {
            self.table().iter().map(|r| json!({ "p": r.p, "pl": r.played, "w": r.w, "d": r.d, "l": r.l, "pts": r.pts, "diff": r.diff })).collect()
        } else {
            Vec::new()
        };
        let prize = me.and_then(|me| self.payout().into_iter().find(|x| x.0 == me)).map(|(_, place, coins)| json!({ "place": place, "coins": coins }));
        json!({
            "code": self.code, "name": self.name, "format": self.format.as_str(), "clock": self.clock,
            "state": match self.state { State::Lobby => "lobby", State::Live => "live", State::Done => "done" },
            "me": me, "host": me == Some(0), "players": players, "matches": matches, "table": table,
            "seeds": self.seeds, "champion": self.champion, "runner": self.runner,
            "feed": self.feed.iter().filter(|l| !l.starts_with('\0')).rev().take(12).collect::<Vec<_>>(), "prize": prize,
            "max": CUP_MAX, "min": CUP_MIN,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl(i: usize) -> CupPlayer {
        CupPlayer { pid: format!("pid-{i:08}"), name: format!("P{i}"), tr: 0 }
    }

    fn cup(n: usize, f: Format) -> Cup {
        let mut c = Cup::new("ABCDEF".into(), "Test".into(), f, 300, pl(0), 0);
        for i in 1..n {
            c.join(pl(i)).unwrap();
        }
        c
    }

    fn pid(i: usize) -> String {
        format!("pid-{i:08}")
    }

    /// Play match `idx` with side a scoring `sa` and side b `sb`.
    fn play(c: &mut Cup, idx: usize, sa: i32, sb: i32) {
        let m = &c.matches[idx];
        let (a, b) = (pid(m.a.unwrap()), pid(m.b.unwrap()));
        let w = match sa.cmp(&sb) {
            std::cmp::Ordering::Greater => Some(a.as_str()),
            std::cmp::Ordering::Less => Some(b.as_str()),
            std::cmp::Ordering::Equal => None,
        };
        assert!(c.record(idx, [&a, &b], [sa, sb], w, 10));
    }

    #[test]
    fn a_lobby_takes_up_to_six_and_needs_three_to_start() {
        let mut c = cup(2, Format::RoundRobin);
        assert_eq!(c.start(&pid(0), &[0, 0], 1), Err(CupError::TooFew));
        for i in 2..6 {
            c.join(pl(i)).unwrap();
        }
        assert_eq!(c.join(pl(6)), Err(CupError::Full));
        assert_eq!(c.join(pl(3)), Ok(()), "joining twice is harmless");
        assert_eq!(c.start(&pid(1), &[0; 6], 1), Err(CupError::NotHost));
        assert_eq!(c.leave(&pid(5)), Ok(false));
        assert_eq!(c.players.len(), 5);
        assert_eq!(c.leave(&pid(0)), Ok(true), "the host leaving closes the lobby");
        c.start(&pid(0), &[0; 5], 1).unwrap();
        assert_eq!(c.join(pl(9)), Err(CupError::Started));
    }

    #[test]
    fn round_robin_pairs_everyone_once_and_nobody_twice_in_a_round() {
        for n in 3..=6 {
            let mut c = cup(n, Format::RoundRobin);
            c.start(&pid(0), &vec![0; n], 100).unwrap();
            assert_eq!(c.matches.len(), n * (n - 1) / 2);
            let mut pairs: Vec<(usize, usize)> = c.matches.iter().map(|m| (m.a.unwrap(), m.b.unwrap())).collect();
            pairs.sort_unstable();
            pairs.dedup();
            assert_eq!(pairs.len(), n * (n - 1) / 2, "every pair once ({n} players)");
            for r in 1..=8u8 {
                let mut seen = Vec::new();
                for m in c.matches.iter().filter(|m| m.round == r) {
                    seen.push(m.a.unwrap());
                    seen.push(m.b.unwrap());
                }
                let k = seen.len();
                seen.sort_unstable();
                seen.dedup();
                assert_eq!(seen.len(), k, "nobody plays twice in round {r} ({n} players)");
            }
            assert!(c.matches.iter().all(|m| m.due == 100 + RR_WINDOW_SECS));
        }
    }

    #[test]
    fn round_robin_table_uses_points_then_head_to_head_then_score() {
        let mut c = cup(3, Format::RoundRobin);
        c.start(&pid(0), &[0, 0, 0], 0).unwrap();
        // Find the three pairings.
        let idx = |x: usize, y: usize| c.matches.iter().position(|m| m.a == Some(x.min(y)) && m.b == Some(x.max(y))).unwrap();
        let (m01, m02, m12) = (idx(0, 1), idx(0, 2), idx(1, 2));
        play(&mut c, m01, 30, 10); // P0 beats P1
        play(&mut c, m12, 25, 5); // P1 beats P2
        assert_eq!(c.state, State::Live);
        play(&mut c, m02, 0, 40); // P2 beats P0: everyone on 3 points
        assert_eq!(c.state, State::Done);
        let t = c.table();
        assert!(t.iter().all(|r| r.pts == 3 && r.played == 2));
        // Score difference: P0 +20-40=-20, P1 -20+20=0, P2 -20+40=+20.
        assert_eq!(t.iter().map(|r| r.p).collect::<Vec<_>>(), [2, 1, 0]);
        assert_eq!((c.champion, c.runner), (Some(2), Some(1)));
        let pay = c.payout();
        assert_eq!(pay, vec![(0, 3, PRIZE[2]), (1, 2, PRIZE[1]), (2, 1, PRIZE[0])]);
    }

    #[test]
    fn head_to_head_breaks_a_tie_on_points() {
        let mut c = cup(4, Format::RoundRobin);
        c.start(&pid(0), &[0; 4], 0).unwrap();
        // P0 and P1 both win everything except P1 beats P0.
        for i in 0..c.matches.len() {
            let m = &c.matches[i];
            let (a, b) = (m.a.unwrap(), m.b.unwrap());
            let (sa, sb) = match (a, b) {
                (0, 1) => (1, 2),
                (0, _) => (50, 0),
                (1, _) => (2, 1),
                _ => (10, 10),
            };
            play(&mut c, i, sa, sb);
        }
        let t = c.table();
        assert_eq!((t[0].p, t[0].pts), (1, 9));
        assert_eq!((t[1].p, t[1].pts), (0, 6));
    }

    #[test]
    fn knockout_brackets_seed_by_trophies_and_give_byes_to_the_top() {
        for n in 3..=6 {
            let mut c = cup(n, Format::Knockout);
            let tr: Vec<u32> = (0..n).map(|i| u32::try_from(i).unwrap() * 100).collect(); // last joiner is best
            c.start(&pid(0), &tr, 0).unwrap();
            assert_eq!(c.seeds[0], n - 1, "best trophies is the top seed");
            assert_eq!(c.matches.len(), n - 1, "one match per player knocked out");
            let first_round: Vec<&CupMatch> = c.matches.iter().filter(|m| m.is_open()).collect();
            let byes = n - 2 * first_round.len();
            let expected_byes = match n {
                3 => 1,
                4 => 0,
                5 => 1,
                _ => 2,
            };
            assert_eq!(byes, expected_byes, "{n} players");
            let top = c.seeds[0];
            if n != 4 {
                assert!(first_round.iter().all(|m| m.side_of(top).is_none()), "the top seed has a bye");
            }
            // Play it out: side a always wins.
            while c.state == State::Live {
                let i = c.matches.iter().position(CupMatch::is_open).expect("something to play");
                play(&mut c, i, 20, 10);
            }
            assert!(c.champion.is_some() && c.runner.is_some() && c.champion != c.runner);
        }
    }

    #[test]
    fn a_drawn_knockout_tie_is_replayed() {
        let mut c = cup(4, Format::Knockout);
        c.start(&pid(0), &[0; 4], 0).unwrap();
        play(&mut c, 0, 12, 12);
        assert!(c.matches[0].is_open());
        assert_eq!(c.matches[0].draws, 1);
        play(&mut c, 0, 13, 12);
        assert!(!c.matches[0].is_open());
        assert!(c.matches[2].a.is_some(), "the winner moved on to the final");
    }

    #[test]
    fn deadlines_give_walkovers_to_whoever_showed_up() {
        let mut c = cup(3, Format::RoundRobin);
        c.start(&pid(0), &[0; 3], 0).unwrap();
        let i = c.matches.iter().position(|m| m.side_of(0).is_some() && m.side_of(1).is_some()).unwrap();
        c.mark_ready(i, &pid(1));
        assert!(!c.expire(RR_WINDOW_SECS - 1));
        assert!(c.expire(RR_WINDOW_SECS));
        assert_eq!(c.state, State::Done);
        let r = c.matches[i].res.unwrap();
        assert_eq!((r.win, r.wo), (Some(1), true), "P1 waited, P0 never came");
        let others: Vec<Res> = c.matches.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, m)| m.res.unwrap()).collect();
        assert!(others.iter().all(|r| r.win.is_none() && r.wo), "unplayed: a draw with no points");
        let t = c.table();
        assert_eq!((t[0].p, t[0].pts, t[0].played), (1, 3, 0));
        assert!(t.iter().skip(1).all(|r| r.pts == 0 && r.d == 0));
        let mut pay = c.payout();
        pay.sort_unstable_by_key(|x| x.1);
        assert_eq!(pay, vec![(1, 1, PRIZE[0]), (c.runner.unwrap(), 2, PRIZE[1])], "nobody else played a match");
    }

    #[test]
    fn knockout_deadlines_send_the_higher_seed_through() {
        let mut c = cup(4, Format::Knockout);
        c.start(&pid(0), &[400, 300, 200, 100], 0).unwrap();
        assert!(c.expire(KO_WINDOW_SECS));
        // Both semis expired at once; the final opened, then expires a day later.
        assert_eq!(c.matches[2].a, Some(0));
        assert_eq!(c.matches[2].b, Some(1));
        assert_eq!(c.state, State::Live);
        assert!(c.expire(2 * KO_WINDOW_SECS));
        assert_eq!(c.champion, Some(0));
    }

    #[test]
    fn only_the_two_players_of_an_open_match_can_play_or_report_it() {
        let mut c = cup(4, Format::Knockout);
        c.start(&pid(0), &[0; 4], 0).unwrap();
        let m = c.matches[0].clone();
        let (a, b) = (pid(m.a.unwrap()), pid(m.b.unwrap()));
        let other = (0..4).map(pid).find(|p| *p != a && *p != b).unwrap();
        assert_eq!(c.playable(0, &a), Ok(m.b.unwrap()));
        assert_eq!(c.playable(0, &other), Err(CupError::NotYourMatch));
        assert_eq!(c.playable(2, &a), Err(CupError::NotYourMatch), "the final is not set yet");
        assert!(!c.record(0, [&a, &other], [1, 0], Some(&a), 1));
        assert!(c.record(0, [&b, &a], [5, 9], Some(&a), 1), "seat order does not matter");
        assert_eq!(c.matches[0].res.unwrap(), Res { win: m.a, sa: 9, sb: 5, wo: false });
        assert!(!c.record(0, [&a, &b], [9, 5], Some(&a), 1), "a match counts once");
        assert_eq!(c.playable(0, &a), Err(CupError::NotOpen));
    }

    #[test]
    fn the_view_never_shows_a_pid_and_names_are_cleaned() {
        let mut c = cup(3, Format::RoundRobin);
        c.start(&pid(0), &[0; 3], 0).unwrap();
        let v = c.view(Some(1), 5).to_string();
        assert!(!v.contains("pid-"));
        assert_eq!(clean_cup_name("  Saturday \n  Cup "), Some("Saturday Cup".into()));
        assert_eq!(clean_cup_name("   "), None);
        assert_eq!(clean_cup_name(&"x".repeat(25)), None);
    }
}
