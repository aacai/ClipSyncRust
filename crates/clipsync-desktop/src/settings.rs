use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use clipsync_core::crypto::{
    aes_gcm_decrypt, aes_gcm_encrypt, random_iv, sha256, IV_SIZE, TAG_SIZE,
};

const SALT: &[u8] = b"clipsync.room-vault.";
const AAD: &[u8] = b"clipsync.settings";

const MAX_NICKNAME: usize = 24;
pub const RETENTION_CHOICES: [u32; 4] = [7, 30, 90, 365];
const DEFAULT_RETENTION_DAYS: u32 = 30;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    System,
    Chinese,
    English,
}

impl Language {
    pub fn resolved(self) -> Self {
        match self {
            Self::System => match sys_locale::get_locale() {
                Some(tag) if tag.to_ascii_lowercase().starts_with("en") => Self::English,
                _ => Self::Chinese,
            },
            other => other,
        }
    }
}

pub fn normalize_nickname(raw: &str) -> String {
    raw.lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_NICKNAME)
        .collect()
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct StoredPreferences {
    #[serde(deserialize_with = "de_theme")]
    theme: ThemeMode,
    #[serde(deserialize_with = "de_language")]
    language: Language,
    remember_room: bool,
    nickname: String,
    watch_clipboard: bool,
    auto_copy: bool,
    watch_images: bool,
    watch_files: bool,
    launch_at_login: bool,
    delete_expired: bool,
    retention_days: u32,
    screenshot_hotkey: String,
    /// 对齐 Compose `screenshotKeepWindow`：默认保留，关掉才在抓屏前藏主窗。
    screenshot_keep_window: bool,
    last_room_code: Option<String>,
}

fn de_theme<'de, D>(deserializer: D) -> Result<ThemeMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer).unwrap_or_default();
    Ok(match raw.as_str() {
        "Light" => ThemeMode::Light,
        "Dark" => ThemeMode::Dark,
        _ => ThemeMode::System,
    })
}

fn de_language<'de, D>(deserializer: D) -> Result<Language, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer).unwrap_or_default();
    Ok(match raw.as_str() {
        "Chinese" => Language::Chinese,
        "English" => Language::English,
        _ => Language::System,
    })
}

impl Default for StoredPreferences {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: Language::System,
            remember_room: true,
            nickname: String::new(),
            watch_clipboard: true,
            auto_copy: true,
            watch_images: true,
            watch_files: true,
            launch_at_login: false,
            delete_expired: false,
            retention_days: DEFAULT_RETENTION_DAYS,
            screenshot_hotkey: crate::screenshot::DEFAULT_HOTKEY.to_string(),
            screenshot_keep_window: true,
            last_room_code: None,
        }
    }
}

pub struct Preferences {
    path: PathBuf,
    stored: StoredPreferences,
}

