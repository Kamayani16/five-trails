//! Persistent accounts, the leaderboards and each player's rewards.
//!
//! This is the only part of the server that survives a restart. Per player it keeps:
//!
//! - **Account**: a public id (`pid`, shown to opponents), a unique display name, and the
//!   SHA-256 hash of a secret key that only the player's device (or their recovery code)
//!   knows. Whoever shows that key *is* that player, so progress follows the person and
//!   not the browser.
//! - **Ranked record**: built only from online Quick matches this server ran itself, once
//!   for all time and once for the current calendar-month season.
//! - **Rewards**: coins, the banked free chest and quest progress. Earned only from matches
//!   this server ran, never from anything the browser reports.
//! - **Backup of lobby progress** (`prog`): the trophies, XP and records a browser reports
//!   about itself. It exists so a new phone can pick up where the old one left off. It is
//!   unverified, so it never feeds the leaderboard or the rewards.
//!
//! The data lives in memory and is written to one JSON file (atomically, via a temp file
//! and rename). The hub task owns the `Store`, so there are no locks.

use crate::cup::{self, Cup, CupError, CupPlayer, Format, Paid, Payout, State as CupState};
use crate::economy::{self, Chest, ClaimError, ItemKind, ShopError, PlayEvent, Quests, Tuning, CHEST_COINS, CHEST_MAX, DAILY_SLOTS, WEEKLY_SLOTS};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const WIN_TROPHIES: u32 = 30;
pub const LOSS_TROPHIES: u32 = 20;
/// Most registered players we keep. Registrations from new keys are refused beyond this.
pub const MAX_PLAYERS: usize = 200_000;
/// Accounts that never played, earned or backed anything up are forgotten after this long.
const UNPLAYED_TTL_SECS: u64 = 14 * 24 * 60 * 60;
/// Any account untouched for this long is forgotten.
const IDLE_TTL_SECS: u64 = 400 * 24 * 60 * 60;
const FILE_VERSION: u32 = 2;
/// Feed marker (never shown) noting that a finished cup has paid its prizes.
const PAID_MARK: &str = "\u{0}paid";

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What a browser says about its own lobby progress (unverified).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prog {
    pub tr: u32,
    pub best: u32,
    pub xp: u32,
    /// Wins, losses, draws against the computer / against players.
    pub ai: [u32; 3],
    pub pvp: [u32; 3],
}

impl Prog {
    pub fn clamped(self) -> Self {
        let c = |v: u32, max: u32| v.min(max);
        Self {
            tr: c(self.tr, 1_000_000),
            best: c(self.best, 1_000_000),
            xp: c(self.xp, 100_000_000),
            ai: self.ai.map(|v| c(v, 10_000_000)),
            pvp: self.pvp.map(|v| c(v, 10_000_000)),
        }
    }
}

/// One player's results in one season.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonStat {
    pub id: u32,
    pub tr: u32,
    pub w: u32,
    pub l: u32,
    pub d: u32,
}

/// What a player earned when a season closed. Shown once, then acknowledged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonReward {
    pub season: u32,
    pub rank: u32,
    pub coins: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerRec {
    pub pid: String,
    pub name: String,
    /// Ranked trophies, earned only in online Quick matches.
    pub tr: u32,
    pub w: u32,
    pub l: u32,
    pub d: u32,
    /// Unix seconds of the last time this player connected or finished a match.
    pub seen: u64,
    /// SHA-256 (hex) of the player's secret key. Empty only for records from an older file.
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub coins: u32,
    #[serde(default)]
    pub chest: Chest,
    #[serde(default)]
    pub quests: Quests,
    #[serde(default)]
    pub s: SeasonStat,
    #[serde(default)]
    pub reward: Option<SeasonReward>,
    #[serde(default)]
    pub prog: Option<Prog>,
    /// Online matches finished (any kind).
    #[serde(default)]
    pub plays: u32,
    /// Shop items bought with coins (free and trophy items are never listed here).
    #[serde(default)]
    pub owned: Vec<String>,
    /// Equipped table theme and card back. Empty means the default.
    #[serde(default)]
    pub table: String,
    #[serde(default)]
    pub back: String,
    /// Coins paid by battles today: (local day, coins), for the daily cap.
    #[serde(default)]
    pub bcoins: (u32, u32),
    /// Cups this player won.
    #[serde(default)]
    pub cup_wins: Vec<CupWin>,
    /// Friends (pids). Always mutual: both players list each other.
    #[serde(default)]
    pub friends: Vec<String>,
    /// Friend requests waiting: received from (`freq_in`) and sent to (`freq_out`) these pids.
    #[serde(default)]
    pub freq_in: Vec<String>,
    #[serde(default)]
    pub freq_out: Vec<String>,
    /// "Come and play" invites from friends, waiting to be seen (newest last).
    #[serde(default)]
    pub invites: Vec<Invite>,
}

/// A friend asked this player to play; delivered at once, or when the player next comes online.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub from: String,
    pub at: u64,
    /// Clock for the battle, seconds per player (one of the allowed choices).
    pub clock: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CupWin {
    pub name: String,
    pub at: u64,
}

impl PlayerRec {
    fn new(pid: String, name: String, auth: String, now: u64, t: &Tuning) -> Self {
        Self {
            pid,
            name,
            tr: 0,
            w: 0,
            l: 0,
            d: 0,
            seen: now,
            auth,
            coins: 0,
            chest: Chest::fresh(now, t),
            quests: Quests::default(),
            s: SeasonStat::default(),
            reward: None,
            prog: None,
            plays: 0,
            owned: Vec::new(),
            table: String::new(),
            back: String::new(),
            bcoins: (0, 0),
            cup_wins: Vec::new(),
            friends: Vec::new(),
            freq_in: Vec::new(),
            freq_out: Vec::new(),
            invites: Vec::new(),
        }
    }

    /// Best trophy count known for this player: ranked trophies, or the browser's own report.
    fn trophies(&self) -> u32 {
        self.tr.max(self.prog.map_or(0, |p| p.tr))
    }

    fn has(&self, it: &economy::Item) -> bool {
        if it.cup {
            return !self.cup_wins.is_empty();
        }
        if it.trophies > 0 {
            return self.trophies() >= it.trophies;
        }
        it.price == 0 || self.owned.iter().any(|o| o == it.id)
    }

    fn equipped(&self, kind: ItemKind) -> &str {
        let (v, d) = match kind {
            ItemKind::Table => (&self.table, economy::DEFAULT_TABLE),
            ItemKind::Back => (&self.back, economy::DEFAULT_BACK),
        };
        if v.is_empty() { d } else { v }
    }

    pub fn games(&self) -> u32 {
        self.w + self.l + self.d
    }

    /// Nothing worth keeping if the owner never comes back.
    fn pristine(&self) -> bool {
        self.plays == 0 && self.games() == 0 && self.coins == 0 && self.prog.is_none() && self.owned.is_empty() && self.cup_wins.is_empty() && self.friends.is_empty()
    }
}

/// At most this many friends, and this many requests waiting each way.
pub const MAX_FRIENDS: usize = 100;
pub const MAX_FRIEND_REQUESTS: usize = 30;

/// Invites last half a day; the same friend can invite again after a short pause.
pub const INVITE_TTL_SECS: u64 = 12 * 3600;
pub const INVITE_COOLDOWN_SECS: u64 = 30;
pub const MAX_INVITES: usize = 20;

#[derive(Debug, PartialEq, Eq)]
pub enum InviteError {
    NotFriend,
    TooSoon,
}

