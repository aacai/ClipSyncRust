//! 应用运行期状态：历史记录、偏好设置、剪贴板监听与同步引擎。

use std::sync::{Arc, Mutex};

use clipsync_core::engine::Command;
use clipsync_core::RoomCrypto;
use tokio::sync::mpsc;

use crate::clipboard::ClipboardWatcher;
use crate::paths;
use crate::settings::{Preferences, Settings};
use crate::store::History;

#[derive(Clone)]
pub struct RoomContext {
    pub room: String,
    pub hash: String,
    pub topic: String,
    pub crypto: Arc<RoomCrypto>,
}

pub struct AppState {
    pub history: Mutex<History>,
    pub prefs: Mutex<Preferences>,
    pub settings: Mutex<Settings>,
    pub clipboard: Mutex<ClipboardWatcher>,
    pub sender: Mutex<Option<mpsc::UnboundedSender<Command>>>,
    pub room: Mutex<Option<RoomContext>>,
}

impl AppState {
    pub fn new() -> Self {
        let dir = paths::app_dir();
        let install_id = paths::install_id();
        Self {
            history: Mutex::new(History::open(dir.join("history.json"))),
            prefs: Mutex::new(Preferences::open(dir.join("preferences.json"))),
            settings: Mutex::new(Settings::open(dir.join("settings.json"), &install_id)),
            clipboard: Mutex::new(ClipboardWatcher::new()),
            sender: Mutex::new(None),
            room: Mutex::new(None),
        }
    }

    pub fn enter_room(&self, room: &str, password: &str) -> Result<(), String> {
        let crypto = RoomCrypto::open(room, password).map_err(|e| e.to_string())?;
        let hash = clipsync_core::protocol::room_hash(room);
        let topic = clipsync_core::protocol::clip_topic(&hash);
        *self.room.lock().unwrap_or_else(|e| e.into_inner()) = Some(RoomContext {
            room: room.to_string(),
            hash,
            topic,
            crypto: Arc::new(crypto),
        });
        Ok(())
    }

    pub fn leave_room(&self) {
        *self.room.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}