impl Preferences {
    pub fn open(path: PathBuf) -> Self {
        let stored = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<StoredPreferences>(&raw).ok())
            .unwrap_or_default();
        Self { path, stored }
    }

    pub fn theme(&self) -> ThemeMode {
        self.stored.theme
    }

    pub fn language(&self) -> Language {
        self.stored.language
    }

    pub fn remember_room(&self) -> bool {
        self.stored.remember_room
    }

    pub fn nickname(&self) -> &str {
        &self.stored.nickname
    }

    pub fn watch_clipboard(&self) -> bool {
        self.stored.watch_clipboard
    }

    pub fn auto_copy(&self) -> bool {
        self.stored.auto_copy
    }

    pub fn watch_images(&self) -> bool {
        self.stored.watch_images
    }

    pub fn watch_files(&self) -> bool {
        self.stored.watch_files
    }

    pub fn launch_at_login(&self) -> bool {
        self.stored.launch_at_login
    }

    pub fn delete_expired(&self) -> bool {
        self.stored.delete_expired
    }

    pub fn retention_days(&self) -> u32 {
        self.stored.retention_days
    }

    pub fn screenshot_hotkey(&self) -> &str {
        &self.stored.screenshot_hotkey
    }

    pub fn screenshot_keep_window(&self) -> bool {
        self.stored.screenshot_keep_window
    }

    pub fn last_room_code(&self) -> Option<&str> {
        self.stored
            .last_room_code
            .as_deref()
            .filter(|code| !code.is_empty())
    }

    pub fn edit(&mut self, change: impl FnOnce(&mut Preferences)) {
        change(self);
        self.save();
    }

    pub fn set_theme(&mut self, theme: ThemeMode) {
        self.stored.theme = theme;
        self.save();
    }

    pub fn set_language(&mut self, language: Language) {
        self.stored.language = language;
        self.save();
    }

    pub fn set_remember_room(&mut self, remember: bool) {
        self.stored.remember_room = remember;
        self.save();
    }

    pub fn set_nickname(&mut self, name: &str) {
        self.stored.nickname = normalize_nickname(name);
        self.save();
    }

    pub fn set_watch_clipboard(&mut self, enabled: bool) {
        self.stored.watch_clipboard = enabled;
        self.save();
    }

    pub fn set_auto_copy(&mut self, enabled: bool) {
        self.stored.auto_copy = enabled;
        self.save();
    }

    pub fn set_watch_images(&mut self, enabled: bool) {
        self.stored.watch_images = enabled;
        self.save();
    }

    pub fn set_watch_files(&mut self, enabled: bool) {
        self.stored.watch_files = enabled;
        self.save();
    }

    pub fn set_launch_at_login(&mut self, enabled: bool) {
        self.stored.launch_at_login = enabled;
        self.save();
    }

    pub fn set_delete_expired(&mut self, enabled: bool) {
        self.stored.delete_expired = enabled;
        self.save();
    }

    pub fn set_retention_days(&mut self, days: u32) {
        self.stored.retention_days = days;
        self.save();
    }

    pub fn set_screenshot_hotkey(&mut self, spec: &str) {
        self.stored.screenshot_hotkey = spec.to_string();
        self.save();
    }

    pub fn set_screenshot_keep_window(&mut self, enabled: bool) {
        self.stored.screenshot_keep_window = enabled;
        self.save();
    }

    pub fn remember_last_room(&mut self, code: &str) {
        let code = code.trim();
        if code.is_empty() {
            return;
        }
        self.stored.last_room_code = Some(code.to_string());
        self.save();
    }

    fn save(&self) {
        let Ok(json) = serde_json::to_string(&self.stored) else {
            return;
        };
        let tmp = self.path.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

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

    fn prefs(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("clipsync-prefs-{name}.json"))
    }

    #[test]
    fn preferences_default_to_the_compose_values() {
        let path = prefs("defaults");
        cleanup(&path);
        let loaded = super::Preferences::open(path.clone());
        assert_eq!(loaded.theme(), super::ThemeMode::System);
        assert_eq!(loaded.language(), super::Language::System);
        assert!(loaded.remember_room());
        assert!(loaded.watch_clipboard());
        assert!(loaded.auto_copy());
        assert!(loaded.watch_images());
        assert!(loaded.watch_files());
        assert!(!loaded.launch_at_login());
        assert!(!loaded.delete_expired());
        assert_eq!(loaded.retention_days(), 30);
        assert_eq!(loaded.screenshot_hotkey(), "Alt+A");
        assert_eq!(loaded.nickname(), "");
        assert_eq!(loaded.last_room_code(), None);
        cleanup(&path);
    }

    #[test]
    fn preferences_survive_a_restart() {
        let path = prefs("roundtrip");
        cleanup(&path);
        {
            let mut loaded = super::Preferences::open(path.clone());
            loaded.set_theme(super::ThemeMode::Light);
            loaded.set_language(super::Language::English);
            loaded.set_remember_room(false);
            loaded.set_nickname("  Desk  ");
            loaded.set_watch_clipboard(false);
            loaded.set_auto_copy(false);
            loaded.set_watch_images(false);
            loaded.set_watch_files(false);
            loaded.set_launch_at_login(true);
            loaded.set_delete_expired(true);
            loaded.set_retention_days(7);
            loaded.set_screenshot_hotkey("Control+Shift+S");
            loaded.remember_last_room(" ROOM-1 ");
        }
        let loaded = super::Preferences::open(path.clone());
        assert_eq!(loaded.theme(), super::ThemeMode::Light);
        assert_eq!(loaded.language(), super::Language::English);
        assert!(!loaded.remember_room());
        assert_eq!(loaded.nickname(), "Desk");
        assert!(!loaded.watch_clipboard());
        assert!(!loaded.auto_copy());
        assert!(!loaded.watch_images());
        assert!(!loaded.watch_files());
        assert!(loaded.launch_at_login());
        assert!(loaded.delete_expired());
        assert_eq!(loaded.retention_days(), 7);
        assert_eq!(loaded.screenshot_hotkey(), "Control+Shift+S");
        assert_eq!(loaded.last_room_code(), Some("ROOM-1"));
        assert!(!path.with_extension("tmp").exists());
        cleanup(&path);
    }

    #[test]
    fn an_unknown_theme_or_language_falls_back_to_system() {
        let path = prefs("unknown");
        cleanup(&path);
        std::fs::write(
            &path,
            r#"{"theme":"Sepia","language":"Klingon","watchImages":false}"#,
        )
        .unwrap();
        let loaded = super::Preferences::open(path.clone());
        assert_eq!(loaded.theme(), super::ThemeMode::System);
        assert_eq!(loaded.language(), super::Language::System);
        assert!(loaded.watch_images());
        // 老文件里没有这个键，得按 compose 的默认值来，不能当成用户关过
        assert!(loaded.auto_copy());
        assert_eq!(loaded.screenshot_hotkey(), "Alt+A");
        cleanup(&path);
    }

    #[test]
    fn a_corrupt_preferences_file_falls_back_to_defaults() {
        let path = prefs("corrupt");
        cleanup(&path);
        std::fs::write(&path, "not json").unwrap();
        let loaded = super::Preferences::open(path.clone());
        assert_eq!(loaded.theme(), super::ThemeMode::System);
        assert_eq!(loaded.retention_days(), 30);
        cleanup(&path);
    }

    #[test]
    fn nicknames_keep_one_short_trimmed_line() {
        assert_eq!(super::normalize_nickname("  zhiqiu  "), "zhiqiu");
        assert_eq!(super::normalize_nickname("first\nsecond"), "first");
        assert_eq!(
            super::normalize_nickname("abcdefghijklmnopqrstuvwxyz0123"),
            "abcdefghijklmnopqrstuvwx"
        );
        assert_eq!(super::normalize_nickname("   "), "");
    }
}
