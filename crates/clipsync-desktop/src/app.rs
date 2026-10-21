use std::collections::BTreeSet;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::text::TextWrapping;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Key, Layout, Margin,
    Modifiers, Order, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, TextFormat, Ui,
    UiBuilder, Vec2,
};

use clipsync_core::engine::{Clip, Config, EngineHandle, Event, Peer, Status};

use crate::clipboard::{ClipboardWatcher, POLL_INTERVAL};
use crate::icons::{self, Icon};
use crate::settings::Settings;
use crate::store::{History, Record, KIND_TEXT};
use crate::theme;

const TOAST_INFO: Duration = Duration::from_secs(4);
const TOAST_ERROR: Duration = Duration::from_secs(6);
const TOAST_UNDO: Duration = Duration::from_secs(9);

const HELP_STEPS: [&str; 6] = [
    "打开「房间共享」：顶栏的钥匙图标，或未共享时列表里的「共享」。填一个房间码（可随机换一个），房间密码可选，设了就必须两端一致。",
    "另一台设备装同一个 App，或直接开网页，填完全一样的房间码和密码，点「进入房间」。",
    "核对两端顶栏的安全码：逐位相同才说明用的是同一把钥匙。不同就是房间码或密码打错了，这种连得上却永远收不到对方的内容。",
    "进房后任意一端复制文本，另一端立刻收到；点条目就写回本机剪贴板。",
    "掉线期间的复制不会补发，两台都显示「已连接」时再传。",
    "只支持文本：文件、图片等类型暂不同步。",
];

pub struct ClipSyncApp {
    engine: EngineHandle,
    history: History,
    settings: Settings,
    clipboard: ClipboardWatcher,
    status: Status,
    peers: Vec<Peer>,
    mismatch: Vec<String>,
    room_code: String,
    password: String,
    joined: bool,
    pending_session: Option<(String, String)>,
    query: String,
    regex: bool,
    case_sensitive: bool,
    write_incoming: bool,
    last_poll: Instant,
    cursor: Option<String>,
    picked: BTreeSet<String>,
    pick_mode: bool,
    nav_scroll: Option<String>,
    overlay: Option<Overlay>,
    focus_dialog: bool,
    focus_search: bool,
    toast: Option<Toast>,
    header_bottom: f32,
}

#[derive(Clone)]
enum Overlay {
    Share,
    Help,
    Safety,
    ClearAll,
    LeaveRoom,
    Settings,
    NewEntry {
        text: String,
    },
    Edit {
        id: String,
        text: String,
    },
    Note {
        id: String,
        text: String,
    },
    Detail {
        id: String,
    },
    MoveTo {
        id: String,
        input: String,
        error: bool,
    },
}

enum DialogAction {
    None,
    Open(Overlay),
    Join,
    Leave,
    ClearAll,
    NewEntry(String),
    SaveEdit(String, String),
    SaveNote(String, String),
    MoveTo(String, usize),
    Copy(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Edit,
    Note,
    Duplicate,
    Detail,
    MoveUp,
    MoveDown,
    MoveTop,
    MoveBottom,
    MoveTo,
    Delete,
}

enum AppEvent {
    Activate(String),
    TogglePick(String),
    Copy(String),
    Publish(String),
    Pin(String),
    Menu(String, MenuAction),
    Share,
    Retry,
}

struct RowView<'a> {
    record: &'a Record,
    position: usize,
    cursor: bool,
    picked: bool,
    pick_mode: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastKind {
    Info,
    Error,
    Undo,
}

#[derive(Clone)]
struct Toast {
    text: String,
    kind: ToastKind,
    born: Instant,
}

impl Toast {
    fn lifetime(&self) -> Duration {
        match self.kind {
            ToastKind::Info => TOAST_INFO,
            ToastKind::Error => TOAST_ERROR,
            ToastKind::Undo => TOAST_UNDO,
        }
    }
}

impl ClipSyncApp {
    pub fn new(engine: EngineHandle, history: History, settings: Settings) -> Self {
        let mut app = Self {
            engine,
            history,
            settings,
            clipboard: ClipboardWatcher::new(),
            status: Status::Idle,
            peers: Vec::new(),
            mismatch: Vec::new(),
            room_code: String::new(),
            password: String::new(),
            joined: false,
            pending_session: None,
            query: String::new(),
            regex: false,
            case_sensitive: false,
            write_incoming: true,
            last_poll: Instant::now(),
            cursor: None,
            picked: BTreeSet::new(),
            pick_mode: false,
            nav_scroll: None,
            overlay: None,
            focus_dialog: false,
            focus_search: false,
            toast: None,
            header_bottom: theme::TOP_INSET + 160.0,
        };
        if let Some(session) = app.settings.session.clone() {
            app.room_code = session.room;
            app.password = session.password;
            app.join();
        } else {
            app.room_code = random_room_code();
        }
        app
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.engine.events.try_recv() {
            match event {
                Event::Status(status) => {
                    if matches!(status, Status::Online { .. }) {
                        if let Some((room, password)) = self.pending_session.take() {
                            self.settings.remember(&room, &password);
                        }
                    }
                    if matches!(status, Status::Idle) {
                        self.joined = false;
                    }
                    self.status = status;
                }
                Event::Peers(peers) => self.peers = peers,
                Event::KeyMismatch(names) => self.mismatch = names,
                Event::Clip(clip) => self.on_clip(clip),
            }
        }
        self.history.flush();
    }

    fn on_clip(&mut self, clip: Clip) {
        let incoming = !clip.mine;
        let inserted = self.history.add(Record::from_clip(&clip));
        if incoming {
            if self.write_incoming {
                self.clipboard.write(&clip.text);
            }
            if inserted {
                let sender = if clip.sender_name.trim().is_empty() {
                    "同屋设备"
                } else {
                    clip.sender_name.as_str()
                };
                let message = if self.write_incoming {
                    format!("收到 {sender} 的文本，已进剪贴板")
                } else {
                    format!("收到 {sender} 的文本")
                };
                self.notify(message);
            }
        }
    }

    fn poll_clipboard(&mut self) {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return;
        }
        self.last_poll = Instant::now();
        if let Some(text) = self.clipboard.poll() {
            if self.joined {
                self.engine.publish_text(&text);
            } else {
                self.record_local(&text);
            }
        }
    }

    fn record_local(&mut self, text: &str) {
        let ts = now_millis();
        self.history.add(Record {
            id: new_id(),
            kind: KIND_TEXT.to_string(),
            ts,
            sender_id: crate::paths::install_id(),
            sender_name: crate::paths::device_name(),
            text: text.to_string(),
            pinned: false,
            order: 0,
            note: String::new(),
        });
        self.history.flush();
    }

    fn publish_manual(&mut self, text: &str) {
        let length = text.chars().count();
        if self.joined {
            self.clipboard.note_published(text);
            self.engine.publish_text(text);
            self.notify(format!("已同步 {length} 字"));
        } else {
            self.record_local(text);
            self.notify(format!("未共享：已记录到本地（{length} 字）"));
        }
    }

    fn send_clipboard(&mut self) {
        match self.clipboard.read_once() {
            Some(text) => self.publish_manual(&text),
            None => self.notify_error("剪贴板是空的"),
        }
    }

    fn join(&mut self) {
        let room = sanitize_room(&self.room_code);
        if room.is_empty() {
            self.notify_error("请输入房间码");
            return;
        }
        self.room_code = room.clone();
        let password = self.password.clone();
        self.pending_session = Some((room.clone(), password.clone()));
        self.engine.join(Config {
            room_id: room,
            password,
            room_key: None,
            device_id: crate::paths::install_id(),
            device_name: crate::paths::device_name(),
            device_os: crate::paths::device_os().to_string(),
        });
        self.joined = true;
        self.status = Status::Connecting;
        self.peers.clear();
        self.mismatch.clear();
        self.overlay = None;
        self.pick_mode = false;
        self.picked.clear();
    }

    fn leave(&mut self) {
        self.pending_session = None;
        self.engine.leave();
        self.settings.forget();
        self.joined = false;
        self.status = Status::Idle;
        self.peers.clear();
        self.mismatch.clear();
        self.overlay = None;
        self.pick_mode = false;
        self.picked.clear();
        self.notify("已退出房间");
    }

    fn copy_to_clipboard(&mut self, text: &str, message: &str) {
        if text.is_empty() {
            return;
        }
        self.clipboard.write(text);
        self.notify(message);
    }

