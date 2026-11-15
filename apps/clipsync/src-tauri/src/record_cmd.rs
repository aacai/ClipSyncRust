//! 录屏的对外操作。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::record::{self, MicInput, StartSpec};
use crate::state::AppState;

struct Active {
    started: Instant,
    path: PathBuf,
    name: String,
}

#[derive(Default)]
pub struct Recorder {
    inner: Mutex<Option<Active>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordStatus {
    pub active: bool,
    pub elapsed_ms: i64,
    pub path: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordResult {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub finished: bool,
}

fn recordings_dir() -> PathBuf {
    crate::paths::app_dir().join("recordings")
}

fn is_active(recorder: &Recorder) -> bool {
    recorder
        .inner
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .is_some()
}

/// 开录。整屏为主；区域录制要选区遮罩窗，那部分还没接。
fn start_inner(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let recorder = app.state::<Recorder>();
    if is_active(&recorder) {
        return Err("已经在录了".to_string());
    }
    let (mic, system_audio, fps, long_edge, mic_device) = {
        let prefs = state.prefs.lock().unwrap_or_else(|error| error.into_inner());
        (
            prefs.record_mic(),
            prefs.record_system_audio(),
            prefs.record_fps() as i32,
            prefs.record_long_edge() as i32,
            prefs.record_mic_device().to_string(),
        )
    };
    let dir = recordings_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let stamp = clipsync_core::protocol::now_millis();
    let name = format!("录屏-{stamp}.mp4");
    let path = dir.join(&name);
    let spec = StartSpec {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
        mic,
        system_audio,
        fps,
        long_edge,
        mic_device,
        path: path.clone(),
    };
    record::start(&spec)?;
    // 主窗自己也要排除掉，否则录像里会有一块自己的窗口。
    let _ = record::exclude_self();
    let mut guard = recorder.inner.lock().unwrap_or_else(|error| error.into_inner());
    *guard = Some(Active {
        started: Instant::now(),
        path,
        name,
    });
    drop(guard);
    open_timer(app);
    // 计时条窗是开录之后才出现的，得再排除一次，否则它会被拍进录像。
    let _ = record::exclude_self();
    Ok(())
}

/// 录屏时浮在其它窗口上面的计时条。
fn open_timer(app: &AppHandle) {
    if let Some(existing) = app.get_webview_window("record-timer") {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    let _ = tauri::WebviewWindowBuilder::new(
        app,
        "record-timer",
        tauri::WebviewUrl::App("timer.html".into()),
    )
    .title("录屏中")
    .decorations(false)
    .always_on_top(true)
    .resizable(false)
    .shadow(false)
    .inner_size(420.0, 56.0)
    .position(40.0, 40.0)
    .build();
}

/// 收尾：文件留在盘上，进历史并挂到房间。
fn stop_inner(app: &AppHandle) -> Result<RecordResult, String> {
    let recorder = app.state::<Recorder>();
    let active = recorder
        .inner
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    let Some(active) = active else {
        return Err("现在没在录".to_string());
    };
    record::stop()?;
    if let Some(window) = app.get_webview_window("record-timer") {
        let _ = window.close();
    }
    let size = std::fs::metadata(&active.path)
        .map(|meta| meta.len() as i64)
        .unwrap_or(0);
    let finished = record::is_finished(&active.path);
    crate::services::record_captured_file(
        app,
        active.name.clone(),
        "video/mp4".to_string(),
        &active.path,
        size,
    );
    Ok(RecordResult {
        path: active.path.to_string_lossy().to_string(),
        name: active.name,
        size,
        finished,
    })
}

/// 热键用：一按就切换录/停，不占着主线程。
pub fn toggle(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let recorder = handle.state::<Recorder>();
        let outcome = if is_active(&recorder) {
            stop_inner(&handle).map(|_| "录屏已收尾".to_string())
        } else {
            start_inner(&handle).map(|_| "开始录屏".to_string())
        };
        match outcome {
            Ok(message) => crate::notify(&handle, message),
            Err(error) => crate::notify(&handle, format!("录屏失败：{error}")),
        }
    });
}

#[tauri::command]
pub async fn record_start(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || start_inner(&app))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn record_stop(app: tauri::AppHandle) -> Result<RecordResult, String> {
    tauri::async_runtime::spawn_blocking(move || stop_inner(&app))
        .await
        .map_err(|error| error.to_string())?
}

/// 现在录了多久。
#[tauri::command]
pub fn record_status(recorder: tauri::State<'_, Recorder>) -> RecordStatus {
    let guard = recorder.inner.lock().unwrap_or_else(|error| error.into_inner());
    match guard.as_ref() {
        Some(active) => RecordStatus {
            active: true,
            elapsed_ms: active.started.elapsed().as_millis() as i64,
            path: Some(active.path.to_string_lossy().to_string()),
            name: Some(active.name.clone()),
        },
        None => RecordStatus {
            active: false,
            elapsed_ms: 0,
            path: None,
            name: None,
        },
    }
}

/// 本机可用的麦克风。
#[tauri::command]
pub fn record_inputs() -> Vec<MicInput> {
    record::inputs()
}
