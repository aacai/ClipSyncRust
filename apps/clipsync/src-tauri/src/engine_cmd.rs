//! 房间连接与引擎状态。

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tokio::sync::mpsc;

use clipsync_core::engine::{Command, Config, Status};

use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub status: Status,
    pub room: Option<String>,
    pub nickname: String,
    pub safety: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequest {
    pub room: String,
    #[serde(default)]
    pub password: String,
}

#[tauri::command]
pub fn engine_snapshot(state: tauri::State<'_, AppState>) -> EngineSnapshot {
    let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    let settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    EngineSnapshot {
        status: Status::Idle,
        room: settings.session.as_ref().map(|session| session.room.clone()),
        nickname: prefs.nickname().to_string(),
        safety: String::new(),
    }
}

#[tauri::command]
pub fn engine_join(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: JoinRequest,
) -> Result<(), String> {
    let room = request.room.trim().to_string();
    if room.is_empty() {
        return Err("房间码不能为空".into());
    }
    let (device_id, device_name, device_os) = {
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        let name = if prefs.nickname().is_empty() {
            crate::paths::device_name()
        } else {
            prefs.nickname().to_string()
        };
        (
            crate::paths::install_id(),
            name,
            crate::paths::device_os().to_string(),
        )
    };
    let sender = ensure_engine(&app, &state)?;
    let config = Config {
        room_id: room.clone(),
        password: request.password.clone(),
        room_key: None,
        device_id,
        device_name,
        device_os,
    };
    let _ = sender.send(Command::Join(config));
    state.enter_room(&room, &request.password)?;
    let remember = {
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.remember_room()
    };
    if remember {
        let mut settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
        settings.remember(&room, &request.password);
        let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.remember_last_room(&room);
    }
    Ok(())
}

#[tauri::command]
pub fn engine_leave(state: tauri::State<'_, AppState>) {
    if let Some(sender) = state.sender.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = sender.send(Command::Leave);
    }
    let mut settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    settings.forget();
    state.leave_room();
}

#[tauri::command]
pub fn engine_retry(state: tauri::State<'_, AppState>) {
    if let Some(sender) = state.sender.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = sender.send(Command::RetryNow);
    }
}

#[tauri::command]
pub fn engine_rename(state: tauri::State<'_, AppState>, name: String) {
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    {
        let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.set_nickname(&name);
    }
    if let Some(sender) = state.sender.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = sender.send(Command::Rename(name));
    }
}

#[tauri::command]
pub fn engine_publish_text(state: tauri::State<'_, AppState>, text: String) {
    if let Some(sender) = state.sender.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = sender.send(Command::PublishText(text));
    }
}

pub fn restore_session(app: &AppHandle, state: &tauri::State<'_, AppState>) {
    let (room, password) = {
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        if !prefs.remember_room() {
            return;
        }
        let settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
        match &settings.session {
            Some(session) => (session.room.clone(), session.password.clone()),
            None => return,
        }
    };
    let Ok(sender) = ensure_engine(app, state) else {
        return;
    };
    let config = Config {
        room_id: room.clone(),
        password: password.clone(),
        room_key: None,
        device_id: crate::paths::install_id(),
        device_name: crate::paths::device_name(),
        device_os: crate::paths::device_os().to_string(),
    };
    let _ = sender.send(Command::Join(config));
    let _ = state.enter_room(&room, &password);
}

fn ensure_engine(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
) -> Result<mpsc::UnboundedSender<Command>, String> {
    let mut slot = state.sender.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        let handle = clipsync_core::engine::spawn_with(clipsync_core::mqtt::BrokerLinks, |fut| {
            tauri::async_runtime::spawn(fut);
        });
        let (sender, mut events) = handle.split();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = events.recv().await {
                crate::receive::handle(&app, event.clone());
                crate::engine_event::emit(&app, event);
            }
        });
        *slot = Some(sender);
    }
    slot.clone().ok_or_else(|| "引擎启动失败".to_string())
}
