//! 应用级命令：退出、窗口显隐与置顶、房间码生成、邀请链接解析、背景图与内存占用。

use base64::Engine;
use serde::Serialize;
use tauri::Manager;

use crate::meminfo;
use crate::paths;
use crate::state::AppState;

const BACKGROUND_ID: &str = "background";
const MAX_BACKGROUND_DIM: f32 = 0.9;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteLink {
    pub room: String,
    pub password: Option<String>,
}

#[tauri::command]
pub fn app_quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn app_hide(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn app_show(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
pub fn app_activate(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    activate_ns_app();
}

#[tauri::command]
pub fn window_always_on_top(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("找不到主窗口")?;
    window.set_always_on_top(on).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_click_through(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("找不到主窗口")?;
    window
        .set_ignore_cursor_events(on)
        .map_err(|error| error.to_string())?;
    set_window_alpha(&window, if on { 0.0 } else { 1.0 })
}

#[tauri::command]
pub fn random_room_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";
    let mut bytes = [0u8; 6];
    if getrandom::fill(&mut bytes).is_err() {
        return "K7M2QP".to_string();
    }
    bytes
        .iter()
        .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
        .collect()
}

#[tauri::command]
pub fn sanitize_room_code(raw: String) -> String {
    sanitize_room(&raw)
}

#[tauri::command]
pub fn parse_invite_link(link: String) -> Option<InviteLink> {
    parse_invite(&link)
}

#[tauri::command]
pub fn memory_usage() -> u64 {
    meminfo::resident().unwrap_or(0)
}

#[tauri::command]
pub fn background_set(state: tauri::State<'_, AppState>, path: String) -> Result<String, String> {
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    if !clipsync_core::files::is_image(clipsync_core::files::mime_for_name(&name)) {
        return Err("只能选图片当背景".to_string());
    }
    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    if size == 0 || size as i64 > clipsync_core::files::MAX_FILE_BYTES {
        return Err("图片太大，换一张小点的".to_string());
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("读不了这张图：{error}"))?;
    let stored = paths::write_cache(BACKGROUND_ID, &name, &bytes)
        .map_err(|error| format!("背景图存不下来：{error}"))?;
    let previous = {
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.background_path().map(str::to_string)
    };
    {
        let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.set_background(Some(stored.clone()));
    }
    if let Some(previous) = previous {
        paths::delete_cache(Some(&previous));
    }
    Ok(stored)
}

#[tauri::command]
pub fn background_clear(state: tauri::State<'_, AppState>) {
    let previous = {
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.background_path().map(str::to_string)
    };
    {
        let mut prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.set_background(None);
    }
    if let Some(previous) = previous {
        paths::delete_cache(Some(&previous));
    }
}

#[tauri::command]
pub fn background_data_url(state: tauri::State<'_, AppState>) -> Option<String> {
    let path = {
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.background_path().map(str::to_string)
    }?;
    let bytes = std::fs::read(&path).ok()?;
    let mime = clipsync_core::files::mime_for_name(
        &std::path::Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
    );
    Some(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
pub fn max_background_dim() -> f32 {
    MAX_BACKGROUND_DIM
}

fn sanitize_room(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .map(|ch| ch.to_ascii_uppercase())
        .take(24)
        .collect()
}

fn parse_invite(raw: &str) -> Option<InviteLink> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let rest = if let Some(rest) = trimmed.strip_prefix("oink://") {
        rest
    } else if let Some(index) = trimmed.find("://") {
        let scheme = trimmed[..index].to_ascii_lowercase();
        if !matches!(scheme.as_str(), "https" | "http" | "clipsync") {
            return None;
        }
        &trimmed[index + 3..]
    } else {
        trimmed
    };
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let room_raw = match segments.first().copied().unwrap_or_default() {
        "room" | "join" | "r" => segments.get(1).copied()?,
        _ => path.trim_matches('/'),
    };
    let room = sanitize_room(room_raw);
    if room.is_empty() {
        return None;
    }
    let password = query.and_then(|query| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            if key.eq_ignore_ascii_case("key")
                || key.eq_ignore_ascii_case("password")
                || key.eq_ignore_ascii_case("pwd")
            {
                Some(percent_decode(value))
            } else {
                None
            }
        })
    });
    Some(InviteLink { room, password })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(target_os = "macos")]
pub fn activate_ns_app() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(marker);
    unsafe {
        let _: bool = objc2::msg_send![&*app, activateIgnoringOtherApps: true];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate_ns_app() {}

#[cfg(target_os = "macos")]
fn set_window_alpha(window: &tauri::WebviewWindow, alpha: f64) -> Result<(), String> {
    use objc2::rc::Retained;
    use objc2_app_kit::NSWindow;

    let pointer = window.ns_window().map_err(|error| error.to_string())?;
    let retained = unsafe { Retained::retain(pointer.cast::<NSWindow>()) }.ok_or("拿不到窗口句柄")?;
    retained.setAlphaValue(alpha);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn set_window_alpha(_window: &tauri::WebviewWindow, _alpha: f64) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_invite, sanitize_room};

    #[test]
    fn room_codes_are_uppercased_and_filtered() {
        assert_eq!(sanitize_room(" ab-12_c "), "AB-12_C");
        assert_eq!(sanitize_room("房间 ab"), "AB");
        assert_eq!(
            sanitize_room("abcdefghijklmnopqrstuvwxyz012345"),
            "ABCDEFGHIJKLMNOPQRSTUVWX"
        );
    }

    #[test]
    fn invite_links_carry_the_room_and_the_password() {
        let invite = parse_invite("oink://room/7kq29wxm?key=secret").expect("invite");
        assert_eq!(invite.room, "7KQ29WXM");
        assert_eq!(invite.password.as_deref(), Some("secret"));

        let plain = parse_invite("oink://room/ABCD").expect("invite");
        assert_eq!(plain.room, "ABCD");
        assert!(plain.password.is_none());
    }

    #[test]
    fn invite_links_accept_https_and_reject_other_schemes() {
        assert_eq!(
            parse_invite("https://clipsync.app/room/ABCD?password=p%20w")
                .expect("invite")
                .password
                .as_deref(),
            Some("p w")
        );
        assert!(parse_invite("ftp://room/ABCD").is_none());
        assert!(parse_invite("").is_none());
        assert!(parse_invite("oink://room/").is_none());
    }
}
