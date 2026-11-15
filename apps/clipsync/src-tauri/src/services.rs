//! 后台服务：剪贴板轮询与捕获内容入库。

use clipsync_core::engine::{Command, FileUpload};
use tauri::{AppHandle, Manager};

use crate::clipboard::Captured;
use crate::file_jobs::TransferJobs;
use crate::jobs::{UploadRequest, UploadTarget};
use crate::state::AppState;

pub fn start_expiry_sweeper(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            let state = app.state::<AppState>();
            let (enabled, days) = {
                let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
                (prefs.delete_expired(), prefs.retention_days())
            };
            if !enabled {
                continue;
            }
            let before = clipsync_core::protocol::now_millis()
                - (days as i64) * 24 * 60 * 60 * 1000;
            let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
            let removed = history.remove_expired(before);
            if removed > 0 {
                history.flush();
            }
        }
    });
}

pub fn start_clipboard_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(crate::clipboard::POLL_INTERVAL).await;
            let state = app.state::<AppState>();
            let (watch_files, watch_images) = {
                let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
                (prefs.watch_files(), prefs.watch_images())
            };
            let captured = {
                let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
                watcher.poll(watch_files, watch_images)
            };
            let Some(captured) = captured else {
                continue;
            };
            match captured {
                Captured::Text(text) => {
                    let sender = state.sender.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    if let Some(sender) = sender {
                        let _ = sender.send(Command::PublishText(text));
                    }
                }
                Captured::Files(files) => {
                    for file in files {
                        let mime = file.mime().to_string();
                        let name = file.name.clone();
                        let id = format!("local-{}", clipsync_core::protocol::random_mid());
                        let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
                        let record = crate::store::Record {
                            id: id.clone(),
                            kind: crate::store::KIND_FILE.to_string(),
                            ts: clipsync_core::protocol::now_millis(),
                            sender_id: crate::paths::install_id(),
                            sender_name: crate::paths::device_name(),
                            text: name.clone(),
                            file: Some(crate::store::FileInfo {
                                name: name.clone(),
                                mime: mime.clone(),
                                size: file.size,
                                sha256: String::new(),
                                link: String::new(),
                                expires_at: 0,
                                cached: true,
                                local_path: Some(file.path.to_string_lossy().to_string()),
                            }),
                            pinned: false,
                            order: 0,
                            note: String::new(),
                        };
                        history.insert(record);
                        history.flush();
                        drop(history);

                        let bytes = match std::fs::read(&file.path) {
                            Ok(bytes) => bytes,
                            Err(_) => continue,
                        };
                        publish_to_room(&app, id, name, mime, bytes);
                    }
                }
                Captured::Image(image) => {
                    let mime = image.mime().to_string();
                    let name = image.name.clone();
                    record_captured_image(&app, name, mime, image.png, image.hash.clone());
                }
            }
        }
    });
}

/// 把一张图片写进本机缓存、记进历史，并挂到房间里。
pub fn record_captured_image(app: &AppHandle, name: String, mime: String, png: Vec<u8>, hash: String) {
    let cached = crate::paths::write_cache(&hash, &name, &png).ok();
    let id = format!("local-{}", clipsync_core::protocol::random_mid());
    {
        let state = app.state::<AppState>();
        let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
        let record = crate::store::Record {
            id: id.clone(),
            kind: crate::store::KIND_FILE.to_string(),
            ts: clipsync_core::protocol::now_millis(),
            sender_id: crate::paths::install_id(),
            sender_name: crate::paths::device_name(),
            text: name.clone(),
            file: Some(crate::store::FileInfo {
                name: name.clone(),
                mime: mime.clone(),
                size: png.len() as i64,
                sha256: hash,
                link: String::new(),
                expires_at: 0,
                cached: cached.is_some(),
                local_path: cached,
            }),
            pinned: false,
            order: 0,
            note: String::new(),
        };
        history.insert(record);
        history.flush();
    }
    publish_to_room(app, id, name, mime, png);
}

/// 把本机生成的文件（录屏等）记进历史，并挂到房间里。
pub fn record_captured_file(
    app: &AppHandle,
    name: String,
    mime: String,
    path: &std::path::Path,
    size: i64,
) -> String {
    let id = format!("local-{}", clipsync_core::protocol::random_mid());
    {
        let state = app.state::<AppState>();
        let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
        let record = crate::store::Record {
            id: id.clone(),
            kind: crate::store::KIND_FILE.to_string(),
            ts: clipsync_core::protocol::now_millis(),
            sender_id: crate::paths::install_id(),
            sender_name: crate::paths::device_name(),
            text: name.clone(),
            file: Some(crate::store::FileInfo {
                name: name.clone(),
                mime: mime.clone(),
                size,
                sha256: String::new(),
                link: String::new(),
                expires_at: 0,
                cached: true,
                local_path: Some(path.to_string_lossy().to_string()),
            }),
            pinned: false,
            order: 0,
            note: String::new(),
        };
        history.insert(record);
        history.flush();
    }
    if let Ok(bytes) = std::fs::read(path) {
        publish_to_room(app, id.clone(), name, mime, bytes);
    }
    id
}

/// 把本机捕获到的内容加密上传并挂进房间，房间里的其它设备凭直链取。
pub fn publish_to_room(app: &AppHandle, record_id: String, name: String, mime: String, bytes: Vec<u8>) {
    let state = app.state::<AppState>();
    let jobs = app.state::<TransferJobs>();
    let Ok(context) = crate::file_jobs::job_context(&state, &jobs) else {
        return;
    };
    let request = UploadRequest {
        name,
        mime,
        bytes,
        target: UploadTarget::Publish { cache: true },
        cached: None,
    };
    let handle = app.clone();
    std::thread::spawn(move || {
        let Ok(done) = crate::jobs::perform_upload(&context, request, |_, _| {}) else {
            return;
        };
        {
            let state = handle.state::<AppState>();
            let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
            history.fill_file_meta(&record_id, &done.link, &done.sha256, done.expires_at);
            history.flush();
        }
        let payload = FileUpload {
            mid: done.id.clone(),
            ts: done.ts,
            name: done.name.clone(),
            mime: done.mime.clone(),
            size: done.size,
            sha256: done.sha256.clone(),
            link: done.link.clone(),
            expires_at: done.expires_at,
        };
        let state = handle.state::<AppState>();
        let sender = state.sender.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(sender) = sender {
            let _ = sender.send(Command::PublishFile {
                file: payload,
                announce: true,
            });
        }
    });
}
