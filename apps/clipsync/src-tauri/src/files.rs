//! 文件落地与文本变换命令。

use serde::Serialize;

use crate::paths;
use crate::text_tools::{self, TextOp};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextOpInfo {
    pub key: String,
    pub label: String,
}

#[tauri::command]
pub fn text_op_list() -> Vec<TextOpInfo> {
    text_tools::OPS
        .iter()
        .map(|op| TextOpInfo {
            key: op.key().to_string(),
            label: op.label().to_string(),
        })
        .collect()
}

#[tauri::command]
pub fn text_transform(text: String, op: String) -> Result<String, String> {
    let op = TextOp::from_key(&op).ok_or_else(|| format!("未知的文本操作：{op}"))?;
    Ok(text_tools::apply(&text, op))
}

#[tauri::command]
pub fn save_text_to_downloads(text: String) -> Result<String, String> {
    let name = format!("clip-{}.txt", clipsync_core::protocol::random_mid());
    paths::save_to_downloads(&name, text.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_file_to_downloads(path: String) -> Result<String, String> {
    let source = std::path::PathBuf::from(&path);
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
    paths::save_to_downloads(&name, &bytes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_local_path(path: String) -> Result<(), String> {
    paths::open_path(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn translate_detect_target(text: String) -> String {
    match crate::translate::detect_target(&text) {
        crate::translate::Target::Chinese => "en2zh".to_string(),
        crate::translate::Target::English => "zh2en".to_string(),
    }
}

#[tauri::command]
pub fn translate_text(text: String, target: String) -> Result<String, String> {
    let target = match target.as_str() {
        "zh2en" => crate::translate::Target::English,
        _ => crate::translate::Target::Chinese,
    };
    crate::translate::translate(&text, target).map_err(|e| e.to_string())
}
