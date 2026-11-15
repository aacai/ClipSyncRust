//! 收到房间消息后的本地处理。

use clipsync_core::engine::Event;
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::store::Record;

pub fn handle(app: &AppHandle, event: Event) {
    let Event::Clip(clip) = &event else {
        return;
    };
    let state = app.state::<AppState>();

    let mine = clip.mine;
    let sender = if clip.sender_name.trim().is_empty() {
        "同屋设备".to_string()
    } else {
        clip.sender_name.clone()
    };
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let added = history.insert(Record::from_clip(clip));
    history.flush();
    drop(history);

    if mine {
        return;
    }

    if let Some(file) = &clip.file {
        let link = file.link.trim();
        if link.is_empty() {
            return;
        }
        crate::file_jobs::auto_download(
            app,
            clip.mid.clone(),
            file.name.clone(),
            file.mime.clone(),
            file.size,
            file.sha256.clone(),
            file.link.clone(),
        );
        return;
    }

    let auto_copy = {
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.auto_copy()
    };
    if !auto_copy {
        if added.inserted {
            crate::notify(app, format!("收到 {sender} 的文本，已存进列表"));
        }
        return;
    }

    let text = clip.text.clone();
    let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
    let placed = watcher.write(&text);
    drop(watcher);
    if added.inserted {
        if placed {
            crate::notify(app, format!("收到 {sender} 的文本，已放进剪贴板"));
        } else {
            crate::notify(app, format!("收到 {sender} 的文本，写入剪贴板失败"));
        }
    }
}