    fn copy_record(&mut self, id: &str) {
        let Some(text) = self
            .history
            .items
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.text.clone())
        else {
            return;
        };
        self.copy_to_clipboard(&text, "已复制");
    }

    fn publish_record(&mut self, id: &str) {
        let Some(text) = self
            .history
            .items
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.text.clone())
        else {
            return;
        };
        if !self.joined {
            self.notify_error("还没进房间：先进入房间再补传");
            return;
        }
        self.clipboard.note_published(&text);
        self.engine.publish_text(&text);
        self.notify("已发送到房间");
    }

    fn toggle_pin_record(&mut self, id: &str) {
        let Some(pinned) = self
            .history
            .items
            .iter()
            .find(|record| record.id == id)
            .map(|record| !record.pinned)
        else {
            return;
        };
        if self.history.toggle_pin(id) {
            self.notify(if pinned {
                "已置顶"
            } else {
                "已取消置顶"
            });
        }
    }

    fn undo(&mut self) {
        if let Some(batch) = self.history.undo() {
            self.notify(format!("已恢复 {} 条", batch.len()));
        }
    }

    fn delete_records(&mut self, ids: Vec<String>) {
        let count = self.history.remove(&ids);
        if count == 0 {
            return;
        }
        for id in &ids {
            if self.cursor.as_deref() == Some(id.as_str()) {
                self.cursor = None;
            }
            self.picked.remove(id);
        }
        self.notify_undo(format!("已删除 {count} 条"));
    }

    fn delete_selection(&mut self) {
        let ids: Vec<String> = self.picked.iter().cloned().collect();
        self.delete_records(ids);
    }

    fn clear_all(&mut self) {
        let count = self.history.clear();
        if count == 0 {
            return;
        }
        self.picked.clear();
        self.cursor = None;
        self.notify_undo(format!("已删除 {count} 条"));
    }

    fn toggle_pick_mode(&mut self) {
        if self.pick_mode {
            self.pick_mode = false;
            self.picked.clear();
        } else {
            self.pick_mode = true;
            self.picked.clear();
        }
    }

    fn toggle_pick(&mut self, id: &str) {
        if self.picked.contains(id) {
            self.picked.remove(id);
        } else {
            self.picked.insert(id.to_string());
        }
        self.pick_mode = true;
        self.cursor = Some(id.to_string());
    }

    fn copy_selection(&mut self) {
        let picked = &self.picked;
        let texts: Vec<&str> = self
            .history
            .items
            .iter()
            .filter(|record| picked.contains(&record.id))
            .map(|record| record.text.as_str())
            .collect();
        let count = texts.len();
        if count == 0 {
            return;
        }
        let joined = texts.join("\n\n");
        self.copy_to_clipboard(&joined, &format!("已复制 {count} 条"));
    }

    fn export_selection(&mut self) {
        let ids: Vec<String> = self.picked.iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        let json = self.history.export_json(&ids);
        self.clipboard.write(&json);
        self.notify(format!("已把 {} 条导出到剪贴板", ids.len()));
    }

    fn pin_selection(&mut self) {
        let ids: Vec<String> = self.picked.iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        let all_pinned = ids.iter().all(|id| {
            self.history
                .items
                .iter()
                .find(|record| &record.id == id)
                .is_some_and(|record| record.pinned)
        });
        let changed = self.history.set_pinned(&ids, !all_pinned);
        if changed > 0 {
            self.notify(if all_pinned {
                format!("已取消置顶 {changed} 条")
            } else {
                format!("已置顶 {changed} 条")
            });
        }
    }

    fn duplicate_record(&mut self, id: &str) {
        if self.history.duplicate(id, new_id(), now_millis()) {
            self.notify("已生成 1 个副本");
        }
    }

    fn open(&mut self, overlay: Overlay) {
        self.focus_dialog = true;
        self.overlay = Some(overlay);
    }

    fn ask_delete(&mut self) {
        if self.pick_mode {
            self.delete_selection();
        } else if !self.history.items.is_empty() {
            self.open(Overlay::ClearAll);
        }
    }

    fn invalid_regex(&self) -> bool {
        self.regex
            && !self.query.trim().is_empty()
            && regex::RegexBuilder::new(self.query.trim())
                .case_insensitive(!self.case_sensitive)
                .build()
                .is_err()
    }

    fn visible_ids(&self) -> Vec<String> {
        self.history
            .search(&self.query, self.regex, self.case_sensitive)
            .into_iter()
            .map(|index| self.history.items[index].id.clone())
            .collect()
    }

    fn move_cursor(&mut self, visible: &[String], delta: isize, extend: bool) {
        if visible.is_empty() {
            self.cursor = None;
            return;
        }
        let current = self
            .cursor
            .as_ref()
            .and_then(|id| visible.iter().position(|candidate| candidate == id));
        let next = match current {
            Some(position) => {
                (position as isize + delta).clamp(0, visible.len() as isize - 1) as usize
            }
            None => {
                if delta > 0 {
                    0
                } else {
                    visible.len() - 1
                }
            }
        };
        let id = visible[next].clone();
        self.cursor = Some(id.clone());
        if extend {
            self.pick_mode = true;
            self.picked.insert(id.clone());
        } else if self.pick_mode {
            self.picked.clear();
            self.picked.insert(id.clone());
        }
        self.nav_scroll = Some(id);
    }

    fn jump_cursor(&mut self, visible: &[String], to_end: bool, extend: bool) {
        if visible.is_empty() {
            return;
        }
        let id = if to_end {
            visible.last().cloned()
        } else {
            visible.first().cloned()
        };
        if let Some(id) = id {
            self.cursor = Some(id.clone());
            if extend {
                self.pick_mode = true;
                self.picked.insert(id.clone());
            } else if self.pick_mode {
                self.picked.clear();
                self.picked.insert(id.clone());
            }
            self.nav_scroll = Some(id);
        }
    }

    fn move_cursor_item(&mut self, delta: isize) {
        let Some(id) = self.cursor.clone() else {
            return;
        };
        if self.history.move_item(&id, delta) {
            self.nav_scroll = Some(id);
            self.history.flush();
        }
    }

    fn duplicate_cursor(&mut self) {
        if let Some(id) = self.cursor.clone() {
            self.duplicate_record(&id);
        }
    }

    fn edit_cursor(&mut self) {
        let Some(id) = self.cursor.clone() else {
            return;
        };
        let Some(text) = self
            .history
            .items
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.text.clone())
        else {
            return;
        };
        self.open(Overlay::Edit { id, text });
    }

    fn delete_cursor(&mut self) {
        if self.pick_mode && !self.picked.is_empty() {
            self.delete_selection();
        } else if let Some(id) = self.cursor.clone() {
            self.delete_records(vec![id]);
        }
    }

    fn select_all(&mut self, visible: &[String]) {
        self.pick_mode = true;
        self.picked = visible.iter().cloned().collect();
        self.cursor = visible.first().cloned();
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.overlay.is_some() || egui::Popup::is_any_open(ctx) {
            return;
        }
        let search = search_id();
        let typing = ctx.egui_wants_keyboard_input();

        if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::F)) {
            ctx.memory_mut(|memory| memory.request_focus(search));
            self.focus_search = true;
            return;
        }

        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            if self.pick_mode {
                self.pick_mode = false;
                self.picked.clear();
            } else if !self.query.is_empty() {
                self.query.clear();
            } else if self.cursor.take().is_some() {
            } else {
                ctx.memory_mut(|memory| memory.surrender_focus(search));
            }
            return;
        }

        let visible = self.visible_ids();

        if !typing {
            if ctx.input_mut(|input| {
                input.consume_key(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::ArrowUp)
            }) {
                self.move_cursor_item(-1);
                return;
            }
            if ctx.input_mut(|input| {
                input.consume_key(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::ArrowDown)
            }) {
                self.move_cursor_item(1);
                return;
            }
            if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Z)) {
                self.undo();
                return;
            }
            if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::C)) {
                if self.pick_mode && !self.picked.is_empty() {
                    self.copy_selection();
                } else if let Some(id) = self.cursor.clone() {
                    self.copy_record(&id);
                }
                return;
            }
            if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::D)) {
                self.duplicate_cursor();
                return;
            }
            if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::N)) {
                self.open(Overlay::NewEntry {
                    text: String::new(),
                });
                return;
            }
            if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::A)) {
                self.select_all(&visible);
                return;
            }
            if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::F2)) {
                self.edit_cursor();
                return;
            }
            if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Delete))
                || ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Backspace))
            {
                self.delete_cursor();
                return;
            }
            if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
                if self.pick_mode {
                    if let Some(id) = self.cursor.clone() {
                        self.toggle_pick(&id);
                    }
                } else if let Some(id) = self.cursor.clone() {
                    self.copy_record(&id);
                }
                return;
            }
        }

        if ctx.input_mut(|input| input.consume_key(Modifiers::SHIFT, Key::ArrowUp)) {
            self.move_cursor(&visible, -1, true);
            return;
        }
        if ctx.input_mut(|input| {
            input.consume_key(Modifiers::SHIFT.plus(Modifiers::COMMAND), Key::ArrowUp)
        }) {
            self.move_cursor_item(-1);
            return;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::SHIFT, Key::ArrowDown)) {
            self.move_cursor(&visible, 1, true);
            return;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowUp)) {
            self.move_cursor(&visible, -1, false);
            return;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowDown)) {
            self.move_cursor(&visible, 1, false);
            return;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Home)) {
            self.jump_cursor(&visible, false, false);
            return;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::End)) {
            self.jump_cursor(&visible, true, false);
        }
    }

    fn apply_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Activate(id) => {
                self.cursor = Some(id.clone());
                self.copy_record(&id);
            }
            AppEvent::TogglePick(id) => self.toggle_pick(&id),
            AppEvent::Copy(id) => self.copy_record(&id),
            AppEvent::Publish(id) => self.publish_record(&id),
            AppEvent::Pin(id) => self.toggle_pin_record(&id),
            AppEvent::Menu(id, action) => self.handle_menu(id, action),
            AppEvent::Share => self.open(Overlay::Share),
            AppEvent::Retry => {
                self.engine.retry_now();
                self.status = Status::Connecting;
            }
        }
    }

    fn handle_menu(&mut self, id: String, action: MenuAction) {
        match action {
            MenuAction::Edit => {
                if let Some(text) = self
                    .history
                    .items
                    .iter()
                    .find(|record| record.id == id)
                    .map(|record| record.text.clone())
                {
                    self.open(Overlay::Edit { id, text });
                }
            }
            MenuAction::Note => {
                if let Some(note) = self
                    .history
                    .items
                    .iter()
                    .find(|record| record.id == id)
                    .map(|record| record.note.clone())
                {
                    self.open(Overlay::Note { id, text: note });
                }
            }
            MenuAction::Duplicate => self.duplicate_record(&id),
            MenuAction::Detail => self.open(Overlay::Detail { id }),
            MenuAction::MoveUp => {
                if self.history.move_item(&id, -1) {
                    self.nav_scroll = Some(id);
                }
            }
            MenuAction::MoveDown => {
                if self.history.move_item(&id, 1) {
                    self.nav_scroll = Some(id);
                }
            }
            MenuAction::MoveTop => {
                if let Some(position) = self.history.move_to(&id, 1) {
                    self.notify(format!("已移动到第 {position} 位"));
                    self.nav_scroll = Some(id);
                }
            }
            MenuAction::MoveBottom => {
                let last = self.history.items.len();
                if let Some(position) = self.history.move_to(&id, last) {
                    self.notify(format!("已移动到第 {position} 位"));
                    self.nav_scroll = Some(id);
                }
            }
            MenuAction::MoveTo => {
                let input = self
                    .history
                    .move_info(&id)
                    .map(|info| info.position.to_string())
                    .unwrap_or_default();
                self.open(Overlay::MoveTo {
                    id,
                    input,
                    error: false,
                });
            }
            MenuAction::Delete => self.delete_records(vec![id]),
        }
        self.history.flush();
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind: ToastKind::Info,
            born: Instant::now(),
        });
    }

    fn notify_error(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind: ToastKind::Error,
            born: Instant::now(),
        });
    }

    fn notify_undo(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind: ToastKind::Undo,
            born: Instant::now(),
        });
    }

    fn expire_toast(&mut self) {
        if let Some(toast) = &self.toast {
            if toast.born.elapsed() > toast.lifetime() {
                self.toast = None;
            }
        }
    }
}

