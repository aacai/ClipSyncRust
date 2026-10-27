use crate::crypto::sha256_hex;
use crate::room::{RoomCrypto, RoomError};

pub const MAX_FILE_BYTES: i64 = 50 * 1024 * 1024;
pub const MAX_NAME_LENGTH: usize = 120;

pub fn max_mb() -> i64 {
    MAX_FILE_BYTES / 1024 / 1024
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    TooLarge,
    Empty,
    InvalidName,
}

pub fn validate_outgoing_file(name: &str, size: i64) -> Option<RejectReason> {
    if size <= 0 {
        return Some(RejectReason::Empty);
    }
    if size > MAX_FILE_BYTES {
        return Some(RejectReason::TooLarge);
    }
    if sanitize_file_name(name).is_empty() {
        return Some(RejectReason::InvalidName);
    }
    None
}

#[derive(Debug, Clone)]
pub struct EncryptedFile {
    pub cipher: Vec<u8>,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FileCryptoError {
    #[error("room crypto rejected the file: {0}")]
    Room(#[from] RoomError),
    #[error("sha256 mismatch")]
    ShaMismatch,
    #[error("size mismatch")]
    SizeMismatch,
}

pub fn file_aad(topic: &str, name: &str, mime: &str, size: i64, sha256: &str) -> Vec<u8> {
    format!("{topic}\n{name}\n{mime}\n{size}\n{sha256}").into_bytes()
}

/// 文件字节用 topic + 元数据做 AAD：换房间搬运、改大小或改名都会在 GCM 阶段失败。
pub fn encrypt_file(
    room: &RoomCrypto,
    topic: &str,
    name: &str,
    mime: &str,
    plain: &[u8],
) -> Result<EncryptedFile, RoomError> {
    let sha = sha256_hex(plain);
    let aad = file_aad(topic, name, mime, plain.len() as i64, &sha);
    Ok(EncryptedFile {
        cipher: room.encrypt_bytes(plain, &aad)?,
        name: name.to_string(),
        mime: mime.to_string(),
        size: plain.len() as i64,
        sha256: sha,
    })
}

pub fn decrypt_file(
    room: &RoomCrypto,
    topic: &str,
    name: &str,
    mime: &str,
    size: i64,
    sha256: &str,
    cipher: &[u8],
) -> Result<Vec<u8>, FileCryptoError> {
    let aad = file_aad(topic, name, mime, size, sha256);
    let plain = room.decrypt_bytes(cipher, &aad)?;
    if sha256_hex(&plain) != sha256 {
        return Err(FileCryptoError::ShaMismatch);
    }
    if plain.len() as i64 != size {
        return Err(FileCryptoError::SizeMismatch);
    }
    Ok(plain)
}

pub fn upload_file_name() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut raw = [0u8; 16];
    getrandom::fill(&mut raw).expect("os random unavailable");
    let body: String = raw
        .iter()
        .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
        .collect();
    format!("cs-{body}.bin")
}

pub fn sanitize_file_name(raw: &str) -> String {
    let base = raw.rsplit('/').next().unwrap_or("");
    let base = base.rsplit('\\').next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|ch| {
            if (ch as u32) < 0x20 || "\"*/:<>?\\|".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .chars()
        .take(MAX_NAME_LENGTH)
        .collect();
    let stem = cleaned
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(cleaned.as_str());
    const RESERVED: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if RESERVED.contains(&stem.to_lowercase().as_str()) {
        format!("_{cleaned}")
    } else {
        cleaned
    }
}

pub fn numbered_name(name: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    for index in 1..1000 {
        let candidate = match ext {
            Some(ext) => format!("{stem} ({index}).{ext}"),
            None => format!("{stem} ({index})"),
        };
        if !exists(&candidate) {
            return candidate;
        }
    }
    name.to_string()
}

pub fn mime_for_name(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "xml" => "text/xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "dmg" => "application/x-apple-diskimage",
        "apk" => "application/vnd.android.package-archive",
        _ => "application/octet-stream",
    }
}

pub fn ext_for_mime(mime: &str) -> String {
    match mime.to_lowercase().as_str() {
        "image/jpeg" => "jpg".into(),
        "image/png" => "png".into(),
        "image/gif" => "gif".into(),
        "image/webp" => "webp".into(),
        "image/bmp" => "bmp".into(),
        "image/heic" => "heic".into(),
        "image/svg+xml" => "svg".into(),
        other => {
            let sub = other
                .rsplit_once('/')
                .map(|(_, sub)| sub)
                .unwrap_or(other)
                .split(';')
                .next()
                .unwrap_or("");
            if (2..=5).contains(&sub.len()) {
                sub.to_string()
            } else {
                "bin".into()
            }
        }
    }
}

pub fn is_image(mime: &str) -> bool {
    mime.starts_with("image/")
}

pub fn human_size(size: i64) -> String {
    if size < 1024 {
        return format!("{size} B");
    }
    if size < 1024 * 1024 {
        return format!("{} KB", dec(size * 10 / 1024));
    }
    if size < 1024 * 1024 * 1024 {
        return format!("{} MB", dec(size * 10 / (1024 * 1024)));
    }
    format!("{} GB", dec(size * 10 / (1024 * 1024 * 1024)))
}

fn dec(tenths: i64) -> String {
    format!("{}.{}", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = "TESTROOM01";
    const TOPIC: &str = "cs/v1/61232506/clip";
    const PAYLOAD: &[u8] = "ClipSync 文件内容 📡\nsecond line".as_bytes();

    #[test]
    fn roundtrip_preserves_content() {
        let alice = RoomCrypto::open(ROOM, "").unwrap();
        let bob = RoomCrypto::open(ROOM, "").unwrap();
        let enc = encrypt_file(&alice, TOPIC, "shot.png", "image/png", PAYLOAD).unwrap();
        assert_eq!(enc.size, PAYLOAD.len() as i64);
        assert_eq!(enc.name, "shot.png");
        assert_eq!(enc.sha256.len(), 64);
        let plain = decrypt_file(
            &bob,
            TOPIC,
            &enc.name,
            &enc.mime,
            enc.size,
            &enc.sha256,
            &enc.cipher,
        )
        .unwrap();
        assert_eq!(plain, PAYLOAD);
    }

    #[test]
    fn tampered_cipher_fails() {
        let room = RoomCrypto::open(ROOM, "").unwrap();
        let enc = encrypt_file(&room, TOPIC, "a.bin", "application/octet-stream", PAYLOAD).unwrap();
        let mut cipher = enc.cipher.clone();
        let middle = cipher.len() / 2;
        cipher[middle] ^= 0xFF;
        assert!(decrypt_file(
            &room,
            TOPIC,
            &enc.name,
            &enc.mime,
            enc.size,
            &enc.sha256,
            &cipher
        )
        .is_err());
    }

    #[test]
    fn wrong_room_password_fails() {
        let alice = RoomCrypto::open(ROOM, "pw1").unwrap();
        let mallory = RoomCrypto::open(ROOM, "pw2").unwrap();
        let enc =
            encrypt_file(&alice, TOPIC, "a.bin", "application/octet-stream", PAYLOAD).unwrap();
        assert!(decrypt_file(
            &mallory,
            TOPIC,
            &enc.name,
            &enc.mime,
            enc.size,
            &enc.sha256,
            &enc.cipher
        )
        .is_err());
    }

    #[test]
    fn metadata_substitution_fails() {
        let room = RoomCrypto::open(ROOM, "").unwrap();
        let enc = encrypt_file(&room, TOPIC, "real.png", "image/png", PAYLOAD).unwrap();
        assert!(decrypt_file(
            &room,
            TOPIC,
            "fake.png",
            &enc.mime,
            enc.size,
            &enc.sha256,
            &enc.cipher
        )
        .is_err());
        assert!(decrypt_file(
            &room,
            TOPIC,
            &enc.name,
            &enc.mime,
            enc.size + 1,
            &enc.sha256,
            &enc.cipher
        )
        .is_err());
    }

    #[test]
    fn another_room_cannot_replay_the_cipher() {
        let room = RoomCrypto::open(ROOM, "").unwrap();
        let enc = encrypt_file(&room, TOPIC, "a.bin", "application/octet-stream", PAYLOAD).unwrap();
        assert!(decrypt_file(
            &room,
            "cs/v1/otherhash/clip",
            &enc.name,
            &enc.mime,
            enc.size,
            &enc.sha256,
            &enc.cipher
        )
        .is_err());
    }

    #[test]
    fn empty_file_roundtrip() {
        let room = RoomCrypto::open(ROOM, "").unwrap();
        let enc = encrypt_file(&room, TOPIC, "empty.txt", "text/plain", b"").unwrap();
        let plain = decrypt_file(
            &room,
            TOPIC,
            &enc.name,
            &enc.mime,
            0,
            &enc.sha256,
            &enc.cipher,
        )
        .unwrap();
        assert!(plain.is_empty());
    }

    #[test]
    fn uploaded_name_is_random_and_opaque() {
        let a = upload_file_name();
        let b = upload_file_name();
        assert!(a.starts_with("cs-") && a.ends_with(".bin"));
        assert!(a.len() <= 40);
        assert_ne!(a, b);
        assert!(a
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '.'));
    }

    #[test]
    fn file_aad_pins_topic_and_metadata() {
        assert_eq!(
            String::from_utf8(file_aad("t", "a.png", "image/png", 7, "ab")).unwrap(),
            "t\na.png\nimage/png\n7\nab"
        );
    }

    #[test]
    fn limits_and_validation_match_kotlin() {
        assert_eq!(max_mb(), 50);
        assert_eq!(
            validate_outgoing_file("a.png", 0),
            Some(RejectReason::Empty)
        );
        assert_eq!(
            validate_outgoing_file("a.png", MAX_FILE_BYTES + 1),
            Some(RejectReason::TooLarge)
        );
        assert_eq!(validate_outgoing_file("a.png", 1024), None);
        assert_eq!(
            validate_outgoing_file("../../../", 10),
            Some(RejectReason::InvalidName)
        );
        assert_eq!(validate_outgoing_file("照片.png", 10), None);
    }

    #[test]
    fn sanitize_matches_the_compose_vectors() {
        assert_eq!(sanitize_file_name("../../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("a/b\\c.png"), "c.png");
        assert_eq!(sanitize_file_name("/tmp/evil/script.sh"), "script.sh");
        assert!(!sanitize_file_name("..").contains('.'));
        assert_eq!(sanitize_file_name("con.txt"), "_con.txt");
        assert_eq!(sanitize_file_name("LPT1.dat"), "_LPT1.dat");
        assert_eq!(sanitize_file_name("a/b:c.txt"), "b_c.txt");
        assert_eq!(sanitize_file_name("we:ird*name"), "we_ird_name");
        assert_eq!(sanitize_file_name("tab\tname"), "tab_name");
        assert_eq!(sanitize_file_name("   "), "");
        assert_eq!(sanitize_file_name("file."), "file");
        assert_eq!(sanitize_file_name(&"x".repeat(500)).len(), 120);
    }

    #[test]
    fn numbered_name_inserts_before_the_extension() {
        assert_eq!(numbered_name("a.png", |_| false), "a.png");
        assert_eq!(numbered_name("a.png", |n| n == "a.png"), "a (1).png");
        assert_eq!(
            numbered_name("a.png", |n| n == "a.png" || n == "a (1).png"),
            "a (2).png"
        );
        assert_eq!(numbered_name("noext", |n| n == "noext"), "noext (1)");
        assert_eq!(numbered_name("a.b.png", |n| n == "a.png"), "a.b.png");
    }

    #[test]
    fn mime_detection_covers_common_types() {
        assert_eq!(mime_for_name("a.PNG"), "image/png");
        assert_eq!(mime_for_name("a.jpeg"), "image/jpeg");
        assert_eq!(mime_for_name("a.pdf"), "application/pdf");
        assert_eq!(mime_for_name("a.txt"), "text/plain");
        assert_eq!(mime_for_name("a.unknown"), "application/octet-stream");
        assert_eq!(mime_for_name("noext"), "application/octet-stream");
        assert!(is_image("image/png"));
        assert!(!is_image("video/mp4"));
        assert_eq!(ext_for_mime("image/jpeg"), "jpg");
        assert_eq!(ext_for_mime("image/svg+xml"), "svg");
        assert_eq!(ext_for_mime("application/octet-stream"), "bin");
        assert_eq!(ext_for_mime("x/y"), "bin");
    }

    #[test]
    fn human_size_matches_compose() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(human_size(1536 * 1024 * 1024), "1.5 GB");
    }
}
