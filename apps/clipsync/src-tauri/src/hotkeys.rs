//! 全局快捷键：截屏与录屏的组合键。

use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::AppState;

pub const SCREENSHOT_CHANNEL: &str = "clipsync://screenshot";

/// 把偏好里存的组合键（如 "Alt+A"）注册成全局快捷键。
pub fn register_all(app: &AppHandle) {
    let (screenshot, record) = {
        let state = app.state::<AppState>();
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        (
            prefs.screenshot_hotkey().to_string(),
            prefs.record_hotkey().to_string(),
        )
    };
    let _ = app.global_shortcut().unregister_all();
    register(app, &screenshot, Hotkey::Screenshot);
    register(app, &record, Hotkey::Record);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    Screenshot,
    Record,
}

fn register(app: &AppHandle, spec: &str, action: Hotkey) {
    let spec = spec.trim();
    if spec.is_empty() {
        return;
    }
    let Ok(shortcut) = spec.parse::<Shortcut>() else {
        return;
    };
    if shortcut.mods.is_empty() {
        return;
    }
    let _ = app.global_shortcut().on_shortcut(shortcut, move |app, _shortcut, event| {
        if event.state() != ShortcutState::Pressed {
            return;
        }
        match action {
            Hotkey::Screenshot => {
                if let Err(error) = run_screenshot(app) {
                    crate::notify(app, format!("截屏失败：{error}"));
                }
            }
            Hotkey::Record => run_record(app),
        }
    });
}

/// 截屏：抓屏、弹遮罩窗框选、贴进剪贴板、进历史、进房间。
pub fn run_screenshot(app: &AppHandle) -> Result<(), String> {
    crate::picker::picker_start(app.clone())
}

/// 抓整屏，必要时先把主窗藏起来，免得把自己拍进去。
pub fn capture_frame_bytes(app: &AppHandle) -> Result<Vec<u8>, String> {
    let keep_window = {
        let state = app.state::<AppState>();
        let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.screenshot_keep_window()
    };
    let window = app.get_webview_window("main");
    if !keep_window {
        if let Some(window) = &window {
            let _ = window.hide();
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let captured = crate::screenshot::capture_primary();
    if !keep_window {
        if let Some(window) = &window {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    captured.map_err(|error| {
        format!("{error}（macOS 首次截屏要在 系统设置 → 隐私与安全性 → 屏幕与系统音频录制 里勾上 ClipSync）")
    })
}

/// 一张 PNG 进剪贴板、进历史、进房间。
pub fn apply_png(app: &AppHandle, png: Vec<u8>) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
        let _ = watcher.write_image(&png);
    }
    let stamp = clipsync_core::protocol::now_millis();
    let name = format!("截图-{stamp}.png");
    let hash = clipsync_core::crypto::sha256_hex(&png);
    crate::services::record_captured_image(app, name, "image/png".to_string(), png, hash);
    Ok(())
}

/// 抓整屏并给前端一张 data URL，框选用。
#[tauri::command]
pub fn capture_frame(app: tauri::AppHandle) -> Result<String, String> {
    use base64::Engine as _;
    let png = capture_frame_bytes(&app)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    ))
}

/// 前端裁好的图，存进剪贴板、历史和房间。
#[tauri::command]
pub fn apply_screenshot(app: tauri::AppHandle, data_url: String) -> Result<(), String> {
    apply_png_from_data_url(&app, &data_url)
}

/// data URL 形式的 PNG，落地。
pub fn apply_png_from_data_url(app: &AppHandle, data_url: &str) -> Result<(), String> {
    use base64::Engine as _;
    let payload = data_url
        .split_once(',')
        .map(|(_, body)| body)
        .ok_or_else(|| "这不是一张图".to_string())?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|error| error.to_string())?;
    apply_png(app, png)?;
    Ok(())
}

/// 录屏：开录或收尾。
pub fn run_record(app: &AppHandle) {
    crate::record_cmd::toggle(app);
}

#[tauri::command]
pub fn capture_screenshot(app: tauri::AppHandle) -> Result<(), String> {
    run_screenshot(&app)
}
