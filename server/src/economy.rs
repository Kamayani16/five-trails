//! Rewards: the free chest, daily and weekly quests, and the season calendar.
//!
//! Everything here is pure (no I/O, no clock reads): callers pass the current Unix time.
//! Rewards are only ever earned from matches the server ran itself, so a modified browser
//! cannot give itself coins.

use serde::{Deserialize, Serialize};

pub const DAY: u64 = 86_400;
/// Free chests that can wait for you at once. While the bank is full the timer is stopped.
pub const CHEST_MAX: u8 = 2;
pub const CHEST_COINS: u32 = 40;
/// A match needs at least this many moves before it counts toward quests, so quitting
/// straight away (or two friends trading instant forfeits) earns nothing.
pub const MIN_SEQ_FOR_QUESTS: u32 = 11;
/// Season reward needs at least this many ranked games in the season.
pub const SEASON_MIN_GAMES: u32 = 5;

/// Knobs that deployments may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tuning {
    /// Added to Unix time before cutting it into days. 19800 s = India (UTC+5:30), so quests
    /// reset at local midnight there.
    pub day_offset: i64,
    /// Seconds for the next free chest to arrive.
    pub chest_every: u64,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { day_offset: 19_800, chest_every: 6 * 3600 }
    }
}

fn local(now: u64, t: &Tuning) -> u64 {
    u64::try_from(i64::try_from(now).unwrap_or(i64::MAX).saturating_add(t.day_offset)).unwrap_or(0)
}

/// Whole days since 1970-01-01 in the deployment's local time.
pub fn day_of(now: u64, t: &Tuning) -> u32 {
    u32::try_from(local(now, t) / DAY).unwrap_or(u32::MAX)
}

/// Weeks start on Monday (1970-01-01 was a Thursday).
pub fn week_of(day: u32) -> u32 {
    (day + 3) / 7
}

pub fn secs_to_next_day(now: u64, t: &Tuning) -> u64 {
    DAY - local(now, t) % DAY
}

pub fn secs_to_next_week(now: u64, t: &Tuning) -> u64 {
    let weekday = u64::from((day_of(now, t) + 3) % 7); // 0 = Monday
    (6 - weekday) * DAY + secs_to_next_day(now, t)
}

// ---------------------------------------------------------------- calendar

/// (year, month 1-12, day 1-31) for a count of days since 1970-01-01 (proleptic Gregorian).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seasons are calendar months: `year * 12 + month0`.
pub fn season_of(now: u64, t: &Tuning) -> u32 {
    let (y, m, _) = civil_from_days(i64::from(day_of(now, t)));
    u32::try_from(y * 12 + i64::from(m) - 1).unwrap_or(0)
}

pub fn season_name(id: u32) -> String {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    format!("{} {}", MONTHS[(id % 12) as usize], id / 12)
}

pub fn season_ends_in(now: u64, t: &Tuning) -> u64 {
    let (y, m, _) = civil_from_days(i64::from(day_of(now, t)));
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let end = u64::try_from(days_from_civil(ny, nm, 1)).unwrap_or(0) * DAY;
    end.saturating_sub(local(now, t))
}

/// Coins for finishing a season at `rank` (1 = best) after `games` ranked games.
pub fn season_reward(rank: usize, games: u32) -> u32 {
    if games < SEASON_MIN_GAMES {
        return 0;
    }
    match rank {
        1 => 500,
        2..=3 => 300,
        4..=10 => 150,
        11..=50 => 75,
        _ => 25,
    }
}

// ---------------------------------------------------------------- chest

/// Banked free chests. `n` are waiting; `next` is when the next one arrives (0 while full).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chest {
    pub n: u8,
    pub next: u64,
}

impl Chest {
    /// New players start with one chest waiting.
    pub fn fresh(now: u64, t: &Tuning) -> Self {
        Self { n: 1, next: now + t.chest_every }
    }

    /// Bring the bank up to date. Returns true if anything changed.
    pub fn refresh(&mut self, now: u64, t: &Tuning) -> bool {
        let before = *self;
        if self.n >= CHEST_MAX {
            self.n = CHEST_MAX;
            self.next = 0;
        } else {
            if self.next == 0 {
                self.next = now + t.chest_every;
            }
            while self.n < CHEST_MAX && now >= self.next {
                self.n += 1;
                self.next += t.chest_every;
            }
            if self.n >= CHEST_MAX {
                self.next = 0;
            }
        }
        *self != before
    }

