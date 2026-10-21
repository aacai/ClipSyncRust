use std::cmp::Ordering;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use clipsync_core::engine::Clip;

pub const MAX_ITEMS: usize = 500;
pub const MAX_UNDO: usize = 20;
pub const KIND_TEXT: &str = "text";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub ts: i64,
    #[serde(default)]
    pub sender_id: String,
    #[serde(default)]
    pub sender_name: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub note: String,
}

fn default_kind() -> String {
    KIND_TEXT.to_string()
}

impl Record {
    pub fn from_clip(clip: &Clip) -> Self {
        Self {
            id: clip.mid.clone(),
            kind: KIND_TEXT.to_string(),
            ts: clip.ts,
            sender_id: clip.sender_id.clone(),
            sender_name: clip.sender_name.clone(),
            text: clip.text.clone(),
            pinned: false,
            order: 0,
            note: String::new(),
        }
    }

    pub fn rank(&self) -> i64 {
        if self.order != 0 {
            self.order
        } else {
            self.ts
        }
    }

    pub fn moved(&self) -> bool {
        self.order != 0 && self.order != self.ts
    }
}

fn ordering(a: &Record, b: &Record) -> Ordering {
    b.pinned
        .cmp(&a.pinned)
        .then_with(|| b.rank().cmp(&a.rank()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveInfo {
    pub position: usize,
    pub from: usize,
    pub to: usize,
}

pub struct History {
    path: PathBuf,
    pub items: Vec<Record>,
    trash: Vec<Vec<Record>>,
    dirty: bool,
}

impl History {
    pub fn open(path: PathBuf) -> Self {
        let items = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<Record>>(&raw).ok())
            .unwrap_or_default();
        Self {
            path,
            items,
            trash: Vec::new(),
            dirty: false,
        }
    }

    pub fn add(&mut self, record: Record) -> bool {
        if let Some(existing) = self.items.iter_mut().find(|item| item.id == record.id) {
            existing.ts = record.ts;
            existing.kind = record.kind;
            existing.text = record.text;
            existing.pinned = record.pinned;
            self.dirty = true;
            self.sort();
            return false;
        }
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|item| same_content(item, &record))
        {
            existing.ts = record.ts;
            existing.order = record.ts;
            self.dirty = true;
            self.sort();
            return false;
        }
        self.items.push(record);
        self.sort();
        self.items.truncate(MAX_ITEMS);
        self.dirty = true;
        true
    }

    pub fn update_text(&mut self, id: &str, text: &str) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        if item.text == text {
            return false;
        }
        item.text = text.to_string();
        self.dirty = true;
        true
    }

    pub fn update_note(&mut self, id: &str, note: &str) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        if item.note == note {
            return false;
        }
        item.note = note.to_string();
        self.dirty = true;
        true
    }

    pub fn set_pinned(&mut self, ids: &[String], pinned: bool) -> usize {
        if ids.is_empty() {
            return 0;
        }
        let mut changed = 0;
        for item in &mut self.items {
            if ids.contains(&item.id) && item.pinned != pinned {
                item.pinned = pinned;
                changed += 1;
            }
        }
        if changed > 0 {
            self.sort();
            self.dirty = true;
        }
        changed
    }

    pub fn toggle_pin(&mut self, id: &str) -> bool {
        let Some(item) = self.items.iter().find(|item| item.id == id) else {
            return false;
        };
        let pinned = !item.pinned;
        self.set_pinned(&[id.to_string()], pinned) > 0
    }

    pub fn move_item(&mut self, id: &str, delta: isize) -> bool {
        let Some(from) = self.items.iter().position(|item| item.id == id) else {
            return false;
        };
        let Some(to) = from.checked_add_signed(delta) else {
            return false;
        };
        if to >= self.items.len() {
            return false;
        }
        let a_rank = self.items[from].rank();
        let b_rank = self.items[to].rank();
        self.items[from].order = b_rank;
        self.items[to].order = a_rank;
        self.sort();
        self.dirty = true;
        true
    }

    pub fn move_to(&mut self, id: &str, target: usize) -> Option<usize> {
        let from = self.items.iter().position(|item| item.id == id)?;
        let (lo, hi) = self.group_bounds(from);
        let to = target.saturating_sub(1).clamp(lo, hi);
        if to == from {
            return None;
        }
        let ranks: Vec<i64> = self.items[lo..=hi].iter().map(Record::rank).collect();
        let mut part: Vec<Record> = self.items[lo..=hi].to_vec();
        let item = part.remove(from - lo);
        part.insert(to - lo, item);
        for (offset, item) in part.iter_mut().enumerate() {
            item.order = ranks[offset];
        }
        self.items.splice(lo..=hi, part);
        self.sort();
        self.dirty = true;
        Some(to + 1)
    }

    pub fn move_info(&self, id: &str) -> Option<MoveInfo> {
        let position = self.items.iter().position(|item| item.id == id)?;
        let (lo, hi) = self.group_bounds(position);
        Some(MoveInfo {
            position: position + 1,
            from: lo + 1,
            to: hi + 1,
        })
    }

    pub fn duplicate(&mut self, id: &str, new_id: String, now: i64) -> bool {
        let Some(item) = self.items.iter().find(|item| item.id == id).cloned() else {
            return false;
        };
        self.items.push(Record {
            id: new_id,
            ts: now,
            order: now,
            ..item
        });
        self.sort();
        self.items.truncate(MAX_ITEMS);
        self.dirty = true;
        true
    }

    pub fn remove(&mut self, ids: &[String]) -> usize {
        if ids.is_empty() {
            return 0;
        }
        let (removed, kept): (Vec<Record>, Vec<Record>) = self
            .items
            .drain(..)
            .partition(|item| ids.contains(&item.id));
        self.items = kept;
        if removed.is_empty() {
            return 0;
        }
        let count = removed.len();
        self.push_trash(removed);
        self.dirty = true;
        count
    }

    pub fn clear(&mut self) -> usize {
        let ids: Vec<String> = self.items.iter().map(|item| item.id.clone()).collect();
        self.remove(&ids)
    }

    pub fn undo(&mut self) -> Option<Vec<Record>> {
        let batch = self.trash.pop()?;
        self.items.extend(batch.iter().cloned());
        self.sort();
        self.items.truncate(MAX_ITEMS);
        self.dirty = true;
        Some(batch)
    }

    pub fn undo_depth(&self) -> usize {
        self.trash.len()
    }

    pub fn search(&self, query: &str, regex: bool, case_sensitive: bool) -> Vec<usize> {
        let query = query.trim();
        if query.is_empty() {
            return (0..self.items.len()).collect();
        }
        if regex {
            let pattern = regex::RegexBuilder::new(query)
                .case_insensitive(!case_sensitive)
                .build();
            let Ok(pattern) = pattern else {
                return Vec::new();
            };
            return self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item_matches(item, |text| pattern.is_match(text)))
                .map(|(index, _)| index)
                .collect();
        }
        if case_sensitive {
            self.items
                .iter()
                .enumerate()
                .filter(|(_, item)| item_matches(item, |text| text.contains(query)))
                .map(|(index, _)| index)
                .collect()
        } else {
            let needle = query.to_lowercase();
            self.items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item_matches(item, |text| text.to_lowercase().contains(&needle))
                })
                .map(|(index, _)| index)
                .collect()
        }
    }

    pub fn export_json(&self, ids: &[String]) -> String {
        let selected: Vec<Value> = self
            .items
            .iter()
            .filter(|item| ids.contains(&item.id))
            .map(|item| {
                json!({
                    "id": item.id,
                    "kind": item.kind,
                    "ts": item.ts,
                    "senderId": item.sender_id,
                    "senderName": item.sender_name,
                    "text": item.text,
                    "file": null,
                    "pinned": item.pinned,
                    "order": item.order,
                    "note": item.note,
                })
            })
            .collect();
        serde_json::to_string(&selected).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        if let Ok(raw) = serde_json::to_string(&self.items) {
            let tmp = self.path.with_extension("tmp");
            if std::fs::write(&tmp, raw).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }

    fn sort(&mut self) {
        self.items.sort_by(ordering);
    }

    fn group_bounds(&self, index: usize) -> (usize, usize) {
        let pinned = self.items[index].pinned;
        let mut lo = index;
        while lo > 0 && self.items[lo - 1].pinned == pinned {
            lo -= 1;
        }
        let mut hi = index;
        while hi + 1 < self.items.len() && self.items[hi + 1].pinned == pinned {
            hi += 1;
        }
        (lo, hi)
    }

    fn push_trash(&mut self, batch: Vec<Record>) {
        self.trash.push(batch);
        while self.trash.len() > MAX_UNDO {
            self.trash.remove(0);
        }
    }
}

