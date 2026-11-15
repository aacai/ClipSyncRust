//! 文件上传与下载任务。

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use clipsync_core::engine::{Command, FileUpload};

use crate::jobs::{
    self, DownloadAfter, DownloadRequest, JobContext, JobEvent, LiveUploads, UploadRequest,
    UploadTarget,
};
use crate::state::AppState;
use crate::transfer::TransferClient;

pub const EVENT_CHANNEL: &str = "clipsync://transfer";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub upload: bool,
    pub name: String,
    pub done: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub id: String,
    pub ts: i64,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
    pub link: String,
    pub expires_at: i64,
    pub local_path: Option<String>,
}

pub struct TransferJobs {
    pub context: Mutex<Option<JobContext>>,
    pub client: Arc<TransferClient>,
    pub live: Arc<LiveUploads>,
}

impl TransferJobs {
    pub fn new() -> Self {
        Self {
            context: Mutex::new(None),
            client: Arc::new(TransferClient::new()),
            live: Arc::new(LiveUploads::default()),
        }
    }
}

pub fn job_context(state: &tauri::State<'_, AppState>, jobs: &TransferJobs) -> Result<JobContext, String> {
    let room = state
        .room
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| "还没有加入房间".to_string())?;
    Ok(JobContext {
        crypto: room.crypto.clone(),
        client: jobs.client.clone(),
        clip_topic: room.topic.clone(),
        live: jobs.live.clone(),
    })
}

#[tauri::command]
pub fn upload_and_publish(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    jobs: tauri::State<'_, TransferJobs>,
    path: String,
) -> Result<UploadResult, String> {
    let context = job_context(&state, &jobs)?;
    let source = std::path::PathBuf::from(&path);
    if !source.is_file() {
        return Err("只能发送文件，目录不行".to_string());
    }
    let size = std::fs::metadata(&source).map(|meta| meta.len()).unwrap_or(0);
    if size == 0 {
        return Err("空文件发不出去".to_string());
    }
    if size as i64 > clipsync_core::files::MAX_FILE_BYTES {
        return Err(format!(
            "文件超过 {}MB，暂不支持",
            clipsync_core::files::max_mb()
        ));
    }
    let bytes = std::fs::read(&source).map_err(|e| format!("读取文件失败：{e}"))?;
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let mime = clipsync_core::files::mime_for_name(&name).to_string();

    let request = UploadRequest {
        name,
        mime,
        bytes,
        target: UploadTarget::Publish { cache: true },
        cached: None,
    };

    let emitter = app.clone();
    let progress_name = request.name.clone();
    let (tx, rx) = std::sync::mpsc::channel::<JobEvent>();
    std::thread::spawn(move || {
        let outcome = jobs::perform_upload(&context, request, |done, total| {
            let _ = tx.send(JobEvent::Progress {
                upload: true,
                name: progress_name.clone(),
                done,
                total,
            });
        });
        let event = match outcome {
            Ok(done) => JobEvent::Uploaded(done),
            Err(message) => JobEvent::Failed(message),
        };
        let _ = tx.send(event);
    });

    let mut done = None;
    for event in rx {
        match event {
            JobEvent::Progress { upload, name, done: bytes_done, total } => {
                let _ = emitter.emit(
                    EVENT_CHANNEL,
                    TransferProgress {
                        upload,
                        name,
                        done: bytes_done,
                        total,
                    },
                );
            }
            JobEvent::Uploaded(result) => {
                done = Some(result);
                break;
            }
            JobEvent::Failed(message) => return Err(message),
            _ => {}
        }
    }

    let result = done.ok_or_else(|| "上传没有返回结果".to_string())?;
    {
        let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
        history.insert(crate::store::Record {
            id: result.id.clone(),
            kind: crate::store::KIND_FILE.to_string(),
            ts: result.ts,
            sender_id: crate::paths::install_id(),
            sender_name: crate::paths::device_name(),
            text: result.name.clone(),
            file: Some(crate::store::FileInfo {
                name: result.name.clone(),
                mime: result.mime.clone(),
                size: result.size,
                sha256: result.sha256.clone(),
                link: result.link.clone(),
                expires_at: result.expires_at,
                cached: true,
                local_path: Some(path.clone()),
            }),
            pinned: false,
            order: 0,
            note: String::new(),
        });
        history.flush();
    }
    let payload = FileUpload {
        mid: result.id.clone(),
        ts: result.ts,
        name: result.name.clone(),
        mime: result.mime.clone(),
        size: result.size,
        sha256: result.sha256.clone(),
        link: result.link.clone(),
        expires_at: result.expires_at,
    };
    if let Some(sender) = state.sender.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = sender.send(Command::PublishFile {
            file: payload,
            announce: true,
        });
    }
    Ok(UploadResult {
        id: result.id,
        ts: result.ts,
        name: result.name,
        mime: result.mime,
        size: result.size,
        sha256: result.sha256,
        link: result.link,
        expires_at: result.expires_at,
        local_path: result.local_path,
    })
}

