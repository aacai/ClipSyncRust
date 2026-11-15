//! 框选遮罩窗：抓屏后弹一层置顶全屏窗，框完把裁好的图直接存进剪贴板、历史和房间。
//!
//! 旧版是把这一层画在 egui 的浮窗里；换到 Tauri 就是单独开一个 webview 窗口，
//! 全屏、置顶、无边框，主窗同时隐藏，免得把自己也框进去。

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::state::AppState;

pub const PICKER_LABEL: &str = "picker";

static FRAME: Mutex<Option<String>> = Mutex::new(None);

/// 抓屏 → 弹遮罩窗。一步到位。
#[tauri::command]
pub fn picker_start(app: AppHandle) -> Result<(), String> {
    let keep = {
        let state = app.state::<AppState>();
        let prefs = state.prefs.lock().unwrap_or_else(|error| error.into_inner());
        prefs.screenshot_keep_window()
    };
    let window = app.get_webview_window("main");
    if !keep {
        if let Some(window) = &window {
            let _ = window.hide();
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let png = match crate::screenshot::capture_primary() {
        Ok(png) => png,
        Err(error) => {
            restore(&app);
            return Err(error);
        }
    };
    use base64::Engine as _;
    let frame = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );
    match picker_open(app.clone(), frame) {
        Ok(()) => Ok(()),
        Err(error) => {
            restore(&app);
            Err(error)
        }
    }
}

/// 遮罩窗关掉后把主窗放回来。
fn restore(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 打开遮罩窗。`frame` 是整屏 PNG 的 data URL。
#[tauri::command]
pub fn picker_open(app: AppHandle, frame: String) -> Result<(), String> {
    {
        let mut slot = FRAME.lock().unwrap_or_else(|error| error.into_inner());
        *slot = Some(frame);
    }
    if let Some(existing) = app.get_webview_window(PICKER_LABEL) {
        let _ = existing.close();
    }
    let window = tauri::WebviewWindowBuilder::new(
        &app,
        PICKER_LABEL,
        tauri::WebviewUrl::App("picker.html".into()),
    )
    .title("框选")
    .inner_size(1280.0, 800.0)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .build()
    .map_err(|error| error.to_string())?;
    let _ = window.set_fullscreen(true);
    let _ = window.set_focus();
    Ok(())
}

/// 遮罩窗要显示的冻结帧。
#[tauri::command]
pub fn picker_frame() -> Option<String> {
    FRAME
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// 框完了：裁好的图直接落地。
#[tauri::command]
pub fn picker_result(app: AppHandle, data_url: String) -> Result<(), String> {
    close(&app);
    restore(&app);
    crate::hotkeys::apply_png_from_data_url(&app, &data_url)?;
    crate::notify(&app, "已截屏并同步".to_string());
    Ok(())
}

/// 取消了。
#[tauri::command]
pub fn picker_cancel(app: AppHandle) {
    close(&app);
    restore(&app);
}

fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PICKER_LABEL) {
        let _ = window.close();
    }
}

/// 屏幕上其它应用的窗口位置，框选时用来吸附。
#[tauri::command]
pub fn snap_windows() -> Vec<WindowRect> {
    crate::windows::snap()
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