impl eframe::App for ClipSyncApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !cfg!(target_os = "macos") {
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
        }
        self.drain_events();
        self.poll_clipboard();
        self.expire_toast();
        let wait = if self.toast.is_some() {
            Duration::from_millis(120)
        } else {
            POLL_INTERVAL
        };
        ctx.request_repaint_after(wait);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        theme::paint_background(&ctx, ctx.content_rect());
        self.paint_window_chrome(ui);
        self.header_bottom = self.header(&ctx);

        let mut events = Vec::new();
        let mut new_entry_clicked = false;
        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| {
                let screen = ui.ctx().content_rect();
                let width = (screen.width() - theme::CONTENT_MARGIN * 2.0).min(theme::MAX_CONTENT);
                let left = screen.center().x - width / 2.0;
                let rect = Rect::from_min_max(
                    Pos2::new(left, screen.top()),
                    Pos2::new(left + width, screen.bottom()),
                );
                ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.set_width(width);
                            let top = self.header_bottom - screen.top() + 12.0;
                            ui.add_space(top);
                            self.banners(ui, &mut events);
                            let visible =
                                self.history
                                    .search(&self.query, self.regex, self.case_sensitive);
                            if visible.is_empty() {
                                new_entry_clicked = self.empty_state(ui);
                            } else {
                                let now = now_millis();
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (position, index) in visible.iter().enumerate() {
                                    let record = &self.history.items[*index];
                                    let view = RowView {
                                        record,
                                        position: position + 1,
                                        cursor: self.cursor.as_deref() == Some(record.id.as_str()),
                                        picked: self.picked.contains(&record.id),
                                        pick_mode: self.pick_mode,
                                    };
                                    let scroll =
                                        self.nav_scroll.as_deref() == Some(record.id.as_str());
                                    clip_row(ui, view, now, &mut events, scroll);
                                }
                            }
                            ui.add_space(96.0);
                        });
                });
            });
        self.nav_scroll = None;

        for event in events {
            self.apply_event(event);
        }
        if new_entry_clicked {
            self.open(Overlay::NewEntry {
                text: String::new(),
            });
        }

        self.selection_bar(&ctx);
        self.show_toast(&ctx);
        self.show_overlay(&ctx);
        self.history.flush();
    }
}

