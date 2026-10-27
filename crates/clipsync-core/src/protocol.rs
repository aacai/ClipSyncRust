use crate::crypto::sha256;

pub const PROTOCOL_VERSION: i32 = 1;
pub const ROOT: &str = "cs/v1";
pub const CH_CLIP: &str = "clip";
pub const CH_PRESENCE: &str = "presence";
pub const CH_SYNC: &str = "sync";
pub const KIND_TEXT: &str = "text";
pub const KIND_FILE: &str = "file";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Clip,
    Presence,
    Sync,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clip => CH_CLIP,
            Self::Presence => CH_PRESENCE,
            Self::Sync => CH_SYNC,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomTopic {
    pub room_hash: String,
    pub channel: Channel,
    pub device_id: Option<String>,
}

pub fn room_hash(room_id: &str) -> String {
    sha256_hex_prefix(room_id.trim().as_bytes(), 8)
}

pub fn clip_topic(hash: &str) -> String {
    format!("{ROOT}/{hash}/{CH_CLIP}")
}

pub fn presence_topic(hash: &str, device_id: &str) -> String {
    format!("{ROOT}/{hash}/{CH_PRESENCE}/{device_id}")
}

pub fn sync_topic(hash: &str) -> String {
    format!("{ROOT}/{hash}/{CH_SYNC}")
}

pub fn room_filter(hash: &str) -> String {
    format!("{ROOT}/{hash}/#")
}

pub fn client_id(hash: &str, device_id: &str) -> String {
    format!("cs-{hash}-{device_id}")
}

pub fn parse_topic(topic: &str) -> Option<RoomTopic> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() <= 3 {
        return None;
    }
    if parts[0] != "cs" || parts[1] != "v1" {
        return None;
    }
    let room_hash = parts[2].to_string();
    let channel = match parts[3] {
        CH_CLIP if parts.len() == 4 => Channel::Clip,
        CH_SYNC if parts.len() == 4 => Channel::Sync,
        CH_PRESENCE if parts.len() == 5 => Channel::Presence,
        _ => return None,
    };
    let device_id = (channel == Channel::Presence).then(|| parts[4].to_string());
    Some(RoomTopic {
        room_hash,
        channel,
        device_id,
    })
}

pub fn safety_number(room_key: &[u8]) -> String {
    sha256(room_key)
        .iter()
        .take(6)
        .map(|b| format!("{:03}", *b as u32 % 1000))
        .collect::<Vec<_>>()
        .join(" ")
}

/// kotlinx 的 String.length 是 UTF-16 码元数，网页端与 Android 都按这个长度显示。
pub fn utf16_len(text: &str) -> usize {
    text.chars()
        .map(|c| if c as u32 <= 0xFFFF { 1 } else { 2 })
        .sum()
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

pub fn sha256_hex_prefix(data: &[u8], hex_chars: usize) -> String {
    hex::encode(&sha256(data)[..hex_chars / 2])
}

/// mid 在信封里是明文：用内容算 id 等于让中转服务器拿一句猜测反查是谁发的。
pub fn random_mid() -> String {
    let mut raw = [0u8; 8];
    getrandom::fill(&mut raw).expect("os random unavailable");
    hex::encode(raw)
}