fn same_content(a: &Record, b: &Record) -> bool {
    a.kind == b.kind && a.kind == KIND_TEXT && a.text == b.text
}

fn item_matches(item: &Record, matches: impl Fn(&str) -> bool) -> bool {
    matches(&item.text) || (!item.note.is_empty() && matches(&item.note))
}

#[cfg(test)]
mod tests {
    use super::{History, Record, KIND_TEXT, MAX_ITEMS};

    fn record_at(id: &str, text: &str, ts: i64) -> Record {
        Record {
            id: id.to_string(),
            kind: KIND_TEXT.to_string(),
            ts,
            sender_id: "me".to_string(),
            sender_name: "Me".to_string(),
            text: text.to_string(),
            pinned: false,
            order: 0,
            note: String::new(),
        }
    }

    fn record(id: &str, text: &str) -> Record {
        record_at(id, text, 1)
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("clipsync-history-{name}.json"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("tmp"));
    }

    fn history(name: &str) -> History {
        let path = scratch(name);
        cleanup(&path);
        let mut history = History::open(path.clone());
        history.path = path;
        history
    }

    #[test]
    fn a_repeat_id_is_rejected() {
        let mut history = history("dup");
        assert!(history.add(record("a", "first")));
        assert!(!history.add(record("a", "different text")));
        assert_eq!(history.items.len(), 1);
        assert_eq!(history.items[0].text, "different text");
    }