    /// Open one chest. False if none is waiting.
    pub fn claim(&mut self, now: u64, t: &Tuning) -> bool {
        self.refresh(now, t);
        if self.n == 0 {
            return false;
        }
        self.n -= 1;
        if self.next == 0 {
            self.next = now + t.chest_every; // the bank was full: the timer starts now
        }
        true
    }

    /// Seconds until the next chest, or `None` while the bank is full.
    pub fn next_in(&self, now: u64) -> Option<u64> {
        (self.n < CHEST_MAX).then(|| self.next.saturating_sub(now))
    }
}

// ---------------------------------------------------------------- quests

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayKind {
    /// Quick match against another player.
    Ranked,
    /// Private room with a friend.
    Friend,
    /// The computer offered while waiting in the quick-match queue.
    Bot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Any,
    Ranked,
    Friend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric {
    Play,
    Win,
    Sabotage,
    Eye,
    Rush,
    Wagers,
    Score(i32),
    LongTrail,
}

#[derive(Clone, Copy, Debug)]
pub struct QuestDef {
    pub id: &'static str,
    pub text: &'static str,
    metric: Metric,
    scope: Scope,
    pub need: u32,
    pub coins: u32,
}

const fn q(id: &'static str, text: &'static str, metric: Metric, scope: Scope, need: u32, coins: u32) -> QuestDef {
    QuestDef { id, text, metric, scope, need, coins }
}

pub const DAILY_SLOTS: usize = 3;
pub const WEEKLY_SLOTS: usize = 2;

pub const DAILY: [QuestDef; 8] = [
    q("play2", "Finish 2 online battles", Metric::Play, Scope::Any, 2, 20),
    q("win1", "Win an online battle", Metric::Win, Scope::Any, 1, 30),
    q("sab1", "Use Sabotage in a battle", Metric::Sabotage, Scope::Any, 1, 25),
    q("eye1", "Use Evil Eye in a battle", Metric::Eye, Scope::Any, 1, 25),
    q("rush1", "Use Rush in a battle", Metric::Rush, Scope::Any, 1, 25),
    q("wager3", "Play 3 wager cards", Metric::Wagers, Scope::Any, 3, 25),
    q("score30", "Score 30 or more in a battle", Metric::Score(30), Scope::Any, 1, 30),
    q("friend1", "Finish a battle with a friend", Metric::Play, Scope::Friend, 1, 30),
];

pub const WEEKLY: [QuestDef; 4] = [
    q("wwin5", "Win 5 online battles", Metric::Win, Scope::Any, 5, 100),
    q("wrank3", "Win 3 quick matches against players", Metric::Win, Scope::Ranked, 3, 120),
    q("wfriend2", "Finish 2 battles with a friend", Metric::Play, Scope::Friend, 2, 80),
    q("wtrail8", "Build a trail of 8 cards", Metric::LongTrail, Scope::Any, 1, 80),
];

/// What one finished online battle looked like from one seat.
#[derive(Clone, Copy, Debug)]
pub struct PlayEvent {
    pub kind: PlayKind,
    pub won: bool,
    pub sab: u8,
    pub eye: u8,
    pub rush: u8,
    pub wagers: u8,
    pub score: i32,
    pub longest: u8,
}

impl QuestDef {
    fn gain(&self, e: &PlayEvent) -> u32 {
        let in_scope = match self.scope {
            Scope::Any => true,
            Scope::Ranked => e.kind == PlayKind::Ranked,
            Scope::Friend => e.kind == PlayKind::Friend,
        };
        if !in_scope {
            return 0;
        }
        match self.metric {
            Metric::Play => 1,
            Metric::Win => u32::from(e.won),
            Metric::Sabotage => u32::from(e.sab),
            Metric::Eye => u32::from(e.eye),
            Metric::Rush => u32::from(e.rush),
            Metric::Wagers => u32::from(e.wagers),
            Metric::Score(n) => u32::from(e.score >= n),
            Metric::LongTrail => u32::from(e.longest >= 8),
        }
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3))
}