impl ClipSyncApp {
    fn paint_window_chrome(&self, ui: &mut Ui) {
        let screen = ui.ctx().content_rect();
        let painter = ui.ctx().layer_painter(egui::LayerId::background());
        painter.text(
            Pos2::new(screen.center().x, screen.top() + 13.0),
            Align2::CENTER_TOP,
            "ClipSync",
            FontId::proportional(12.0),
            Color32::from_rgb(0x8d, 0x8d, 0x93),
        );
        #[cfg(target_os = "macos")]
        {
            let strip = Rect::from_min_max(
                Pos2::new(screen.left() + 84.0, screen.top()),
                Pos2::new(screen.right() - 12.0, screen.top() + theme::TOP_INSET - 8.0),
            );
            let response = ui.interact(strip, Id::unique("titlebar-drag"), Sense::drag());
            if response.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
    }

    fn header(&mut self, ctx: &egui::Context) -> f32 {
        let screen = ctx.content_rect();
        let width =
            (screen.width().min(theme::MAX_CONTENT) - theme::CONTENT_MARGIN * 2.0).max(320.0);
        let pos = Pos2::new(
            screen.center().x - width / 2.0,
            screen.top() + theme::TOP_INSET + theme::CONTENT_MARGIN,
        );
        let response = egui::Area::new(Id::unique("clip-header"))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.set_width(width);
                theme::glass_frame().show(ui, |ui| {
                    self.header_title(ui);
                    if matches!(self.status, Status::Online { .. }) {
                        ui.add_space(1.0);
                        self.header_detail(ui);
                    }
                    ui.add_space(8.0);
                    self.header_toolbar(ui);
                });
            });
        response.response.rect.bottom()
    }

    fn header_title(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(30.0);
            let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
            ui.painter()
                .circle_filled(dot_rect.center(), 4.0, status_color(&self.status));
            ui.add_space(1.0);
            if self.joined {
                let room = self.room_code.clone();
                let pill = Frame::new()
                    .fill(theme::CARD)
                    .stroke(Stroke::new(1.0, theme::with_alpha(theme::BORDER, 153)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(7, 3))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            let (key_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                            icons::paint(ui.painter(), key_rect, Icon::Key, theme::ACCENT);
                            ui.label(
                                RichText::new(&room)
                                    .monospace()
                                    .size(14.0)
                                    .color(theme::TEXT),
                            );
                        });
                    });
                let response = ui
                    .interact(pill.response.rect, Id::unique("room-pill"), Sense::click())
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("点击复制房间码");
                if response.clicked() {
                    self.copy_to_clipboard(&room, "已复制房间码");
                }
            } else {
                ui.label(
                    RichText::new("本地记录")
                        .size(14.5)
                        .strong()
                        .color(theme::TEXT),
                );
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(status_label(&self.status))
                    .size(11.5)
                    .color(theme::MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if self.joined && icons::icon_button(ui, Icon::Logout, true, "离开房间").clicked()
                {
                    self.open(Overlay::LeaveRoom);
                }
                if icons::icon_button(ui, Icon::Settings, true, "设置").clicked() {
                    self.open(Overlay::Settings);
                }
                if icons::icon_button(ui, Icon::Help, true, "使用说明").clicked() {
                    self.open(Overlay::Help);
                }
                if !self.joined
                    && icons::icon_button(ui, Icon::Key, true, "进入或切换房间").clicked()
                {
                    self.open(Overlay::Share);
                }
                if matches!(self.status, Status::Online { .. }) && self.devices_button(ui) {
                    self.open(Overlay::Safety);
                }
            });
        });
    }

    fn devices_button(&mut self, ui: &mut Ui) -> bool {
        let (rect, response) = ui.allocate_exact_size(Vec2::new(48.0, 30.0), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(9), Color32::from_white_alpha(9));
        }
        icons::paint(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(rect.left() + 14.0, rect.center().y),
                Vec2::splat(16.0),
            ),
            Icon::Devices,
            theme::MUTED,
        );
        ui.painter().text(
            Pos2::new(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            self.peers.len().to_string(),
            FontId::proportional(11.0),
            theme::MUTED,
        );
        response
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("同屋设备")
            .clicked()
    }

    fn header_detail(&mut self, ui: &mut Ui) {
        let Status::Online { safety } = &self.status else {
            return;
        };
        let mut text = format!("安全码 {safety}");
        if !self.peers.is_empty() {
            let names = self
                .peers
                .iter()
                .map(|peer| peer.name.as_str())
                .collect::<Vec<_>>()
                .join(" · ");
            text.push_str(&format!(
                " · 同屋 {} 台：{}",
                self.peers.len(),
                truncate_chars(&names, 40)
            ));
        }
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (shield_rect, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
            icons::paint(ui.painter(), shield_rect, Icon::Shield, theme::ACCENT);
            ui.label(RichText::new(text).size(11.5).color(theme::MUTED));
        });
        let response = ui
            .interact(
                row.response.rect,
                Id::unique("safety-detail"),
                Sense::click(),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("查看安全码说明");
        if response.clicked() {
            self.open(Overlay::Safety);
        }
    }

    fn header_toolbar(&mut self, ui: &mut Ui) {
        self.search_field(ui);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if icons::icon_button(ui, Icon::Send, true, "发送剪贴板").clicked() {
                self.send_clipboard();
            }
            if icons::icon_button(ui, Icon::Plus, true, "新建一条").clicked() {
                self.open(Overlay::NewEntry {
                    text: String::new(),
                });
            }
            let select_tip = if self.pick_mode {
                "退出多选"
            } else {
                "多选条目"
            };
            if icons::icon_button(ui, Icon::Select, true, select_tip).clicked() {
                self.toggle_pick_mode();
            }
            let can_undo = self.history.undo_depth() > 0;
            if icons::icon_button(ui, Icon::Undo, can_undo, "撤销").clicked() {
                self.undo();
            }
            let can_delete = if self.pick_mode {
                !self.picked.is_empty()
            } else {
                !self.history.items.is_empty()
            };
            let delete_tip = if self.pick_mode && !self.picked.is_empty() {
                "删除所选"
            } else {
                "清空历史"
            };
            if icons::icon_button_tinted(ui, Icon::Trash, theme::RED, can_delete, delete_tip)
                .clicked()
            {
                self.ask_delete();
            }
        });
    }

    fn search_field(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 52.0), Sense::hover());
        let focused = ui.ctx().memory(|memory| memory.has_focus(search_id()));
        let border = if focused {
            theme::ACCENT.gamma_multiply(0.6)
        } else {
            Color32::from_white_alpha(12)
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(12),
            Color32::from_white_alpha(6),
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
        icons::paint(
            painter,
            Rect::from_center_size(
                Pos2::new(rect.left() + 20.0, rect.center().y),
                Vec2::splat(18.0),
            ),
            Icon::Search,
            theme::MUTED,
        );

        let chip_h = 26.0;
        let chip = |right: f32, width: f32| {
            Rect::from_min_max(
                Pos2::new(right - width, rect.center().y - chip_h / 2.0),
                Pos2::new(right, rect.center().y + chip_h / 2.0),
            )
        };
        let mut right = rect.right() - 8.0;
        let regex_rect = chip(right, 32.0);
        right -= 34.0;
        let case_rect = chip(right, 32.0);
        right -= 34.0;
        let clear_rect = if self.query.is_empty() {
            None
        } else {
            let clear = chip(right, 24.0);
            right -= 26.0;
            Some(clear)
        };

        let edit_rect = Rect::from_min_max(
            Pos2::new(rect.left() + 36.0, rect.top() + 8.0),
            Pos2::new((right - 4.0).max(rect.left() + 60.0), rect.bottom() - 8.0),
        );
        let mut field = ui.new_child(UiBuilder::new().max_rect(edit_rect));
        let response = field.add_sized(
            edit_rect.size(),
            TextEdit::singleline(&mut self.query)
                .id(search_id())
                .hint_text("搜索历史")
                .frame(Frame::new())
                .margin(Margin::ZERO)
                .font(FontId::proportional(14.0))
                .align(Align2::LEFT_CENTER),
        );
        if self.focus_search {
            response.request_focus();
            self.focus_search = false;
        }

        let grep_response = ui.interact(regex_rect, Id::unique("search-regex"), Sense::click());
        paint_chip(ui, regex_rect, ".*", self.regex, FontId::monospace(12.5));
        if grep_response
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("按正则搜索")
            .clicked()
        {
            self.regex = !self.regex;
        }

        let case_response = ui.interact(case_rect, Id::unique("search-case"), Sense::click());
        paint_chip(
            ui,
            case_rect,
            "Aa",
            self.case_sensitive,
            FontId::proportional(12.0),
        );
        if case_response
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("区分大小写")
            .clicked()
        {
            self.case_sensitive = !self.case_sensitive;
        }

        if let Some(clear_rect) = clear_rect {
            let clear_response =
                ui.interact(clear_rect, Id::unique("search-clear"), Sense::click());
            icons::paint(
                ui.painter(),
                Rect::from_center_size(clear_rect.center(), Vec2::splat(14.0)),
                Icon::Close,
                theme::MUTED,
            );
            if clear_response
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("清空搜索")
                .clicked()
            {
                self.query.clear();
            }
        }
    }

    fn banners(&mut self, ui: &mut Ui, events: &mut Vec<AppEvent>) {
        if !self.joined {
            if banner(
                ui,
                Icon::Key,
                theme::ACCENT,
                false,
                "未共享：复制的内容只留在本机，进房间后才会同步给其他设备",
                Some("共享"),
            ) {
                events.push(AppEvent::Share);
            }
        } else if matches!(self.status, Status::Offline { .. }) {
            let text = format!("与房间 {} 的连接已断开", self.room_code);
            if banner(ui, Icon::Warning, theme::AMBER, false, &text, Some("重连")) {
                events.push(AppEvent::Retry);
            }
        }
        if !self.mismatch.is_empty() {
            let text = format!(
                "同屋的 {} 安全码与本机不一致：多半是某一端的房间密码打错了，两边收不到彼此的剪贴板",
                self.mismatch.join("、")
            );
            banner(ui, Icon::Warning, theme::RED, true, &text, None);
        }
        if self.invalid_regex() {
            banner(
                ui,
                Icon::Warning,
                theme::RED,
                true,
                "正则表达式写错了，暂时搜不出结果",
                None,
            );
        }
    }

    fn empty_state(&mut self, ui: &mut Ui) -> bool {
        let mut clicked = false;
        ui.add_space(54.0);
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(62.0), Sense::hover());
            ui.painter().circle_filled(
                rect.center(),
                31.0,
                Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 20),
            );
            ui.painter().circle_stroke(
                rect.center(),
                31.0,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 60)),
            );
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), Vec2::splat(26.0)),
                Icon::Clipboard,
                theme::ACCENT,
            );
            ui.add_space(14.0);
            if self.query.trim().is_empty() {
                ui.label(
                    RichText::new("还没有剪贴板记录")
                        .size(16.0)
                        .strong()
                        .color(theme::TEXT),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("复制过的内容会自动出现在这里，并在同房间的设备间同步")
                        .size(12.0)
                        .color(theme::MUTED),
                );
                ui.add_space(16.0);
                if accent_button(ui, "新建一条").clicked() {
                    clicked = true;
                }
            } else {
                ui.label(
                    RichText::new(format!("没有匹配「{}」的记录", self.query.trim()))
                        .size(16.0)
                        .strong()
                        .color(theme::TEXT),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("换个词试试，或关掉正则再搜")
                        .size(12.0)
                        .color(theme::MUTED),
                );
            }
        });
        clicked
    }

    fn selection_bar(&mut self, ctx: &egui::Context) {
        if !self.pick_mode {
            return;
        }
        let count = self.picked.len();
        let all_pinned = !self.picked.is_empty()
            && self.picked.iter().all(|id| {
                self.history
                    .items
                    .iter()
                    .find(|record| &record.id == id)
                    .is_some_and(|record| record.pinned)
            });
        let pin_tip = if all_pinned { "取消置顶" } else { "置顶" };
        egui::Area::new(Id::unique("selection-bar"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -20.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(0x1e, 0x24, 0x2d, 250))
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(CornerRadius::same(18))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 30,
                        spread: 0,
                        color: Color32::from_black_alpha(100),
                    })
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(
                                RichText::new(format!("已选 {count} 条"))
                                    .size(12.0)
                                    .color(theme::MUTED),
                            );
                            ui.add_space(4.0);
                            let enabled = count > 0;
                            if icons::icon_button(ui, Icon::Copy, enabled, "复制所选").clicked()
                            {
                                self.copy_selection();
                            }
                            if icons::icon_button(ui, Icon::Pin, enabled, pin_tip).clicked() {
                                self.pin_selection();
                            }
                            if icons::icon_button(ui, Icon::Export, enabled, "导出所选为 JSON")
                                .clicked()
                            {
                                self.export_selection();
                            }
                            if icons::icon_button_tinted(
                                ui,
                                Icon::Trash,
                                theme::RED,
                                enabled,
                                "删除所选",
                            )
                            .clicked()
                            {
                                self.delete_selection();
                            }
                            if icons::icon_button(ui, Icon::Close, true, "退出多选").clicked() {
                                self.pick_mode = false;
                                self.picked.clear();
                            }
                        });
                    });
            });
    }

    fn show_toast(&mut self, ctx: &egui::Context) {
        let Some(toast) = self.toast.clone() else {
            return;
        };
        let offset = if self.pick_mode { -84.0 } else { -24.0 };
        let mut undo_clicked = false;
        let error = toast.kind == ToastKind::Error;
        let fill = if error {
            theme::ERROR_CONTAINER
        } else {
            theme::INVERSE_SURFACE
        };
        let text_color = if error {
            theme::ON_ERROR_CONTAINER
        } else {
            theme::INVERSE_TEXT
        };
        egui::Area::new(Id::unique("toast"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, offset))
            .interactable(toast.kind == ToastKind::Undo)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(fill)
                    .corner_radius(CornerRadius::same(14))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(90),
                    })
                    .inner_margin(Margin::symmetric(16, 9))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&toast.text).size(12.5).color(text_color));
                            if toast.kind == ToastKind::Undo {
                                ui.add_space(6.0);
                                let action = egui::Button::new(
                                    RichText::new("撤销")
                                        .size(12.5)
                                        .strong()
                                        .color(Color32::from_rgb(0x9d, 0x2c, 0x58)),
                                )
                                .frame(false);
                                if ui.add(action).clicked() {
                                    undo_clicked = true;
                                }
                            }
                        });
                    });
            });
        if undo_clicked {
            self.undo();
        }
    }

    fn show_overlay(&mut self, ctx: &egui::Context) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let focus = std::mem::take(&mut self.focus_dialog);
        let mut action = DialogAction::None;
        let mut close = false;
        let width = match &overlay {
            Overlay::Help => 470.0,
            Overlay::Detail { .. } => 500.0,
            Overlay::Safety => 440.0,
            _ => 420.0,
        };
        let response = egui::Modal::new(Id::unique("clip-dialog"))
            .frame(theme::dialog_frame())
            .show(ctx, |ui| {
                ui.set_width(width);
                match &mut overlay {
                    Overlay::Share => {
                        dialog_header(ui, Icon::Key, "房间共享");
                        ui.add_space(12.0);
                        field_label(ui, "房间码");
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let field_width = (ui.available_width() - 40.0).max(120.0);
                            let edit = ui.add_sized(
                                Vec2::new(field_width, 34.0),
                                TextEdit::singleline(&mut self.room_code)
                                    .hint_text("例如 K7M2QP")
                                    .margin(Margin::symmetric(10, 8))
                                    .font(FontId::monospace(12.5)),
                            );
                            if edit.changed() {
                                self.room_code = sanitize_room(&self.room_code);
                            }
                            if focus {
                                edit.request_focus();
                            }
                            if icons::icon_button(ui, Icon::Refresh, true, "换一个随机房间码")
                                .clicked()
                            {
                                self.room_code = random_room_code();
                            }
                        });
                        hint(ui, "在其他设备上输入同一个房间码即可互联；随机生成适合新开一间。");
                        ui.add_space(12.0);
                        field_label(ui, "房间密码");
                        ui.add_space(4.0);
                        let _password = ui.add_sized(
                            Vec2::new(ui.available_width(), 34.0),
                            TextEdit::singleline(&mut self.password)
                                .password(true)
                                .hint_text("可选，留空即只用房间码")
                                .margin(Margin::symmetric(10, 8))
                                .font(FontId::proportional(13.0)),
                        );
                        hint(ui, "可选，留空即只用房间码；设了就必须两端一致。");
                        if matches!(self.status, Status::Online { .. }) {
                            ui.add_space(8.0);
                            hint(ui, "当前已在这个房间。改动后点「进入房间」会切换过去。");
                        }
                        ui.add_space(16.0);
                        let enter = ui.input(|input| input.key_pressed(Key::Enter));
                        ui.horizontal(|ui| {
                            if self.joined
                                && secondary_button(ui, "退出房间").clicked()
                            {
                                action = DialogAction::Open(Overlay::LeaveRoom);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if accent_button(ui, "进入房间").clicked() || enter {
                                    action = DialogAction::Join;
                                }
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
                        });
                    }
                    Overlay::Help => {
                        dialog_header(ui, Icon::Help, "怎么用");
                        ui.add_space(12.0);
                        egui::ScrollArea::vertical()
                            .max_height(360.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (index, step) in HELP_STEPS.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::splat(22.0),
                                            Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            rect.center(),
                                            10.0,
                                            Color32::from_rgba_unmultiplied(
                                                0xf4, 0xa8, 0xc0, 30,
                                            ),
                                        );
                                        ui.painter().text(
                                            rect.center(),
                                            Align2::CENTER_CENTER,
                                            (index + 1).to_string(),
                                            FontId::proportional(10.5),
                                            theme::ACCENT,
                                        );
                                        ui.add_space(2.0);
                                        ui.vertical(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(
                                                RichText::new(*step)
                                                    .size(12.0)
                                                    .color(theme::MUTED),
                                            );
                                        });
                                    });
                                    ui.add_space(8.0);
                                }
                            });
                        ui.add_space(12.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary_button(ui, "知道了").clicked() {
                                close = true;
                            }
                        });
                    }
                    Overlay::Safety => {
                        dialog_header(ui, Icon::Shield, "安全码");
                        ui.add_space(12.0);
                        let (safety, connected) = match &self.status {
                            Status::Online { safety } => (Some(safety.clone()), true),
                            _ => (None, false),
                        };
                        match &safety {
                            Some(safety) => {
                                ui.label(
                                    RichText::new(safety)
                                        .monospace()
                                        .size(20.0)
                                        .color(theme::ACCENT),
                                );
                                hint(ui, "本机安全码 · 两台设备逐位相同才算同一把钥匙");
                            }
                            None => hint(ui, "还没连上房间，暂时没有安全码。"),
                        }
                        ui.add_space(12.0);
                        field_label(ui, "同屋设备");
                        ui.add_space(4.0);
                        if self.peers.is_empty() {
                            hint(ui, "暂时没有其他设备。");
                        } else {
                            for peer in &self.peers {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(&peer.name)
                                                .size(12.5)
                                                .color(theme::TEXT),
                                        );
                                        ui.label(
                                            RichText::new(&peer.os)
                                                .size(10.5)
                                                .color(theme::FAINT),
                                        );
                                    });
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            let (label, color) = if !connected {
                                                ("未知", theme::FAINT)
                                            } else if safety
                                                .as_deref()
                                                .is_some_and(|mine| {
                                                    !peer.safety.is_empty()
                                                        && mine.starts_with(&peer.safety)
                                                })
                                            {
                                                ("一致", theme::GREEN)
                                            } else if peer.safety.is_empty() {
                                                ("未知", theme::FAINT)
                                            } else {
                                                ("不一致", theme::RED)
                                            };
                                            ui.label(
                                                RichText::new(label)
                                                    .size(11.0)
                                                    .strong()
                                                    .color(color),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                            }
                        }
                        ui.add_space(10.0);
                        for point in [
                            "安全码是本房间密钥的指纹：由房间码加上房间密码算出来，只用来当面核对，本身泄露不了内容。",
                            "两台设备每一段数字都对得上，才说明房间码和密码完全一致，彼此的内容才解得开。",
                            "数字不一致时先别查网络：几乎一定是某一端的房间密码打错了，两边都显示已连接，却收不到对方的剪贴板。",
                        ] {
                            bullet(ui, point);
                        }
                        ui.add_space(14.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary_button(ui, "知道了").clicked() {
                                close = true;
                            }
                        });
                    }
                    Overlay::ClearAll => {
                        dialog_header(ui, Icon::Trash, "清空全部历史？");
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(
                                "只删除本机保存的这些条目，房间另一端的历史与已经收到的文件不受影响。",
                            )
                            .size(12.5)
                            .color(theme::MUTED),
                        );
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if danger_button(ui, "清空").clicked() {
                                    action = DialogAction::ClearAll;
                                }
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
                        });
                    }
                    Overlay::LeaveRoom => {
                        dialog_header(ui, Icon::Logout, "退出房间？");
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(format!(
                                "退出后会与房间 {} 断开，并清掉保存的会话，下次要重新输房间码。本机的历史记录会留下。",
                                self.room_code
                            ))
                            .size(12.5)
                            .color(theme::MUTED),
                        );
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if danger_button(ui, "退出").clicked() {
                                    action = DialogAction::Leave;
                                }
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
                        });
                    }
                    Overlay::Settings => {
                        dialog_header(ui, Icon::Settings, "设置");
                        ui.add_space(12.0);
                        section_label(ui, "同步");
                        toggle_row(ui, "收到后写入剪贴板", &mut self.write_incoming);
                        hint(ui, "关掉后收到的文本只进历史，不动本机剪贴板。");
                        ui.add_space(12.0);
                        section_label(ui, "房间");
                        ui.horizontal(|ui| {
                            if self.joined {
                                ui.label(
                                    RichText::new(self.room_code.clone())
                                        .monospace()
                                        .size(12.5)
                                        .color(theme::TEXT),
                                );
                                ui.label(
                                    RichText::new(status_label(&self.status))
                                        .size(11.0)
                                        .color(theme::MUTED),
                                );
                            } else {
                                ui.label(
                                    RichText::new("未共享")
                                        .size(12.5)
                                        .color(theme::MUTED),
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if self.joined {
                                    if secondary_button(ui, "退出房间").clicked() {
                                        action = DialogAction::Open(Overlay::LeaveRoom);
                                    }
                                    if secondary_button(ui, "切换房间").clicked() {
                                        action = DialogAction::Open(Overlay::Share);
                                    }
                                } else if accent_button(ui, "进入房间").clicked() {
                                    action = DialogAction::Open(Overlay::Share);
                                }
                            });
                        });
                        ui.add_space(12.0);
                        section_label(ui, "本机");
                        info_row(ui, "设备名称", &crate::paths::device_name());
                        info_row(ui, "所属系统", crate::paths::device_os());
                        info_row(ui, "版本", env!("CARGO_PKG_VERSION"));
                        ui.add_space(12.0);
                        hint(ui, "房间密码加密保存在本机；离开房间即清除。");
                        ui.add_space(14.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary_button(ui, "知道了").clicked() {
                                close = true;
                            }
                        });
                    }
                    Overlay::NewEntry { text } => {
                        dialog_header(ui, Icon::Plus, "新建一条");
                        ui.add_space(10.0);
                        let edit = ui.add_sized(
                            Vec2::new(ui.available_width(), 150.0),
                            TextEdit::multiline(text)
                                .hint_text("输入或粘贴要同步的文本")
                                .margin(Margin::symmetric(10, 8))
                                .font(FontId::proportional(13.0)),
                        );
                        if focus {
                            edit.request_focus();
                        }
                        let send = ui.input_mut(|input| {
                            input.consume_key(Modifiers::COMMAND, Key::Enter)
                        });
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if accent_button_enabled(ui, "发送", !text.trim().is_empty())
                                    .clicked()
                                    || (send && !text.trim().is_empty())
                                {
                                    action = DialogAction::NewEntry(text.clone());
                                }
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
                        });
                    }
                    Overlay::Edit { id, text } => {
                        dialog_header(ui, Icon::Edit, "编辑内容");
                        ui.add_space(10.0);
                        let edit = ui.add_sized(
                            Vec2::new(ui.available_width(), 170.0),
                            TextEdit::multiline(text)
                                .margin(Margin::symmetric(10, 8))
                                .font(FontId::proportional(13.0)),
                        );
                        if focus {
                            edit.request_focus();
                        }
                        let save = ui.input(|input| input.key_pressed(Key::Enter));
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if accent_button(ui, "保存").clicked() || save {
                                    action =
                                        DialogAction::SaveEdit(id.clone(), text.clone());
                                }
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
                        });
                    }
                    Overlay::Note { id, text } => {
                        dialog_header(ui, Icon::Note, "备注");
                        ui.add_space(10.0);
                        let edit = ui.add_sized(
                            Vec2::new(ui.available_width(), 110.0),
                            TextEdit::multiline(text)
                                .hint_text("给这条记点什么，只在本机可见")
                                .margin(Margin::symmetric(10, 8))
                                .font(FontId::proportional(13.0)),
                        );
                        if focus {
                            edit.request_focus();
                        }
                        hint(ui, "备注只保存在本机，不会同步给其他设备。");
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if accent_button(ui, "保存").clicked() {
                                    action =
                                        DialogAction::SaveNote(id.clone(), text.clone());
                                }
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
                        });
                    }
                    Overlay::Detail { id } => {
                        dialog_header(ui, Icon::Info, "条目详情");
                        ui.add_space(10.0);
                        let record = self
                            .history
                            .items
                            .iter()
                            .find(|record| &record.id == id)
                            .cloned();
                        if let Some(record) = record {
                            egui::ScrollArea::vertical()
                                .max_height(260.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let body = if record.text.trim().is_empty() {
                                        "（空文本）"
                                    } else {
                                        record.text.as_str()
                                    };
                                    ui.label(
                                        RichText::new(body)
                                            .size(12.5)
                                            .color(theme::TEXT),
                                    );
                                    if !record.note.is_empty() {
                                        ui.add_space(10.0);
                                        field_label(ui, "备注");
                                        ui.label(
                                            RichText::new(&record.note)
                                                .size(11.5)
                                                .color(theme::MUTED),
                                        );
                                    }
                                    ui.add_space(10.0);
                                    field_label(ui, "记录信息");
                                    let mut meta = format!(
                                        "{} · 来自 {}",
                                        relative_time(record.ts, now_millis()),
                                        record.sender_name
                                    );
                                    if record.pinned {
                                        meta.push_str(" · 已置顶");
                                    }
                                    if record.moved() {
                                        meta.push_str(" · 已手动排序");
                                    }
                                    ui.label(
                                        RichText::new(meta)
                                            .size(11.0)
                                            .color(theme::MUTED),
                                    );
                                    ui.add_space(6.0);
                                    field_label(ui, "记录 ID");
                                    ui.label(
                                        RichText::new(&record.id)
                                            .monospace()
                                            .size(10.5)
                                            .color(theme::FAINT),
                                    );
                                });
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                if secondary_button(ui, "复制").clicked() {
                                    action = DialogAction::Copy(record.text.clone());
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if secondary_button(ui, "关闭").clicked() {
                                        close = true;
                                    }
                                });
                            });
                        } else {
                            ui.label(
                                RichText::new("这条记录已经不在了。")
                                    .size(12.5)
                                    .color(theme::MUTED),
                            );
                            ui.add_space(14.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if secondary_button(ui, "关闭").clicked() {
                                    close = true;
                                }
                            });
                        }
                    }
                    Overlay::MoveTo { id, input, error } => {
                        dialog_header(ui, Icon::Move, "移动到指定位置");
                        ui.add_space(12.0);
                        let info = self.history.move_info(id);
                        if let Some(info) = info {
                            hint(
                                ui,
                                &format!(
                                    "可移动到第 {}–{} 位，当前第 {} 位",
                                    info.from, info.to, info.position
                                ),
                            );
                            ui.add_space(6.0);
                            let edit = ui.add_sized(
                                Vec2::new(ui.available_width(), 34.0),
                                TextEdit::singleline(input)
                                    .hint_text("位置")
                                    .margin(Margin::symmetric(10, 8))
                                    .font(FontId::proportional(13.0)),
                            );
                            if focus {
                                edit.request_focus();
                            }
                            if edit.changed() {
                                *error = false;
                            }
                            let parsed = input.trim().parse::<usize>().ok();
                            let valid = parsed
                                .is_some_and(|target| target >= info.from && target <= info.to);
                            if *error || (parsed.is_some() && !valid) {
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(format!(
                                        "请输入 {} 到 {} 之间的位置数字",
                                        info.from, info.to
                                    ))
                                    .size(10.5)
                                    .color(theme::RED),
                                );
                            }
                            let enter = ui.input(|input_state| {
                                input_state.key_pressed(Key::Enter)
                            });
                            ui.add_space(14.0);
                            let mut target = None;
                            ui.horizontal(|ui| {
                                ui.with_layout(
                                    Layout::right_to_left(Align::Center),
                                    |ui| {
                                        if accent_button_enabled(ui, "确定", valid).clicked()
                                            || (enter && valid)
                                        {
                                            target = parsed;
                                        }
                                        if secondary_button(ui, "取消").clicked() {
                                            close = true;
                                        }
                                    },
                                );
                            });
                            if let Some(target) = target {
                                action = DialogAction::MoveTo(id.clone(), target);
                            }
                        } else {
                            hint(ui, "这条记录已经不在了。");
                            ui.add_space(14.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if secondary_button(ui, "关闭").clicked() {
                                    close = true;
                                }
                            });
                        }
                    }
                }
            });
        if response.should_close() {
            close = true;
        }

        let mut keep = !close;
        match action {
            DialogAction::None => {}
            DialogAction::Open(next) => {
                self.focus_dialog = true;
                self.overlay = Some(next);
                keep = false;
            }
            DialogAction::Join => {
                self.join();
                keep = false;
            }
            DialogAction::Leave => {
                self.leave();
                keep = false;
            }
            DialogAction::ClearAll => {
                self.clear_all();
                keep = false;
            }
            DialogAction::NewEntry(text) => {
                self.publish_manual(text.trim_end());
                keep = false;
            }
            DialogAction::SaveEdit(id, text) => {
                if self.history.update_text(&id, &text) {
                    self.notify("已改好 1 条");
                }
                keep = false;
            }
            DialogAction::SaveNote(id, text) => {
                if self.history.update_note(&id, &text) {
                    self.notify("备注已保存");
                }
                keep = false;
            }
            DialogAction::MoveTo(id, target) => {
                if let Some(position) = self.history.move_to(&id, target) {
                    self.cursor = Some(id.clone());
                    self.nav_scroll = Some(id);
                    self.notify(format!("已移动到第 {position} 位"));
                }
                keep = false;
            }
            DialogAction::Copy(text) => {
                self.copy_to_clipboard(&text, "已复制");
            }
        }
        if keep && self.overlay.is_none() {
            self.overlay = Some(overlay);
        }
        self.history.flush();
    }
}

