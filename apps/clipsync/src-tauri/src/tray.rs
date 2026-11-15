//! 菜单栏 / 系统托盘：关窗后进程继续守剪贴板，点托盘再把窗口抬起来。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

const SHOW_ID: &str = "clipsync-show";
const QUIT_ID: &str = "clipsync-quit";

pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, SHOW_ID, "显示窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_ID, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id("clipsync-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("ClipSync")
        .on_menu_event(|app, event| match event.id.as_ref() {
            SHOW_ID => show_window(app),
            QUIT_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });

    // 托盘槽位只有 16~24px，拿 256 的原图过去会被系统缩成一团糊。
    if let Some(icon) = tray_icon() {
        builder = builder.icon(icon);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// 托盘用的小图标。
fn tray_icon() -> Option<tauri::image::Image<'static>> {
    let bytes = include_bytes!("../icons/tray.png");
    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    Some(tauri::image::Image::new_owned(decoded.into_raw(), width, height))
}

pub fn show_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    crate::app_cmd::activate_ns_app();
}