/// `slots` distinct indexes below `len`, the same for the same player and period forever
/// (own tiny generator, so a library upgrade can never reshuffle someone's quests mid-day).
fn pick(pid: &str, period: u32, salt: u64, len: usize, slots: usize) -> Vec<usize> {
    let mut state = fnv1a(pid) ^ u64::from(period).wrapping_mul(0xD6E8_FEB8_6659_FD93) ^ salt;
    let mut pool: Vec<usize> = (0..len).collect();
    let mut out = Vec::with_capacity(slots);
    for _ in 0..slots.min(len) {
        let i = (splitmix64(&mut state) % pool.len() as u64) as usize;
        out.push(pool.swap_remove(i));
    }
    out
}

pub fn daily_for(pid: &str, day: u32) -> [&'static QuestDef; DAILY_SLOTS] {
    let p = pick(pid, day, 1, DAILY.len(), DAILY_SLOTS);
    [&DAILY[p[0]], &DAILY[p[1]], &DAILY[p[2]]]
}

pub fn weekly_for(pid: &str, week: u32) -> [&'static QuestDef; WEEKLY_SLOTS] {
    let p = pick(pid, week, 2, WEEKLY.len(), WEEKLY_SLOTS);
    [&WEEKLY[p[0]], &WEEKLY[p[1]]]
}

/// A player's quest progress for the current day and week.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quests {
    pub day: u32,
    pub d: [u32; DAILY_SLOTS],
    /// Bit i set = daily quest i was claimed.
    pub dc: u8,
    pub week: u32,
    pub w: [u32; WEEKLY_SLOTS],
    pub wc: u8,
}

impl Quests {
    /// Start a new day and/or week if the calendar moved on. Returns true if anything changed.
    pub fn refresh(&mut self, now: u64, t: &Tuning) -> bool {
        let day = day_of(now, t);
        let week = week_of(day);
        let mut changed = false;
        if self.day != day {
            self.day = day;
            self.d = [0; DAILY_SLOTS];
            self.dc = 0;
            changed = true;
        }
        if self.week != week {
            self.week = week;
            self.w = [0; WEEKLY_SLOTS];
            self.wc = 0;
            changed = true;
        }
        changed
    }

    pub fn record(&mut self, pid: &str, e: &PlayEvent, now: u64, t: &Tuning) {
        self.refresh(now, t);
        for (i, def) in daily_for(pid, self.day).iter().enumerate() {
            self.d[i] = (self.d[i] + def.gain(e)).min(def.need);
        }
        for (i, def) in weekly_for(pid, self.week).iter().enumerate() {
            self.w[i] = (self.w[i] + def.gain(e)).min(def.need);
        }
    }