impl InviteError {
    pub fn code(&self) -> &'static str {
        match self {
            InviteError::NotFriend => "invite_not_friend",
            InviteError::TooSoon => "invite_too_soon",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FriendReply {
    /// Request sent (or it already was); the other player has to accept.
    Sent,
    /// They had asked you too, so you are now friends.
    Accepted,
    /// You were friends already.
    Already,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FriendError {
    NotFound,
    Yourself,
    NoRequest,
    Full,
    TooMany,
}

impl FriendError {
    pub fn code(&self) -> &'static str {
        match self {
            FriendError::NotFound => "friend_not_found",
            FriendError::Yourself => "friend_self",
            FriendError::NoRequest => "friend_no_request",
            FriendError::Full => "friend_full",
            FriendError::TooMany => "friend_too_many",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RenameError {
    /// Someone else already uses that name (compared ignoring case).
    Taken,
    /// The id was never registered.
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoginError {
    /// The registry is full and this key has no account yet.
    Full,
    /// Restoring was requested but no account has this key.
    Unknown,
}

/// The account a connection belongs to after `Store::login`.
#[derive(Debug, PartialEq, Eq)]
pub struct Login {
    pub pid: String,
    pub name: String,
    /// True when this key had no account and one was just made.
    pub created: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The seat (0 or 1) that won.
    Win(usize),
    Draw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankScope {
    AllTime,
    Season,
}

#[derive(Serialize, Deserialize)]
struct FileData {
    version: u32,
    #[serde(default)]
    season_id: u32,
    players: Vec<PlayerRec>,
    #[serde(default)]
    cups: Vec<Cup>,
    #[serde(default)]
    iap: Vec<IapRec>,
}

/// One App Store purchase that paid coins. Kept so a transaction can never pay twice and a
/// refund can take the coins back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IapRec {
    pub tx: String,
    pub pid: String,
    pub product: String,
    pub coins: u32,
    pub at: u64,
    #[serde(default)]
    pub revoked: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Credit {
    /// Coins added; the new balance.
    Paid(u32),
    /// This account already got the coins for this transaction; the balance.
    Already(u32),
    /// Another account already used this transaction.
    OtherAccount,
    Unknown,
}

/// Name comparison key: names are unique ignoring case.
fn name_key(name: &str) -> String {
    name.to_lowercase()
}

#[derive(Default)]
pub struct Store {
    players: HashMap<String, PlayerRec>,
    /// `name_key` -> pid
    names: HashMap<String, String>,
    /// key hash -> pid
    by_auth: HashMap<String, String>,
    season_id: u32,
    tuning: Tuning,
    dirty: bool,
    cups: HashMap<String, Cup>,
    iap: HashMap<String, IapRec>,
}

/// One line of a leaderboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankRow {
    pub pid: String,
    pub name: String,
    pub tr: u32,
    pub w: u32,
    pub l: u32,
    pub d: u32,
}

impl RankRow {
    pub fn games(&self) -> u32 {
        self.w + self.l + self.d
    }
}

/// A sorted snapshot of everyone who has played a ranked match in a scope.
#[derive(Default)]
pub struct Ranking {
    pub rows: Vec<RankRow>,
    index: HashMap<String, usize>,
}

impl Ranking {
    /// 1-based rank of `pid`, if it has played a ranked match.
    pub fn rank_of(&self, pid: &str) -> Option<(usize, &RankRow)> {
        self.index.get(pid).map(|&i| (i + 1, &self.rows[i]))
    }
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tuning(mut self, tuning: Tuning) -> Self {
        self.tuning = tuning;
        self
    }

    pub fn tuning(&self) -> Tuning {
        self.tuning
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn get(&self, pid: &str) -> Option<&PlayerRec> {
        self.players.get(pid)
    }

    #[cfg(test)]
    pub fn get_mut(&mut self, pid: &str) -> Option<&mut PlayerRec> {
        self.players.get_mut(pid)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn season_id(&self) -> u32 {
        self.season_id
    }

    /// Build a store from saved records. A duplicate name or key (which a hand-edited file
    /// could contain) is repaired so both stay unique.
    pub fn from_records(records: Vec<PlayerRec>, season_id: u32) -> Self {
        let mut store = Self::new();
        store.season_id = season_id;
        for mut rec in records {
            if store.players.contains_key(&rec.pid) {
                continue;
            }
            if store.names.contains_key(&name_key(&rec.name)) {
                match store.free_variant(&rec.name) {
                    Some(n) => rec.name = n,
                    None => continue,
                }
            }
            if !rec.auth.is_empty() && store.by_auth.contains_key(&rec.auth) {
                rec.auth.clear(); // becomes claimable by its player id, like an old record
            }
            if !rec.auth.is_empty() {
                store.by_auth.insert(rec.auth.clone(), rec.pid.clone());
            }
            store.names.insert(name_key(&rec.name), rec.pid.clone());
            store.players.insert(rec.pid.clone(), rec);
        }
        store
    }

    /// `base` with a short number appended, trying until a free name turns up.
    fn free_variant(&self, base: &str) -> Option<String> {
        let stem: String = base.chars().take(11).collect::<String>().trim_end().to_owned();
        let stem = if stem.is_empty() { "Player".to_owned() } else { stem };
        let mut rng = rand::thread_rng();
        (0..64).map(|_| format!("{stem}-{:04}", rng.gen_range(0..10_000u32))).find(|n| !self.names.contains_key(&name_key(n)))
    }

    fn fresh_pid(&self) -> String {
        let mut rng = rand::thread_rng();
        loop {
            let pid: String = (0..32).map(|_| format!("{:x}", rng.gen_range(0..16u8))).collect();
            if !self.players.contains_key(&pid) {
                return pid;
            }
        }
    }

    /// Identify a connecting player by their secret key (`auth` is its hash).
    ///
    /// - A key that already has an account gets that account, whatever id or name the
    ///   browser sent. This is how a recovery code restores progress on a new device.
    /// - A record from an older data file (no key yet) is claimed by the first browser to
    ///   present its player id.
    /// - Otherwise a new account is made. It gets `pid_hint` unless that id belongs to
    ///   someone else, and `wanted` as its name, or `wanted` with a number if it is taken.
    ///
    /// With `create` false (the player is restoring an account from a recovery code) only the
    /// first case can succeed: a mistyped code must never quietly make a new, empty account.
    pub fn login(&mut self, pid_hint: &str, auth: &str, wanted: &str, create: bool, now: u64) -> Result<Login, LoginError> {
        if let Some(pid) = self.by_auth.get(auth).cloned() {
            if let Some(p) = self.players.get_mut(&pid) {
                p.seen = now;
                self.dirty = true;
                return Ok(Login { pid, name: p.name.clone(), created: false });
            }
        }
        if !create {
            return Err(LoginError::Unknown);
        }
        if let Some(p) = self.players.get_mut(pid_hint) {
            if p.auth.is_empty() {
                p.auth = auth.to_owned();
                p.seen = now;
                self.by_auth.insert(auth.to_owned(), pid_hint.to_owned());
                self.dirty = true;
                return Ok(Login { pid: pid_hint.to_owned(), name: p.name.clone(), created: false });
            }
        }
        if self.players.len() >= MAX_PLAYERS {
            return Err(LoginError::Full);
        }
        let pid = if self.players.contains_key(pid_hint) { self.fresh_pid() } else { pid_hint.to_owned() };
        let name = if self.names.contains_key(&name_key(wanted)) { self.free_variant(wanted).ok_or(LoginError::Full)? } else { wanted.to_owned() };
        self.names.insert(name_key(&name), pid.clone());
        self.by_auth.insert(auth.to_owned(), pid.clone());
        self.players.insert(pid.clone(), PlayerRec::new(pid.clone(), name.clone(), auth.to_owned(), now, &self.tuning));
        self.dirty = true;
        Ok(Login { pid, name, created: true })
    }

    /// Test shorthand: log in with a key derived from the id, so the same id always maps to
    /// the same account.
    #[cfg(test)]
    pub fn register(&mut self, pid: &str, wanted: &str, now: u64) -> Option<String> {
        self.login(pid, &test_auth(pid), wanted, true, now).ok().map(|l| l.name)
    }

    /// Change a registered player's name. Fails if someone else has it.
    pub fn rename(&mut self, pid: &str, wanted: &str, now: u64) -> Result<String, RenameError> {
        let key = name_key(wanted);
        let Some(rec) = self.players.get(pid) else { return Err(RenameError::Unknown) };
        if rec.name == wanted {
            return Ok(rec.name.clone());
        }
        if self.names.get(&key).is_some_and(|owner| owner != pid) {
            return Err(RenameError::Taken);
        }
        let old_key = name_key(&rec.name);
        self.names.remove(&old_key);
        self.names.insert(key, pid.to_owned());
        if let Some(rec) = self.players.get_mut(pid) {
            rec.name = wanted.to_owned();
            rec.seen = now;
        }
        self.dirty = true;
        Ok(wanted.to_owned())
    }

    /// Apply the result of a ranked match between two registered players, to their
    /// all-time and current-season records.
    pub fn record_match(&mut self, pids: [&str; 2], outcome: Outcome, now: u64) {
        if pids[0] == pids[1] || pids.iter().any(|p| !self.players.contains_key(*p)) {
            return;
        }
        let season = self.season_id;
        for (seat, pid) in pids.iter().enumerate() {
            let Some(p) = self.players.get_mut(*pid) else { continue };
            if p.s.id != season {
                p.s = SeasonStat { id: season, ..SeasonStat::default() };
            }
            match outcome {
                Outcome::Win(s) if s == seat => {
                    p.w += 1;
                    p.s.w += 1;
                    p.tr = p.tr.saturating_add(WIN_TROPHIES);
                    p.s.tr = p.s.tr.saturating_add(WIN_TROPHIES);
                }
                Outcome::Win(_) => {
                    p.l += 1;
                    p.s.l += 1;
                    p.tr = p.tr.saturating_sub(LOSS_TROPHIES);
                    p.s.tr = p.s.tr.saturating_sub(LOSS_TROPHIES);
                }
                Outcome::Draw => {
                    p.d += 1;
                    p.s.d += 1;
                }
            }
            p.seen = now;
        }
        self.dirty = true;
    }

    /// Note a finished online battle for one player. It always counts as a play; it only
    /// moves quests when `counted` (the battle was long enough, see `MIN_SEQ_FOR_QUESTS`).
    /// Count a finished online battle for one player. Returns the coins it paid (0 for a
    /// battle too short to count, or once today's battle coins are used up).
    pub fn record_play(&mut self, pid: &str, ev: &PlayEvent, draw: bool, counted: bool, now: u64) -> u32 {
        let t = self.tuning;
        let Some(p) = self.players.get_mut(pid) else { return 0 };
        p.plays = p.plays.saturating_add(1);
        p.seen = now;
        self.dirty = true;
        if !counted {
            return 0;
        }
        p.quests.record(pid, ev, now, &t);
        let day = economy::day_of(now, &t);
        if p.bcoins.0 != day {
            p.bcoins = (day, 0);
        }
        let coins = economy::battle_coins(ev.kind, ev.won, draw).min(economy::BATTLE_COINS_PER_DAY.saturating_sub(p.bcoins.1));
        p.bcoins.1 += coins;
        p.coins = p.coins.saturating_add(coins);
        coins
    }

    /// Open one free chest. Returns the coins it held.
    pub fn claim_chest(&mut self, pid: &str, now: u64) -> Result<u32, ClaimError> {
        let t = self.tuning;
        let p = self.players.get_mut(pid).ok_or(ClaimError::Unknown)?;
        if !p.chest.claim(now, &t) {
            return Err(ClaimError::NothingToClaim);
        }
        p.coins = p.coins.saturating_add(CHEST_COINS);
        self.dirty = true;
        Ok(CHEST_COINS)
    }

    /// Buy a shop item with coins. Returns the coins left.
    pub fn buy(&mut self, pid: &str, id: &str) -> Result<u32, ShopError> {
        let it = economy::item(id).ok_or(ShopError::Unknown)?;
        let p = self.players.get_mut(pid).ok_or(ShopError::Unknown)?;
        if it.price == 0 {
            return Err(ShopError::NotForSale);
        }
        if p.has(it) {
            return Err(ShopError::Owned);
        }
        if p.coins < it.price {
            return Err(ShopError::Poor);
        }
        p.coins -= it.price;
        p.owned.push(it.id.to_owned());
        self.dirty = true;
        Ok(p.coins)
    }

    /// Use an item the player has. Returns the card back now equipped (rivals see it).
    pub fn equip(&mut self, pid: &str, kind: &str, id: &str) -> Result<String, ShopError> {
        let kind = ItemKind::parse(kind).ok_or(ShopError::Unknown)?;
        let it = economy::item(id).filter(|i| i.kind == kind).ok_or(ShopError::Unknown)?;
        let p = self.players.get_mut(pid).ok_or(ShopError::Unknown)?;
        if !p.has(it) {
            return Err(if it.trophies > 0 || it.cup { ShopError::Locked } else { ShopError::Poor });
        }
        match kind {
            ItemKind::Table => p.table = it.id.to_owned(),
            ItemKind::Back => p.back = it.id.to_owned(),
        }
        self.dirty = true;
        Ok(p.equipped(ItemKind::Back).to_owned())
    }

    /// The card back a player shows to rivals. Falls back to the default if they no longer
    /// qualify for a trophy item.
    pub fn back_of(&self, pid: &str) -> String {
        let Some(p) = self.players.get(pid) else { return economy::DEFAULT_BACK.to_owned() };
        let id = p.equipped(ItemKind::Back);
        match economy::item(id) {
            Some(it) if p.has(it) => id.to_owned(),
            _ => economy::DEFAULT_BACK.to_owned(),
        }
    }

    // ------------------------------------------------------------ in-app purchases

    pub fn coins_of(&self, pid: &str) -> u32 {
        self.players.get(pid).map_or(0, |p| p.coins)
    }

    /// Was this App Store transaction already used, and by whom?
    pub fn iap_owner(&self, tx: &str) -> Option<&str> {
        self.iap.get(tx).map(|r| r.pid.as_str())
    }

    /// Add the coins of a verified App Store transaction, once.
    pub fn iap_credit(&mut self, pid: &str, tx: &str, product: &str, coins: u32, now: u64) -> Credit {
        if let Some(r) = self.iap.get(tx) {
            if r.pid != pid {
                return Credit::OtherAccount;
            }
            return self.players.get(pid).map_or(Credit::Unknown, |p| Credit::Already(p.coins));
        }
        let Some(p) = self.players.get_mut(pid) else { return Credit::Unknown };
        p.coins = p.coins.saturating_add(coins);
        let bal = p.coins;
        self.iap.insert(tx.to_owned(), IapRec { tx: tx.to_owned(), pid: pid.to_owned(), product: product.to_owned(), coins, at: now, revoked: false });
        self.dirty = true;
        Credit::Paid(bal)
    }

    /// A purchase was refunded: take its coins back (as many as are left). Returns the pid.
    pub fn iap_revoke(&mut self, tx: &str) -> Option<String> {
        let r = self.iap.get_mut(tx).filter(|r| !r.revoked)?;
        r.revoked = true;
        let (pid, coins) = (r.pid.clone(), r.coins);
        if let Some(p) = self.players.get_mut(&pid) {
            p.coins = p.coins.saturating_sub(coins);
        }
        self.dirty = true;
        Some(pid)
    }

    // ------------------------------------------------------------ cups

    pub fn cup(&self, code: &str) -> Option<&Cup> {
        self.cups.get(code)
    }

    pub fn cup_code_taken(&self, code: &str) -> bool {
        self.cups.contains_key(code)
    }

    /// Cups this player is in, newest first.
    pub fn cups_of(&self, pid: &str) -> Vec<&Cup> {
        let mut v: Vec<&Cup> = self.cups.values().filter(|c| c.index_of(pid).is_some()).collect();
        v.sort_by(|a, b| b.created.cmp(&a.created).then(a.code.cmp(&b.code)));
        v
    }

    fn cup_player(&self, pid: &str) -> Result<CupPlayer, CupError> {
        let p = self.players.get(pid).ok_or(CupError::NotFound)?;
        let active = self.cups.values().filter(|c| c.state != CupState::Done && c.index_of(pid).is_some()).count();
        if active >= cup::ACTIVE_CUPS_PER_PLAYER {
            return Err(CupError::TooMany);
        }
        Ok(CupPlayer { pid: pid.to_owned(), name: p.name.clone(), tr: p.trophies() })
    }

    pub fn cup_create(&mut self, pid: &str, code: String, name: &str, format: Format, clock: u32, now: u64) -> Result<&Cup, CupError> {
        let name = cup::clean_cup_name(name).ok_or(CupError::BadName)?;
        let host = self.cup_player(pid)?;
        self.dirty = true;
        Ok(self.cups.entry(code.clone()).or_insert_with(|| Cup::new(code, name, format, clock, host, now)))
    }

    pub fn cup_join(&mut self, pid: &str, code: &str) -> Result<&Cup, CupError> {
        if self.cups.get(code).ok_or(CupError::NotFound)?.index_of(pid).is_some() {
            return Ok(&self.cups[code]);
        }
        let p = self.cup_player(pid)?;
        let c = self.cups.get_mut(code).ok_or(CupError::NotFound)?;
        c.join(p)?;
        self.dirty = true;
        Ok(c)
    }

    /// Leave a cup before it starts. Returns the members still in it (empty if it closed).
    pub fn cup_leave(&mut self, pid: &str, code: &str) -> Result<Vec<String>, CupError> {
        let c = self.cups.get_mut(code).ok_or(CupError::NotFound)?;
        let close = c.leave(pid)?;
        self.dirty = true;
        if close {
            let gone = self.cups.remove(code).map(|c| c.players.into_iter().map(|p| p.pid).collect()).unwrap_or_default();
            return Ok(gone);
        }
        Ok(Vec::new())
    }

    pub fn cup_start(&mut self, pid: &str, code: &str, now: u64) -> Result<(), CupError> {
        let tr: Vec<u32> = {
            let c = self.cups.get(code).ok_or(CupError::NotFound)?;
            c.players.iter().map(|p| self.players.get(&p.pid).map_or(p.tr, PlayerRec::trophies)).collect()
        };
        let c = self.cups.get_mut(code).ok_or(CupError::NotFound)?;
        c.start(pid, &tr, now)?;
        self.dirty = true;
        Ok(())
    }

    pub fn cup_mark_ready(&mut self, code: &str, idx: usize, pid: &str) {
        if let Some(c) = self.cups.get_mut(code) {
            c.mark_ready(idx, pid);
            self.dirty = true;
        }
    }

    /// A cup match was played. Returns what the cup paid out if this ended it.
    pub fn cup_record(&mut self, code: &str, idx: usize, pids: [&str; 2], scores: [i32; 2], winner: Option<&str>, now: u64) -> Option<Paid> {
        let c = self.cups.get_mut(code)?;
        if !c.record(idx, pids, scores, winner, now) {
            return None;
        }
        self.dirty = true;
        Some(self.settle(code, now))
    }

    /// Settle cups whose deadlines passed. Returns the changed codes and any payouts.
    pub fn cup_expire(&mut self, now: u64) -> Vec<(String, Paid)> {
        let due: Vec<String> = self.cups.iter_mut().filter_map(|(k, c)| c.expire(now).then(|| k.clone())).collect();
        if !due.is_empty() {
            self.dirty = true;
        }
        due.into_iter().map(|k| {
            let paid = self.settle(&k, now);
            (k, paid)
        }).collect()
    }

    /// Pay a cup that has just ended (once). Returns (pid, place, coins) for each payee.
    fn settle(&mut self, code: &str, now: u64) -> Paid {
        let Some(c) = self.cups.get(code) else { return Vec::new() };
        if c.state != CupState::Done || c.feed.iter().any(|l| l == PAID_MARK) {
            return Vec::new();
        }
        let payout: Payout = c.payout();
        let name = c.name.clone();
        let pids: Vec<String> = c.players.iter().map(|p| p.pid.clone()).collect();
        let mut out = Vec::new();
        for (p, place, coins) in payout {
            let Some(pid) = pids.get(p) else { continue };
            if let Some(rec) = self.players.get_mut(pid) {
                rec.coins = rec.coins.saturating_add(coins);
                if place == 1 {
                    rec.cup_wins.push(CupWin { name: name.clone(), at: now });
                }
            }
            out.push((pid.clone(), place, coins));
        }
        if let Some(c) = self.cups.get_mut(code) {
            c.feed.push(PAID_MARK.to_owned());
        }
        self.dirty = true;
        out
    }

    /// Collect a finished quest ("d0".."d2", "w0", "w1"). Returns its coins.
    pub fn claim_quest(&mut self, pid: &str, slot: &str, now: u64) -> Result<u32, ClaimError> {
        let t = self.tuning;
        let p = self.players.get_mut(pid).ok_or(ClaimError::Unknown)?;
        let coins = p.quests.claim(pid, slot, now, &t)?;
        p.coins = p.coins.saturating_add(coins);
        self.dirty = true;
        Ok(coins)
    }

    /// Remember a browser's own report of its lobby progress, for restoring on another device.
    pub fn set_prog(&mut self, pid: &str, prog: Prog) {
        if let Some(p) = self.players.get_mut(pid) {
            p.prog = Some(prog.clamped());
            self.dirty = true;
        }
    }

    pub fn prog(&self, pid: &str) -> Option<Prog> {
        self.players.get(pid).and_then(|p| p.prog)
    }

    /// The player has seen their season reward.
    pub fn ack_season(&mut self, pid: &str) {
        if let Some(p) = self.players.get_mut(pid) {
            if p.reward.take().is_some() {
                self.dirty = true;
            }
        }
    }

    /// Close the current season if the calendar has moved on: pay out coins by final rank and
    /// start the new one. Returns how many players were paid.
    pub fn rollover(&mut self, now: u64) -> usize {
        let current = economy::season_of(now, &self.tuning);
        if current <= self.season_id {
            return 0;
        }
        let old = self.season_id;
        let mut paid = 0;
        if old != 0 {
            let standings = self.ranking(RankScope::Season);
            for (i, row) in standings.rows.iter().enumerate() {
                let rank = i + 1;
                let coins = economy::season_reward(rank, row.games());
                if let Some(p) = self.players.get_mut(&row.pid).filter(|_| coins > 0) {
                    p.coins = p.coins.saturating_add(coins);
                    p.reward = Some(SeasonReward { season: old, rank: u32::try_from(rank).unwrap_or(u32::MAX), coins });
                    paid += 1;
                }
            }
        }
        self.season_id = current;
        self.dirty = true;
        paid
    }

    /// Everything the lobby shows about a player's rewards, as a `me` message.
    pub fn me_json(&mut self, pid: &str, now: u64) -> Option<Value> {
        let t = self.tuning;
        let season = self.season_id;
        let p = self.players.get_mut(pid)?;
        let changed = p.chest.refresh(now, &t) | p.quests.refresh(now, &t);
        let q = &p.quests;
        let slot = |prefix: &str, i: usize, def: &economy::QuestDef, have: u32, mask: u8| {
            json!({ "slot": format!("{prefix}{i}"), "id": def.id, "text": def.text, "need": def.need, "have": have.min(def.need), "coins": def.coins, "claimed": mask & (1 << i) != 0 })
        };
        let daily: Vec<Value> = economy::daily_for(pid, q.day).iter().enumerate().map(|(i, d)| slot("d", i, d, q.d[i], q.dc)).collect();
        let weekly: Vec<Value> = economy::weekly_for(pid, q.week).iter().enumerate().map(|(i, d)| slot("w", i, d, q.w[i], q.wc)).collect();
        debug_assert!(daily.len() == DAILY_SLOTS && weekly.len() == WEEKLY_SLOTS);
        let reward = p.reward.map(|r| json!({ "season": economy::season_name(r.season), "rank": r.rank, "coins": r.coins }));
        let msg = json!({
            "t": "me",
            "coins": p.coins,
            "chest": { "n": p.chest.n, "max": CHEST_MAX, "nextIn": p.chest.next_in(now), "every": t.chest_every, "coins": CHEST_COINS },
            "daily": daily, "weekly": weekly,
            "dailyIn": economy::secs_to_next_day(now, &t), "weeklyIn": economy::secs_to_next_week(now, &t),
            "season": { "name": economy::season_name(season), "endsIn": economy::season_ends_in(now, &t) },
            "reward": reward,
            "cupWins": p.cup_wins.iter().rev().take(10).map(|w| json!({ "name": w.name, "at": w.at })).collect::<Vec<_>>(),
            "shop": {
                "items": economy::SHOP.iter().map(|i| json!({ "id": i.id, "kind": i.kind.as_str(), "price": i.price, "tr": i.trophies, "cup": i.cup, "have": p.has(i) })).collect::<Vec<_>>(),
                "table": p.equipped(ItemKind::Table),
                "back": p.equipped(ItemKind::Back),
            },
        });
        if changed {
            self.dirty = true;
        }
        Some(msg)
    }

    /// Everyone with at least one ranked match in `scope`, best first: trophies, then wins,
    /// then fewer losses, then name.
    pub fn ranking(&self, scope: RankScope) -> Ranking {
        let mut rows: Vec<RankRow> = match scope {
            RankScope::AllTime => self
                .players
                .values()
                .filter(|p| p.games() > 0)
                .map(|p| RankRow { pid: p.pid.clone(), name: p.name.clone(), tr: p.tr, w: p.w, l: p.l, d: p.d })
                .collect(),
            RankScope::Season => self
                .players
                .values()
                .filter(|p| p.s.id == self.season_id && p.s.w + p.s.l + p.s.d > 0)
                .map(|p| RankRow { pid: p.pid.clone(), name: p.name.clone(), tr: p.s.tr, w: p.s.w, l: p.s.l, d: p.s.d })
                .collect(),
        };
        rows.sort_by(|a, b| {
            b.tr.cmp(&a.tr)
                .then(b.w.cmp(&a.w))
                .then(a.l.cmp(&b.l))
                .then_with(|| name_key(&a.name).cmp(&name_key(&b.name)))
                .then_with(|| a.pid.cmp(&b.pid))
        });
        let index = rows.iter().enumerate().map(|(i, r)| (r.pid.clone(), i)).collect();
        Ranking { rows, index }
    }

    /// Forget accounts nobody uses any more.
    /// The pid of the player with this name (ignoring case).
    pub fn pid_by_name(&self, name: &str) -> Option<&str> {
        self.names.get(&name_key(name)).map(String::as_str)
    }

    /// Mark a player as active now (used when they go offline, for "last active").
    pub fn touch(&mut self, pid: &str, now: u64) {
        if let Some(p) = self.players.get_mut(pid) {
            if p.seen < now {
                p.seen = now;
                self.dirty = true;
            }
        }
    }

    /// Ask `to` to be friends. If `to` already asked `from`, they become friends at once.
    pub fn friend_request(&mut self, from: &str, to: &str) -> Result<FriendReply, FriendError> {
        if from == to {
            return Err(FriendError::Yourself);
        }
        let (Some(a), Some(b)) = (self.players.get(from), self.players.get(to)) else { return Err(FriendError::NotFound) };
        if a.friends.iter().any(|f| f == to) {
            return Ok(FriendReply::Already);
        }
        if a.freq_in.iter().any(|f| f == to) {
            return self.friend_accept(from, to).map(|_| FriendReply::Accepted);
        }
        if a.freq_out.iter().any(|f| f == to) {
            return Ok(FriendReply::Sent);
        }
        if a.friends.len() >= MAX_FRIENDS || b.friends.len() >= MAX_FRIENDS {
            return Err(FriendError::Full);
        }
        if a.freq_out.len() >= MAX_FRIEND_REQUESTS || b.freq_in.len() >= MAX_FRIEND_REQUESTS {
            return Err(FriendError::TooMany);
        }
        self.players.get_mut(from).expect("checked").freq_out.push(to.to_owned());
        self.players.get_mut(to).expect("checked").freq_in.push(from.to_owned());
        self.dirty = true;
        Ok(FriendReply::Sent)
    }

    /// `me` accepts the request `other` sent.
    pub fn friend_accept(&mut self, me: &str, other: &str) -> Result<(), FriendError> {
        let (Some(a), Some(b)) = (self.players.get(me), self.players.get(other)) else { return Err(FriendError::NotFound) };
        if !a.freq_in.iter().any(|f| f == other) {
            return Err(FriendError::NoRequest);
        }
        if a.friends.len() >= MAX_FRIENDS || b.friends.len() >= MAX_FRIENDS {
            return Err(FriendError::Full);
        }
        self.friend_drop(me, other);
        self.players.get_mut(me).expect("checked").friends.push(other.to_owned());
        self.players.get_mut(other).expect("checked").friends.push(me.to_owned());
        self.dirty = true;
        Ok(())
    }

    /// End whatever there is between two players: a friendship, or a request either way.
    pub fn friend_drop(&mut self, me: &str, other: &str) {
        for (x, y) in [(me, other), (other, me)] {
            if let Some(p) = self.players.get_mut(x) {
                let n = p.friends.len() + p.freq_in.len() + p.freq_out.len();
                p.friends.retain(|f| f != y);
                p.freq_in.retain(|f| f != y);
                p.freq_out.retain(|f| f != y);
                p.invites.retain(|i| i.from != y);
                if n != p.friends.len() + p.freq_in.len() + p.freq_out.len() {
                    self.dirty = true;
                }
            }
        }
    }

    /// `from` invites friend `to` to play. One invite per friend: a new one replaces the old.
    pub fn invite(&mut self, from: &str, to: &str, clock: u32, now: u64) -> Result<(), InviteError> {
        if !self.players.get(from).is_some_and(|p| p.friends.iter().any(|f| f == to)) {
            return Err(InviteError::NotFriend);
        }
        let p = self.players.get_mut(to).ok_or(InviteError::NotFriend)?;
        if p.invites.iter().any(|i| i.from == from && now.saturating_sub(i.at) < INVITE_COOLDOWN_SECS) {
            return Err(InviteError::TooSoon);
        }
        p.invites.retain(|i| i.from != from && now.saturating_sub(i.at) < INVITE_TTL_SECS);
        if p.invites.len() >= MAX_INVITES {
            p.invites.remove(0);
        }
        p.invites.push(Invite { from: from.to_owned(), at: now, clock });
        self.dirty = true;
        Ok(())
    }

    /// Invites `pid` still has (older ones expire).
    pub fn invites_of(&mut self, pid: &str, now: u64) -> Vec<Invite> {
        let Some(p) = self.players.get_mut(pid) else { return Vec::new() };
        let n = p.invites.len();
        p.invites.retain(|i| now.saturating_sub(i.at) < INVITE_TTL_SECS);
        if p.invites.len() != n {
            self.dirty = true;
        }
        p.invites.clone()
    }

    /// Remove and return the invite `from` sent `me`, if it is still there.
    pub fn invite_take(&mut self, me: &str, from: &str, now: u64) -> Option<Invite> {
        let p = self.players.get_mut(me)?;
        let i = p.invites.iter().position(|i| i.from == from)?;
        let inv = p.invites.remove(i);
        self.dirty = true;
        (now.saturating_sub(inv.at) < INVITE_TTL_SECS).then_some(inv)
    }

    pub fn prune(&mut self, now: u64) {
        let before = self.cups.len();
        self.cups.retain(|_, c| match c.state {
            CupState::Lobby => now.saturating_sub(c.created) < cup::LOBBY_TTL_SECS,
            CupState::Done => now.saturating_sub(c.ended.unwrap_or(c.created)) < cup::DONE_TTL_SECS,
            CupState::Live => true,
        });
        if self.cups.len() != before {
            self.dirty = true;
        }
        let stale: Vec<String> = self
            .players
            .values()
            .filter(|p| {
                let idle = now.saturating_sub(p.seen);
                (p.pristine() && idle > UNPLAYED_TTL_SECS) || idle > IDLE_TTL_SECS
            })
            .map(|p| p.pid.clone())
            .collect();
        let mut gone = std::collections::HashSet::new();
        for pid in stale {
            if let Some(p) = self.players.remove(&pid) {
                self.names.remove(&name_key(&p.name));
                self.by_auth.remove(&p.auth);
                self.dirty = true;
                gone.insert(pid);
            }
        }
        if !gone.is_empty() {
            for p in self.players.values_mut() {
                p.friends.retain(|f| !gone.contains(f));
                p.freq_in.retain(|f| !gone.contains(f));
                p.freq_out.retain(|f| !gone.contains(f));
                p.invites.retain(|i| !gone.contains(&i.from));
            }
        }
    }

    pub fn to_json(&self) -> String {
        let mut players: Vec<PlayerRec> = self.players.values().cloned().collect();
        players.sort_by(|a, b| a.pid.cmp(&b.pid));
        let mut cups: Vec<Cup> = self.cups.values().cloned().collect();
        cups.sort_by(|a, b| a.code.cmp(&b.code));
        let mut iap: Vec<IapRec> = self.iap.values().cloned().collect();
        iap.sort_by(|a, b| a.tx.cmp(&b.tx));
        serde_json::to_string(&FileData { version: FILE_VERSION, season_id: self.season_id, players, cups, iap }).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let data: FileData = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if !(1..=FILE_VERSION).contains(&data.version) {
            return Err(format!("unsupported data file version {}", data.version));
        }
        let mut s = Self::from_records(data.players, data.season_id);
        s.cups = data.cups.into_iter().map(|c| (c.code.clone(), c)).collect();
        s.iap = data.iap.into_iter().map(|r| (r.tx.clone(), r)).collect();
        Ok(s)
    }

    /// Load from `path`. A missing file is a fresh start; an unreadable or corrupt one is an
    /// error, so a bad file is never silently overwritten.
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Key hash used by tests for an id.
#[cfg(test)]
pub fn test_auth(pid: &str) -> String {
    format!("auth-{pid}")
}

/// Write `json` to `path` atomically: temp file in the same folder, then rename.
pub fn write_atomic(path: &Path, json: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::{PlayKind, DAY};

    const T: Tuning = Tuning { day_offset: 0, chest_every: 100 };

    fn store() -> Store {
        Store::new().with_tuning(T)
    }

    fn ev(kind: PlayKind, won: bool) -> PlayEvent {
        PlayEvent { kind, won, sab: 1, eye: 1, rush: 1, wagers: 3, score: 40, longest: 8 }
    }

    #[test]
    fn names_are_unique_ignoring_case() {
        let mut s = store();
        assert_eq!(s.register("pid-aaaaaaaa", "Maya", 1).as_deref(), Some("Maya"));
        let second = s.register("pid-bbbbbbbb", "maya", 1).unwrap();
        assert_ne!(second.to_lowercase(), "maya");
        assert!(second.starts_with("maya-"));
        assert!(second.chars().count() <= 16);
    }

    #[test]
    fn a_known_account_keeps_its_name_when_the_browser_sends_another() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Maya", 1);
        s.register("pid-bbbbbbbb", "Zed", 1);
        // Reconnecting never renames: that is what the profile message is for.
        assert_eq!(s.register("pid-bbbbbbbb", "Maya", 2).as_deref(), Some("Zed"));
        assert_eq!(s.register("pid-bbbbbbbb", "Zara", 3).as_deref(), Some("Zed"));
        assert_eq!(s.rename("pid-bbbbbbbb", "Zara", 3), Ok("Zara".into()));
        assert!(s.rename("pid-aaaaaaaa", "zara", 4) == Err(RenameError::Taken));
        // The old name is free again.
        assert_eq!(s.rename("pid-aaaaaaaa", "Zed", 5), Ok("Zed".into()));
    }

    #[test]
    fn rename_to_own_name_and_unknown_ids() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Maya", 1);
        assert_eq!(s.rename("pid-aaaaaaaa", "Maya", 2), Ok("Maya".into()));
        // Changing only the letter case of your own name is allowed.
        assert_eq!(s.rename("pid-aaaaaaaa", "MAYA", 3), Ok("MAYA".into()));
        assert_eq!(s.rename("pid-cccccccc", "Nope", 2), Err(RenameError::Unknown));
    }

    #[test]
    fn the_key_decides_who_you_are() {
        let mut s = store();
        let a = s.login("pid-aaaaaaaa", "key-a", "Maya", true, 1).unwrap();
        assert_eq!((a.pid.as_str(), a.name.as_str(), a.created), ("pid-aaaaaaaa", "Maya", true));
        // Same key from a brand-new device (different id, different default name): same account.
        let again = s.login("pid-zzzzzzzz", "key-a", "SwiftOtter123", true, 2).unwrap();
        assert_eq!((again.pid.as_str(), again.name.as_str(), again.created), ("pid-aaaaaaaa", "Maya", false));
        assert_eq!(s.len(), 1);
        // A different key that claims someone else's id does not get their account:
        // it gets a fresh id and nothing of theirs.
        let thief = s.login("pid-aaaaaaaa", "key-thief", "Mallory", true, 3).unwrap();
        assert_ne!(thief.pid, "pid-aaaaaaaa");
        assert!(thief.created);
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().name, "Maya");
        assert!(crate::protocol::clean_pid(&thief.pid).is_some(), "server-made ids are valid ids");
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn restoring_with_a_wrong_code_never_makes_an_account() {
        let mut s = store();
        s.login("pid-aaaaaaaa", "key-a", "Maya", true, 1).unwrap();
        assert_eq!(s.login("pid-new-device", "key-typo", "SwiftOtter123", false, 2), Err(LoginError::Unknown));
        assert_eq!(s.len(), 1, "nothing was created");
        let ok = s.login("pid-new-device", "key-a", "SwiftOtter123", false, 3).unwrap();
        assert_eq!((ok.pid.as_str(), ok.name.as_str(), ok.created), ("pid-aaaaaaaa", "Maya", false));
        // It cannot be used to claim an old, keyless record either.
        let mut old = store();
        old.register("pid-bbbbbbbb", "Old", 1);
        old.get_mut("pid-bbbbbbbb").unwrap().auth.clear();
        let mut s2 = Store::from_json(&old.to_json()).unwrap();
        assert_eq!(s2.login("pid-bbbbbbbb", "key-x", "Old", false, 2), Err(LoginError::Unknown));
    }

    #[test]
    fn records_from_an_older_file_are_claimed_once_by_their_id() {
        let mut old = store();
        old.register("pid-aaaaaaaa", "Maya", 1);
        old.get_mut("pid-aaaaaaaa").unwrap().auth.clear();
        let mut s = Store::from_json(&old.to_json()).unwrap().with_tuning(T);
        let first = s.login("pid-aaaaaaaa", "key-1", "Whatever", true, 2).unwrap();
        assert_eq!((first.pid.as_str(), first.name.as_str(), first.created), ("pid-aaaaaaaa", "Maya", false));
        // Once claimed, another key cannot take it.
        let second = s.login("pid-aaaaaaaa", "key-2", "Whatever", true, 3).unwrap();
        assert_ne!(second.pid, "pid-aaaaaaaa");
    }

    #[test]
    fn ranked_results_move_trophies_and_floor_at_zero() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "A", 1);
        s.register("pid-bbbbbbbb", "B", 1);
        s.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Win(0), 2);
        assert_eq!((s.get("pid-aaaaaaaa").unwrap().tr, s.get("pid-aaaaaaaa").unwrap().w), (30, 1));
        assert_eq!((s.get("pid-bbbbbbbb").unwrap().tr, s.get("pid-bbbbbbbb").unwrap().l), (0, 1));
        s.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Draw, 3);
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().d, 1);
        s.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Win(1), 4);
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().tr, 10);
        assert_eq!(s.get("pid-bbbbbbbb").unwrap().tr, 30);
    }

