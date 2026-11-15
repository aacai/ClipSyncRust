//! 引擎事件转发：把引擎产生的事件转成前端能订阅的载荷。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use clipsync_core::engine::{Clip, Event, Peer, Status};

pub const EVENT_CHANNEL: &str = "clipsync://engine";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineEvent {
    pub kind: &'static str,
    pub status: Option<Status>,
    pub clip: Option<Clip>,
    pub peers: Option<Vec<Peer>>,
    pub key_mismatch: Option<Vec<String>>,
    pub file_too_large: Option<String>,
}

impl From<Event> for EngineEvent {
    fn from(event: Event) -> Self {
        match event {
            Event::Status(status) => EngineEvent {
                kind: "status",
                status: Some(status),
                clip: None,
                peers: None,
                key_mismatch: None,
                file_too_large: None,
            },
            Event::Clip(clip) => EngineEvent {
                kind: "clip",
                status: None,
                clip: Some(clip),
                peers: None,
                key_mismatch: None,
                file_too_large: None,
            },
            Event::Peers(peers) => EngineEvent {
                kind: "peers",
                status: None,
                clip: None,
                peers: Some(peers),
                key_mismatch: None,
                file_too_large: None,
            },
            Event::KeyMismatch(names) => EngineEvent {
                kind: "keyMismatch",
                status: None,
                clip: None,
                peers: None,
                key_mismatch: Some(names),
                file_too_large: None,
            },
            Event::FileTooLarge(name) => EngineEvent {
                kind: "fileTooLarge",
                status: None,
                clip: None,
                peers: None,
                key_mismatch: None,
                file_too_large: Some(name),
            },
        }
    }
}

pub fn emit(app: &AppHandle, event: Event) {
    let _ = app.emit(EVENT_CHANNEL, EngineEvent::from(event));
}
