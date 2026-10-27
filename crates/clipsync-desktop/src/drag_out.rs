//! 从列表拖出到访达/资源管理器：文件走本机路径，文本写成临时 .txt。

use std::path::PathBuf;

use crate::store::{Record, KIND_FILE};

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
        let preview = drag::Image::File(canon[0].clone());
        let item = drag::DragItem::Files(canon);
        let _ = drag::start_drag(
            frame,
            item,
            preview,
            |_result, _pos| {},
            drag::Options {
                mode: drag::DragMode::Copy,
                skip_animatation_on_cancel_or_failure: true,
            },
        );
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (frame, paths);
    }
}
