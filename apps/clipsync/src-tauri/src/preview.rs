//! 录像回放窗：单独开一个置顶小窗播 mp4，主窗继续干别的。

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

pub const PREVIEW_LABEL: &str = "preview";

static PATH: Mutex<Option<String>> = Mutex::new(None);
static NAME: Mutex<Option<String>> = Mutex::new(None);

/// 打开回放窗。
#[tauri::command]
pub fn preview_open(app: AppHandle, path: String, name: String) -> Result<(), String> {
    {
        let mut slot = PATH.lock().unwrap_or_else(|error| error.into_inner());
        *slot = Some(path);
    }
    {
        let mut slot = NAME.lock().unwrap_or_else(|error| error.into_inner());
        *slot = Some(name);
    }
    if let Some(existing) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = existing.close();
    }
    let window = tauri::WebviewWindowBuilder::new(
        &app,
        PREVIEW_LABEL,
        tauri::WebviewUrl::App("preview.html".into()),
    )
    .title("回放")
    .inner_size(720.0, 460.0)
    .min_inner_size(420.0, 280.0)
    .always_on_top(true)
    .resizable(true)
    .build()
    .map_err(|error| error.to_string())?;
    let _ = window.set_focus();
    Ok(())
}

/// 回放窗要播的文件。
#[tauri::command]
pub fn preview_file() -> (Option<String>, Option<String>) {
    let path = PATH
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let name = NAME
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    (path, name)
}

/// 关掉回放窗。
#[tauri::command]
pub fn preview_close(app: AppHandle) {
    if let Some(window) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = window.close();
    }
}
