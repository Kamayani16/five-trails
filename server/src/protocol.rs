//! Wire format. Every client message is a small JSON object tagged by `t`.
//! Anything that fails to parse or validate is rejected; nothing from a client
//! is trusted beyond the shape checked here.

use serde::Deserialize;
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: u32 = 2;
pub const MAX_MESSAGE_BYTES: usize = 2048;
pub const ROOM_CODE_LEN: usize = 5;
/// Letters used for random room codes: unambiguous only (no I, L, O). A code the host picks
/// themselves may use any letter A-Z.
pub const ROOM_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ";
pub const CLOCK_CHOICES: [u32; 4] = [0, 180, 300, 600];
pub const EMOTE_COUNT: u8 = 6;

#[derive(Deserialize, Debug)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMsg {
    Hello {
        v: u32,
        name: String,
        pid: String,
        /// The player's secret account key (see `clean_key`). Missing from version 1 clients,
        /// which are told to reload.
        #[serde(default)]
        key: String,
        /// True when the player typed a recovery code to get an existing account back: the
        /// server must find it and may not make a new one.
        #[serde(default)]
        restore: bool,
        tr: u32,
        lv: u32,
        #[serde(default)]
        resume: Option<String>,
    },
    Ping,
    /// Ask for a leaderboard (all-time, or this season's) and this player's place on it.
    Leaderboard {
        #[serde(default)]
        season: bool,
    },
    /// Ask for the rewards screen data (`me`) again.
    Me,
    /// Back up this browser's lobby progress on the server (unverified, for restoring on a new device).
    Sync {
        tr: u32,
        best: u32,
        xp: u32,
        ai: [u32; 3],
        pvp: [u32; 3],
    },
    ClaimChest,
    /// Collect a finished quest: "d0".."d2" (daily) or "w0", "w1" (weekly).
    ClaimQuest {
        slot: String,
    },
    /// Spend coins on a shop item.
    Buy {
        item: String,
    },
    /// Use an owned item: `kind` is "table" or "back".
    Equip {
        kind: String,
        item: String,
    },
    /// Say the season-end reward has been seen.
    AckSeason,
    /// Cups (private tournaments for up to six players).
    CupCreate {
        name: String,
        format: String,
        clock: u32,
    },
    CupJoin {
        code: String,
    },
    CupLeave {
        code: String,
    },
    CupStart {
        code: String,
    },
    CupList,
    CupGet {
        code: String,
    },
    /// Play cup match `m`: starts it if the opponent is waiting, otherwise wait for them.
    CupPlay {
        code: String,
        m: usize,
    },
    /// Watch cup match `m` while it is being played.
    Watch {
        code: String,
        m: usize,
    },
    Unwatch,
    /// While waiting in the quick-match queue: play the computer instead.
    Bot,
    /// Rename and update the trophy count shown to opponents. The server answers a name
    /// change with `name` (accepted) or `error name_taken`.
    Profile {
        name: String,
        tr: u32,
        lv: u32,
    },
    Queue {
        clock: u32,
    },
    Unqueue,
    Create {
        clock: u32,
        /// A code the host chose (5 letters). Omit for a random one.
        #[serde(default)]
        code: Option<String>,
    },
    CancelRoom,
    Join {
        code: String,
    },
    Move {
        seq: u32,
        a: Action,
        #[serde(default)]
        id: Option<u8>,
        #[serde(default)]
        pile: Option<i8>,
    },
    Emote {
        k: u8,
    },
    Forfeit,
    Rematch,
    LeaveMatch,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Play,
    Disc,
    Draw,
    Sab,
    Eye,
    Rush,
}

/// What a player tells the server about themselves. Self-reported and unverified:
/// used only for display and to pair players of similar trophy counts.
#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub pid: String,
    pub trophies: u32,
    pub level: u32,
    /// The card back this player has equipped (their rival sees it).
    pub back: String,
}

impl Profile {
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "pid": self.pid, "tr": self.trophies, "lv": self.level, "back": self.back })
    }
}

