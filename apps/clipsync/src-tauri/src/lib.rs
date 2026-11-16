#[allow(dead_code)]
mod autostart;
mod clipboard;
mod clipboard_cmd;
mod drag;
mod engine_cmd;
mod files;
mod engine_event;
mod history;
#[allow(dead_code)]
mod file_jobs;
mod media;
mod paths;
mod prefs;
mod qrcode_cmd;
mod receive;
mod services;
#[allow(dead_code)]
#[allow(dead_code)]
mod settings;
mod single_instance;
mod transfer;
mod state;
#[allow(dead_code)]
#[allow(dead_code)]
mod startup;
mod store;
mod tray;
mod jobs;
mod text_tools;
#[allow(dead_code)]
mod translate;
mod app_cmd;
mod app_menu;
mod meminfo;
mod update;
mod hotkeys;
mod screenshot;
mod record;
#[cfg(not(target_os = "macos"))]
mod record_portable;
mod record_cmd;
pub mod gpu;
mod win_caption;
mod picker;
mod preview;
mod windows;

use tauri::Manager;

use state::AppState;

pub const NOTIFY_CHANNEL: &str = "clipsync://notify";

/// 给前端递一条提示，前端弹 toast。
pub fn notify(app: &tauri::AppHandle, message: String) {
    use tauri::Emitter;
    let _ = app.emit(NOTIFY_CHANNEL, message);
}

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn hotkey_reload(app: tauri::AppHandle) {
    hotkeys::register_all(&app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if !single_instance::claim_or_notify() {
        return;
    }

    // 子进程在这里把退路告诉 WebView2，得赶在窗口创建之前。
    gpu::apply_env();
    gpu::disable_error_dialogs();

    startup::install_panic_hook();
    startup::append(&format!("启动 {}", env!("CARGO_PKG_VERSION")));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::new())
        .manage(file_jobs::TransferJobs::new())
        .manage(record_cmd::Recorder::default())
        .setup(|app| {
            let handle = app.handle().clone();
            services::start_clipboard_watcher(handle.clone());
            services::start_expiry_sweeper(handle.clone());
            let state = handle.state::<AppState>();
            engine_cmd::restore_session(&handle, &state);
            tray::install(&handle)?;
            app_menu::install(&handle);
            hotkeys::register_all(&handle);
            let hidden = std::env::args().any(|arg| arg == autostart::HIDDEN_ARGUMENT);
            if hidden {
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            hotkey_reload,
            hotkeys::capture_screenshot,
            hotkeys::capture_frame,
            hotkeys::apply_screenshot,
            record_cmd::record_start,
            record_cmd::record_stop,
            record_cmd::record_status,
            record_cmd::record_inputs,
            drag::inspect_dropped_paths,
            drag::begin_drag,
            files::text_op_list,
            files::text_transform,
            files::save_text_to_downloads,
            files::save_file_to_downloads,
            files::open_local_path,
            files::translate_detect_target,
            files::translate_text,
            qrcode_cmd::qr_code_png,
            qrcode_cmd::qr_code_png_for,
            media::image_thumbnail,
            media::image_thumbnail_by_hash,
            media::image_preview,
            media::download_file,
            autostart::set_enabled,
            autostart::is_available,
            file_jobs::upload_and_publish,
            file_jobs::download_to_cache,
            history::history_list,
            history::history_set_pinned,
            history::history_toggle_pin,
            history::history_remove,
            history::history_clear,
            history::history_remove_since,
            history::history_count_since,
            history::history_remove_expired,
            history::history_undo,
            history::history_update_text,
            history::history_update_note,
            history::history_duplicate,
            history::history_move_item,
            history::history_move_to,
            history::history_move_to_edge,
            history::history_move_info,
            history::history_search,
            history::history_export_json,
            history::history_mark_cached,
            prefs::prefs_get,
            prefs::prefs_set_theme,
            prefs::prefs_set_language,
            prefs::prefs_set_nickname,
            prefs::prefs_set_remember_room,
            prefs::prefs_set_watch_clipboard,
            prefs::prefs_set_auto_copy,
            prefs::prefs_set_auto_paste_file,
            prefs::prefs_set_watch_images,
            prefs::prefs_set_watch_files,
            prefs::prefs_set_launch_at_login,
            prefs::prefs_set_delete_expired,
            prefs::prefs_set_retention_days,
            prefs::prefs_set_screenshot_hotkey,
            prefs::prefs_set_screenshot_keep_window,
            prefs::prefs_set_record_hotkey,
            prefs::prefs_set_record_region,
            prefs::prefs_set_record_mic,
            prefs::prefs_set_record_mic_device,
            prefs::prefs_set_record_system_audio,
            prefs::prefs_set_record_long_edge,
            prefs::prefs_set_record_fps,
            prefs::prefs_set_background,
            prefs::prefs_set_background_dim,
            clipboard_cmd::clipboard_read_text,
            clipboard_cmd::clipboard_write_text,
            clipboard_cmd::clipboard_write_files,
            clipboard_cmd::clipboard_write_image,
            clipboard_cmd::clipboard_reseed,
            engine_cmd::engine_snapshot,
            engine_cmd::engine_join,
            engine_cmd::engine_leave,
            engine_cmd::engine_retry,
            engine_cmd::engine_rename,
            engine_cmd::engine_publish_text,
            app_cmd::app_quit,
            app_cmd::app_hide,
            app_cmd::app_show,
            app_cmd::app_activate,
            win_caption::window_tint,
            picker::picker_start,
            picker::picker_open,
            picker::picker_frame,
            picker::picker_result,
            picker::picker_cancel,
            picker::snap_windows,
            preview::preview_open,
            preview::preview_file,
            preview::preview_close,
            app_cmd::window_always_on_top,
            app_cmd::window_click_through,
            app_cmd::random_room_code,
            app_cmd::sanitize_room_code,
            app_cmd::parse_invite_link,
            app_cmd::memory_usage,
            app_cmd::background_set,
            app_cmd::background_clear,
            app_cmd::background_data_url,
            app_cmd::max_background_dim,
            update::update_check,
            update::update_download,
        ])
        .build(tauri::generate_context!())
        .expect("error while building ClipSync")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => tray::show_window(app),
            _ => {
                let _ = app;
            }
        });
}
