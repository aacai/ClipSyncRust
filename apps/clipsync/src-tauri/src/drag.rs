//! 文件拖放：拖入读取本机元信息，拖出把文本/本机文件交给系统原生拖拽。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroppedFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub too_large: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DragSource {
    pub text: Option<String>,
    pub file_path: Option<String>,
}

#[tauri::command]
pub fn inspect_dropped_paths(paths: Vec<String>) -> Vec<DroppedFile> {
    let max = clipsync_core::files::max_mb() as u64 * 1024 * 1024;
    paths
        .iter()
        .filter_map(|path| {
            let p = Path::new(path);
            if !p.is_file() {
                return None;
            }
            let name = p.file_name()?.to_string_lossy().to_string();
            let size = std::fs::metadata(p).ok()?.len();
            Some(DroppedFile {
                path: path.clone(),
                name: name.clone(),
                size,
                mime: clipsync_core::files::mime_for_name(&name).to_string(),
                too_large: size > max,
            })
        })
        .collect()
}

fn random_txt_name() -> String {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).ok();
    format!("clip-{}.txt", hex::encode(bytes))
}

fn write_temp_txt(text: &str) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("clipsync-drag-out");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(random_txt_name());
    std::fs::write(&path, text.as_bytes()).ok()?;
    Some(path)
}

fn prepare_paths(sources: &[DragSource]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for source in sources {
        if let Some(file_path) = &source.file_path {
            let path = PathBuf::from(file_path);
            if path.is_file() {
                paths.push(path);
            }
            continue;
        }
        if let Some(text) = &source.text {
            if let Some(path) = write_temp_txt(text) {
                paths.push(path);
            }
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
fn generic_file_preview() -> Vec<u8> {
    use image::{DynamicImage, Rgba, RgbaImage};
    let mut img = RgbaImage::new(64, 64);
    for pixel in img.pixels_mut() {
        *pixel = Rgba([120, 160, 220, 255]);
    }
    let mut buf = Vec::new();
    let _ = DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png);
    buf
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn preview_image(path: Option<&Path>) -> drag::Image {
    let bytes = path
        .filter(|p| is_raster_image(p))
        .and_then(|p| std::fs::read(p).ok())
        .unwrap_or_else(generic_file_preview);
    drag::Image::Raw(bytes)
}

#[tauri::command]
pub fn begin_drag(window: tauri::Window, sources: Vec<DragSource>) -> Result<(), String> {
    let paths = prepare_paths(&sources);
    if paths.is_empty() {
        return Err("没有可拖出的本机文件".into());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let preview = preview_image(paths.first().map(|p| p.as_path()));
        let drag_window = window.clone();
        window
            .run_on_main_thread(move || {
                let canon: Vec<PathBuf> = paths
                    .iter()
                    .filter_map(|p| std::fs::canonicalize(p).ok())
                    .collect();
                if canon.is_empty() {
                    return;
                }
                let _ = drag::start_drag(
                    &drag_window,
                    drag::DragItem::Files(canon),
                    preview,
                    |_, _| {},
                    drag::Options {
                        mode: drag::DragMode::Copy,
                        skip_animatation_on_cancel_or_failure: true,
                    },
                );
            })
            .map_err(|e| e.to_string())?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window, paths);

    Ok(())
}