    #[test]
    fn matches_with_unknown_or_identical_players_are_ignored() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "A", 1);
        s.record_match(["pid-aaaaaaaa", "pid-aaaaaaaa"], Outcome::Win(0), 2);
        s.record_match(["pid-aaaaaaaa", "pid-zzzzzzzz"], Outcome::Win(0), 2);
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().games(), 0);
    }

    #[test]
    fn ranking_orders_and_only_lists_players_who_played() {
        let mut s = store();
        for (pid, name) in [("pid-aaaaaaaa", "Ann"), ("pid-bbbbbbbb", "Bob"), ("pid-cccccccc", "Cy"), ("pid-dddddddd", "Dee")] {
            s.register(pid, name, 1);
        }
        s.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Win(0), 2); // Ann 30, Bob 0
        s.record_match(["pid-cccccccc", "pid-bbbbbbbb"], Outcome::Win(0), 3); // Cy 30, Bob 0
        let r = s.ranking(RankScope::AllTime);
        let names: Vec<&str> = r.rows.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Ann", "Cy", "Bob"]); // tie on 30 and 1 win: name decides
        assert_eq!(r.rank_of("pid-cccccccc").map(|(n, _)| n), Some(2));
        assert!(r.rank_of("pid-dddddddd").is_none());
    }

    #[test]
    fn prune_drops_only_accounts_nobody_uses() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Old", 100);
        s.register("pid-bbbbbbbb", "Played", 100);
        s.register("pid-cccccccc", "Rival", 100);
        s.register("pid-dddddddd", "Backup", 100);
        s.register("pid-eeeeeeee", "Coins", 100);
        s.record_match(["pid-bbbbbbbb", "pid-cccccccc"], Outcome::Win(0), 100);
        s.set_prog("pid-dddddddd", Prog { xp: 500, ..Prog::default() });
        s.get_mut("pid-eeeeeeee").unwrap().coins = 40;
        s.prune(100 + UNPLAYED_TTL_SECS + 1);
        assert!(s.get("pid-aaaaaaaa").is_none(), "never did anything");
        for kept in ["pid-bbbbbbbb", "pid-dddddddd", "pid-eeeeeeee"] {
            assert!(s.get(kept).is_some(), "{kept} has something worth keeping");
        }
        // Its name and key are free again.
        assert_eq!(s.register("pid-ffffffff", "Old", 200_000_000).as_deref(), Some("Old"));
        assert!(s.login("pid-gggggggg", &test_auth("pid-aaaaaaaa"), "New", true, 1).unwrap().created);
        // Even real accounts go after more than a year away.
        s.prune(100 + IDLE_TTL_SECS + 1);
        assert!(s.get("pid-bbbbbbbb").is_none());
    }

    #[test]
    fn registry_refuses_new_keys_when_full() {
        let mut s = store();
        for i in 0..MAX_PLAYERS {
            s.players.insert(format!("p{i}"), PlayerRec::new(format!("p{i}"), format!("n{i}"), String::new(), 1, &T));
        }
        assert!(s.register("pid-newnewnew", "Fresh", 2).is_none());
        // Known players can still connect (a claim of an old record needs no new room).
        assert!(s.login("p5", "key-p5", "n5", true, 2).is_ok());
    }

    #[test]
    fn json_round_trip_and_file_persistence() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Ann", 1);
        s.register("pid-bbbbbbbb", "Bob", 1);
        s.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Win(0), 2);
        s.set_prog("pid-aaaaaaaa", Prog { tr: 90, best: 120, xp: 800, ai: [3, 1, 0], pvp: [1, 0, 0] });
        s.claim_chest("pid-aaaaaaaa", 5).unwrap();
        let back = Store::from_json(&s.to_json()).unwrap();
        assert_eq!(back.get("pid-aaaaaaaa"), s.get("pid-aaaaaaaa"));
        assert_eq!(back.ranking(RankScope::AllTime).rows.len(), 2);
        // Names and keys are re-reserved after loading.
        let mut back = back.with_tuning(T);
        assert_ne!(back.register("pid-cccccccc", "ann", 3).as_deref(), Some("ann"));
        assert_eq!(back.login("pid-new", &test_auth("pid-aaaaaaaa"), "x", true, 3).unwrap().pid, "pid-aaaaaaaa");

        let dir = std::env::temp_dir().join(format!("five-trails-test-{}", std::process::id()));
        let path = dir.join("players.json");
        assert!(Store::load(&path).unwrap().len() == 0, "missing file starts empty");
        write_atomic(&path, &s.to_json()).unwrap();
        assert_eq!(Store::load(&path).unwrap().len(), 2);
        std::fs::write(&path, "not json").unwrap();
        assert!(Store::load(&path).is_err(), "a corrupt file is an error, not a silent reset");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_1_files_still_load() {
        let v1 = r#"{"version":1,"players":[{"pid":"pid-aaaaaaaa","name":"Old","tr":60,"w":3,"l":1,"d":0,"seen":5}]}"#;
        let s = Store::from_json(v1).unwrap();
        let p = s.get("pid-aaaaaaaa").unwrap();
        assert_eq!((p.tr, p.coins, p.auth.as_str()), (60, 0, ""));
        assert!(Store::from_json(r#"{"version":9,"players":[]}"#).is_err());
    }

    #[test]
    fn duplicate_names_and_keys_in_a_file_are_made_unique() {
        let rec = |pid: &str, name: &str, auth: &str| PlayerRec::new(pid.into(), name.into(), auth.into(), 1, &T);
        let s = Store::from_records(vec![rec("pid-aaaaaaaa", "Sam", "k"), rec("pid-bbbbbbbb", "sam", "k")], 0);
        assert_eq!(s.len(), 2);
        assert_ne!(s.get("pid-bbbbbbbb").unwrap().name.to_lowercase(), "sam");
        assert!(s.get("pid-bbbbbbbb").unwrap().auth.is_empty(), "a shared key stays with the first holder");
    }

    #[test]
    fn rewards_only_come_from_the_server_and_chests_pay_once() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Ann", 1000);
        assert_eq!(s.claim_chest("pid-aaaaaaaa", 1000), Ok(CHEST_COINS), "new players start with a chest");
        assert_eq!(s.claim_chest("pid-aaaaaaaa", 1000), Err(ClaimError::NothingToClaim));
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().coins, CHEST_COINS);
        assert_eq!(s.claim_chest("pid-aaaaaaaa", 1000 + 100), Ok(CHEST_COINS), "the next one arrives on time");
        assert_eq!(s.claim_chest("pid-nobody00", 1), Err(ClaimError::Unknown));
    }

    #[test]
    fn battles_pay_coins_up_to_a_daily_cap_and_only_when_counted() {
        let mut s = store();
        let pid = "pid-aaaaaaaa";
        s.register(pid, "Ann", 1000);
        assert_eq!(s.record_play(pid, &ev(PlayKind::Ranked, true), false, false, 1000), 0, "too short to count");
        assert_eq!(s.record_play(pid, &ev(PlayKind::Ranked, true), false, true, 1000), 15);
        assert_eq!(s.record_play(pid, &ev(PlayKind::Ranked, false), false, true, 1000), 5);
        assert_eq!(s.record_play(pid, &ev(PlayKind::Ranked, false), true, true, 1000), 8);
        assert_eq!(s.get(pid).unwrap().coins, 28);
        let mut paid = 28;
        for _ in 0..20 {
            paid += s.record_play(pid, &ev(PlayKind::Ranked, true), false, true, 1000);
        }
        assert_eq!(paid, economy::BATTLE_COINS_PER_DAY, "capped for the day");
        assert_eq!(s.record_play(pid, &ev(PlayKind::Ranked, true), false, true, 1000 + DAY), 15, "a new day pays again");
    }

    #[test]
    fn cups_are_saved_with_the_players_and_survive_a_restart() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Ann", 1);
        s.register("pid-bbbbbbbb", "Bob", 1);
        s.cup_create("pid-aaaaaaaa", "QWERTY".into(), "Fun Cup", Format::Knockout, 300, 1).unwrap();
        s.cup_join("pid-bbbbbbbb", "QWERTY").unwrap();
        let back = Store::from_json(&s.to_json()).unwrap();
        let c = back.cup("QWERTY").expect("cup came back");
        assert_eq!((c.name.as_str(), c.players.len(), c.format), ("Fun Cup", 2, Format::Knockout));
        assert_eq!(back.cups_of("pid-bbbbbbbb").len(), 1);
        let mut old = Store::from_json(r#"{"version":2,"players":[]}"#).unwrap();
        assert!(old.cups_of("x").is_empty(), "files from before cups still load");
        old.prune(10);
    }

    #[test]
    fn a_purchase_pays_once_to_one_account_and_a_refund_takes_it_back() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Ann", 1);
        s.register("pid-bbbbbbbb", "Bob", 1);
        assert_eq!(s.iap_credit("pid-aaaaaaaa", "2000001", "coins_1000", 1000, 5), Credit::Paid(1000));
        assert_eq!(s.iap_credit("pid-aaaaaaaa", "2000001", "coins_1000", 1000, 6), Credit::Already(1000), "replaying it pays nothing");
        assert_eq!(s.iap_credit("pid-bbbbbbbb", "2000001", "coins_1000", 1000, 6), Credit::OtherAccount);
        assert_eq!(s.iap_owner("2000001"), Some("pid-aaaaaaaa"));
        let back = Store::from_json(&s.to_json()).unwrap();
        assert_eq!(back.iap_owner("2000001"), Some("pid-aaaaaaaa"), "the ledger survives a restart");
        s.get_mut("pid-aaaaaaaa").unwrap().coins = 400; // spent some
        assert_eq!(s.iap_revoke("2000001").as_deref(), Some("pid-aaaaaaaa"));
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().coins, 0, "never below zero");
        assert_eq!(s.iap_revoke("2000001"), None, "a refund counts once");
    }

    #[test]
    fn the_shop_takes_coins_once_and_only_lets_you_equip_what_you_have() {
        let mut s = store();
        let pid = "pid-aaaaaaaa";
        s.register(pid, "Ann", 1000);
        assert_eq!(s.buy(pid, "candy"), Err(ShopError::Poor));
        assert_eq!(s.equip(pid, "table", "candy"), Err(ShopError::Poor));
        s.get_mut(pid).unwrap().coins = 700;
        assert_eq!(s.buy(pid, "candy"), Ok(400));
        assert_eq!(s.buy(pid, "candy"), Err(ShopError::Owned));
        assert_eq!(s.buy(pid, "peach"), Err(ShopError::NotForSale), "free items are not sold");
        assert_eq!(s.buy(pid, "summit"), Err(ShopError::NotForSale), "trophy items are not sold");
        assert_eq!(s.buy(pid, "nope"), Err(ShopError::Unknown));
        assert_eq!(s.equip(pid, "table", "candy"), Ok("stripes".to_owned()));
        assert_eq!(s.equip(pid, "back", "candy"), Err(ShopError::Unknown), "a table is not a card back");
        assert_eq!(s.equip(pid, "back", "summit"), Err(ShopError::Locked));
        assert_eq!(s.equip(pid, "back", "dots"), Ok("dots".to_owned()));
        assert_eq!(s.back_of(pid), "dots");
        let me = s.me_json(pid, 0).unwrap();
        assert_eq!((me["coins"].as_u64(), me["shop"]["table"].as_str(), me["shop"]["back"].as_str()), (Some(400), Some("candy"), Some("dots")));
        let have: Vec<&str> = me["shop"]["items"].as_array().unwrap().iter().filter(|i| i["have"] == true).map(|i| i["id"].as_str().unwrap()).collect();
        assert_eq!(have, ["classic", "peach", "slate", "candy", "stripes", "dots", "chevron"]);
        s.get_mut(pid).unwrap().tr = 1400;
        assert_eq!(s.equip(pid, "back", "summit"), Ok("summit".to_owned()));
        s.get_mut(pid).unwrap().tr = 0;
        assert_eq!(s.back_of(pid), "stripes", "a trophy back is not shown once you no longer qualify");
    }

    #[test]
    fn quests_pay_coins_once_and_only_for_counted_battles() {
        let mut s = store();
        let pid = "pid-aaaaaaaa";
        s.register(pid, "Ann", 10 * DAY);
        let now = 10 * DAY;
        // A battle that was quit straight away is a play but moves no quest.
        s.record_play(pid, &ev(PlayKind::Friend, true), false, false, now);
        let me = s.me_json(pid, now).unwrap();
        assert!(me["daily"].as_array().unwrap().iter().all(|q| q["have"] == 0));
        assert_eq!(s.get(pid).unwrap().plays, 1);
        // Two real battles finish every daily quest that a single game ticks.
        s.record_play(pid, &ev(PlayKind::Friend, true), false, true, now);
        s.record_play(pid, &ev(PlayKind::Friend, true), false, true, now);
        let me = s.me_json(pid, now).unwrap();
        let q = me["daily"].as_array().unwrap();
        assert!(q.iter().all(|q| q["have"].as_u64() > Some(0)), "{me}");
        let done = q.iter().find(|q| q["have"] == q["need"]).expect("something is finished");
        let slot = done["slot"].as_str().unwrap().to_owned();
        let coins = done["coins"].as_u64().unwrap();
        assert_eq!(s.claim_quest(pid, &slot, now), Ok(u32::try_from(coins).unwrap()));
        assert_eq!(s.claim_quest(pid, &slot, now), Err(ClaimError::AlreadyClaimed));
        assert_eq!(s.me_json(pid, now).unwrap()["coins"], coins + 30, "plus 15 for each of the two counted wins");
        assert_eq!(s.claim_quest(pid, "d7", now), Err(ClaimError::Unknown));
        // Tomorrow it is all fresh again.
        let next = s.me_json(pid, now + DAY).unwrap();
        assert!(next["daily"].as_array().unwrap().iter().all(|q| q["have"] == 0 && q["claimed"] == false));
    }

    #[test]
    fn me_json_describes_the_whole_lobby_state() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Ann", 1000);
        let me = s.me_json("pid-aaaaaaaa", 1000).unwrap();
        assert_eq!(me["t"], "me");
        assert_eq!((me["chest"]["n"].as_u64(), me["chest"]["max"].as_u64(), me["chest"]["nextIn"].as_u64()), (Some(1), Some(2), Some(100)));
        assert_eq!(me["daily"].as_array().unwrap().len(), 3);
        assert_eq!(me["weekly"].as_array().unwrap().len(), 2);
        assert!(me["season"]["name"].is_string() && me["season"]["endsIn"].as_u64().unwrap() > 0);
        assert!(me["reward"].is_null());
        assert!(!me.to_string().contains("pid-aaaaaaaa"), "no id or key in the payload");
        assert!(s.me_json("pid-nobody00", 1).is_none());
    }

    #[test]
    fn seasons_rank_separately_roll_over_and_pay_coins() {
        let mut s = store();
        let t = s.tuning();
        let oct = u64::try_from(economy::days_from_civil(2026, 10, 10)).unwrap() * DAY;
        let nov = u64::try_from(economy::days_from_civil(2026, 11, 2)).unwrap() * DAY;
        s.rollover(oct);
        assert_eq!(s.season_id(), economy::season_of(oct, &t));
        for (pid, name) in [("pid-aaaaaaaa", "Ann"), ("pid-bbbbbbbb", "Bob"), ("pid-cccccccc", "Cy")] {
            s.register(pid, name, oct);
        }
        // Ann beats Bob five times, Cy beats Bob once.
        for _ in 0..5 {
            s.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Win(0), oct);
        }
        s.record_match(["pid-cccccccc", "pid-bbbbbbbb"], Outcome::Win(0), oct);
        let season = s.ranking(RankScope::Season);
        assert_eq!(season.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Ann", "Cy", "Bob"]);
        assert_eq!(season.rows[0].tr, 150);

        // Ann (rank 1) and Bob (rank 3, 6 games) are paid; Cy has only one game, too few.
        assert_eq!(s.rollover(nov), 2);
        assert_eq!(s.get("pid-cccccccc").unwrap().coins, 0);
        assert_eq!(s.get("pid-bbbbbbbb").unwrap().coins, 300);
    }

    #[test]
    fn the_new_season_starts_empty_while_all_time_carries_on() {
        let mut s = store();
        let t = s.tuning();
        let oct = u64::try_from(economy::days_from_civil(2026, 10, 10)).unwrap() * DAY;
        let nov = u64::try_from(economy::days_from_civil(2026, 11, 2)).unwrap() * DAY;
        s.rollover(oct);
        s.register("pid-aaaaaaaa", "Ann", oct);
        s.register("pid-bbbbbbbb", "Bob", oct);
        for _ in 0..5 {
            s.record_match(["pid-aaaaaaaa", "pid-bbbbbbbb"], Outcome::Win(0), oct);
        }
        // Before the month ends nothing is paid; the same month again is a no-op.
        assert_eq!(s.rollover(oct + DAY), 0);
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().coins, 0);

        let paid = s.rollover(nov);
        assert_eq!(s.season_id(), economy::season_of(nov, &t));
        // Ann: rank 1 with 5 games = 500. Bob: rank 2 with 5 games = 300.
        assert_eq!(paid, 2);
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().coins, 500);
        assert_eq!(s.get("pid-bbbbbbbb").unwrap().coins, 300);
        let r = s.get("pid-aaaaaaaa").unwrap().reward.unwrap();
        assert_eq!((r.rank, r.coins, economy::season_name(r.season).as_str()), (1, 500, "October 2026"));
        let me = s.me_json("pid-aaaaaaaa", nov).unwrap();
        assert_eq!((me["reward"]["rank"].as_u64(), me["reward"]["coins"].as_u64(), me["reward"]["season"].as_str()), (Some(1), Some(500), Some("October 2026")));
        s.ack_season("pid-aaaaaaaa");
        assert!(s.me_json("pid-aaaaaaaa", nov).unwrap()["reward"].is_null());

        assert!(s.ranking(RankScope::Season).rows.is_empty(), "a fresh season board");
        assert_eq!(s.ranking(RankScope::AllTime).rows.len(), 2, "all-time is untouched");
        // The first ranked game of the new season starts that player's season record from zero.
        s.record_match(["pid-bbbbbbbb", "pid-aaaaaaaa"], Outcome::Win(0), nov);
        let r = s.ranking(RankScope::Season);
        assert_eq!((r.rows[0].name.as_str(), r.rows[0].tr, r.rows[0].w), ("Bob", 30, 1));
        assert_eq!(s.get("pid-bbbbbbbb").unwrap().games(), 6, "all-time keeps counting");
        // A clock that jumps backwards never reopens an old season.
        assert_eq!(s.rollover(oct), 0);
        assert_eq!(s.season_id(), economy::season_of(nov, &t));
    }

    #[test]
    fn progress_backup_is_clamped() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Ann", 1);
        s.set_prog("pid-aaaaaaaa", Prog { tr: u32::MAX, best: u32::MAX, xp: u32::MAX, ai: [u32::MAX; 3], pvp: [1, 2, 3] });
        let p = s.prog("pid-aaaaaaaa").unwrap();
        assert_eq!((p.tr, p.xp, p.ai[0], p.pvp), (1_000_000, 100_000_000, 10_000_000, [1, 2, 3]));
        assert!(s.prog("pid-nobody00").is_none());
    }

    #[test]
    fn friends_need_both_sides_and_can_be_undone() {
        let mut s = store();
        for (p, n) in [("pid-aaaaaaaa", "Asha"), ("pid-bbbbbbbb", "Ben"), ("pid-cccccccc", "Cara")] {
            s.register(p, n, 100);
        }
        let (a, b, c) = ("pid-aaaaaaaa", "pid-bbbbbbbb", "pid-cccccccc");
        assert_eq!(s.pid_by_name("bEn"), Some(b), "names are found ignoring case");
        assert_eq!(s.friend_request(a, a), Err(FriendError::Yourself));
        assert_eq!(s.friend_request(a, "pid-nobody00"), Err(FriendError::NotFound));
        assert_eq!(s.friend_request(a, b), Ok(FriendReply::Sent));
        assert_eq!(s.friend_request(a, b), Ok(FriendReply::Sent), "asking twice is harmless");
        assert_eq!(s.get(b).unwrap().freq_in, vec![a.to_owned()]);
        assert!(s.get(a).unwrap().friends.is_empty(), "nothing is shared until accepted");
        assert_eq!(s.friend_accept(c, a), Err(FriendError::NoRequest), "only the one asked can accept");
        assert_eq!(s.friend_accept(b, a), Ok(()));
        assert_eq!((s.get(a).unwrap().friends.clone(), s.get(b).unwrap().friends.clone()), (vec![b.to_owned()], vec![a.to_owned()]));
        assert!(s.get(a).unwrap().freq_out.is_empty() && s.get(b).unwrap().freq_in.is_empty());
        assert_eq!(s.friend_request(b, a), Ok(FriendReply::Already));
        // Two players asking each other become friends straight away.
        assert_eq!(s.friend_request(c, a), Ok(FriendReply::Sent));
        assert_eq!(s.friend_request(a, c), Ok(FriendReply::Accepted));
        assert_eq!(s.get(c).unwrap().friends, vec![a.to_owned()]);
        // Removing works from either side and clears both lists.
        s.friend_drop(b, a);
        assert!(s.get(b).unwrap().friends.is_empty() && !s.get(a).unwrap().friends.contains(&b.to_owned()));
        // Declining a request clears it on both sides.
        s.friend_request(b, c).unwrap();
        s.friend_drop(c, b);
        assert!(s.get(b).unwrap().freq_out.is_empty() && s.get(c).unwrap().freq_in.is_empty());
    }

    #[test]
    fn requests_are_capped_and_deleted_accounts_leave_friend_lists() {
        let mut s = store();
        s.register("pid-target00", "Target", 100);
        for i in 0..MAX_FRIEND_REQUESTS {
            let p = format!("pid-asker{i:03}");
            s.register(&p, &format!("Asker{i}"), 100);
            assert_eq!(s.friend_request(&p, "pid-target00"), Ok(FriendReply::Sent));
        }
        s.register("pid-onemore0", "One More", 100);
        assert_eq!(s.friend_request("pid-onemore0", "pid-target00"), Err(FriendError::TooMany));
        // A friend list keeps an account; an account deleted for being unused leaves every list.
        s.friend_accept("pid-target00", "pid-asker000").unwrap();
        s.prune(100 + UNPLAYED_TTL_SECS + 1);
        assert!(s.get("pid-target00").is_some() && s.get("pid-asker000").is_some(), "friends count as using the account");
        assert!(s.get("pid-asker001").is_none());
        assert!(s.get("pid-target00").unwrap().freq_in.is_empty(), "requests from deleted accounts are gone");
    }

    #[test]
    fn touch_moves_last_active_forward_only() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Asha", 100);
        s.touch("pid-aaaaaaaa", 500);
        s.touch("pid-aaaaaaaa", 300);
        assert_eq!(s.get("pid-aaaaaaaa").unwrap().seen, 500);
    }

    #[test]
    fn invites_need_friends_wait_for_the_player_and_expire() {
        let mut s = store();
        s.register("pid-aaaaaaaa", "Asha", 100);
        s.register("pid-bbbbbbbb", "Ben", 100);
        let (a, b) = ("pid-aaaaaaaa", "pid-bbbbbbbb");
        assert_eq!(s.invite(a, b, 300, 1000), Err(InviteError::NotFriend));
        s.friend_request(a, b).unwrap();
        s.friend_accept(b, a).unwrap();
        assert_eq!(s.invite(a, b, 300, 1000), Ok(()));
        assert_eq!(s.invite(a, b, 300, 1010), Err(InviteError::TooSoon), "no spamming");
        assert_eq!(s.invite(a, b, 600, 1000 + INVITE_COOLDOWN_SECS), Ok(()));
        let list = s.invites_of(b, 1100);
        assert_eq!((list.len(), list[0].clock), (1, 600), "a new invite replaces the old one");
        assert_eq!(s.invite_take(b, a, 1100).map(|i| i.clock), Some(600));
        assert!(s.invite_take(b, a, 1100).is_none(), "taken once");
        s.invite(a, b, 300, 2000).unwrap();
        assert!(s.invites_of(b, 2000 + INVITE_TTL_SECS).is_empty(), "invites expire");
        s.invite(a, b, 300, 3000 + INVITE_TTL_SECS).unwrap();
        s.friend_drop(b, a);
        assert!(s.invites_of(b, 3000 + INVITE_TTL_SECS).is_empty(), "removing a friend removes their invites");
    }
}