/// Letters, digits, spaces and `-_.'`; 1..=16 characters after collapsing whitespace.
pub fn clean_name(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = collapsed.chars().count();
    if n == 0 || n > 16 {
        return None;
    }
    if !collapsed.chars().all(|c| c.is_alphanumeric() || " -_.'".contains(c)) {
        return None;
    }
    Some(collapsed)
}

/// A browser-generated id (UUID or similar): 8..=64 ASCII letters, digits and dashes.
pub fn clean_pid(raw: &str) -> Option<String> {
    let ok = (8..=64).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    ok.then(|| raw.to_owned())
}

/// A room code as typed by a person: exactly 5 letters, any case. Random codes only use
/// `ROOM_ALPHABET`, but a host-chosen one may use any letter.
/// A cup code: 6 letters.
pub fn clean_cup_code(raw: &str) -> Option<String> {
    let up = raw.trim().to_ascii_uppercase();
    let ok = up.len() == crate::cup::CUP_CODE_LEN && up.bytes().all(|b| b.is_ascii_uppercase());
    ok.then_some(up)
}

pub fn clean_code(raw: &str) -> Option<String> {
    let up = raw.trim().to_ascii_uppercase();
    let ok = up.len() == ROOM_CODE_LEN && up.bytes().all(|b| b.is_ascii_uppercase());
    ok.then_some(up)
}

/// Letters and digits used in account keys: 32 symbols with no 0/O or 1/I, so a key can be
/// read out loud or copied by hand.
pub const KEY_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const KEY_LEN: usize = 20;

/// An account key as typed by a person: 20 symbols from `KEY_ALPHABET`, in any case, with
/// dashes or spaces allowed anywhere. Returns the plain upper-case form.
pub fn clean_key(raw: &str) -> Option<String> {
    let key: String = raw.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_ascii_uppercase();
    let ok = key.len() == KEY_LEN && key.bytes().all(|b| KEY_ALPHABET.contains(&b));
    ok.then_some(key)
}

