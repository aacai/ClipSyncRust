//! 从列表拖出到访达/资源管理器：文件走本机路径，文本写成临时 .txt。

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::store::{Record, KIND_FILE};

static DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);
static DRAG_STARTED: Mutex<Option<Instant>> = Mutex::new(None);

pub fn drag_active() -> bool {
    if !DRAG_ACTIVE.load(Ordering::Acquire) {
        return false;
    }
    let started = DRAG_STARTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .unwrap_or_else(Instant::now);
    if started.elapsed().as_secs() > 30 {
        DRAG_ACTIVE.store(false, Ordering::Release);
        return false;
    }
    true
}

fn drag_begin() {
    DRAG_ACTIVE.store(true, Ordering::Release);
    *DRAG_STARTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
}

fn drag_end() {
    DRAG_ACTIVE.store(false, Ordering::Release);
}

/// 文本拖出用：`clip-` + 8 hex + `.txt`。
fn random_txt_name() -> String {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).ok();
    format!("clip-{}.txt", hex::encode(bytes))
}

fn drag_temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("clipsync-drag-out");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn write_temp_txt(text: &str) -> Option<PathBuf> {
    let path = drag_temp_dir().join(random_txt_name());
    std::fs::write(&path, text.as_bytes()).ok()?;
    Some(path)
}

/// 已缓存文件用本机路径；文本写成临时 txt；未下载的文件跳过。
pub fn prepare_paths(records: &[&Record]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for record in records {
        if record.kind == KIND_FILE {
            if let Some(file) = record.file.as_ref() {
                if file.cached {
                    if let Some(path) = file.local_path.as_ref() {
                        let pb = PathBuf::from(path);
                        if pb.is_file() {
                            paths.push(pb);
                        }
                    }
                }
            }
            continue;
        }
        if let Some(path) = write_temp_txt(&record.text) {
            paths.push(path);
        }
    }
    paths
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn is_raster_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("bmp") | Some("tiff")
    )
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn preview_image(path: &Path) -> drag::Image {
    if is_raster_image(path) {
        if let Ok(bytes) = std::fs::read(path) {
            return drag::Image::Raw(bytes);
        }
    }
    drag::Image::Raw(generic_file_preview())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn generic_file_preview() -> Vec<u8> {
    use image::{DynamicImage, Rgba, RgbaImage};
    let s = 64u32;
    let mut img = RgbaImage::new(s, s);
    for p in img.pixels_mut() {
        *p = Rgba([120, 160, 220, 255]);
    }
    let mut buf = Vec::new();
    let _ = DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png);
    buf
}

/// 启动系统级拖出（macOS / Windows）。Linux 暂不支持。
pub fn begin(frame: &eframe::Frame, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let canon: Vec<PathBuf> = paths
            .iter()
            .filter_map(|p| std::fs::canonicalize(p).ok())
            .collect();
        if canon.is_empty() {
            return;
        }
        let preview = preview_image(&canon[0]);
        let item = drag::DragItem::Files(canon);
        drag_begin();
        let result = drag::start_drag(
            frame,
            item,
            preview,
            |_result, _pos| drag_end(),
            drag::Options {
                mode: drag::DragMode::Copy,
                skip_animatation_on_cancel_or_failure: true,
            },
        );
        if result.is_err() {
            drag_end();
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (frame, paths);
    }
}