fn clip_row(ui: &mut Ui, view: RowView<'_>, now: i64, events: &mut Vec<AppEvent>, scroll_to: bool) {
    let RowView {
        record,
        position,
        cursor,
        picked,
        pick_mode,
    } = view;
    let font = FontId::proportional(14.0);
    let left_pad = 12.0;
    let number_w = 26.0;
    let action_reserve = 8.0 + 4.0 * 30.0 + 3.0 * 2.0 + 8.0;
    let avail = ui.available_width();
    let card_w = (avail - theme::ROW_INSET * 2.0).max(240.0);
    let text_w = (card_w - left_pad - number_w - 8.0 - action_reserve - 10.0).max(150.0);

    let empty = record.text.is_empty();
    let display = if empty {
        "（空文本）"
    } else {
        record.text.as_str()
    };
    let mut body = egui::text::LayoutJob {
        wrap: TextWrapping {
            max_width: text_w,
            max_rows: 24,
            overflow_character: Some('…'),
            ..Default::default()
        },
        ..Default::default()
    };
    body.append(
        display,
        0.0,
        TextFormat {
            font_id: font.clone(),
            color: if empty { theme::FAINT } else { theme::TEXT },
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(body));
    let text_h = galley.size().y.max(20.0);
    let height = 9.0 + text_h + 5.0 + 16.0 + 9.0;
    let (outer, _) = ui.allocate_exact_size(Vec2::new(avail, height + 8.0), Sense::hover());
    let card = Rect::from_min_max(
        Pos2::new(outer.left() + theme::ROW_INSET, outer.top() + 4.0),
        Pos2::new(
            outer.left() + theme::ROW_INSET + card_w,
            outer.bottom() - 4.0,
        ),
    );
    let id = ui.scope_id().with(("clip-row", record.id.as_str()));
    let card_response = ui.interact(card, id.with("card"), Sense::click());
    if scroll_to {
        ui.scroll_to_rect(card, Some(Align::Center));
    }

    let painter = ui.painter();
    let hovered = card_response.contains_pointer();
    let revealed = hovered || cursor;
    let fill = if picked {
        Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 38)
    } else if cursor {
        Color32::from_white_alpha(9)
    } else {
        theme::with_alpha(theme::CARD, 128)
    };
    let border = if picked {
        Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 130)
    } else if cursor {
        Color32::from_white_alpha(48)
    } else {
        Color32::from_rgba_unmultiplied(0x3a, 0x2a, 0x33, 110)
    };
    painter.rect(
        card,
        CornerRadius::same(18),
        fill,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    painter.hline(
        (card.left() + 16.0)..=(card.right() - 16.0),
        card.top() + 1.0,
        Stroke::new(1.0, Color32::from_white_alpha(10)),
    );

    let text_pos = Pos2::new(card.left() + left_pad + number_w + 8.0, card.top() + 10.0);
    if pick_mode {
        let center = Pos2::new(card.left() + left_pad + 9.0, card.top() + 19.0);
        painter.circle_stroke(
            center,
            8.0,
            Stroke::new(1.4, if picked { theme::ACCENT } else { theme::MUTED }),
        );
        if picked {
            painter.circle_filled(center, 7.0, theme::ACCENT);
            icons::paint(
                painter,
                Rect::from_center_size(center, Vec2::splat(11.0)),
                Icon::Check,
                theme::ACCENT_TEXT,
            );
        }
    } else {
        painter.text(
            Pos2::new(card.left() + left_pad + number_w, card.top() + 10.0),
            Align2::RIGHT_TOP,
            position.to_string(),
            FontId::proportional(13.0),
            if record.pinned {
                theme::AMBER
            } else {
                theme::MUTED
            },
        );
    }

    painter.galley(text_pos, galley, theme::TEXT);

    let mut meta = relative_time(record.ts, now);
    if !record.sender_name.is_empty() {
        meta.push_str(" · 来自 ");
        meta.push_str(&record.sender_name);
    }
    if record.pinned {
        meta.push_str(" · 已置顶");
    }
    if !record.note.is_empty() {
        meta.push_str(" · 有备注");
    }
    if record.moved() {
        meta.push_str(" · 已手动排序");
    }
    paint_line(
        ui,
        Pos2::new(text_pos.x, text_pos.y + text_h + 5.0),
        &meta,
        FontId::proportional(11.5),
        theme::MUTED,
        card.right() - text_pos.x - 12.0,
    );

    let action_size = 30.0;
    let mut right = card.right() - 8.0;
    let more_rect = Rect::from_min_size(
        Pos2::new(right - action_size, card.top() + 7.0),
        Vec2::splat(action_size),
    );
    right -= action_size + 2.0;
    let pin_rect = Rect::from_min_size(
        Pos2::new(right - action_size, card.top() + 7.0),
        Vec2::splat(action_size),
    );
    right -= action_size + 2.0;
    let copy_rect = Rect::from_min_size(
        Pos2::new(right - action_size, card.top() + 7.0),
        Vec2::splat(action_size),
    );
    right -= action_size + 2.0;
    let send_rect = Rect::from_min_size(
        Pos2::new(right - action_size, card.top() + 7.0),
        Vec2::splat(action_size),
    );

    let more_response = ui.interact(more_rect, id.with("more"), Sense::click());
    if more_response.hovered() {
        painter.rect_filled(
            more_rect,
            CornerRadius::same(9),
            Color32::from_white_alpha(10),
        );
    }
    icons::paint(
        painter,
        Rect::from_center_size(more_rect.center(), Vec2::splat(17.0)),
        Icon::More,
        if more_response.hovered() {
            theme::TEXT
        } else {
            theme::MUTED
        },
    );

    let mut menu_choice: Option<MenuAction> = None;
    egui::Popup::menu(&more_response)
        .width(216.0)
        .frame(menu_frame())
        .show(|ui| {
            ui.set_width(216.0);
            if menu_item(ui, Icon::Edit, "编辑内容", false) {
                menu_choice = Some(MenuAction::Edit);
            }
            if menu_item(ui, Icon::Note, "写备注", false) {
                menu_choice = Some(MenuAction::Note);
            }
            if menu_item(ui, Icon::Duplicate, "生成副本", false) {
                menu_choice = Some(MenuAction::Duplicate);
            }
            if menu_item(ui, Icon::Info, "查看详情", false) {
                menu_choice = Some(MenuAction::Detail);
            }
            menu_separator(ui);
            if menu_item(ui, Icon::ArrowUp, "向上移", false) {
                menu_choice = Some(MenuAction::MoveUp);
            }
            if menu_item(ui, Icon::ArrowDown, "向下移", false) {
                menu_choice = Some(MenuAction::MoveDown);
            }
            if menu_item(ui, Icon::Top, "移到最顶部", false) {
                menu_choice = Some(MenuAction::MoveTop);
            }
            if menu_item(ui, Icon::Bottom, "移到最底部", false) {
                menu_choice = Some(MenuAction::MoveBottom);
            }
            if menu_item(ui, Icon::Move, "移动到指定位置…", false) {
                menu_choice = Some(MenuAction::MoveTo);
            }
            menu_separator(ui);
            if menu_item(ui, Icon::Trash, "删除", true) {
                menu_choice = Some(MenuAction::Delete);
            }
        });

    let mut icon_action: Option<AppEvent> = None;
    if revealed {
        let send_response = row_action(ui, send_rect, id.with("send"), Icon::Send, "补传到房间");
        if send_response.clicked() {
            icon_action = Some(AppEvent::Publish(record.id.clone()));
        }
        let copy_response = row_action(ui, copy_rect, id.with("copy"), Icon::Copy, "复制");
        if copy_response.clicked() {
            icon_action = Some(AppEvent::Copy(record.id.clone()));
        }
        let pin_response = row_action(
            ui,
            pin_rect,
            id.with("pin"),
            Icon::Pin,
            if record.pinned {
                "取消置顶"
            } else {
                "置顶"
            },
        );
        if pin_response.clicked() {
            icon_action = Some(AppEvent::Pin(record.id.clone()));
        }
    }

    if let Some(action) = menu_choice {
        events.push(AppEvent::Menu(record.id.clone(), action));
    } else if let Some(event) = icon_action {
        events.push(event);
    } else if card_response.clicked() && !more_response.clicked() {
        if pick_mode {
            events.push(AppEvent::TogglePick(record.id.clone()));
        } else {
            events.push(AppEvent::Activate(record.id.clone()));
        }
    }

    let cursor = CursorIcon::PointingHand;
    card_response.on_hover_cursor(cursor);
}