    #[test]
    fn identical_text_moves_to_the_front() {
        let mut history = history("front");
        history.add(record_at("a", "hello", 1));
        history.add(record_at("b", "world", 2));
        history.add(record_at("c", "hello", 3));
        let ids: Vec<&str> = history.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!(history.items[0].ts, 3);
        assert_eq!(history.items[0].order, 3);
    }

    #[test]
    fn the_history_is_capped() {
        let mut history = history("cap");
        for index in 0..MAX_ITEMS + 10 {
            history.add(record_at(
                &index.to_string(),
                &format!("clip {index}"),
                index as i64,
            ));
        }
        assert_eq!(history.items.len(), MAX_ITEMS);
        assert_eq!(history.items[0].text, format!("clip {}", MAX_ITEMS + 9));
    }

    #[test]
    fn flush_survives_a_reopen() {
        let path = scratch("persist");
        cleanup(&path);
        {
            let mut history = History::open(path.clone());
            history.add(record("a", "keep me"));
            history.flush();
        }
        let reopened = History::open(path.clone());
        assert_eq!(reopened.items.len(), 1);
        assert_eq!(reopened.items[0].text, "keep me");
        cleanup(&path);
    }

    #[test]
    fn an_old_history_file_loads_with_defaults() {
        let path = scratch("legacy");
        cleanup(&path);
        std::fs::write(
            &path,
            r#"[{"id":"a","kind":"text","ts":5,"sender_id":"x","sender_name":"X","text":"old","pinned":false}]"#,
        )
        .unwrap();
        let history = History::open(path.clone());
        assert_eq!(history.items.len(), 1);
        assert_eq!(history.items[0].order, 0);
        assert_eq!(history.items[0].note, "");
        cleanup(&path);
    }

