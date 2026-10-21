//! Wire format. Every client message is a small JSON object tagged by `t`.
//! Anything that fails to parse or validate is rejected; nothing from a client
//! is trusted beyond the shape checked here.

use serde::Deserialize;
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 2048;
pub const ROOM_CODE_LEN: usize = 6;
/// Unambiguous letters only (no I, L, O).
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
        tr: u32,
        lv: u32,
        #[serde(default)]
        resume: Option<String>,
    },
    Ping,
    /// Update name and trophy count between matches (they change on the player's side).
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
}

impl Profile {
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "pid": self.pid, "tr": self.trophies, "lv": self.level })
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

pub fn clean_code(raw: &str) -> Option<String> {
    let up = raw.trim().to_ascii_uppercase();
    let ok = up.len() == ROOM_CODE_LEN && up.bytes().all(|b| ROOM_ALPHABET.contains(&b));
    ok.then_some(up)
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
        assert_eq!(clean_code("abcdef"), Some("ABCDEF".into()));
        assert!(clean_code("ABCDEI").is_none());
        assert!(clean_code("ABC").is_none());
    }
}