fn row_action(ui: &mut Ui, rect: Rect, id: Id, icon: Icon, tip: &str) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(9), Color32::from_white_alpha(10));
    }
    icons::paint(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        icon,
        if response.hovered() {
            theme::TEXT
        } else {
            theme::MUTED
        },
    );
    response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tip)
}

fn banner(
    ui: &mut Ui,
    icon: Icon,
    color: Color32,
    error: bool,
    text: &str,
    action: Option<&str>,
) -> bool {
    let avail = ui.available_width();
    let width = (avail - theme::ROW_INSET * 2.0).max(240.0);
    let (outer, _) = ui.allocate_exact_size(Vec2::new(avail, 62.0), Sense::hover());
    let card = Rect::from_min_max(
        Pos2::new(outer.left() + theme::ROW_INSET, outer.top() + 4.0),
        Pos2::new(
            outer.left() + theme::ROW_INSET + width,
            outer.bottom() - 4.0,
        ),
    );
    let fill = if error {
        Color32::from_rgba_unmultiplied(0x4c, 0x1d, 0x1d, 210)
    } else {
        Color32::from_rgba_unmultiplied(0x26, 0x1b, 0x21, 190)
    };
    let stroke = if error {
        Color32::from_rgba_unmultiplied(0xf8, 0x71, 0x71, 90)
    } else {
        Color32::from_rgba_unmultiplied(0x3a, 0x2a, 0x33, 140)
    };
    let text_color = if error {
        theme::ON_ERROR_CONTAINER
    } else {
        theme::TEXT
    };
    ui.painter().rect(
        card,
        CornerRadius::same(16),
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(card.left() + 20.0, card.center().y),
            Vec2::splat(17.0),
        ),
        icon,
        color,
    );
    let action_width = if action.is_some() { 74.0 } else { 0.0 };
    let text_width = (card.width() - 40.0 - action_width - 20.0).max(80.0);
    paint_line(
        ui,
        Pos2::new(card.left() + 40.0, card.center().y - 7.0),
        text,
        FontId::proportional(12.0),
        text_color,
        text_width,
    );
    let Some(label) = action else {
        return false;
    };
    let pill = Rect::from_min_max(
        Pos2::new(card.right() - 12.0 - 72.0, card.center().y - 16.0),
        Pos2::new(card.right() - 8.0, card.center().y + 16.0),
    );
    let response = ui.interact(pill, Id::unique(("banner-action", text)), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(pill, CornerRadius::same(8), Color32::from_white_alpha(12));
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(pill.left() + 16.0, pill.center().y),
            Vec2::splat(14.0),
        ),
        Icon::Key,
        text_color,
    );
    ui.painter().text(
        Pos2::new(pill.left() + 28.0, pill.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        text_color,
    );
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn menu_frame() -> Frame {
    Frame::new()
        .fill(theme::SURFACE_HIGH)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(6))
        .shadow(egui::epaint::Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(120),
        })
}