    #[test]
    fn pinned_items_sort_first() {
        let mut history = history("pin");
        history.add(record_at("a", "one", 100));
        history.add(record_at("b", "two", 200));
        history.toggle_pin("a");
        let ids: Vec<&str> = history.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        history.toggle_pin("a");
        let ids: Vec<&str> = history.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"]);
    }

    #[test]
    fn moving_swaps_ranks_and_stops_at_the_edges() {
        let mut history = history("move");
        history.add(record_at("a", "one", 100));
        history.add(record_at("b", "two", 200));
        history.add(record_at("c", "three", 300));
        assert!(history.move_item("c", 1));
        let ids: Vec<&str> = history.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c", "a"]);
        assert!(!history.move_item("b", -1));
        assert!(!history.move_item("a", 1));
    }

    #[test]
    fn move_to_stays_inside_the_pinned_group() {
        let mut history = history("moveto");
        history.add(record_at("a", "one", 100));
        history.add(record_at("b", "two", 200));
        history.add(record_at("c", "three", 300));
        history.toggle_pin("c");
        assert_eq!(history.move_to("c", 3), None);
        assert_eq!(history.move_to("a", 2), Some(2));
        let ids: Vec<&str> = history.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a", "b"]);
        let info = history.move_info("b").unwrap();
        assert_eq!((info.from, info.to), (2, 3));
    }

    #[test]
    fn remove_and_undo_round_trip() {
        let mut history = history("undo");
        history.add(record_at("a", "one", 100));
        history.add(record_at("b", "two", 200));
        assert_eq!(history.remove(&["b".to_string()]), 1);
        assert_eq!(history.items.len(), 1);
        assert_eq!(history.undo_depth(), 1);
        let batch = history.undo().expect("batch");
        assert_eq!(batch.len(), 1);
        assert_eq!(history.items.len(), 2);
        assert_eq!(history.undo_depth(), 0);
    }

    #[test]
    fn clear_can_be_undone() {
        let mut history = history("clear");
        history.add(record_at("a", "one", 100));
        history.add(record_at("b", "two", 200));
        assert_eq!(history.clear(), 2);
        assert!(history.items.is_empty());
        history.undo();
        assert_eq!(history.items.len(), 2);
    }

    #[test]
    fn duplicate_keeps_content_and_gets_a_new_id() {
        let mut history = history("duplicate");
        history.add(record_at("a", "one", 100));
        assert!(history.duplicate("a", "copy".to_string(), 500));
        assert_eq!(history.items.len(), 2);
        assert_eq!(history.items[0].id, "copy");
        assert_eq!(history.items[0].text, "one");
        assert_eq!(history.items[0].rank(), 500);
    }

    #[test]
    fn search_handles_case_regex_and_notes() {
        let mut history = history("search");
        history.add(record_at("a", "Hello World", 100));
        history.add(record_at("b", "clip%sync", 200));
        history.update_note("a", "daily note");
        assert_eq!(history.search("hello", false, false), vec![1]);
        assert!(history.search("hello", false, true).is_empty());
        assert_eq!(history.search("^clip.*sync$", true, true).len(), 1);
        assert_eq!(history.search("^clip.*sync$", false, true).len(), 0);
        assert_eq!(history.search("daily", false, false).len(), 1);
        assert!(history.search("[", true, false).is_empty());
    }

    #[test]
    fn export_uses_the_shared_json_shape() {
        let mut history = history("export");
        history.add(record_at("a", "one", 100));
        history.update_note("a", "keep");
        let raw = history.export_json(&["a".to_string()]);
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed[0]["senderId"], "me");
        assert_eq!(parsed[0]["note"], "keep");
        assert!(parsed[0].get("file").is_some());
        assert!(parsed[0].get("order").is_some());
    }
}