/// 收到房间里的文件就自动拉回本机缓存；按设置决定要不要直接进剪贴板。
pub fn auto_download(
    app: &AppHandle,
    id: String,
    name: String,
    mime: String,
    size: i64,
    sha256: String,
    link: String,
) {
    let state = app.state::<AppState>();
    let jobs = app.state::<TransferJobs>();
    let Ok(context) = job_context(&state, &jobs) else {
        return;
    };
    let request = DownloadRequest {
        id: id.clone(),
        name: name.clone(),
        mime,
        size,
        sha256,
        link,
        after: DownloadAfter::Auto,
    };
    let handle = app.clone();
    std::thread::spawn(move || {
        let Ok(done) = jobs::perform_download(&context, request, |_, _| {}) else {
            crate::notify(&handle, format!("下载失败：{name}"));
            return;
        };
        {
            let state = handle.state::<AppState>();
            let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
            history.mark_cached(&id, &done.path);
            history.flush();
        }
        let auto_paste = {
            let state = handle.state::<AppState>();
            let prefs = state.prefs.lock().unwrap_or_else(|e| e.into_inner());
            prefs.auto_paste_file()
        };
        if !auto_paste {
            crate::notify(&handle, format!("已收到文件 {name}"));
            return;
        }
        let state = handle.state::<AppState>();
        let mut watcher = state.clipboard.lock().unwrap_or_else(|e| e.into_inner());
        let placed = watcher.write_files(&[std::path::PathBuf::from(&done.path)]);
        drop(watcher);
        if placed {
            crate::notify(&handle, format!("{name} 已放进剪贴板，可直接粘贴"));
        } else {
            crate::notify(&handle, "写入剪贴板失败".to_string());
        }
    });
}

#[tauri::command]
pub fn download_to_cache(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    jobs: tauri::State<'_, TransferJobs>,
    id: String,
    name: String,
    mime: String,
    size: i64,
    sha256: String,
    link: String,
) -> Result<String, String> {
    let context = job_context(&state, &jobs)?;
    let request = DownloadRequest {
        id: id.clone(),
        name,
        mime,
        size,
        sha256,
        link,
        after: DownloadAfter::Cache,
    };
    let emitter = app.clone();
    let progress_name = request.name.clone();
    let (tx, rx) = std::sync::mpsc::channel::<JobEvent>();
    std::thread::spawn(move || {
        let outcome = jobs::perform_download(&context, request, |done, total| {
            let _ = tx.send(JobEvent::Progress {
                upload: false,
                name: progress_name.clone(),
                done,
                total,
            });
        });
        let event = match outcome {
            Ok(done) => JobEvent::Downloaded(done),
            Err(message) => JobEvent::DownloadFailed {
                id: String::new(),
                message,
            },
        };
        let _ = tx.send(event);
    });

    let mut path = None;
    for event in rx {
        match event {
            JobEvent::Progress { upload, name, done, total } => {
                let _ = emitter.emit(
                    EVENT_CHANNEL,
                    TransferProgress {
                        upload,
                        name,
                        done,
                        total,
                    },
                );
            }
            JobEvent::Downloaded(result) => {
                path = Some(result.path);
                break;
            }
            JobEvent::DownloadFailed { message, .. } => return Err(message),
            _ => {}
        }
    }
    let path = path.ok_or_else(|| "下载没有返回结果".to_string())?;
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    history.mark_cached(&id, &path);
    history.flush();
    Ok(path)
}
