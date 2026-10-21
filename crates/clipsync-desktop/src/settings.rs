use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use clipsync_core::crypto::{
    aes_gcm_decrypt, aes_gcm_encrypt, random_iv, sha256, IV_SIZE, TAG_SIZE,
};

const SALT: &[u8] = b"clipsync.room-vault.";
const AAD: &[u8] = b"clipsync.settings";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomSession {
    pub room: String,
    pub password: String,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    room: String,
    vault: String,
}

pub struct Settings {
    path: PathBuf,
    kek: [u8; 32],
    pub session: Option<RoomSession>,
}

impl Settings {
    pub fn open(path: PathBuf, install_id: &str) -> Self {
        let mut seed = SALT.to_vec();
        seed.extend_from_slice(install_id.as_bytes());
        let kek = sha256(&seed);
        let session = read_session(&path, &kek);
        Self { path, kek, session }
    }

    pub fn remember(&mut self, room: &str, password: &str) {
        let room = room.trim();
        if room.is_empty() {
            return;
        }
        self.session = Some(RoomSession {
            room: room.to_string(),
            password: password.to_string(),
        });
        let iv = random_iv();
        let Ok(sealed) = aes_gcm_encrypt(&self.kek, &iv, AAD, password.as_bytes()) else {
            return;
        };
        let mut raw = iv.to_vec();
        raw.extend_from_slice(&sealed);
        let stored = Stored {
            room: room.to_string(),
            vault: hex::encode(raw),
        };
        let Ok(json) = serde_json::to_string(&stored) else {
            return;
        };
        let tmp = self.path.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }

    pub fn forget(&mut self) {
        self.session = None;
        let _ = std::fs::remove_file(&self.path);
    }
}

fn read_session(path: &std::path::Path, kek: &[u8]) -> Option<RoomSession> {
    let stored: Stored = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    if stored.room.trim().is_empty() {
        return None;
    }
    let raw = hex::decode(stored.vault).ok()?;
    if raw.len() < IV_SIZE + TAG_SIZE {
        return None;
    }
    let password = aes_gcm_decrypt(kek, &raw[..IV_SIZE], AAD, &raw[IV_SIZE..]).ok()?;
    Some(RoomSession {
        room: stored.room,
        password: String::from_utf8(password).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::Settings;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("clipsync-settings-{name}.json"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("tmp"));
    }

    #[test]
    fn a_session_survives_a_restart() {
        let path = scratch("roundtrip");
        cleanup(&path);
        {
            let mut settings = Settings::open(path.clone(), "install-1");
            settings.remember("  room-abc ", "s3cret");
        }
        let reopened = Settings::open(path.clone(), "install-1");
        assert_eq!(
            reopened.session,
            Some(super::RoomSession {
                room: "room-abc".to_string(),
                password: "s3cret".to_string(),
            })
        );
        cleanup(&path);
    }

    #[test]
    fn the_password_is_not_written_in_the_clear() {
        let path = scratch("sealed");
        cleanup(&path);
        let mut settings = Settings::open(path.clone(), "install-1");
        settings.remember("room-abc", "plaintext-marker");
        let raw = std::fs::read_to_string(&path).expect("settings file");
        assert!(!raw.contains("plaintext-marker"));
        assert!(raw.contains("room-abc"));
        cleanup(&path);
    }

    #[test]
    fn another_install_cannot_open_the_vault() {
        let path = scratch("foreign");
        cleanup(&path);
        let mut settings = Settings::open(path.clone(), "install-1");
        settings.remember("room-abc", "s3cret");
        let stranger = Settings::open(path.clone(), "install-2");
        assert_eq!(stranger.session, None);
        cleanup(&path);
    }

    #[test]
    fn a_corrupt_file_is_treated_as_no_session() {
        let path = scratch("corrupt");
        cleanup(&path);
        std::fs::write(&path, "{\"room\":\"x\",\"vault\":\"zz\"}").unwrap();
        assert_eq!(Settings::open(path.clone(), "install-1").session, None);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::open(path.clone(), "install-1").session, None);
        cleanup(&path);
    }

    #[test]
    fn forget_removes_both_memory_and_disk() {
        let path = scratch("forget");
        cleanup(&path);
        let mut settings = Settings::open(path.clone(), "install-1");
        settings.remember("room-abc", "s3cret");
        settings.forget();
        assert!(settings.session.is_none());
        assert!(!path.exists());
        cleanup(&path);
    }

    #[test]
    fn a_blank_room_is_never_saved() {
        let path = scratch("blank");
        cleanup(&path);
        let mut settings = Settings::open(path.clone(), "install-1");
        settings.remember("   ", "s3cret");
        assert!(settings.session.is_none());
        assert!(!path.exists());
        cleanup(&path);
    }
}
