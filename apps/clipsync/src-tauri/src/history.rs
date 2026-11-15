//! 剪贴板历史记录的对外操作。

use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::store::{MoveInfo, Record};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySnapshot {
    pub items: Vec<Record>,
    pub undo_depth: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    pub query: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[tauri::command]
pub fn history_list(state: tauri::State<'_, AppState>) -> HistorySnapshot {
    let history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    HistorySnapshot {
        items: history.items.clone(),
        undo_depth: history.undo_depth(),
    }
}

#[tauri::command]
pub fn history_set_pinned(state: tauri::State<'_, AppState>, ids: Vec<String>, pinned: bool) -> usize {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let changed = history.set_pinned(&ids, pinned);
    history.flush();
    changed
}

#[tauri::command]
pub fn history_toggle_pin(state: tauri::State<'_, AppState>, id: String) -> bool {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let changed = history.toggle_pin(&id);
    history.flush();
    changed
}

#[tauri::command]
pub fn history_remove(state: tauri::State<'_, AppState>, ids: Vec<String>) -> usize {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let removed = history.remove(&ids);
    history.flush();
    removed
}

#[tauri::command]
pub fn history_clear(state: tauri::State<'_, AppState>) -> usize {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let removed = history.clear();
    history.flush();
    removed
}

#[tauri::command]
pub fn history_remove_since(state: tauri::State<'_, AppState>, after: i64) -> usize {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let removed = history.remove_since(after);
    history.flush();
    removed
}

#[tauri::command]
pub fn history_count_since(state: tauri::State<'_, AppState>, after: i64) -> usize {
    let history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    history.count_since(after)
}

#[tauri::command]
pub fn history_remove_expired(state: tauri::State<'_, AppState>, before: i64) -> usize {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let removed = history.remove_expired(before);
    history.flush();
    removed
}

#[tauri::command]
pub fn history_undo(state: tauri::State<'_, AppState>) -> Vec<Record> {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let restored = history.undo().unwrap_or_default();
    history.flush();
    restored
}

#[tauri::command]
pub fn history_update_text(state: tauri::State<'_, AppState>, id: String, text: String) -> bool {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let changed = history.update_text(&id, &text);
    if changed {
        history.flush();
    }
    changed
}

#[tauri::command]
pub fn history_update_note(state: tauri::State<'_, AppState>, id: String, note: String) -> bool {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let changed = history.update_note(&id, &note);
    if changed {
        history.flush();
    }
    changed
}

#[tauri::command]
pub fn history_duplicate(state: tauri::State<'_, AppState>, id: String, new_id: String) -> bool {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let now = clipsync_core::protocol::now_millis();
    let changed = history.duplicate(&id, new_id, now);
    if changed {
        history.flush();
    }
    changed
}

#[tauri::command]
pub fn history_move_item(state: tauri::State<'_, AppState>, id: String, delta: i64) -> bool {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let changed = history.move_item(&id, delta as isize);
    if changed {
        history.flush();
    }
    changed
}

#[tauri::command]
pub fn history_move_to(state: tauri::State<'_, AppState>, id: String, target: usize) -> Option<usize> {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let moved = history.move_to(&id, target);
    if moved.is_some() {
        history.flush();
    }
    moved
}

#[tauri::command]
pub fn history_move_to_edge(state: tauri::State<'_, AppState>, id: String, top: bool) -> bool {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let total = history.items.len();
    let target = if top { 0 } else { total.saturating_sub(1) };
    let moved = history.move_to(&id, target);
    if moved.is_some() {
        history.flush();
    }
    moved.is_some()
}

#[tauri::command]
pub fn history_move_info(state: tauri::State<'_, AppState>, id: String) -> Option<MoveInfo> {
    let history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    history.move_info(&id)
}

#[tauri::command]
pub fn history_search(state: tauri::State<'_, AppState>, options: SearchOptions) -> Vec<usize> {
    let history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    history.search(&options.query, options.regex, options.case_sensitive)
}

#[tauri::command]
pub fn history_mark_cached(
    state: tauri::State<'_, AppState>,
    id: String,
    local_path: String,
) -> bool {
    let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    let changed = history.mark_cached(&id, &local_path);
    if changed {
        history.flush();
    }
    changed
}

#[tauri::command]
pub fn history_export_json(state: tauri::State<'_, AppState>, ids: Vec<String>) -> String {
    let history = state.history.lock().unwrap_or_else(|e| e.into_inner());
    history.export_json(&ids)
}
