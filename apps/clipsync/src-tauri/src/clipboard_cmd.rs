//! 剪贴板读写的对外操作。

use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardRead {
    pub text: Option<String>,
}

#[tauri::command]
pub fn clipboard_read_text(state: tauri::State<'_, AppState>) -> ClipboardRead {
    let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
    ClipboardRead {
        text: watcher.read_once(),
    }
}

#[tauri::command]
pub fn clipboard_write_text(state: tauri::State<'_, AppState>, text: String) -> bool {
    let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
    watcher.write(&text)
}

#[tauri::command]
pub fn clipboard_write_files(state: tauri::State<'_, AppState>, paths: Vec<String>) -> bool {
    let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
    watcher.write_files(&paths)
}

#[tauri::command]
pub fn clipboard_write_image(state: tauri::State<'_, AppState>, path: String) -> Result<bool, String> {
    let bytes = std::fs::read(&path).map_err(|error| format!("读不了这张图：{error}"))?;
    let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
    watcher.write_image(&bytes).map(|()| true)
}

#[tauri::command]
pub fn clipboard_reseed(state: tauri::State<'_, AppState>) {
    let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
    watcher.reseed();
}