    /// Claim a finished quest by slot name ("d0".."d2", "w0", "w1"). Returns its coins.
    pub fn claim(&mut self, pid: &str, slot: &str, now: u64, t: &Tuning) -> Result<u32, ClaimError> {
        self.refresh(now, t);
        let (weekly, i) = match slot.as_bytes() {
            [b'd', n @ b'0'..=b'9'] if usize::from(n - b'0') < DAILY_SLOTS => (false, usize::from(n - b'0')),
            [b'w', n @ b'0'..=b'9'] if usize::from(n - b'0') < WEEKLY_SLOTS => (true, usize::from(n - b'0')),
            _ => return Err(ClaimError::Unknown),
        };
        let (def, have, mask) = if weekly {
            (weekly_for(pid, self.week)[i], self.w[i], &mut self.wc)
        } else {
            (daily_for(pid, self.day)[i], self.d[i], &mut self.dc)
        };
        if *mask & (1 << i) != 0 {
            return Err(ClaimError::AlreadyClaimed);
        }
        if have < def.need {
            return Err(ClaimError::NotDone);
        }
        *mask |= 1 << i;
        Ok(def.coins)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimError {
    Unknown,
    NotDone,
    AlreadyClaimed,
    NothingToClaim,
}

impl ClaimError {
    pub fn code(self) -> &'static str {
        match self {
            ClaimError::Unknown => "bad_quest",
            ClaimError::NotDone => "quest_not_done",
            ClaimError::AlreadyClaimed => "already_claimed",
            ClaimError::NothingToClaim => "no_chest",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Tuning = Tuning { day_offset: 0, chest_every: 100 };

    fn ev(kind: PlayKind, won: bool) -> PlayEvent {
        PlayEvent { kind, won, sab: 0, eye: 0, rush: 0, wagers: 0, score: 0, longest: 0 }
    }

    #[test]
    fn calendar_math_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_582), (2026, 5, 9));
        for d in [-800, 0, 59, 60, 11_016, 19_782, 20_000, 30_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d, "round trip {d}");
        }
        // 2026-10-24 12:00 UTC
        let now = 1_792_843_200;
        assert_eq!(season_name(season_of(now, &T)), "October 2026");
        assert_eq!(season_of(now, &T), 2026 * 12 + 9);
    }

    #[test]
    fn season_ends_at_the_start_of_next_month() {
        let t = Tuning { day_offset: 0, ..T };
        let last_second_of_oct = 1_793_491_199; // 2026-10-31 23:59:59
        assert_eq!(season_ends_in(last_second_of_oct, &t), 1);
        assert_eq!(season_of(last_second_of_oct, &t) + 1, season_of(last_second_of_oct + 1, &t));
        // December rolls into January of the next year.
        let dec = u64::try_from(days_from_civil(2026, 12, 15)).unwrap() * DAY;
        assert_eq!(season_name(season_of(dec, &t) + 1), "January 2027");
        assert!(season_ends_in(dec, &t) > 16 * DAY && season_ends_in(dec, &t) < 18 * DAY);
    }

    #[test]
    fn day_offset_moves_the_reset() {
        let ist = Tuning { day_offset: 19_800, ..T };
        // 2026-10-24 18:29:59 UTC is 23:59:59 in India: one second to the daily reset.
        let now = u64::try_from(days_from_civil(2026, 10, 24)).unwrap() * DAY + 18 * 3600 + 29 * 60 + 59;
        assert_eq!(secs_to_next_day(now, &ist), 1);
        assert_eq!(day_of(now + 1, &ist), day_of(now, &ist) + 1);
    }

    #[test]
    fn weeks_start_on_monday() {
        let monday = u64::try_from(days_from_civil(2026, 10, 19)).unwrap() * DAY;
        let sunday = monday - DAY;
        assert_eq!(week_of(day_of(monday, &T)), week_of(day_of(sunday, &T)) + 1);
        assert_eq!(week_of(day_of(monday, &T)), week_of(day_of(monday + 6 * DAY, &T)));
        assert_eq!(secs_to_next_week(monday, &T), 7 * DAY);
        assert_eq!(secs_to_next_week(monday + 6 * DAY + DAY - 1, &T), 1);
    }

    #[test]
    fn chest_banks_up_to_the_cap_and_stops_the_timer() {
        let mut c = Chest::fresh(1000, &T);
        assert_eq!((c.n, c.next_in(1000)), (1, Some(100)));
        assert!(!c.refresh(1050, &T));
        c.refresh(1100, &T);
        assert_eq!((c.n, c.next_in(1100)), (2, None), "full bank: no timer");
        c.refresh(5000, &T);
        assert_eq!(c.n, CHEST_MAX, "never above the cap, nothing piles up while away");
        assert!(c.claim(5000, &T));
        assert_eq!((c.n, c.next_in(5000)), (1, Some(100)), "timer starts when you drop below the cap");
        assert!(c.claim(5000, &T));
        assert!(!c.claim(5000, &T), "none left");
        assert_eq!(c.n, 0);
        // Two periods away refills two.
        c.refresh(5000 + 250, &T);
        assert_eq!(c.n, 2);
    }

    #[test]
    fn chest_catches_up_after_a_long_absence_with_the_right_remainder() {
        let mut c = Chest { n: 0, next: 1100 };
        c.refresh(1150, &T);
        assert_eq!((c.n, c.next), (1, 1200));
    }

    #[test]
    fn quests_are_stable_distinct_and_differ_between_players_and_days() {
        let a = daily_for("pid-aaaaaaaa", 20_000);
        assert_eq!(a.map(|d| d.id), daily_for("pid-aaaaaaaa", 20_000).map(|d| d.id));
        assert!(a[0].id != a[1].id && a[1].id != a[2].id && a[0].id != a[2].id);
        let ids: std::collections::HashSet<_> = (0..40).map(|d| daily_for("pid-aaaaaaaa", 20_000 + d).map(|q| q.id)).collect();
        assert!(ids.len() > 3, "a different mix on different days");
        let others: std::collections::HashSet<_> = (0..40).map(|i| daily_for(&format!("pid-{i:08}"), 20_000).map(|q| q.id)).collect();
        assert!(others.len() > 3, "a different mix for different players");
        let w = weekly_for("pid-aaaaaaaa", 2_857);
        assert_ne!(w[0].id, w[1].id);
    }

    #[test]
    fn progress_counts_the_right_things_and_claims_once() {
        let pid = "pid-aaaaaaaa";
        let day = day_of(10 * DAY, &T);
        let mut q = Quests::default();
        let now = 10 * DAY;
        let defs = daily_for(pid, day);
        // A game that ticks every metric at once.
        let big = PlayEvent { kind: PlayKind::Friend, won: true, sab: 1, eye: 1, rush: 1, wagers: 3, score: 40, longest: 8 };
        q.record(pid, &big, now, &T);
        for (i, def) in defs.iter().enumerate() {
            let expected = match def.id {
                "play2" => 1,
                "friend1" | "win1" | "sab1" | "rush1" | "score30" => 1,
                "wager3" => 3,
                _ => unreachable!(),
            };
            assert_eq!(q.d[i], expected, "{}", def.id);
        }
        // Not finished yet -> cannot claim; finished -> claim once.
        let i = defs.iter().position(|d| d.id == "play2");
        if let Some(i) = i {
            assert_eq!(q.claim(pid, &format!("d{i}"), now, &T), Err(ClaimError::NotDone));
            q.record(pid, &big, now, &T);
            assert_eq!(q.claim(pid, &format!("d{i}"), now, &T), Ok(20));
            assert_eq!(q.claim(pid, &format!("d{i}"), now, &T), Err(ClaimError::AlreadyClaimed));
        }
        assert_eq!(q.claim(pid, "d9", now, &T), Err(ClaimError::Unknown));
        assert_eq!(q.claim(pid, "x0", now, &T), Err(ClaimError::Unknown));
        assert_eq!(q.claim(pid, "", now, &T), Err(ClaimError::Unknown));
    }

    #[test]
    fn scope_and_progress_cap() {
        let pid = "pid-bbbbbbbb";
        let week = week_of(day_of(40 * DAY, &T));
        let defs = weekly_for(pid, week);
        let mut q = Quests::default();
        for _ in 0..30 {
            q.record(pid, &PlayEvent { kind: PlayKind::Bot, won: true, sab: 0, eye: 0, rush: 0, wagers: 0, score: 0, longest: 8 }, 40 * DAY, &T);
        }
        for (i, def) in defs.iter().enumerate() {
            match def.id {
                "wrank3" | "wfriend2" => assert_eq!(q.w[i], 0, "{} needs a player or a friend, not the computer", def.id),
                _ => assert!(q.w[i] <= def.need && q.w[i] > 0, "{} is capped at its target", def.id),
            }
        }
    }

    #[test]
    fn a_new_day_resets_daily_but_not_weekly_and_vice_versa() {
        let pid = "pid-cccccccc";
        let mut q = Quests::default();
        let monday = u64::try_from(days_from_civil(2026, 10, 19)).unwrap() * DAY;
        let e = PlayEvent { kind: PlayKind::Friend, won: true, sab: 1, eye: 1, rush: 1, wagers: 3, score: 40, longest: 8 };
        q.record(pid, &e, monday, &T);
        let (d0, w0) = (q.d, q.w);
        assert!(d0.iter().any(|&x| x > 0) && w0.iter().any(|&x| x > 0));
        q.refresh(monday + DAY, &T); // Tuesday
        assert_eq!(q.d, [0; DAILY_SLOTS]);
        assert_eq!(q.w, w0, "weekly progress survives a new day");
        q.refresh(monday + 7 * DAY, &T); // next Monday
        assert_eq!(q.w, [0; WEEKLY_SLOTS]);
    }

    #[test]
    fn season_rewards_by_rank() {
        assert_eq!(season_reward(1, 10), 500);
        assert_eq!(season_reward(3, 10), 300);
        assert_eq!(season_reward(10, 10), 150);
        assert_eq!(season_reward(50, 10), 75);
        assert_eq!(season_reward(51, 5), 25);
        assert_eq!(season_reward(1, 4), 0, "needs a few games to count");
    }

    #[test]
    fn claim_errors_have_codes() {
        for e in [ClaimError::Unknown, ClaimError::NotDone, ClaimError::AlreadyClaimed, ClaimError::NothingToClaim] {
            assert!(!e.code().is_empty());
        }
        let _ = ev(PlayKind::Ranked, true);
    }
}