fn menu_item(ui: &mut Ui, icon: Icon, label: &str, danger: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let color = if danger { theme::RED } else { theme::TEXT };
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            if danger {
                Color32::from_rgba_unmultiplied(0xf8, 0x71, 0x71, 24)
            } else {
                Color32::from_white_alpha(9)
            },
        );
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.left() + 17.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        icon,
        if danger { theme::RED } else { theme::MUTED },
    );
    ui.painter().text(
        Pos2::new(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.5),
        color,
    );
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(
        (rect.left() + 8.0)..=(rect.right() - 8.0),
        rect.center().y,
        Stroke::new(1.0, theme::BORDER),
    );
}

fn paint_chip(ui: &Ui, rect: Rect, label: &str, active: bool, font: FontId) {
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(8),
        if active {
            Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 34)
        } else {
            Color32::TRANSPARENT
        },
        Stroke::new(
            1.0,
            if active {
                Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 150)
            } else {
                Color32::from_white_alpha(14)
            },
        ),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        if active { theme::ACCENT } else { theme::MUTED },
    );
}

fn dialog_header(ui: &mut Ui, icon: Icon, title: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), Vec2::splat(20.0)),
            icon,
            theme::ACCENT,
        );
        ui.label(RichText::new(title).size(18.0).strong().color(theme::TEXT));
    });
}

fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(theme::MUTED));
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(10.5).color(theme::FAINT));
}

fn section_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(10.0).strong().color(theme::FAINT));
    ui.add_space(4.0);
}

fn bullet(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 16.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 2.0, theme::ACCENT);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).size(11.0).color(theme::MUTED));
        });
    });
    ui.add_space(6.0);
}

fn info_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.5).color(theme::MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).size(11.5).color(theme::TEXT));
        });
    });
    ui.add_space(2.0);
}

fn toggle_row(ui: &mut Ui, label: &str, value: &mut bool) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    ui.painter().text(
        Pos2::new(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.5),
        theme::TEXT,
    );
    let switch = Rect::from_center_size(
        Pos2::new(rect.right() - 18.0, rect.center().y),
        Vec2::new(36.0, 20.0),
    );
    ui.painter().rect_filled(
        switch,
        CornerRadius::same(10),
        if *value {
            theme::ACCENT
        } else {
            Color32::from_white_alpha(18)
        },
    );
    let knob_x = if *value {
        switch.right() - 10.0
    } else {
        switch.left() + 10.0
    };
    ui.painter().circle_filled(
        Pos2::new(knob_x, switch.center().y),
        7.0,
        if *value {
            theme::ACCENT_TEXT
        } else {
            theme::MUTED
        },
    );
    if response.clicked() {
        *value = !*value;
    }
}

fn accent_button(ui: &mut Ui, label: &str) -> egui::Response {
    accent_button_enabled(ui, label, true)
}

fn accent_button_enabled(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    let button = egui::Button::new(RichText::new(label).size(13.0).strong().color(if enabled {
        theme::ACCENT_TEXT
    } else {
        theme::FAINT
    }))
    .fill(if enabled {
        theme::ACCENT
    } else {
        Color32::from_white_alpha(8)
    })
    .corner_radius(CornerRadius::same(10))
    .min_size(Vec2::new(84.0, 32.0));
    ui.add_enabled(enabled, button)
}

fn secondary_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(13.0).color(theme::MUTED))
            .fill(Color32::from_white_alpha(7))
            .stroke(Stroke::new(1.0, Color32::from_white_alpha(14)))
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(72.0, 32.0)),
    )
}

fn danger_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .size(13.0)
                .strong()
                .color(theme::ON_RED),
        )
        .fill(theme::RED)
        .corner_radius(CornerRadius::same(10))
        .min_size(Vec2::new(84.0, 32.0)),
    )
}

fn status_color(status: &Status) -> Color32 {
    match status {
        Status::Idle => theme::FAINT,
        Status::Connecting => theme::AMBER,
        Status::Online { .. } => theme::GREEN,
        Status::Offline { .. } => theme::RED,
    }
}

fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Idle => "未共享",
        Status::Connecting => "连接中…",
        Status::Online { .. } => "已连接",
        Status::Offline { .. } => "未连接",
    }
}

fn paint_line(ui: &Ui, pos: Pos2, text: &str, font: FontId, color: Color32, max_width: f32) {
    let mut job = egui::text::LayoutJob {
        wrap: TextWrapping {
            max_width,
            max_rows: 1,
            overflow_character: Some('…'),
            ..Default::default()
        },
        ..Default::default()
    };
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(pos, galley, color);
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn search_id() -> Id {
    Id::unique("search-field")
}

fn sanitize_room(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .map(|ch| ch.to_ascii_uppercase())
        .take(24)
        .collect()
}

fn random_room_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";
    let mut bytes = [0u8; 6];
    if getrandom::fill(&mut bytes).is_err() {
        return "K7M2QP".to_string();
    }
    bytes
        .iter()
        .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
        .collect()
}

fn new_id() -> String {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        return now_millis().to_string();
    }
    hex::encode(bytes)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn relative_time(ts: i64, now: i64) -> String {
    let seconds = ((now - ts) / 1000).max(0);
    if seconds < 60 {
        "刚刚".to_string()
    } else if seconds < 3_600 {
        format!("{} 分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 小时前", seconds / 3_600)
    } else if seconds < 30 * 86_400 {
        format!("{} 天前", seconds / 86_400)
    } else {
        format!("{} 个月前", seconds / (30 * 86_400))
    }
}

#[cfg(test)]
mod tests {
    use super::{random_room_code, relative_time, sanitize_room};

    #[test]
    fn relative_time_speaks_chinese() {
        assert_eq!(relative_time(1_000, 30_000), "刚刚");
        assert_eq!(relative_time(0, 60_000), "1 分钟前");
        assert_eq!(relative_time(0, 59 * 60_000), "59 分钟前");
        assert_eq!(relative_time(0, 3 * 3_600_000), "3 小时前");
        assert_eq!(relative_time(0, 2 * 86_400_000), "2 天前");
        assert_eq!(relative_time(0, 40 * 86_400_000), "1 个月前");
        assert_eq!(relative_time(5_000, 1_000), "刚刚");
    }

    #[test]
    fn room_codes_are_uppercased_and_filtered() {
        assert_eq!(sanitize_room(" ab-12_c "), "AB-12_C");
        assert_eq!(sanitize_room("房间 ab"), "AB");
        assert_eq!(
            sanitize_room("abcdefghijklmnopqrstuvwxyz012345"),
            "ABCDEFGHIJKLMNOPQRSTUVWX"
        );
    }

    #[test]
    fn random_room_codes_have_the_expected_shape() {
        for _ in 0..32 {
            let code = random_room_code();
            assert_eq!(code.chars().count(), 6);
            assert!(code
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()));
        }
    }
}
