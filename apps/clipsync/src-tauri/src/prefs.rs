//! 偏好设置与房间会话的对外操作。

use serde::Serialize;

use crate::state::AppState;
use crate::settings::{Language, ThemeMode};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesSnapshot {
    pub theme: ThemeMode,
    pub language: Language,
    pub remember_room: bool,
    pub nickname: String,
    pub watch_clipboard: bool,
    pub auto_copy: bool,
    pub auto_paste_file: bool,
    pub watch_images: bool,
    pub watch_files: bool,
    pub launch_at_login: bool,
    pub delete_expired: bool,
    pub retention_days: u32,
    pub screenshot_hotkey: String,
    pub screenshot_keep_window: bool,
    pub record_hotkey: String,
    pub record_region: bool,
    pub record_mic: bool,
    pub record_mic_device: String,
    pub record_system_audio: bool,
    pub record_long_edge: i32,
    pub record_fps: u32,
    pub last_room_code: Option<String>,
    pub background_path: Option<String>,
    pub background_dim: f32,
}

#[tauri::command]
pub fn prefs_get(state: tauri::State<'_, AppState>) -> PreferencesSnapshot {
    let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    PreferencesSnapshot {
        theme: prefs.theme(),
        language: prefs.language(),
        remember_room: prefs.remember_room(),
        nickname: prefs.nickname().to_string(),
        watch_clipboard: prefs.watch_clipboard(),
        auto_copy: prefs.auto_copy(),
        auto_paste_file: prefs.auto_paste_file(),
        watch_images: prefs.watch_images(),
        watch_files: prefs.watch_files(),
        launch_at_login: prefs.launch_at_login(),
        delete_expired: prefs.delete_expired(),
        retention_days: prefs.retention_days(),
        screenshot_hotkey: prefs.screenshot_hotkey().to_string(),
        screenshot_keep_window: prefs.screenshot_keep_window(),
        record_hotkey: prefs.record_hotkey().to_string(),
        record_region: prefs.record_region(),
        record_mic: prefs.record_mic(),
        record_mic_device: prefs.record_mic_device().to_string(),
        record_system_audio: prefs.record_system_audio(),
        record_long_edge: prefs.record_long_edge(),
        record_fps: prefs.record_fps(),
        last_room_code: prefs.last_room_code().map(|code| code.to_string()),
        background_path: prefs.background_path().map(|path| path.to_string()),
        background_dim: prefs.background_dim(),
    }
}

#[tauri::command]
pub fn prefs_set_theme(state: tauri::State<'_, AppState>, theme: ThemeMode) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_theme(theme);
}

#[tauri::command]
pub fn prefs_set_language(state: tauri::State<'_, AppState>, language: Language) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_language(language);
}

#[tauri::command]
pub fn prefs_set_nickname(state: tauri::State<'_, AppState>, nickname: String) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_nickname(&nickname);
}

#[tauri::command]
pub fn prefs_set_remember_room(state: tauri::State<'_, AppState>, remember_room: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_remember_room(remember_room);
}

#[tauri::command]
pub fn prefs_set_watch_clipboard(state: tauri::State<'_, AppState>, watch_clipboard: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_watch_clipboard(watch_clipboard);
}

#[tauri::command]
pub fn prefs_set_auto_copy(state: tauri::State<'_, AppState>, auto_copy: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_auto_copy(auto_copy);
}

#[tauri::command]
pub fn prefs_set_auto_paste_file(state: tauri::State<'_, AppState>, auto_paste_file: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_auto_paste_file(auto_paste_file);
}

#[tauri::command]
pub fn prefs_set_watch_images(state: tauri::State<'_, AppState>, watch_images: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_watch_images(watch_images);
}

#[tauri::command]
pub fn prefs_set_watch_files(state: tauri::State<'_, AppState>, watch_files: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_watch_files(watch_files);
}

#[tauri::command]
pub fn prefs_set_launch_at_login(state: tauri::State<'_, AppState>, launch_at_login: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_launch_at_login(launch_at_login);
}

#[tauri::command]
pub fn prefs_set_delete_expired(state: tauri::State<'_, AppState>, delete_expired: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_delete_expired(delete_expired);
}

#[tauri::command]
pub fn prefs_set_retention_days(state: tauri::State<'_, AppState>, retention_days: u32) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_retention_days(retention_days);
}

#[tauri::command]
pub fn prefs_set_screenshot_hotkey(state: tauri::State<'_, AppState>, screenshot_hotkey: String) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_screenshot_hotkey(&screenshot_hotkey);
}

#[tauri::command]
pub fn prefs_set_screenshot_keep_window(state: tauri::State<'_, AppState>, screenshot_keep_window: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_screenshot_keep_window(screenshot_keep_window);
}

#[tauri::command]
pub fn prefs_set_record_hotkey(state: tauri::State<'_, AppState>, record_hotkey: String) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_record_hotkey(&record_hotkey);
}

#[tauri::command]
pub fn prefs_set_record_region(state: tauri::State<'_, AppState>, record_region: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_record_region(record_region);
}

#[tauri::command]
pub fn prefs_set_record_mic(state: tauri::State<'_, AppState>, record_mic: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_record_mic(record_mic);
}

#[tauri::command]
pub fn prefs_set_record_mic_device(state: tauri::State<'_, AppState>, record_mic_device: String) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_record_mic_device(&record_mic_device);
}

#[tauri::command]
pub fn prefs_set_record_system_audio(state: tauri::State<'_, AppState>, record_system_audio: bool) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_record_system_audio(record_system_audio);
}

#[tauri::command]
pub fn prefs_set_record_long_edge(state: tauri::State<'_, AppState>, record_long_edge: i32) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_record_long_edge(record_long_edge);
}

#[tauri::command]
pub fn prefs_set_record_fps(state: tauri::State<'_, AppState>, record_fps: u32) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_record_fps(record_fps);
}

#[tauri::command]
pub fn prefs_set_background(state: tauri::State<'_, AppState>, background: Option<String>) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_background(background);
}

#[tauri::command]
pub fn prefs_set_background_dim(state: tauri::State<'_, AppState>, background_dim: f32) {
    let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
    prefs.set_background_dim(background_dim);
}