/// What the server stores instead of a key: SHA-256, lower-case hex. Keys have 100 bits of
/// entropy, so a fast hash is enough; it only has to keep a leaked data file from being a
/// list of working keys.
pub fn hash_key(key: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(key.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn clean_token(raw: &str) -> Option<&str> {
    let ok = raw.len() == 64 && raw.bytes().all(|b| b.is_ascii_hexdigit());
    ok.then_some(raw)
}

pub fn profile_from_hello(name: &str, pid: &str, tr: u32, lv: u32) -> Option<Profile> {
    Some(Profile {
        name: clean_name(name)?,
        pid: clean_pid(pid)?,
        trophies: tr.min(1_000_000),
        level: lv.clamp(1, 10_000),
        back: crate::economy::DEFAULT_BACK.to_owned(),
    })
}

pub fn msg(v: Value) -> String {
    v.to_string()
}

pub fn error(code: &str) -> String {
    msg(json!({ "t": "error", "code": code }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_messages_and_rejects_unknown_fields() {
        let m: ClientMsg = serde_json::from_str(r#"{"t":"move","seq":4,"a":"draw","pile":-1}"#).unwrap();
        assert!(matches!(m, ClientMsg::Move { seq: 4, a: Action::Draw, pile: Some(-1), .. }));
        assert!(serde_json::from_str::<ClientMsg>(r#"{"t":"queue","clock":300,"x":1}"#).is_err());
        assert!(serde_json::from_str::<ClientMsg>(r#"{"t":"admin"}"#).is_err());
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"leaderboard"}"#), Ok(ClientMsg::Leaderboard { season: false })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"leaderboard","season":true}"#), Ok(ClientMsg::Leaderboard { season: true })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"claim_chest"}"#), Ok(ClientMsg::ClaimChest)));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"buy","item":"candy"}"#), Ok(ClientMsg::Buy { .. })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"cup_create","name":"Sat","format":"rr","clock":300}"#), Ok(ClientMsg::CupCreate { .. })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"cup_play","code":"ABCDEF","m":2}"#), Ok(ClientMsg::CupPlay { m: 2, .. })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"cup_list"}"#), Ok(ClientMsg::CupList)));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"watch","code":"ABCDEF","m":0}"#), Ok(ClientMsg::Watch { .. })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"unwatch"}"#), Ok(ClientMsg::Unwatch)));
        assert_eq!(clean_cup_code(" abcdef "), Some("ABCDEF".into()));
        assert_eq!(clean_cup_code("ABCDE"), None);
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"equip","kind":"back","item":"dots"}"#), Ok(ClientMsg::Equip { .. })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"claim_quest","slot":"d1"}"#), Ok(ClientMsg::ClaimQuest { .. })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"bot"}"#), Ok(ClientMsg::Bot)));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"sync","tr":1,"best":2,"xp":3,"ai":[1,2,3],"pvp":[0,0,0]}"#), Ok(ClientMsg::Sync { xp: 3, .. })));
        assert!(serde_json::from_str::<ClientMsg>(r#"{"t":"sync","tr":1,"best":2,"xp":3,"ai":[1,2],"pvp":[0,0,0]}"#).is_err());
        // A version 1 hello (no key) still parses, so it can be told to reload.
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"hello","v":1,"name":"A","pid":"pid-aaaaaaaa","tr":0,"lv":1}"#), Ok(ClientMsg::Hello { v: 1, .. })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"create","clock":300}"#), Ok(ClientMsg::Create { code: None, .. })));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"create","clock":0,"code":"tiger"}"#), Ok(ClientMsg::Create { code: Some(_), .. })));
        assert!(serde_json::from_str::<ClientMsg>(r#"{"t":"move","seq":1,"a":"cheat"}"#).is_err());
    }

    #[test]
    fn validates_names_ids_and_codes() {
        assert_eq!(clean_name("  Kamayani   S "), Some("Kamayani S".into()));
        assert_eq!(clean_name("Ana-María"), Some("Ana-María".into()));
        assert!(clean_name("<script>").is_none());
        assert!(clean_name("").is_none());
        assert!(clean_name("abcdefghijklmnopq").is_none());
        assert!(clean_name("zero\u{200b}width").is_none());
        assert!(clean_pid("123e4567-e89b-12d3-a456-426614174000").is_some());
        assert!(clean_pid("short").is_none());
        assert!(clean_pid("has space in it!!").is_none());
        assert_eq!(clean_code("abcde"), Some("ABCDE".into()));
        assert_eq!(clean_code(" tiger "), Some("TIGER".into())); // host-chosen codes may use I, L, O
        assert!(clean_code("ABCDEF").is_none()); // too long
        assert!(clean_code("ABCD").is_none()); // too short
        assert!(clean_code("ABC1E").is_none());
        assert!(clean_code("ÅBCDE").is_none());
    }

    #[test]
    fn account_keys_are_forgiving_to_type_and_strictly_checked() {
        assert_eq!(clean_key("abcde-fghjk-mnpqr-stuvw").as_deref(), Some("ABCDEFGHJKMNPQRSTUVW"));
        assert_eq!(clean_key(" ABCDE FGHJK MNPQR STUVW ").as_deref(), Some("ABCDEFGHJKMNPQRSTUVW"));
        assert_eq!(clean_key("22222-33333-44444-55555").as_deref(), Some("22222333334444455555"));
        assert!(clean_key("ABCDE-FGHJK-MNPQR-STUV").is_none(), "too short");
        assert!(clean_key("ABCDE-FGHJK-MNPQR-STUVWX").is_none(), "too long");
        assert!(clean_key("ABCDE-FGHJK-MNPQR-STUV0").is_none(), "no zero");
        assert!(clean_key("ABCDE-FGHJK-MNPQR-STUVO").is_none(), "no letter O");
        assert!(clean_key("ÅBCDE-FGHJK-MNPQR-STUVW").is_none());
        assert!(clean_key("").is_none());
    }

    #[test]
    fn key_hash_is_sha256_hex() {
        // Known SHA-256 test vector.
        assert_eq!(hash_key("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_ne!(hash_key("ABCDEFGHJKMNPQRSTUVW"), hash_key("ABCDEFGHJKMNPQRSTUVX"));
    }
}
