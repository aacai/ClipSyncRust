use serde::{Deserialize, Serialize};

use crate::protocol::PROTOCOL_VERSION;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipEnvelope {
    #[serde(default = "protocol_version")]
    pub v: i32,
    pub mid: String,
    pub ts: i64,
    pub dev: String,
    pub name: String,
    pub kind: String,
    pub ct: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipPayload {
    pub text: String,
    pub len: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePayload {
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
    pub link: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceEnvelope {
    #[serde(default = "protocol_version")]
    pub v: i32,
    pub dev: String,
    pub name: String,
    pub online: bool,
    pub ts: i64,
    pub sp: String,
    pub os: String,
    #[serde(default)]
    pub answer: bool,
}

fn protocol_version() -> i32 {
    PROTOCOL_VERSION
}

impl Default for ClipEnvelope {
    fn default() -> Self {
        Self {
            v: PROTOCOL_VERSION,
            mid: String::new(),
            ts: 0,
            dev: String::new(),
            name: String::new(),
            kind: String::new(),
            ct: String::new(),
        }
    }
}

impl Default for PresenceEnvelope {
    fn default() -> Self {
        Self {
            v: PROTOCOL_VERSION,
            dev: String::new(),
            name: String::new(),
            online: false,
            ts: 0,
            sp: String::new(),
            os: String::new(),
            answer: false,
        }
    }
}
