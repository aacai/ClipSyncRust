use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::text::TextWrapping;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Key, Layout, Margin,
    Modifiers, Order, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, TextFormat,
    TextureHandle, TextureOptions, Ui, UiBuilder, Vec2, ViewportCommand,
};

use clipsync_core::engine::{Clip, Config, EngineHandle, Event, FileUpload, Peer, Status};
use clipsync_core::files::{human_size, is_image, sanitize_file_name, MAX_FILE_BYTES};
use clipsync_core::protocol::room_hash;
use clipsync_core::RoomCrypto;

use crate::clipboard::{Captured, ClipFile, ClipboardWatcher, POLL_INTERVAL};
use crate::icons::{self, Icon};
use crate::jobs::{
    self, DownloadAfter, DownloadRequest, JobContext, JobEvent, UploadRequest, UploadTarget,
};
use crate::settings::{
    normalize_nickname, Language, Preferences, Settings, ThemeMode, RETENTION_CHOICES,
};
use crate::store::{FileInfo, History, Record, KIND_FILE, KIND_TEXT};
use crate::text_tools::{self, TextOp};
use crate::theme;
use crate::transfer::TransferClient;
use crate::translate::{self, Failure as TranslateFailure, Target as TranslateTarget};
use crate::update::{self, UpdateEvent, UpdateState};
use crate::{autostart, overlay, paths, screenshot};

const TOAST_INFO: Duration = Duration::from_secs(4);
const TOAST_ERROR: Duration = Duration::from_secs(6);
const TOAST_UNDO: Duration = Duration::from_secs(9);
/// Time for the app window to actually leave the screen before the grab.
const CAPTURE_HIDE_DELAY: Duration = Duration::from_millis(140);
const THUMB_MAX_SIDE: u32 = 256;
const ZOOM_MAX_SIDE: u32 = 2048;
const REPO_URL: &str = "https://github.com/aacai/ClipSyncRust";

const HELP_STEPS: [&str; 6] = [
    "打开「房间共享」：顶栏的钥匙图标，或未共享时列表里的「共享」。填一个房间码（可随机换一个），房间密码可选，设了就必须两端一致。",
    "另一台设备装同一个 App，或直接开网页，填完全一样的房间码和密码，点「进入房间」。",
    "核对两端顶栏的安全码：逐位相同才说明用的是同一把钥匙。不同就是房间码或密码打错了，这种连得上却永远收不到对方的内容。",
    "进房后任意一端复制文本，另一端立刻收到；点条目就写回本机剪贴板，文件走工具栏的发送文件。",
    "网页端例外：浏览器不允许后台读剪贴板，网页上复制完要手动点「发送剪贴板」。",
    "掉线期间的复制不会补发，两台都显示「已连接」时再传。",
];

pub struct ClipSyncApp {
    engine: EngineHandle,
    history: History,
    settings: Settings,
    preferences: Preferences,
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
    last_poll: Instant,
    cursor: Option<String>,
    picked: BTreeSet<String>,
    pick_mode: bool,
    nav_scroll: Option<String>,
    overlay: Option<Overlay>,
    screen: Screen,
    theme_mode: ThemeMode,
    language: Language,
    focus_dialog: bool,
    focus_search: bool,
    toast: Option<Toast>,
    header_bottom: f32,
    translate_rx: Option<mpsc::Receiver<Result<String, translate::TranslationError>>>,
    room_crypto: Option<Arc<RoomCrypto>>,
    room_hash: String,
    client: Arc<TransferClient>,
    job_tx: mpsc::Sender<JobEvent>,
    job_rx: mpsc::Receiver<JobEvent>,
    transfer: Option<TransferUi>,
    pending_downloads: HashSet<String>,
    pending_cache: HashMap<String, String>,
    thumbnails: HashMap<String, ThumbEntry>,
    zoom: Option<String>,
    zoom_texture: Option<(String, TextureHandle)>,
    zoom_failed: Option<String>,
    watch_clipboard: bool,
    auto_copy: bool,
    watch_files: bool,
    watch_images: bool,
    update: UpdateState,
    update_rx: Option<mpsc::Receiver<UpdateEvent>>,
    autostart_available: bool,
    hotkeys: Option<screenshot::Hotkeys>,
    /// The screenshot overlay frozen on screen right now, shared with its own window.
    shot: Option<Arc<Mutex<overlay::Session>>>,
    /// The overlay window is on screen with the frozen frame in it.
    shot_visible: bool,
    /// When the app window will be gone long enough to grab the screen behind it.
    capture_at: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsChoice {
    Theme,
    Language,
    Retention,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsAction {
    Theme,
    Language,
    Retention,
    Hotkey,
    Nickname,
    RememberRoom(bool),
    LaunchAtLogin(bool),
    WatchClipboard(bool),
    AutoCopy(bool),
    WatchImages(bool),
    WatchFiles(bool),
    DeleteExpired(bool),
    EditRoom,
    About,
    LeaveRoom,
    Quit,
    CheckUpdate,
    DownloadUpdate,
}

struct TransferUi {
    upload: bool,
    name: String,
    done: i64,
    total: i64,
}

impl TransferUi {
    fn fraction(&self) -> f32 {
        if self.total > 0 {
            (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

struct ThumbEntry {
    path: String,
    texture: Option<TextureHandle>,
    attempted: bool,
}

#[derive(Clone)]
enum Overlay {
    Share,
    Help,
    About,
    Safety,
    ClearAll,
    LeaveRoom,
    Choice(SettingsChoice),
    Nickname {
        input: String,
    },
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
    Transform {
        id: String,
    },
    Translate {
        id: String,
        source: String,
        target: TranslateTarget,
        running: bool,
        result: Option<String>,
        failure: Option<String>,
    },
    Detail {
        id: String,
    },
    MoveTo {
        id: String,
        input: String,
        error: bool,
    },
    Hotkey {
        error: Option<String>,
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
    CopyRecord(String),
    ApplyOp(String, TextOp),
    AddLocal(String),
    ReplaceText(String, String),
    SetTheme(ThemeMode),
    SetLanguage(Language),
    SetRetention(u32),
    SetHotkey(String),
    SaveNickname(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Edit,
    Note,
    Transform,
    Translate,
    Duplicate,
    SaveText,
    SaveFile,
    Resend,
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
    Open(String),
    Download(String),
    Zoom(String),
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
    online: bool,
    can_up: bool,
    can_down: bool,
    thumb: Option<&'a TextureHandle>,
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
    pub fn new(
        engine: EngineHandle,
        history: History,
        settings: Settings,
        preferences: Preferences,
        hotkeys: Option<screenshot::Hotkeys>,
    ) -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let watch_clipboard = preferences.watch_clipboard();
        let auto_copy = preferences.auto_copy();
        let watch_files = preferences.watch_files();
        let watch_images = preferences.watch_images();
        let theme_mode = preferences.theme();
        let language = preferences.language();
        let autostart_available = autostart::is_available();
        let mut app = Self {
            engine,
            history,
            settings,
            preferences,
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
            last_poll: Instant::now(),
            cursor: None,
            picked: BTreeSet::new(),
            pick_mode: false,
            nav_scroll: None,
            overlay: None,
            screen: Screen::Main,
            theme_mode,
            language,
            focus_dialog: false,
            focus_search: false,
            toast: None,
            header_bottom: theme::TOP_INSET + 160.0,
            translate_rx: None,
            room_crypto: None,
            room_hash: String::new(),
            client: Arc::new(TransferClient::new()),
            job_tx,
            job_rx,
            transfer: None,
            pending_downloads: HashSet::new(),
            pending_cache: HashMap::new(),
            thumbnails: HashMap::new(),
            zoom: None,
            zoom_texture: None,
            zoom_failed: None,
            watch_clipboard,
            auto_copy,
            watch_files,
            watch_images,
            update: UpdateState::Idle,
            update_rx: None,
            autostart_available,
            hotkeys,
            shot: None,
            shot_visible: false,
            capture_at: None,
        };
        let restore = app.preferences.remember_room() && app.settings.session.is_some();
        if !app.preferences.remember_room() {
            app.settings.forget();
        }
        if restore {
            if let Some(session) = app.settings.session.clone() {
                app.room_code = session.room;
                app.password = session.password;
                app.join();
            }
        } else {
            app.room_code = app
                .preferences
                .last_room_code()
                .map(str::to_string)
                .unwrap_or_else(random_room_code);
        }
        if app.preferences.delete_expired() {
            app.prune_expired();
        }
        if autostart_available && app.preferences.launch_at_login() {
            let _ = autostart::set_enabled(true);
        }
        app.apply_hotkey();
        app
    }

    fn sender_name(&self) -> String {
        let nickname = self.preferences.nickname();
        if nickname.is_empty() {
            paths::device_name()
        } else {
            nickname.to_string()
        }
    }

    fn prune_expired(&mut self) {
        let days = self.preferences.retention_days() as i64;
        let cutoff = now_millis() - days * 86_400_000;
        let count = self.history.remove_expired(cutoff);
        if count > 0 {
            self.notify(format!("清掉了 {count} 条过期条目"));
        }
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.engine.events.try_recv() {
            match event {
                Event::Status(status) => {
                    if matches!(status, Status::Online { .. }) {
                        if let Some((room, password)) = self.pending_session.take() {
                            self.preferences.remember_last_room(&room);
                            if self.preferences.remember_room() {
                                self.settings.remember(&room, &password);
                            }
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
                Event::FileTooLarge(name) => self.notify_error(format!(
                    "收到超过 {}MB 的文件，已忽略：{name}",
                    clipsync_core::files::max_mb()
                )),
            }
        }
        self.history.flush();
    }

    fn on_clip(&mut self, clip: Clip) {
        let incoming = !clip.mine;
        let known = self.history.items.iter().any(|item| item.id == clip.mid);
        let inserted = self.history.add(Record::from_clip(&clip));
        if let Some(path) = self.pending_cache.remove(&clip.mid) {
            self.history.mark_cached(&clip.mid, &path);
        }
        if incoming {
            let sender = if clip.sender_name.trim().is_empty() {
                "同屋设备"
            } else {
                clip.sender_name.as_str()
            };
            if clip.file.is_some() {
                let has_link = self
                    .history
                    .file(&clip.mid)
                    .is_some_and(|file| !file.link.trim().is_empty());
                if !known && has_link {
                    self.request_download(&clip.mid, DownloadAfter::Auto);
                }
                return;
            }
            if !self.auto_copy {
                if inserted {
                    self.notify(format!("收到 {sender} 的文本，已存进列表"));
                }
                return;
            }
            // 屋里另一台机器（或本机另一个 ClipSync）把我们刚发出去的内容又推回来。
            // 这时候用户的剪贴板可能已经换成更新的一份了，写下去就是把新的盖掉。
            if self.clipboard.is_echo(&clip.text) {
                return;
            }
            self.clipboard.write(&clip.text);
            if inserted {
                self.notify(format!("收到 {sender} 的文本，已进剪贴板"));
            }
        }
    }

    fn drain_jobs(&mut self) {
        while let Ok(event) = self.job_rx.try_recv() {
            match event {
                JobEvent::Progress {
                    upload,
                    name,
                    done,
                    total,
                } => {
                    self.transfer = Some(TransferUi {
                        upload,
                        name,
                        done,
                        total,
                    });
                }
                JobEvent::Uploaded(done) => {
                    self.transfer = None;
                    self.finish_upload(done);
                }
                JobEvent::Downloaded(done) => {
                    self.transfer = None;
                    self.pending_downloads.remove(&done.id);
                    self.finish_download(done);
                }
                JobEvent::Failed(message) => {
                    self.transfer = None;
                    self.notify_error(message);
                }
            }
        }
    }

    fn finish_upload(&mut self, done: jobs::UploadDone) {
        match done.target {
            UploadTarget::Resend { .. } => {
                self.engine.publish_file(
                    FileUpload {
                        mid: done.id.clone(),
                        ts: done.ts,
                        name: done.name.clone(),
                        mime: done.mime.clone(),
                        size: done.size,
                        sha256: done.sha256.clone(),
                        link: done.link.clone(),
                        expires_at: done.expires_at,
                    },
                    false,
                );
                self.history
                    .fill_file_meta(&done.id, &done.link, &done.sha256, done.expires_at);
                self.notify(format!("已发送 {}", done.name));
            }
            UploadTarget::Publish { .. } => {
                if let Some(path) = &done.local_path {
                    self.pending_cache.insert(done.id.clone(), path.clone());
                }
                self.engine.publish_file(
                    FileUpload {
                        mid: done.id.clone(),
                        ts: done.ts,
                        name: done.name.clone(),
                        mime: done.mime.clone(),
                        size: done.size,
                        sha256: done.sha256.clone(),
                        link: done.link.clone(),
                        expires_at: done.expires_at,
                    },
                    true,
                );
                self.notify(format!("已发送 {}", done.name));
            }
        }
    }

    fn finish_download(&mut self, done: jobs::DownloadDone) {
        if !self.history.mark_cached(&done.id, &done.path) {
            return;
        }
        self.thumbnails.remove(&done.id);
        let name = sanitize_file_name(&done.name);
        match done.after {
            DownloadAfter::Auto => {}
            DownloadAfter::Cache => self.notify(format!("已下载 {name}")),
            DownloadAfter::Open => {
                if let Err(error) = crate::paths::open_path(&done.path) {
                    self.notify_error(format!("打开失败：{error}"));
                }
            }
            DownloadAfter::Save => self.save_downloaded(&name, &done.path),
            DownloadAfter::CopyImage => {
                let bytes = done.bytes.as_deref().unwrap_or_default();
                match self.clipboard.write_image(bytes) {
                    Ok(()) => self.notify("已把图片放进剪贴板，可直接粘贴"),
                    Err(error) => self.notify_error(format!("写入剪贴板失败：{error}")),
                }
            }
        }
    }

    fn save_downloaded(&mut self, name: &str, path: &str) {
        let name = if name.is_empty() { "file" } else { name };
        match std::fs::read(path) {
            Ok(bytes) => match crate::paths::save_to_downloads(name, &bytes) {
                Ok(written) => self.notify(format!("已存到下载目录：{written}")),
                Err(error) => self.notify_error(format!("保存失败：{error}")),
            },
            Err(error) => self.notify_error(format!("保存失败：{error}")),
        }
    }

    fn poll_clipboard(&mut self) {
        if !self.watch_clipboard || self.last_poll.elapsed() < POLL_INTERVAL {
            return;
        }
        self.last_poll = Instant::now();
        match self.clipboard.poll(self.watch_files, self.watch_images) {
            Some(Captured::Text(text)) => {
                if matches!(self.status, Status::Online { .. }) {
                    self.clipboard.note_published(&text);
                    self.engine.publish_text(&text);
                } else {
                    self.record_local(&text);
                }
            }
            Some(Captured::Files(files)) => self.capture_files(files),
            Some(Captured::Image(image)) => self.capture_image(image),
            None => {}
        }
    }

    fn capture_files(&mut self, files: Vec<ClipFile>) {
        let online = matches!(self.status, Status::Online { .. });
        let mut uploads = Vec::new();
        for file in files {
            let name = sanitize_file_name(&file.name);
            if name.is_empty() {
                continue;
            }
            if file.size <= 0 {
                self.notify_error(format!("空文件无法分享：{name}"));
                continue;
            }
            if file.size > MAX_FILE_BYTES {
                self.notify_error(format!(
                    "文件超过 {}MB，暂不支持：{name}",
                    clipsync_core::files::max_mb()
                ));
                continue;
            }
            let Ok(bytes) = std::fs::read(&file.path) else {
                self.notify_error(format!("读取本机文件失败：{name}"));
                continue;
            };
            if bytes.is_empty() {
                self.notify_error(format!("空文件无法分享：{name}"));
                continue;
            }
            if online {
                if self.room_crypto.is_some() {
                    uploads.push(UploadRequest {
                        name,
                        mime: file.mime().to_string(),
                        bytes,
                        target: UploadTarget::Publish { cache: true },
                    });
                }
            } else {
                self.record_local_file(&name, file.mime(), &bytes);
            }
        }
        self.start_uploads(uploads);
    }

    fn capture_image(&mut self, image: crate::clipboard::ClipImage) {
        let name = sanitize_file_name(&image.name);
        if matches!(self.status, Status::Online { .. }) && self.room_crypto.is_some() {
            self.start_uploads(vec![UploadRequest {
                name,
                mime: image.mime().to_string(),
                bytes: image.png,
                target: UploadTarget::Publish { cache: true },
            }]);
        } else {
            self.record_local_file(&name, image.mime(), &image.png);
        }
    }

    fn record_local_file(&mut self, name: &str, mime: &str, bytes: &[u8]) {
        let ts = now_millis();
        let id = new_id();
        let path = crate::paths::write_cache(&id, name, bytes).ok();
        let sender_name = self.sender_name();
        self.history.add(Record {
            id: id.clone(),
            kind: KIND_FILE.to_string(),
            ts,
            sender_id: crate::paths::install_id(),
            sender_name,
            text: name.to_string(),
            file: Some(FileInfo {
                name: name.to_string(),
                mime: mime.to_string(),
                size: bytes.len() as i64,
                sha256: clipsync_core::crypto::sha256_hex(bytes),
                link: String::new(),
                expires_at: 0,
                cached: path.is_some(),
                local_path: path,
            }),
            pinned: false,
            order: 0,
            note: String::new(),
        });
        self.history.flush();
    }

    fn start_uploads(&mut self, requests: Vec<UploadRequest>) {
        if requests.is_empty() {
            return;
        }
        let Some(context) = self.job_context() else {
            self.notify_error("还没进房间：文件暂时只能留在本机");
            return;
        };
        // 先把进度条架起来：托管方从握手到回链接可能要几十秒，别让界面看着像没反应
        let first = &requests[0];
        self.transfer = Some(TransferUi {
            upload: true,
            name: first.name.clone(),
            done: 0,
            total: first.bytes.len() as i64,
        });
        jobs::spawn_uploads(context, requests, self.job_tx.clone());
    }

    fn job_context(&self) -> Option<JobContext> {
        Some(JobContext {
            crypto: self.room_crypto.clone()?,
            client: self.client.clone(),
            device_id: crate::paths::install_id(),
        })
    }

    fn record_local(&mut self, text: &str) {
        let ts = now_millis();
        let sender_name = self.sender_name();
        self.history.add(Record {
            id: new_id(),
            kind: KIND_TEXT.to_string(),
            ts,
            sender_id: crate::paths::install_id(),
            sender_name,
            text: text.to_string(),
            file: None,
            pinned: false,
            order: 0,
            note: String::new(),
        });
        self.history.flush();
    }

    fn publish_manual(&mut self, text: &str) {
        let length = text.encode_utf16().count();
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
        let crypto = match RoomCrypto::open(&room, &password) {
            Ok(crypto) => crypto,
            Err(error) => {
                self.notify_error(format!("无法进入房间：{error}"));
                return;
            }
        };
        let key = crypto.key_material().to_vec();
        self.room_crypto = Some(Arc::new(crypto));
        self.room_hash = room_hash(&room);
        // 换房间：上一间的钥匙先清掉，新链路没连上才不会留着旧的再自动回去
        if self
            .settings
            .session
            .as_ref()
            .is_some_and(|session| session.room != room)
        {
            self.settings.forget();
        }
        self.pending_session = Some((room.clone(), password.clone()));
        let device_name = self.sender_name();
        self.engine.join(Config {
            room_id: room,
            password,
            room_key: Some(key),
            device_id: crate::paths::install_id(),
            device_name,
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
        self.room_crypto = None;
        self.room_hash.clear();
        self.transfer = None;
        self.joined = false;
        self.status = Status::Idle;
        self.peers.clear();
        self.mismatch.clear();
        self.overlay = None;
        self.pick_mode = false;
        self.picked.clear();
        self.notify("已退出房间");
    }

    fn open_settings(&mut self) {
        self.screen = Screen::Settings;
        self.overlay = None;
        self.pick_mode = false;
        self.picked.clear();
    }

    fn close_settings(&mut self) {
        self.screen = Screen::Main;
        self.overlay = None;
    }

    fn set_theme(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.theme_mode = mode;
        self.preferences.set_theme(mode);
        theme::apply(ctx, mode);
        ctx.request_repaint();
    }

    fn set_language(&mut self, language: Language) {
        self.language = language;
        self.preferences.set_language(language);
    }

    fn set_remember_room(&mut self, enabled: bool) {
        self.preferences.set_remember_room(enabled);
        if !enabled {
            self.settings.forget();
        }
    }

    fn set_watch_clipboard(&mut self, enabled: bool) {
        self.watch_clipboard = enabled;
        self.preferences.set_watch_clipboard(enabled);
        if enabled {
            self.clipboard.reseed();
            self.last_poll = Instant::now();
        }
    }

    fn set_auto_copy(&mut self, enabled: bool) {
        self.auto_copy = enabled;
        self.preferences.set_auto_copy(enabled);
    }

    fn set_watch_images(&mut self, enabled: bool) {
        self.watch_images = enabled;
        self.preferences.set_watch_images(enabled);
        if enabled {
            self.clipboard.reseed();
        }
    }

    fn set_watch_files(&mut self, enabled: bool) {
        self.watch_files = enabled;
        self.preferences.set_watch_files(enabled);
        if enabled {
            self.clipboard.reseed();
        }
    }

    fn set_launch_at_login(&mut self, enabled: bool) {
        if !self.autostart_available {
            return;
        }
        match autostart::set_enabled(enabled) {
            Ok(()) => self.preferences.set_launch_at_login(enabled),
            Err(_) => self.notify_error("没能改到系统的登录项里"),
        }
    }

    fn set_delete_expired(&mut self, enabled: bool) {
        self.preferences.set_delete_expired(enabled);
        if enabled {
            self.prune_expired();
        }
    }

    fn set_retention_days(&mut self, days: u32) {
        self.preferences.set_retention_days(days);
        if self.preferences.delete_expired() {
            self.prune_expired();
        }
    }

    fn set_nickname(&mut self, name: &str) {
        let name = normalize_nickname(name);
        self.preferences.set_nickname(&name);
        let sender = self.sender_name();
        self.engine.rename(&sender);
    }

    fn apply_hotkey(&mut self) {
        let spec = self.preferences.screenshot_hotkey().to_string();
        if let Some(hotkeys) = self.hotkeys.as_mut() {
            if let Err(error) = hotkeys.apply(&spec) {
                self.notify_error(error);
            }
        }
    }

    fn set_screenshot_hotkey(&mut self, spec: &str) -> Result<(), String> {
        // 注册没成功就别写盘：否则下次启动还是同一个坏组合键，每次都报错
        let result = match self.hotkeys.as_mut() {
            Some(hotkeys) => hotkeys.apply(spec),
            None => Ok(()),
        };
        result?;
        self.preferences.set_screenshot_hotkey(spec);
        self.notify(format!("截屏快捷键已改为 {spec}"));
        Ok(())
    }

    /// Freeze the screen, then open the WeChat overlay on top of everything: the frame goes grey
    /// and the user picks a window with a click or draws a region before anything is cut.
    fn start_capture(&mut self, ctx: &egui::Context) {
        if self.shot.is_some() || self.capture_at.is_some() {
            return;
        }
        if screenshot::permission_missing() {
            self.notify_error(
                "缺少屏幕录制权限：系统设置 → 隐私与安全性 → 屏幕与系统音频录制，勾上 ClipSyncRust",
            );
            return;
        }
        // Our own window is never part of the screenshot: it is what the user is leaving behind.
        // A transparent GL window also comes back as a black hole from the screen grabber.
        self.capture_at = Some(Instant::now() + CAPTURE_HIDE_DELAY);
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        ctx.request_repaint_after(CAPTURE_HIDE_DELAY);
    }

    /// Grabs the screen once the app window has actually left it and hands the frame to the
    /// overlay window, which is already built. The app window stays hidden until the overlay has
    /// the frozen screen painted, so nothing flashes between the two.
    fn take_capture(&mut self, ctx: &egui::Context) {
        let Some(at) = self.capture_at else { return };
        let now = Instant::now();
        if now < at {
            ctx.request_repaint_after(at - now);
            return;
        }
        self.capture_at = None;
        match screenshot::freeze() {
            Ok(freeze) => {
                self.shot = Some(Arc::new(Mutex::new(overlay::Session::new(freeze))));
            }
            Err(error) => {
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                self.notify_error(format!("截屏失败：{error}"));
            }
        }
    }

    /// Feed the one overlay window — built at startup, never closed — with the frozen screen,
    /// show it only once it covers the whole display, and collect the region the moment the user
    /// settles on one.
    fn service_shot(&mut self, ctx: &egui::Context) {
        let session = self.shot.clone();
        ctx.show_viewport_deferred(overlay::viewport_id(), overlay::viewport(), move |ui, _| {
            let Some(session) = &session else { return };
            if let Ok(mut session) = session.lock() {
                session.draw(ui);
            }
        });

        let Some(session) = self.shot.clone() else {
            return;
        };

        if !self.shot_visible {
            let Some((position, size)) = session.lock().ok().map(|guard| guard.screen()) else {
                return;
            };
            if overlay::cover(ctx, position, size) {
                self.shot_visible = true;
                overlay::reveal(ctx, true);
            } else {
                // The window is not up yet; asking for another pass is cheaper than a cut-out
                // that lands on a half-moved overlay.
                ctx.request_repaint();
            }
            return;
        }

        // None until the user is done, Some(None) for a cancelled shot, Some(Some) for a crop.
        let shot = session.lock().ok().and_then(|mut guard| {
            let outcome = guard.take_outcome()?;
            Some(match outcome {
                overlay::Outcome::Cancel => None,
                overlay::Outcome::Crop(rect) => {
                    let image = guard.take_image()?;
                    let size = Vec2::new(image.width() as f32, image.height() as f32);
                    let (x, y, width, height) = overlay::crop_px(rect, size, guard.scale());
                    let cropped = image::imageops::crop_imm(&image, x, y, width, height).to_image();
                    screenshot::encode(cropped).ok()
                }
            })
        });
        let Some(shot) = shot else { return };
        overlay::reveal(ctx, false);
        self.shot_visible = false;
        self.shot = None;
        if let Some(shot) = shot {
            self.commit_shot(shot);
        }
    }

    fn commit_shot(&mut self, shot: screenshot::Shot) {
        if shot.png.len() as i64 > MAX_FILE_BYTES {
            self.notify_error(format!(
                "这块区域超过 {}MB，这次没有截",
                clipsync_core::files::max_mb()
            ));
            return;
        }
        if let Err(error) = self.clipboard.write_image(&shot.png) {
            self.notify_error(format!("写入剪贴板失败：{error}"));
            return;
        }
        let name = format!("screenshot-{}.png", &shot.hash[..8]);
        self.capture_image(crate::clipboard::ClipImage {
            png: shot.png,
            hash: shot.hash,
            name,
        });
        self.notify("已截屏：图片进了剪贴板，也存进了列表");
    }

    fn check_updates(&mut self) {
        if matches!(
            self.update,
            UpdateState::Checking | UpdateState::Downloading { .. }
        ) {
            return;
        }
        self.update = UpdateState::Checking;
        let (sender, receiver) = mpsc::channel();
        self.update_rx = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(UpdateEvent::Checked(update::latest()));
        });
    }

    fn download_update(&mut self) {
        let UpdateState::Available {
            asset: Some(asset), ..
        } = &self.update
        else {
            return;
        };
        let asset = asset.clone();
        self.update = UpdateState::Downloading {
            asset: asset.clone(),
            received: 0,
        };
        let (sender, receiver) = mpsc::channel();
        self.update_rx = Some(receiver);
        std::thread::spawn(move || {
            let progress = asset.clone();
            let result = update::download(&asset.url, |received| {
                let _ = sender.send(UpdateEvent::Progress(progress.clone(), received));
            });
            let _ = sender.send(UpdateEvent::Downloaded(asset, result));
        });
    }

    fn drain_updates(&mut self) {
        let mut events = Vec::new();
        if let Some(receiver) = &self.update_rx {
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                UpdateEvent::Checked(Ok(release)) => {
                    self.update = if update::is_behind(env!("CARGO_PKG_VERSION"), &release.version)
                    {
                        let asset =
                            update::asset_for(&release, paths::device_os(), &update::arch())
                                .cloned();
                        UpdateState::Available { release, asset }
                    } else {
                        UpdateState::UpToDate(release.version)
                    };
                }
                UpdateEvent::Checked(Err(message)) => {
                    self.update = UpdateState::Failed(message);
                }
                UpdateEvent::Progress(asset, received) => {
                    if matches!(self.update, UpdateState::Downloading { .. }) {
                        self.update = UpdateState::Downloading { asset, received };
                    }
                }
                UpdateEvent::Downloaded(asset, Ok(bytes)) => {
                    self.update = match paths::save_to_downloads(&asset.name, &bytes) {
                        Ok(name) => UpdateState::Downloaded(name),
                        Err(error) => UpdateState::Failed(error.to_string()),
                    };
                }
                UpdateEvent::Downloaded(_, Err(message)) => {
                    self.update = UpdateState::Failed(message);
                }
            }
        }
    }

    fn settings_screen(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 20.0).clamp(280.0, 640.0);
        let bar_bottom = self.settings_bar(ctx);
        let mut action: Option<SettingsAction> = None;

        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| {
                let left = screen.center().x - width / 2.0;
                let rect = Rect::from_min_max(
                    Pos2::new(left, screen.top()),
                    Pos2::new(left + width, screen.bottom()),
                );
                ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    let bar_top = screen.top() + theme::TOP_INSET + 4.0;
                    ui.add_space(bar_top - screen.top());
                    egui::ScrollArea::vertical()
                        .id_salt("settings-scroll")
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .show(ui, |ui| {
                            ui.set_width(width);
                            ui.add_space(bar_bottom - bar_top + 14.0);
                            ui.spacing_mut().item_spacing.y = 0.0;

                            settings_group(ui, "外观", |ui| {
                                if settings_nav_row(
                                    ui,
                                    "主题",
                                    theme_label(self.theme_mode),
                                ) {
                                    action = Some(SettingsAction::Theme);
                                }
                                settings_divider(ui);
                                if settings_nav_row(
                                    ui,
                                    "语言",
                                    language_label(self.language),
                                ) {
                                    action = Some(SettingsAction::Language);
                                }
                            });
                            ui.add_space(22.0);

                            let joined = self.joined;
                            let room = self.room_code.clone();
                            let label = status_label(&self.status);
                            let color = room_color(&self.status, joined);
                            let remember = self.preferences.remember_room();
                            let launch = self.preferences.launch_at_login();
                            settings_group(ui, "同步", |ui| {
                                if settings_value_row(
                                    ui,
                                    "房间",
                                    if joined { &room } else { "未共享" },
                                    joined.then_some((label, color.unwrap_or(theme::p().muted))),
                                    joined,
                                    if joined { color } else { None },
                                    true,
                                ) {
                                    action = Some(SettingsAction::EditRoom);
                                }
                                if joined {
                                    settings_divider(ui);
                                    if settings_action_row(ui, "退出房间") {
                                        action = Some(SettingsAction::LeaveRoom);
                                    }
                                }
                                settings_divider(ui);
                                if settings_switch_row(
                                    ui,
                                    "记住房间",
                                    "重开时自动回到上次的房间。房间码明文存着方便重填，派生密钥和你输入的密码加密后存在本机设置里；离开房间即清除",
                                    remember,
                                ) {
                                    action = Some(SettingsAction::RememberRoom(!remember));
                                }
                                if self.autostart_available {
                                    settings_divider(ui);
                                    if settings_switch_row(
                                        ui,
                                        "开机自动启动",
                                        "登录后自动开始守剪贴板，先收进托盘不弹主窗口；要界面就点托盘图标",
                                        launch,
                                    ) {
                                        action =
                                            Some(SettingsAction::LaunchAtLogin(!launch));
                                    }
                                }
                            });
                            ui.add_space(22.0);

                            let watch = self.watch_clipboard;
                            let auto_copy = self.auto_copy;
                            let watch_images = self.watch_images;
                            let watch_files = self.watch_files;
                            settings_group(ui, "剪贴板", |ui| {
                                if settings_switch_row(
                                    ui,
                                    "记录剪贴板",
                                    "不进房间也会自动把你复制过的内容留进列表，仅保存在本机；关闭后只记录手动发送的条目",
                                    watch,
                                ) {
                                    action = Some(SettingsAction::WatchClipboard(!watch));
                                }
                                if watch {
                                    settings_divider(ui);
                                    if settings_switch_row(
                                        ui,
                                        "记录截图",
                                        "复制图片或截图后自动存进列表；在房间里同时发给同伴。超过上限的大图会被跳过",
                                        watch_images,
                                    ) {
                                        action =
                                            Some(SettingsAction::WatchImages(!watch_images));
                                    }
                                    settings_divider(ui);
                                    if settings_switch_row(
                                        ui,
                                        "记录文件",
                                        "复制文件后自动存进列表；在房间里同时发给同伴。超过上限的文件会被跳过",
                                        watch_files,
                                    ) {
                                        action =
                                            Some(SettingsAction::WatchFiles(!watch_files));
                                    }
                                }
                                settings_divider(ui);
                                if settings_switch_row(
                                    ui,
                                    "收到即进剪贴板",
                                    "关掉后，同伴发来的文本只存进本机列表，不碰你的剪贴板；要点条目里的复制才会写入",
                                    auto_copy,
                                ) {
                                    action = Some(SettingsAction::AutoCopy(!auto_copy));
                                }
                            });
                            ui.add_space(22.0);

                            let hotkey = self.preferences.screenshot_hotkey().to_string();
                            if self.hotkeys.is_some() {
                                settings_group(ui, "截屏", |ui| {
                                    if settings_nav_row(ui, "截屏快捷键", &hotkey) {
                                        action = Some(SettingsAction::Hotkey);
                                    }
                                    settings_divider(ui);
                                    settings_footnote(
                                        ui,
                                        "按一下截下整屏：图直接进剪贴板，也存进列表发给同屋",
                                    );
                                });
                                ui.add_space(22.0);
                            }

                            let delete_expired = self.preferences.delete_expired();
                            let retention = self.preferences.retention_days();
                            let nickname = self.sender_name();
                            settings_group(ui, "关于", |ui| {
                                settings_value_row(
                                    ui,
                                    "版本",
                                    env!("CARGO_PKG_VERSION"),
                                    None,
                                    false,
                                    None,
                                    false,
                                );
                                settings_divider(ui);
                                if let Some(chosen) = settings_update_row(ui, &self.update) {
                                    action = Some(chosen);
                                }
                                settings_divider(ui);
                                settings_value_row(
                                    ui,
                                    "本地记录",
                                    &format!("{} 条", self.history.items.len()),
                                    None,
                                    false,
                                    None,
                                    false,
                                );
                                settings_divider(ui);
                                if settings_switch_row(
                                    ui,
                                    "删除过期条目",
                                    "开启后，超过保留时长的条目连同本机缓存一起清掉；置顶的条目不参与",
                                    delete_expired,
                                ) {
                                    action =
                                        Some(SettingsAction::DeleteExpired(!delete_expired));
                                }
                                if delete_expired {
                                    settings_divider(ui);
                                    if settings_nav_row(
                                        ui,
                                        "保留时长",
                                        &format!("{retention} 天"),
                                    ) {
                                        action = Some(SettingsAction::Retention);
                                    }
                                }
                                settings_divider(ui);
                                if settings_nav_row(ui, "昵称", &nickname) {
                                    action = Some(SettingsAction::Nickname);
                                }
                                settings_divider(ui);
                                settings_value_row(
                                    ui,
                                    "所属系统",
                                    paths::device_os(),
                                    None,
                                    false,
                                    None,
                                    false,
                                );
                                settings_divider(ui);
                                if settings_nav_row(ui, "关于", "项目信息") {
                                    action = Some(SettingsAction::About);
                                }
                                settings_divider(ui);
                                if settings_action_row(ui, "退出 ClipSyncRust") {
                                    action = Some(SettingsAction::Quit);
                                }
                            });
                            ui.add_space(30.0);
                        });
                });
            });

        if let Some(action) = action {
            self.apply_settings_action(ctx, action);
        }
    }

    fn settings_bar(&mut self, ctx: &egui::Context) -> f32 {
        let screen = ctx.content_rect();
        let width = (screen.width() - 20.0).clamp(280.0, 640.0);
        let pos = Pos2::new(
            screen.center().x - width / 2.0,
            screen.top() + theme::TOP_INSET + 4.0,
        );
        let mut back = false;
        let response = egui::Area::new(Id::unique("settings-bar"))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.set_width(width);
                Frame::new()
                    .fill(theme::glass_fill())
                    .stroke(Stroke::new(1.0, theme::p().glass_border))
                    .corner_radius(CornerRadius::same(theme::BAR_RADIUS))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(if theme::p().dark { 70 } else { 36 }),
                    })
                    .inner_margin(Margin {
                        left: 8,
                        right: 16,
                        top: 9,
                        bottom: 9,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if icons::icon_button(ui, Icon::ArrowBack, true, "返回").clicked() {
                                back = true;
                            }
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("设置")
                                    .size(16.0)
                                    .strong()
                                    .color(theme::p().text),
                            );
                        });
                    });
            });
        theme::frost(
            ctx,
            response.response.rect,
            theme::BAR_RADIUS,
            theme::frost_layer(),
        );
        if back {
            self.close_settings();
        }
        response.response.rect.bottom()
    }

    fn apply_settings_action(&mut self, ctx: &egui::Context, action: SettingsAction) {
        match action {
            SettingsAction::Theme => self.open(Overlay::Choice(SettingsChoice::Theme)),
            SettingsAction::Language => self.open(Overlay::Choice(SettingsChoice::Language)),
            SettingsAction::Retention => self.open(Overlay::Choice(SettingsChoice::Retention)),
            SettingsAction::Hotkey => self.open(Overlay::Hotkey { error: None }),
            SettingsAction::Nickname => self.open(Overlay::Nickname {
                input: self.preferences.nickname().to_string(),
            }),
            SettingsAction::RememberRoom(enabled) => self.set_remember_room(enabled),
            SettingsAction::LaunchAtLogin(enabled) => self.set_launch_at_login(enabled),
            SettingsAction::WatchClipboard(enabled) => self.set_watch_clipboard(enabled),
            SettingsAction::AutoCopy(enabled) => self.set_auto_copy(enabled),
            SettingsAction::WatchImages(enabled) => self.set_watch_images(enabled),
            SettingsAction::WatchFiles(enabled) => self.set_watch_files(enabled),
            SettingsAction::DeleteExpired(enabled) => self.set_delete_expired(enabled),
            SettingsAction::EditRoom => self.open(Overlay::Share),
            SettingsAction::About => self.open(Overlay::About),
            SettingsAction::LeaveRoom => self.open(Overlay::LeaveRoom),
            SettingsAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            SettingsAction::CheckUpdate => self.check_updates(),
            SettingsAction::DownloadUpdate => self.download_update(),
        }
    }

    fn copy_to_clipboard(&mut self, text: &str, message: &str) {
        if text.is_empty() {
            return;
        }
        self.clipboard.write(text);
        self.notify(message);
    }

    fn copy_record(&mut self, id: &str) {
        let Some(record) = self.record(id).cloned() else {
            return;
        };
        let Some(file) = record.file else {
            self.copy_to_clipboard(&record.text, "已复制");
            return;
        };
        if let Some(path) = file.local_path.filter(|_| file.cached) {
            if self.clipboard.write_files(&[PathBuf::from(path)]) {
                self.notify("已把文件放进剪贴板，可直接粘贴");
            } else {
                self.notify_error("写入剪贴板失败");
            }
        } else if is_image(&file.mime) {
            self.request_download(id, DownloadAfter::CopyImage);
        } else {
            self.notify_error("这条文件还没有下载到本机");
        }
    }

    fn activate_record(&mut self, id: &str) {
        let Some(record) = self.record(id).cloned() else {
            return;
        };
        let Some(file) = record.file else {
            self.copy_record(id);
            return;
        };
        if is_image(&file.mime) {
            if file.cached {
                self.copy_record(id);
            } else {
                self.request_download(id, DownloadAfter::CopyImage);
            }
        } else if file.cached {
            self.open_record(id);
        } else {
            self.request_download(id, DownloadAfter::Cache);
        }
    }

    fn open_record(&mut self, id: &str) {
        let Some(file) = self.history.file(id) else {
            return;
        };
        match file.local_path.filter(|_| file.cached) {
            Some(path) => {
                if let Err(error) = crate::paths::open_path(&path) {
                    self.notify_error(format!("打开失败：{error}"));
                }
            }
            None => self.request_download(id, DownloadAfter::Open),
        }
    }

    fn download_record(&mut self, id: &str) {
        self.request_download(id, DownloadAfter::Cache);
    }

    fn request_download(&mut self, id: &str, after: DownloadAfter) {
        if self.pending_downloads.contains(id) {
            return;
        }
        let Some(file) = self.history.file(id) else {
            return;
        };
        let link = file.link.clone();
        if link.trim().is_empty() {
            self.notify_error(format!(
                "下载失败：{} 还没有上传链接",
                sanitize_file_name(&file.name)
            ));
            return;
        }
        let Some(context) = self.job_context() else {
            self.notify_error("还没进房间，暂时无法下载");
            return;
        };
        self.pending_downloads.insert(id.to_string());
        jobs::spawn_download(
            context,
            DownloadRequest {
                id: id.to_string(),
                name: file.name,
                mime: file.mime,
                size: file.size,
                sha256: file.sha256,
                link,
                after,
            },
            self.job_tx.clone(),
        );
    }

    fn save_record(&mut self, id: &str) {
        let Some(record) = self.record(id).cloned() else {
            return;
        };
        let Some(file) = record.file else {
            let name = format!("text-{}.txt", &new_id()[..8]);
            match crate::paths::save_to_downloads(&name, record.text.as_bytes()) {
                Ok(written) => self.notify(format!("已存到下载目录：{written}")),
                Err(error) => self.notify_error(format!("保存失败：{error}")),
            }
            return;
        };
        if let Some(path) = file.local_path.filter(|_| file.cached) {
            self.save_downloaded(&sanitize_file_name(&file.name), &path);
        } else {
            self.request_download(id, DownloadAfter::Save);
        }
    }

    fn resend_record(&mut self, id: &str) {
        if !matches!(self.status, Status::Online { .. }) {
            self.notify_error("还没进房间：先进入房间再补传");
            return;
        }
        let Some(record) = self.record(id).cloned() else {
            return;
        };
        let Some(file) = record.file else {
            return;
        };
        if !file.link.is_empty() {
            return;
        }
        let Some(path) = file.local_path else {
            self.notify_error(format!("发送失败：{} 没有本地缓存", file.name));
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            self.notify_error(format!("发送失败：读取本机缓存失败（{}）", file.name));
            return;
        };
        self.start_uploads(vec![UploadRequest {
            name: sanitize_file_name(&file.name),
            mime: file.mime,
            bytes,
            target: UploadTarget::Resend {
                id: id.to_string(),
                ts: record.ts,
            },
        }]);
    }

    fn pick_files(&mut self) {
        if !matches!(self.status, Status::Online { .. }) || self.room_crypto.is_none() {
            return;
        }
        let Some(paths) = rfd::FileDialog::new().pick_files() else {
            return;
        };
        let mut uploads = Vec::new();
        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let name = sanitize_file_name(&name);
            let size = std::fs::metadata(&path)
                .map(|meta| meta.len() as i64)
                .unwrap_or(0);
            if size <= 0 || size > MAX_FILE_BYTES {
                self.notify_error(format!(
                    "文件超过 {}MB，暂不支持：{}",
                    clipsync_core::files::max_mb(),
                    if name.is_empty() { "文件" } else { &name }
                ));
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                self.notify_error(format!("读取本机文件失败：{name}"));
                continue;
            };
            uploads.push(UploadRequest {
                name: name.clone(),
                mime: clipsync_core::files::mime_for_name(&name).to_string(),
                bytes,
                target: UploadTarget::Publish { cache: false },
            });
        }
        self.start_uploads(uploads);
    }

    fn record(&self, id: &str) -> Option<&Record> {
        self.history.items.iter().find(|record| record.id == id)
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
        self.notify(format!("已同步 {} 字", text.encode_utf16().count()));
    }

    fn toggle_pin_record(&mut self, id: &str) {
        self.history.toggle_pin(id);
    }

    fn undo(&mut self) {
        if let Some(batch) = self.history.undo() {
            self.notify(format!("已恢复 {} 条", batch.len()));
            if batch.len() == 1 {
                if let Some(record) = batch.first() {
                    if record
                        .file
                        .as_ref()
                        .is_some_and(|file| !file.link.is_empty())
                    {
                        self.request_download(&record.id, DownloadAfter::Auto);
                    }
                }
            }
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
            self.thumbnails.remove(id);
            if self.zoom.as_deref() == Some(id.as_str()) {
                self.zoom = None;
            }
        }
        self.notify_undo(format!("已删除 {count} 条"));
    }

    fn delete_selection(&mut self) {
        let ids: Vec<String> = self.picked.iter().cloned().collect();
        self.delete_records(ids);
    }

    fn ensure_thumbnails(&mut self, ctx: &egui::Context) {
        let wanted: Vec<(String, String)> = self
            .history
            .items
            .iter()
            .filter_map(|record| {
                let file = record.file.as_ref()?;
                if !is_image(&file.mime) {
                    return None;
                }
                let path = file.local_path.as_ref().filter(|_| file.cached)?;
                Some((record.id.clone(), path.clone()))
            })
            .collect();
        for (id, path) in wanted {
            let stale = self
                .thumbnails
                .get(&id)
                .map_or(true, |entry| entry.path != path);
            if stale {
                let texture = load_texture(ctx, &path, THUMB_MAX_SIDE, format!("thumb-{id}"));
                self.thumbnails.insert(
                    id,
                    ThumbEntry {
                        path,
                        texture,
                        attempted: true,
                    },
                );
            } else if let Some(entry) = self.thumbnails.get_mut(&id) {
                if !entry.attempted {
                    entry.attempted = true;
                    entry.texture =
                        load_texture(ctx, &entry.path, THUMB_MAX_SIDE, format!("thumb-{id}"));
                }
            }
        }
        self.thumbnails
            .retain(|id, _| self.history.items.iter().any(|record| &record.id == id));
    }

    fn open_zoom(&mut self, id: &str) {
        if self.history.file(id).is_none() {
            return;
        }
        self.zoom = Some(id.to_string());
        self.zoom_texture = None;
        self.zoom_failed = None;
    }

    fn close_zoom(&mut self) {
        self.zoom = None;
        self.zoom_texture = None;
        self.zoom_failed = None;
    }

    fn zoom_file_path(&self) -> Option<String> {
        let file = self.history.file(self.zoom.as_deref()?)?;
        if !is_image(&file.mime) {
            return None;
        }
        file.local_path.filter(|_| file.cached)
    }

    fn ensure_zoom(&mut self, ctx: &egui::Context) {
        if self.zoom.is_none() {
            self.zoom_texture = None;
            self.zoom_failed = None;
            return;
        }
        let Some(path) = self.zoom_file_path() else {
            return;
        };
        if self
            .zoom_texture
            .as_ref()
            .is_some_and(|(cached, _)| cached == &path)
            || self.zoom_failed.as_deref() == Some(path.as_str())
        {
            return;
        }
        let name = format!("zoom-{}", self.zoom.as_deref().unwrap_or_default());
        match load_texture(ctx, &path, ZOOM_MAX_SIDE, name) {
            Some(texture) => self.zoom_texture = Some((path, texture)),
            None => self.zoom_failed = Some(path),
        }
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
        let Some(payload) = self.selection_payload() else {
            return;
        };
        self.copy_to_clipboard(&payload, "已复制");
    }

    fn selection_payload(&self) -> Option<String> {
        selection_payload(&self.history.items, &self.picked)
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

    fn save_text_record(&mut self, id: &str) {
        let Some(text) = self
            .history
            .items
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.text.clone())
        else {
            return;
        };
        let name = format!("text-{}.txt", &new_id()[..8]);
        match crate::paths::save_to_downloads(&name, text.as_bytes()) {
            Ok(written) => self.notify(format!("已存到下载目录：{written}")),
            Err(error) => self.notify_error(format!("保存失败：{error}")),
        }
    }

    fn add_local_text(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        self.history.add(Record {
            id: new_id(),
            kind: KIND_TEXT.to_string(),
            ts: now_millis(),
            sender_id: crate::paths::install_id(),
            sender_name: crate::paths::device_name(),
            text: text.to_string(),
            file: None,
            pinned: false,
            order: 0,
            note: String::new(),
        });
        self.history.flush();
    }

    fn poll_translation(&mut self) {
        let Some(receiver) = self.translate_rx.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(outcome) => {
                if let Some(Overlay::Translate {
                    running,
                    result,
                    failure,
                    ..
                }) = self.overlay.as_mut()
                {
                    *running = false;
                    match outcome {
                        Ok(text) => {
                            *failure = None;
                            *result = Some(text);
                        }
                        Err(error) => {
                            *result = None;
                            *failure = Some(translate_failure_text(&error));
                        }
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => self.translate_rx = Some(receiver),
            Err(mpsc::TryRecvError::Disconnected) => {
                if let Some(Overlay::Translate {
                    running, failure, ..
                }) = self.overlay.as_mut()
                {
                    *running = false;
                    *failure = Some("翻译失败，请稍后再试".to_string());
                }
            }
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
        if self.zoom.is_some() {
            if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
                self.close_zoom();
            }
            return;
        }
        if self.screen == Screen::Settings {
            if self.overlay.is_none()
                && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
            {
                self.close_settings();
            }
            return;
        }
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
                    self.activate_record(&id);
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
                self.activate_record(&id);
            }
            AppEvent::TogglePick(id) => self.toggle_pick(&id),
            AppEvent::Copy(id) => self.copy_record(&id),
            AppEvent::Publish(id) => self.publish_record(&id),
            AppEvent::Pin(id) => self.toggle_pin_record(&id),
            AppEvent::Open(id) => self.open_record(&id),
            AppEvent::Download(id) => self.download_record(&id),
            AppEvent::Zoom(id) => self.open_zoom(&id),
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
            MenuAction::Transform => {
                if self.history.items.iter().any(|record| record.id == id) {
                    self.open(Overlay::Transform { id });
                }
            }
            MenuAction::Translate => {
                if let Some(text) = self
                    .history
                    .items
                    .iter()
                    .find(|record| record.id == id)
                    .map(|record| record.text.clone())
                {
                    self.open(Overlay::Translate {
                        id,
                        target: translate::detect_target(&text),
                        source: text,
                        running: false,
                        result: None,
                        failure: None,
                    });
                }
            }
            MenuAction::Duplicate => self.duplicate_record(&id),
            MenuAction::SaveText => self.save_text_record(&id),
            MenuAction::SaveFile => self.save_record(&id),
            MenuAction::Resend => self.resend_record(&id),
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

impl ClipSyncApp {
    fn tick(&mut self, ctx: &egui::Context) {
        if self.theme_mode == ThemeMode::System
            && theme::resolve_dark(ctx, self.theme_mode) != theme::p().dark
        {
            theme::apply(ctx, self.theme_mode);
        }
        self.drain_events();
        self.drain_jobs();
        self.drain_updates();
        if self
            .hotkeys
            .as_ref()
            .is_some_and(|hotkeys| hotkeys.take_pressed())
        {
            self.start_capture(ctx);
        }
        self.take_capture(ctx);
        self.service_shot(ctx);
        self.poll_clipboard();
        self.poll_translation();
        self.expire_toast();
        let update_busy = matches!(
            self.update,
            UpdateState::Checking | UpdateState::Downloading { .. }
        );
        let wait = if self.toast.is_some()
            || self.translate_rx.is_some()
            || self.transfer.is_some()
            || !self.pending_downloads.is_empty()
            || update_busy
        {
            Duration::from_millis(120)
        } else {
            POLL_INTERVAL
        };
        ctx.request_repaint_after(wait);
    }

    fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        theme::paint_background(&ctx, ctx.content_rect());
        self.paint_window_chrome(ui);
        if self.screen == Screen::Settings {
            self.settings_screen(ui, &ctx);
            self.show_toast(&ctx);
            self.show_overlay(&ctx);
            self.history.flush();
            return;
        }
        self.ensure_thumbnails(&ctx);
        self.ensure_zoom(&ctx);
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
                    let header_top = screen.top() + theme::TOP_INSET + theme::CONTENT_MARGIN;
                    // 视口裁剪在卡片顶沿：内容可以滑到毛玻璃后面，但不会露出卡片上方
                    ui.add_space(header_top - screen.top());
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.set_width(width);
                            ui.add_space(self.header_bottom - header_top + 12.0);
                            self.banners(ui, &mut events);
                            let visible =
                                self.history
                                    .search(&self.query, self.regex, self.case_sensitive);
                            if visible.is_empty() {
                                new_entry_clicked = self.empty_state(ui);
                            } else {
                                let now = now_millis();
                                let online = matches!(self.status, Status::Online { .. });
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (position, index) in visible.iter().enumerate() {
                                    let record = &self.history.items[*index];
                                    let info = self.history.move_info(&record.id);
                                    let view = RowView {
                                        record,
                                        position: position + 1,
                                        cursor: self.cursor.as_deref() == Some(record.id.as_str()),
                                        picked: self.picked.contains(&record.id),
                                        pick_mode: self.pick_mode,
                                        online,
                                        can_up: info.is_some_and(|info| info.position > info.from),
                                        can_down: info.is_some_and(|info| info.position < info.to),
                                        thumb: self
                                            .thumbnails
                                            .get(&record.id)
                                            .and_then(|entry| entry.texture.as_ref()),
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
        self.ensure_zoom(&ctx);
        self.show_zoom(&ctx);
        self.history.flush();
    }
}

impl eframe::App for ClipSyncApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
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
        theme::frost(
            ctx,
            response.response.rect,
            theme::GLASS_RADIUS,
            theme::frost_layer(),
        );
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
                    .fill(theme::p().card)
                    .stroke(Stroke::new(1.0, theme::with_alpha(theme::p().border, 153)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(7, 3))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            let (key_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                            icons::paint(ui.painter(), key_rect, Icon::Key, theme::p().accent);
                            ui.label(
                                RichText::new(&room)
                                    .monospace()
                                    .size(14.0)
                                    .color(theme::p().text),
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
                        .color(theme::p().text),
                );
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(status_label(&self.status))
                    .size(11.5)
                    .color(theme::p().muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if self.joined && icons::icon_button(ui, Icon::Logout, true, "离开房间").clicked()
                {
                    self.open(Overlay::LeaveRoom);
                }
                if icons::icon_button(ui, Icon::Settings, true, "设置").clicked() {
                    self.open_settings();
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
            theme::p().muted,
        );
        ui.painter().text(
            Pos2::new(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            self.peers.len().to_string(),
            FontId::proportional(11.0),
            theme::p().muted,
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
            icons::paint(ui.painter(), shield_rect, Icon::Shield, theme::p().accent);
            ui.label(RichText::new(text).size(11.5).color(theme::p().muted));
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
        let online = matches!(self.status, Status::Online { .. });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if icons::icon_button(ui, Icon::Send, online, "发送剪贴板").clicked() {
                self.send_clipboard();
            }
            if icons::icon_button(ui, Icon::Plus, online, "新建一条").clicked() {
                self.open(Overlay::NewEntry {
                    text: String::new(),
                });
            }
            if icons::icon_button(ui, Icon::Attach, online, "发送文件").clicked() {
                self.pick_files();
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
            if icons::icon_button(ui, Icon::Undo, can_undo, "撤销删除").clicked() {
                self.undo();
            }
            ui.add_space(4.0);
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
            if icons::icon_button_tinted(ui, Icon::Trash, theme::p().red, can_delete, delete_tip)
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
            theme::p().accent.gamma_multiply(0.6)
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
            theme::p().muted,
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
                theme::p().muted,
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
        if let Some(transfer) = &self.transfer {
            transfer_banner(ui, transfer);
        }
        if !self.joined {
            if banner(
                ui,
                Icon::Key,
                theme::p().accent,
                false,
                "未共享：复制的内容只留在本机，进房间后才会同步给其他设备",
                Some("共享"),
            ) {
                events.push(AppEvent::Share);
            }
        } else if matches!(self.status, Status::Offline { .. }) {
            let text = format!("与房间 {} 的连接已断开", self.room_code);
            if banner(
                ui,
                Icon::Warning,
                theme::p().amber,
                false,
                &text,
                Some("重连"),
            ) {
                events.push(AppEvent::Retry);
            }
        }
        if !self.mismatch.is_empty() {
            let text = format!(
                "同屋的 {} 安全码与本机不一致：多半是某一端的房间密码打错了，两边收不到彼此的剪贴板",
                self.mismatch.join("、")
            );
            banner(ui, Icon::Warning, theme::p().red, true, &text, None);
        }
        if self.invalid_regex() {
            banner(
                ui,
                Icon::Warning,
                theme::p().red,
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
                theme::p().accent,
            );
            ui.add_space(14.0);
            if self.query.trim().is_empty() {
                ui.label(
                    RichText::new("还没有剪贴板记录")
                        .size(16.0)
                        .strong()
                        .color(theme::p().text),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("复制过的内容会自动出现在这里，并在同房间的设备间同步")
                        .size(12.0)
                        .color(theme::p().muted),
                );
                ui.add_space(16.0);
                if matches!(self.status, Status::Online { .. })
                    && accent_button(ui, "新建一条").clicked()
                {
                    clicked = true;
                }
            } else {
                ui.label(
                    RichText::new(format!("没有匹配「{}」的记录", self.query.trim()))
                        .size(16.0)
                        .strong()
                        .color(theme::p().text),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("换个词试试，或关掉正则再搜")
                        .size(12.0)
                        .color(theme::p().muted),
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
        let response = egui::Area::new(Id::unique("selection-bar"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -20.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::p().glass_fill)
                    .stroke(Stroke::new(1.0, theme::p().glass_border))
                    .corner_radius(CornerRadius::same(theme::BAR_RADIUS))
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
                                    .color(theme::p().muted),
                            );
                            ui.add_space(4.0);
                            let enabled = count > 0;
                            if icons::icon_button(ui, Icon::Copy, enabled, "复制到剪贴板").clicked()
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
                                theme::p().red,
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
        theme::frost(
            ctx,
            response.response.rect,
            theme::BAR_RADIUS,
            theme::frost_layer(),
        );
    }

    fn show_toast(&mut self, ctx: &egui::Context) {
        let Some(toast) = self.toast.clone() else {
            return;
        };
        let offset = if self.pick_mode { -84.0 } else { -24.0 };
        let mut undo_clicked = false;
        let error = toast.kind == ToastKind::Error;
        let fill = if error {
            theme::p().error_container
        } else {
            theme::p().inverse_surface
        };
        let text_color = if error {
            theme::p().on_error_container
        } else {
            theme::p().inverse_text
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
            Overlay::Transform { .. } => 460.0,
            Overlay::Translate { .. } => 520.0,
            Overlay::Choice(_) => 360.0,
            Overlay::Nickname { .. } => 400.0,
            Overlay::Hotkey { .. } => 380.0,
            _ => 420.0,
        };
        let hotkey = self.preferences.screenshot_hotkey().to_string();
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
                                            theme::p().accent,
                                        );
                                        ui.add_space(2.0);
                                        ui.vertical(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(
                                                RichText::new(*step)
                                                    .size(12.0)
                                                    .color(theme::p().muted),
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
                    Overlay::About => {
                        dialog_header(ui, Icon::Info, "关于 ClipSyncRust");
                        ui.add_space(12.0);
                        let room = if self.joined {
                            self.room_code.clone()
                        } else {
                            "未共享".to_string()
                        };
                        about_row(ui, "版本", env!("CARGO_PKG_VERSION"), false);
                        about_row(ui, "所属系统", paths::device_os(), false);
                        about_row(ui, "设备号", &paths::install_id(), true);
                        about_row(ui, "本地记录", &format!("{} 条", self.history.items.len()), false);
                        about_row(ui, "房间", &room, self.joined);
                        about_row(ui, "文件托管", "uguu.se，拒收时退回 x0.at", false);
                        about_row(ui, "界面字体", "HarmonyOS Sans（华为授权）", false);
                        ui.add_space(6.0);
                        field_label(ui, "缓存目录");
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(paths::cache_dir().display().to_string())
                                .monospace()
                                .size(11.0)
                                .color(theme::p().muted),
                        );
                        ui.add_space(10.0);
                        hint(ui, "房间码连同密码在本机派生密钥，托管方只会收到密文。");
                        ui.add_space(6.0);
                        ui.hyperlink_to("github.com/aacai/ClipSyncRust", REPO_URL);
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
                                        .color(theme::p().accent),
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
                                                .color(theme::p().text),
                                        );
                                        ui.label(
                                            RichText::new(&peer.os)
                                                .size(10.5)
                                                .color(theme::p().faint),
                                        );
                                    });
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            let (label, color) = if !connected {
                                                ("未知", theme::p().faint)
                                            } else if safety
                                                .as_deref()
                                                .is_some_and(|mine| {
                                                    !peer.safety.is_empty()
                                                        && mine.starts_with(&peer.safety)
                                                })
                                            {
                                                ("一致", theme::p().green)
                                            } else if peer.safety.is_empty() {
                                                ("未知", theme::p().faint)
                                            } else {
                                                ("不一致", theme::p().red)
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
                            .color(theme::p().muted),
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
                            .color(theme::p().muted),
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
                    Overlay::Choice(choice) => {
                        let (title, icon, labels, selected) = match choice {
                            SettingsChoice::Theme => (
                                "主题",
                                Icon::Settings,
                                vec!["跟随系统".to_string(), "浅色".to_string(), "深色".to_string()],
                                match self.theme_mode {
                                    ThemeMode::System => 0,
                                    ThemeMode::Light => 1,
                                    ThemeMode::Dark => 2,
                                },
                            ),
                            SettingsChoice::Language => (
                                "语言",
                                Icon::Translate,
                                vec![
                                    "跟随系统".to_string(),
                                    "简体中文".to_string(),
                                    "English".to_string(),
                                ],
                                match self.language {
                                    Language::System => 0,
                                    Language::Chinese => 1,
                                    Language::English => 2,
                                },
                            ),
                            SettingsChoice::Retention => (
                                "保留时长",
                                Icon::Trash,
                                RETENTION_CHOICES
                                    .iter()
                                    .map(|days| format!("{days} 天"))
                                    .collect(),
                                RETENTION_CHOICES
                                    .iter()
                                    .position(|days| {
                                        *days == self.preferences.retention_days()
                                    })
                                    .unwrap_or(1),
                            ),
                        };
                        dialog_header(ui, icon, title);
                        ui.add_space(8.0);
                        for (index, label) in labels.into_iter().enumerate() {
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 36.0),
                                Sense::click(),
                            );
                            if response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    CornerRadius::same(8),
                                    theme::p().hover,
                                );
                            }
                            let dot = Rect::from_center_size(
                                Pos2::new(rect.left() + 14.0, rect.center().y),
                                Vec2::splat(16.0),
                            );
                            let active = index == selected;
                            ui.painter().circle_stroke(
                                dot.center(),
                                7.0,
                                Stroke::new(
                                    1.5,
                                    if active {
                                        theme::p().accent
                                    } else {
                                        theme::p().faint
                                    },
                                ),
                            );
                            if active {
                                ui.painter().circle_filled(dot.center(), 3.6, theme::p().accent);
                            }
                            ui.painter().text(
                                Pos2::new(rect.left() + 32.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                &label,
                                FontId::proportional(13.5),
                                theme::p().text,
                            );
                            if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                action = match choice {
                                    SettingsChoice::Theme => DialogAction::SetTheme(match index {
                                        1 => ThemeMode::Light,
                                        2 => ThemeMode::Dark,
                                        _ => ThemeMode::System,
                                    }),
                                    SettingsChoice::Language => {
                                        DialogAction::SetLanguage(match index {
                                            1 => Language::Chinese,
                                            2 => Language::English,
                                            _ => Language::System,
                                        })
                                    }
                                    SettingsChoice::Retention => DialogAction::SetRetention(
                                        RETENTION_CHOICES[index],
                                    ),
                                };
                            }
                        }
                        ui.add_space(6.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary_button(ui, "取消").clicked() {
                                close = true;
                            }
                        });
                    }
                    Overlay::Nickname { input } => {
                        dialog_header(ui, Icon::Edit, "昵称");
                        ui.add_space(10.0);
                        let edit = ui.add_sized(
                            Vec2::new(ui.available_width(), 34.0),
                            TextEdit::singleline(input)
                                .hint_text("给这台设备起个名字")
                                .margin(Margin::symmetric(10, 8))
                                .font(FontId::proportional(13.0)),
                        );
                        if focus {
                            edit.request_focus();
                        }
                        hint(ui, "同屋设备看到的就是它；留空则使用系统设备名");
                        let save = ui.input_mut(|input| {
                            input.consume_key(Modifiers::COMMAND, Key::Enter)
                        });
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if accent_button(ui, "保存").clicked() || save {
                                    action = DialogAction::SaveNickname(input.clone());
                                }
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
                        });
                    }
                    Overlay::Hotkey { error } => {
                        dialog_header(ui, Icon::Select, "截屏快捷键");
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(hotkey.as_str())
                                .size(20.0)
                                .color(theme::p().text),
                        );
                        ui.add_space(10.0);
                        match error.as_deref() {
                            Some(reason) => {
                                ui.label(
                                    RichText::new(reason)
                                        .size(10.5)
                                        .color(theme::p().red),
                                );
                            }
                            None => hint(
                                ui,
                                "直接按下新的组合键就能替换，需要带上 Command、Control 或 Alt",
                            ),
                        }
                        let pressed = ui.input_mut(|input| {
                            let index = input.events.iter().position(|event| {
                                matches!(event, egui::Event::Key { pressed: true, .. })
                            })?;
                            match input.events.remove(index) {
                                egui::Event::Key {
                                    key, modifiers, ..
                                } => Some((key, modifiers)),
                                _ => None,
                            }
                        });
                        if let Some((key, modifiers)) = pressed {
                            if key == Key::Escape {
                                close = true;
                            } else if let Some(spec) = screenshot::spec(key, modifiers) {
                                *error = None;
                                action = DialogAction::SetHotkey(spec);
                            } else {
                                *error =
                                    Some("这个键做不了全局快捷键，再按一次别的组合键".to_string());
                            }
                        }
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
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
                    Overlay::Transform { id } => {
                        dialog_header(ui, Icon::Edit, "文本处理");
                        ui.add_space(10.0);
                        let source = self
                            .history
                            .items
                            .iter()
                            .find(|record| &record.id == id)
                            .map(|record| record.text.clone());
                        if let Some(source) = source {
                            let preview = text_tools::first_line(&source);
                            ui.label(
                                RichText::new(truncate_chars(
                                    if preview.is_empty() {
                                        "（空文本）"
                                    } else {
                                        &preview
                                    },
                                    72,
                                ))
                                .size(10.5)
                                .color(theme::p().muted),
                            );
                            ui.add_space(6.0);
                            hint(ui, "处理结果会直接替换这条内容");
                            ui.add_space(4.0);
                            let mut choice: Option<TextOp> = None;
                            for pair in text_tools::OPS.chunks(2) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let width = (ui.available_width() - 6.0) / 2.0;
                                    for op in pair {
                                        if transform_button(ui, *op, width).clicked() {
                                            choice = Some(*op);
                                        }
                                    }
                                });
                                ui.add_space(2.0);
                            }
                            if let Some(op) = choice {
                                action = DialogAction::ApplyOp(id.clone(), op);
                            }
                            ui.add_space(8.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if secondary_button(ui, "取消").clicked() {
                                    close = true;
                                }
                            });
                        } else {
                            hint(ui, "这条记录已经不在了。");
                            ui.add_space(12.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if secondary_button(ui, "关闭").clicked() {
                                    close = true;
                                }
                            });
                        }
                    }
                    Overlay::Translate {
                        id,
                        source,
                        target,
                        running,
                        result,
                        failure,
                    } => {
                        dialog_header(ui, Icon::Translate, "中英互译");
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if direction_chip(
                                ui,
                                "译成英文",
                                *target == TranslateTarget::English,
                            )
                            .clicked()
                            {
                                *target = TranslateTarget::English;
                            }
                            if direction_chip(
                                ui,
                                "译成中文",
                                *target == TranslateTarget::Chinese,
                            )
                            .clicked()
                            {
                                *target = TranslateTarget::Chinese;
                            }
                        });
                        ui.add_space(10.0);
                        field_label(ui, "原文");
                        ui.add_space(4.0);
                        read_only_box(ui, source);
                        let length = source.encode_utf16().count();
                        if length > translate::MAX_TRANSLATE_CHARS {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!(
                                    "一次最多翻 {} 字，这条 {} 字",
                                    translate::MAX_TRANSLATE_CHARS, length
                                ))
                                .size(10.5)
                                .color(theme::p().red),
                            );
                        }
                        if let Some(error) = failure {
                            ui.add_space(4.0);
                            ui.label(RichText::new(error.as_str()).size(10.5).color(theme::p().red));
                        }
                        if let Some(translated) = result {
                            ui.add_space(10.0);
                            field_label(ui, "译文");
                            ui.add_space(4.0);
                            read_only_box(ui, translated.as_str());
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if link_button(ui, "复制译文").clicked() {
                                    action = DialogAction::Copy(translated.clone());
                                }
                                if link_button(ui, "另存为新条目").clicked() {
                                    action = DialogAction::AddLocal(translated.clone());
                                }
                                if link_button(ui, "替换原文").clicked() {
                                    action =
                                        DialogAction::ReplaceText(id.clone(), translated.clone());
                                }
                            });
                        }
                        ui.add_space(14.0);
                        let too_long = length > translate::MAX_TRANSLATE_CHARS;
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if accent_button_enabled(
                                    ui,
                                    if *running { "翻译中…" } else { "翻译" },
                                    !*running && !too_long,
                                )
                                .clicked()
                                {
                                    *running = true;
                                    *failure = None;
                                    let text = source.clone();
                                    let direction = *target;
                                    let (sender, receiver) = mpsc::channel();
                                    self.translate_rx = Some(receiver);
                                    std::thread::spawn(move || {
                                        let _ = sender.send(translate::translate(&text, direction));
                                    });
                                }
                                if *running {
                                    ui.add(egui::Spinner::new().size(14.0));
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
                            if let Some(texture) = self
                                .thumbnails
                                .get(id)
                                .and_then(|entry| entry.texture.clone())
                            {
                                let image = ui.add(
                                    egui::Image::new(&texture)
                                        .max_width(ui.available_width())
                                        .max_height(196.0)
                                        .corner_radius(10)
                                        .sense(Sense::click()),
                                );
                                if image
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("查看大图")
                                    .clicked()
                                {
                                    self.open_zoom(id);
                                }
                                ui.add_space(10.0);
                            }
                            egui::ScrollArea::vertical()
                                .max_height(240.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let body = match &record.file {
                                        Some(file) => file
                                            .local_path
                                            .clone()
                                            .unwrap_or_else(|| "未下载".to_string()),
                                        None if record.text.trim().is_empty() => {
                                            "（空文本）".to_string()
                                        }
                                        None => record.text.clone(),
                                    };
                                    ui.label(
                                        RichText::new(body)
                                            .monospace()
                                            .size(12.0)
                                            .color(theme::p().text),
                                    );
                                    if !record.note.is_empty() {
                                        ui.add_space(10.0);
                                        field_label(ui, "备注");
                                        ui.label(
                                            RichText::new(&record.note)
                                                .size(11.5)
                                                .color(theme::p().muted),
                                        );
                                    }
                                    ui.add_space(10.0);
                                    field_label(ui, "记录信息");
                                    let mut meta = format!(
                                        "{} · 来自 {}",
                                        relative_time(record.ts, now_millis()),
                                        record.sender_name
                                    );
                                    if let Some(file) = &record.file {
                                        meta.push_str(&format!(
                                            " · {} · {} · {}",
                                            file.name,
                                            file.mime,
                                            human_size(file.size)
                                        ));
                                        let left = file.expires_at - now_millis();
                                        if left > 0 {
                                            meta.push_str(&format!(" · {}后过期", human_left(left)));
                                        }
                                    }
                                    if record.pinned {
                                        meta.push_str(" · 已置顶");
                                    }
                                    if record.moved() {
                                        meta.push_str(" · 已手动排序");
                                    }
                                    ui.label(
                                        RichText::new(meta)
                                            .size(11.0)
                                            .color(theme::p().muted),
                                    );
                                    ui.add_space(6.0);
                                    field_label(ui, "记录 ID");
                                    ui.label(
                                        RichText::new(&record.id)
                                            .monospace()
                                            .size(10.5)
                                            .color(theme::p().faint),
                                    );
                                });
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                if secondary_button(ui, "复制").clicked() {
                                    action = DialogAction::CopyRecord(id.clone());
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if secondary_button(ui, "取消").clicked() {
                                        close = true;
                                    }
                                });
                            });
                        } else {
                            ui.label(
                                RichText::new("这条记录已经不在了。")
                                    .size(12.5)
                                    .color(theme::p().muted),
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
                        dialog_header(ui, Icon::Position, "移动到指定位置");
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
                                    .color(theme::p().red),
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
            DialogAction::CopyRecord(id) => {
                self.copy_record(&id);
            }
            DialogAction::ApplyOp(id, op) => {
                if let Some(text) = self
                    .history
                    .items
                    .iter()
                    .find(|record| record.id == id)
                    .map(|record| text_tools::apply(&record.text, op))
                {
                    if self.history.update_text(&id, &text) {
                        self.notify("已改好 1 条");
                    }
                }
                keep = false;
            }
            DialogAction::AddLocal(text) => {
                self.add_local_text(&text);
                keep = false;
            }
            DialogAction::ReplaceText(id, text) => {
                if self.history.update_text(&id, &text) {
                    self.notify("已改好 1 条");
                }
                keep = false;
            }
            DialogAction::SetTheme(mode) => {
                self.set_theme(ctx, mode);
                keep = false;
            }
            DialogAction::SetLanguage(language) => {
                self.set_language(language);
                ctx.request_repaint();
                keep = false;
            }
            DialogAction::SetRetention(days) => {
                self.set_retention_days(days);
                keep = false;
            }
            DialogAction::SaveNickname(name) => {
                self.set_nickname(&name);
                keep = false;
            }
            DialogAction::SetHotkey(spec) => match self.set_screenshot_hotkey(&spec) {
                Ok(()) => keep = false,
                Err(error) => {
                    overlay = Overlay::Hotkey { error: Some(error) };
                }
            },
        }
        if keep && self.overlay.is_none() {
            self.overlay = Some(overlay);
        }
        self.history.flush();
    }

    fn show_zoom(&mut self, ctx: &egui::Context) {
        let Some(id) = self.zoom.clone() else {
            return;
        };
        let screen = ctx.content_rect();
        let has_path = self.zoom_file_path().is_some();
        let texture = self
            .zoom_texture
            .as_ref()
            .map(|(_, texture)| texture.clone());
        let mut close = false;
        let mut save = false;
        egui::Area::new(Id::unique("zoom-layer"))
            .order(Order::Tooltip)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                let painter = ui.ctx().layer_painter(ui.layer_id());
                painter.rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(226));
                let backdrop = ui.interact(screen, Id::unique("zoom-backdrop"), Sense::click());
                if backdrop.clicked() {
                    close = true;
                }
                let button = 34.0;
                let mut right = screen.right() - 12.0;
                let close_rect = Rect::from_min_size(
                    Pos2::new(right - button, screen.top() + 12.0),
                    Vec2::splat(button),
                );
                right -= button + 4.0;
                let save_rect = Rect::from_min_size(
                    Pos2::new(right - button, screen.top() + 12.0),
                    Vec2::splat(button),
                );
                let close_response =
                    ui.interact(close_rect, Id::unique("zoom-close"), Sense::click());
                let save_response = ui.interact(save_rect, Id::unique("zoom-save"), Sense::click());
                if close_response
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("关闭")
                    .clicked()
                {
                    close = true;
                }
                if save_response
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("存到下载目录")
                    .clicked()
                {
                    save = true;
                }
                match &texture {
                    Some(texture) => {
                        let available = Rect::from_min_max(
                            Pos2::new(screen.left() + 28.0, screen.top() + 28.0),
                            Pos2::new(screen.right() - 28.0, screen.bottom() - 28.0),
                        );
                        let size = texture.size_vec2();
                        let scale = (available.width() / size.x)
                            .min(available.height() / size.y)
                            .max(0.01);
                        let rect = Rect::from_center_size(screen.center(), size * scale);
                        painter.image(
                            texture.id(),
                            rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None if !has_path => {
                        painter.text(
                            screen.center(),
                            Align2::CENTER_CENTER,
                            "未下载",
                            FontId::proportional(14.0),
                            Color32::WHITE,
                        );
                    }
                    None => {
                        painter.text(
                            screen.center(),
                            Align2::CENTER_CENTER,
                            "正在加载…",
                            FontId::proportional(13.0),
                            theme::p().muted,
                        );
                    }
                }
                icons::paint(
                    &painter,
                    Rect::from_center_size(save_rect.center(), Vec2::splat(20.0)),
                    Icon::Download,
                    Color32::WHITE,
                );
                icons::paint(
                    &painter,
                    Rect::from_center_size(close_rect.center(), Vec2::splat(20.0)),
                    Icon::Close,
                    Color32::WHITE,
                );
            });
        if save {
            self.save_record(&id);
        }
        if close {
            self.close_zoom();
        }
    }
}

fn clip_row(ui: &mut Ui, view: RowView<'_>, now: i64, events: &mut Vec<AppEvent>, scroll_to: bool) {
    let RowView {
        record,
        position,
        cursor,
        picked,
        pick_mode,
        online,
        can_up,
        can_down,
        thumb,
    } = view;
    let font = FontId::proportional(14.0);
    let left_pad = 12.0;
    let number_w = 26.0;
    let file = record.file.as_ref();
    let is_file = record.kind == KIND_FILE && file.is_some();
    let cached = file.is_some_and(|file| file.cached && file.local_path.is_some());
    let copyable = !is_file || cached || file.is_some_and(|file| is_image(&file.mime));
    let quick_actions = if is_file {
        3.0 + if copyable { 1.0 } else { 0.0 }
    } else {
        4.0
    };
    let action_reserve = 8.0 + quick_actions * 30.0 + (quick_actions - 1.0) * 2.0 + 8.0;
    let thumb_gap = if thumb.is_some() { 54.0 } else { 8.0 };
    let avail = ui.available_width();
    let card_w = (avail - theme::ROW_INSET * 2.0).max(240.0);
    let text_w = (card_w - left_pad - number_w - thumb_gap - action_reserve - 10.0).max(150.0);

    let empty = !is_file && record.text.is_empty();
    let display = match file {
        Some(file) => file.name.as_str(),
        None if empty => "（空文本）",
        None => record.text.as_str(),
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
            color: if empty {
                theme::p().faint
            } else {
                theme::p().text
            },
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(body));
    let text_h = galley.size().y.max(20.0);
    let line_center = first_line_center(&galley);
    let (top_pad, thumb_top) = if thumb.is_some() {
        ((9.0 + 16.0 - line_center).max(10.0), 9.0)
    } else {
        (10.0, 0.0)
    };
    let content_h = text_h + 21.0;
    let height = top_pad + content_h + 9.0;
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
    let text_y = card.top() + top_pad;
    let line_y = text_y + line_center;
    if scroll_to {
        ui.scroll_to_rect(card, Some(Align::Center));
    }

    let painter = ui.painter().clone();
    let hovered = card_response.contains_pointer();
    let revealed = hovered || cursor;
    let fill = if picked {
        Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 38)
    } else if cursor {
        Color32::from_white_alpha(9)
    } else {
        theme::with_alpha(theme::p().card, 128)
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

    if pick_mode {
        let center = Pos2::new(card.left() + left_pad + 9.0, line_y);
        painter.circle_stroke(
            center,
            8.0,
            Stroke::new(
                1.4,
                if picked {
                    theme::p().accent
                } else {
                    theme::p().muted
                },
            ),
        );
        if picked {
            painter.circle_filled(center, 7.0, theme::p().accent);
            icons::paint(
                &painter,
                Rect::from_center_size(center, Vec2::splat(11.0)),
                Icon::Check,
                theme::p().accent_text,
            );
        }
    } else {
        painter.text(
            Pos2::new(card.left() + left_pad + number_w, line_y),
            Align2::RIGHT_CENTER,
            position.to_string(),
            FontId::proportional(13.0),
            if record.pinned {
                theme::p().amber
            } else {
                theme::p().muted
            },
        );
    }

    let mut zoom_clicked = false;
    if let Some(texture) = thumb {
        let thumb_rect = Rect::from_min_size(
            Pos2::new(
                card.left() + left_pad + number_w + 8.0,
                card.top() + thumb_top,
            ),
            Vec2::splat(32.0),
        );
        let thumb_response = ui.interact(thumb_rect, id.with("thumb"), Sense::click());
        // put() 会把父布局的游标推到缩略图下沿，下一行因此被拽上来压在时间戳上；
        // paint_at 只画不进布局。
        egui::Image::new(texture)
            .fit_to_exact_size(Vec2::splat(32.0))
            .corner_radius(6)
            .paint_at(ui, thumb_rect);
        if thumb_response
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("查看大图")
            .clicked()
        {
            zoom_clicked = true;
        }
    }

    let text_pos = Pos2::new(card.left() + left_pad + number_w + thumb_gap, text_y);
    painter.galley(text_pos, galley, theme::p().text);

    let mut meta = relative_time(record.ts, now);
    if !record.sender_name.is_empty() {
        meta.push_str(" · 来自 ");
        meta.push_str(&record.sender_name);
    }
    if let Some(file) = file {
        meta.push_str(" · ");
        meta.push_str(&human_size(file.size));
        meta.push_str(if cached {
            " · 已缓存"
        } else {
            " · 未下载"
        });
        let left = file.expires_at - now;
        if left > 0 {
            meta.push_str(&format!(" · {}后过期", human_left(left)));
        }
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
        theme::p().muted,
        card.right() - text_pos.x - 12.0,
    );

    let action_size = 30.0;
    let action_top = card.top() + (line_y - card.top() - action_size / 2.0).max(7.0);
    let mut right = card.right() - 8.0;
    let place = |right: &mut f32| {
        let rect = Rect::from_min_size(
            Pos2::new(*right - action_size, action_top),
            Vec2::splat(action_size),
        );
        *right -= action_size + 2.0;
        rect
    };
    let more_rect = place(&mut right);
    let pin_rect = place(&mut right);
    let file_rect = is_file.then(|| place(&mut right));
    let copy_rect = copyable.then(|| place(&mut right));
    let send_rect = (!is_file).then(|| place(&mut right));

    let more_response = ui
        .interact(more_rect, id.with("more"), Sense::click())
        .on_hover_text("更多操作");
    if more_response.hovered() {
        painter.rect_filled(
            more_rect,
            CornerRadius::same(9),
            Color32::from_white_alpha(10),
        );
    }
    icons::paint(
        &painter,
        Rect::from_center_size(more_rect.center(), Vec2::splat(17.0)),
        Icon::More,
        if more_response.hovered() {
            theme::p().text
        } else {
            theme::p().muted
        },
    );

    let mut menu_choice: Option<MenuAction> = None;
    egui::Popup::menu(&more_response)
        .width(216.0)
        .frame(menu_frame())
        .show(|ui| {
            ui.set_width(216.0);
            if menu_item(ui, Icon::Edit, "编辑内容", !is_file, false) {
                menu_choice = Some(MenuAction::Edit);
            }
            if menu_item(ui, Icon::Note, "写备注", true, false) {
                menu_choice = Some(MenuAction::Note);
            }
            if menu_item(ui, Icon::Edit, "文本处理", !is_file, false) {
                menu_choice = Some(MenuAction::Transform);
            }
            if menu_item(ui, Icon::Translate, "中英互译", !is_file, false) {
                menu_choice = Some(MenuAction::Translate);
            }
            if menu_item(ui, Icon::Duplicate, "生成副本", true, false) {
                menu_choice = Some(MenuAction::Duplicate);
            }
            if menu_item(ui, Icon::Export, "存到下载目录", true, false) {
                menu_choice = Some(if is_file {
                    MenuAction::SaveFile
                } else {
                    MenuAction::SaveText
                });
            }
            if is_file && file.is_some_and(|file| file.link.is_empty()) {
                if menu_item(ui, Icon::Send, "补传到房间", online, false) {
                    menu_choice = Some(MenuAction::Resend);
                }
            }
            if menu_item(ui, Icon::Info, "查看详情", true, false) {
                menu_choice = Some(MenuAction::Detail);
            }
            menu_separator(ui);
            if menu_item(ui, Icon::ArrowUp, "向上移", can_up, false) {
                menu_choice = Some(MenuAction::MoveUp);
            }
            if menu_item(ui, Icon::ArrowDown, "向下移", can_down, false) {
                menu_choice = Some(MenuAction::MoveDown);
            }
            if menu_item(ui, Icon::Top, "移到最顶部", can_up, false) {
                menu_choice = Some(MenuAction::MoveTop);
            }
            if menu_item(ui, Icon::Bottom, "移到最底部", can_down, false) {
                menu_choice = Some(MenuAction::MoveBottom);
            }
            if menu_item(
                ui,
                Icon::Position,
                "移动到指定位置…",
                can_up || can_down,
                false,
            ) {
                menu_choice = Some(MenuAction::MoveTo);
            }
            menu_separator(ui);
            if menu_item(ui, Icon::Trash, "删除", true, true) {
                menu_choice = Some(MenuAction::Delete);
            }
        });

    let mut icon_action: Option<AppEvent> = None;
    if revealed {
        if let Some(rect) = send_rect {
            let send_response =
                row_action(ui, rect, id.with("send"), Icon::Send, "发送到房间", online);
            if send_response.clicked() {
                icon_action = Some(AppEvent::Publish(record.id.clone()));
            }
        }
        if let Some(rect) = copy_rect {
            let tip = if is_file {
                if file.is_some_and(|file| is_image(&file.mime)) {
                    "复制图片到剪贴板"
                } else {
                    "复制文件到剪贴板"
                }
            } else {
                "复制到剪贴板"
            };
            let copy_response = row_action(ui, rect, id.with("copy"), Icon::Copy, tip, true);
            if copy_response.clicked() {
                icon_action = Some(AppEvent::Copy(record.id.clone()));
            }
        }
        if let Some(rect) = file_rect {
            if cached {
                let open_response =
                    row_action(ui, rect, id.with("open"), Icon::OpenInNew, "打开", true);
                if open_response.clicked() {
                    icon_action = Some(AppEvent::Open(record.id.clone()));
                }
            } else {
                let download_response =
                    row_action(ui, rect, id.with("download"), Icon::Download, "下载", true);
                if download_response.clicked() {
                    icon_action = Some(AppEvent::Download(record.id.clone()));
                }
            }
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
            true,
        );
        if pin_response.clicked() {
            icon_action = Some(AppEvent::Pin(record.id.clone()));
        }
    }

    if let Some(action) = menu_choice {
        events.push(AppEvent::Menu(record.id.clone(), action));
    } else if let Some(event) = icon_action {
        events.push(event);
    } else if zoom_clicked {
        events.push(AppEvent::Zoom(record.id.clone()));
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

fn row_action(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    icon: Icon,
    tip: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();
    if response.hovered() && enabled {
        painter.rect_filled(rect, CornerRadius::same(9), Color32::from_white_alpha(10));
    }
    icons::paint(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        icon,
        if !enabled {
            theme::p().faint
        } else if response.hovered() {
            theme::p().text
        } else {
            theme::p().muted
        },
    );
    if enabled {
        response
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(tip)
    } else {
        response.on_hover_text(tip)
    }
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
        theme::p().on_error_container
    } else {
        theme::p().text
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

fn transfer_banner(ui: &mut Ui, transfer: &TransferUi) {
    let avail = ui.available_width();
    let width = (avail - theme::ROW_INSET * 2.0).max(240.0);
    let (outer, _) = ui.allocate_exact_size(Vec2::new(avail, 50.0), Sense::hover());
    let card = Rect::from_min_max(
        Pos2::new(outer.left() + theme::ROW_INSET, outer.top() + 4.0),
        Pos2::new(
            outer.left() + theme::ROW_INSET + width,
            outer.bottom() - 4.0,
        ),
    );
    let painter = ui.painter();
    theme::frost(ui.ctx(), card, theme::BANNER_RADIUS, painter.layer_id());
    painter.rect(
        card,
        CornerRadius::same(theme::BANNER_RADIUS),
        theme::glass_fill(),
        Stroke::new(1.0, theme::p().glass_border),
        StrokeKind::Inside,
    );
    let kind = if transfer.upload { "上传" } else { "下载" };
    let percent = (transfer.fraction() * 100.0).round() as i32;
    paint_line(
        ui,
        Pos2::new(card.left() + 12.0, card.top() + 7.0),
        &format!("{kind} {} · {percent}%", transfer.name),
        FontId::proportional(11.5),
        theme::p().text,
        card.width() - 24.0,
    );
    let bar = Rect::from_min_max(
        Pos2::new(card.left() + 12.0, card.top() + 28.0),
        Pos2::new(card.right() - 12.0, card.top() + 33.0),
    );
    painter.rect_filled(bar, CornerRadius::same(2), Color32::from_white_alpha(12));
    let filled = Rect::from_min_max(
        bar.min,
        Pos2::new(bar.left() + bar.width() * transfer.fraction(), bar.bottom()),
    );
    painter.rect_filled(filled, CornerRadius::same(2), theme::p().accent);
}

fn menu_frame() -> Frame {
    Frame::new()
        .fill(theme::p().surface_high)
        .stroke(Stroke::new(1.0, theme::p().border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(6))
        .shadow(egui::epaint::Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(120),
        })
}

fn menu_item(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, danger: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let color = if !enabled {
        theme::p().faint
    } else if danger {
        theme::p().red
    } else {
        theme::p().text
    };
    if response.hovered() && enabled {
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
        if !enabled {
            theme::p().faint
        } else if danger {
            theme::p().red
        } else {
            theme::p().muted
        },
    );
    ui.painter().text(
        Pos2::new(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.5),
        color,
    );
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand).clicked()
    } else {
        response.on_hover_cursor(CursorIcon::NotAllowed).clicked()
    }
}

fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(
        (rect.left() + 8.0)..=(rect.right() - 8.0),
        rect.center().y,
        Stroke::new(1.0, theme::p().border),
    );
}

fn transform_button(ui: &mut Ui, op: TextOp, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
    let fill = if response.hovered() {
        Color32::from_white_alpha(10)
    } else {
        Color32::from_white_alpha(4)
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(9),
        fill,
        Stroke::new(1.0, theme::p().border),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        op.label(),
        FontId::proportional(12.0),
        theme::p().text,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn direction_chip(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let font = FontId::proportional(12.0);
    let galley = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout_no_wrap(label.to_string(), font, theme::p().text));
    let width = galley.size().x + 28.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    let fill = if selected {
        Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 34)
    } else {
        Color32::from_white_alpha(5)
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(9),
        fill,
        Stroke::new(
            1.0,
            if selected {
                Color32::from_rgba_unmultiplied(0xf4, 0xa8, 0xc0, 150)
            } else {
                theme::p().border
            },
        ),
        StrokeKind::Inside,
    );
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        if selected {
            theme::p().accent
        } else {
            theme::p().muted
        },
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn read_only_box(ui: &mut Ui, text: &str) {
    Frame::new()
        .fill(Color32::from_white_alpha(5))
        .stroke(Stroke::new(1.0, theme::p().border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(RichText::new(text).size(12.0).color(theme::p().text))
                            .wrap()
                            .selectable(true),
                    );
                });
        });
}

fn link_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label).size(12.0).color(theme::p().accent)).frame(false))
        .on_hover_cursor(CursorIcon::PointingHand)
}

fn translate_failure_text(error: &translate::TranslationError) -> String {
    match error.failure {
        TranslateFailure::Request => format!("翻译请求没发出去：{}", error.detail),
        TranslateFailure::Malformed => "翻译服务的回复看不懂".to_string(),
        TranslateFailure::Empty => "翻译服务没给出内容".to_string(),
        TranslateFailure::Rejected => {
            format!("翻译服务拒绝了这个请求：{}", error.detail)
        }
    }
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
        if active {
            theme::p().accent
        } else {
            theme::p().muted
        },
    );
}

fn dialog_header(ui: &mut Ui, icon: Icon, title: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), Vec2::splat(20.0)),
            icon,
            theme::p().accent,
        );
        ui.label(
            RichText::new(title)
                .size(18.0)
                .strong()
                .color(theme::p().text),
        );
    });
}

fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(theme::p().muted));
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(10.5).color(theme::p().faint));
}

fn bullet(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 16.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 2.0, theme::p().accent);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).size(11.0).color(theme::p().muted));
        });
    });
    ui.add_space(6.0);
}

/// 关于面板里的一行：左边项目名，右边取值。
fn about_row(ui: &mut Ui, title: &str, value: &str, monospace: bool) {
    let font = if monospace {
        FontId::monospace(11.5)
    } else {
        FontId::proportional(11.5)
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(11.5).color(theme::p().faint));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .size(11.5)
                    .color(theme::p().text)
                    .font(font),
            );
        });
    });
    ui.add_space(7.0);
}

fn accent_button(ui: &mut Ui, label: &str) -> egui::Response {
    accent_button_enabled(ui, label, true)
}

fn accent_button_enabled(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    let button = egui::Button::new(RichText::new(label).size(13.0).strong().color(if enabled {
        theme::p().accent_text
    } else {
        theme::p().faint
    }))
    .fill(if enabled {
        theme::p().accent
    } else {
        Color32::from_white_alpha(8)
    })
    .corner_radius(CornerRadius::same(10))
    .min_size(Vec2::new(84.0, 32.0));
    ui.add_enabled(enabled, button)
}

fn secondary_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(13.0).color(theme::p().muted))
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
                .color(theme::p().on_red),
        )
        .fill(theme::p().red)
        .corner_radius(CornerRadius::same(10))
        .min_size(Vec2::new(84.0, 32.0)),
    )
}

fn status_color(status: &Status) -> Color32 {
    match status {
        Status::Idle => theme::p().faint,
        Status::Connecting => theme::p().amber,
        Status::Online { .. } => theme::p().green,
        Status::Offline { .. } => theme::p().red,
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
    let galley = line_galley(ui, text, font, color, max_width);
    ui.painter().galley(pos, galley, color);
}

fn line_galley(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> Arc<egui::Galley> {
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
    ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
}

/// Centre of the first line's inked glyphs, galley-local: what thumbnails and
/// badges are levelled against, since a glyph box is taller than its ink.
fn first_line_center(galley: &egui::Galley) -> f32 {
    let Some(row) = galley.rows.first() else {
        return 0.0;
    };
    let local = if row.visuals.mesh_bounds.is_finite() {
        row.visuals.mesh_bounds.center().y
    } else {
        row.height() * 0.5
    };
    row.pos.y + local
}

struct RowLine {
    galley: Arc<egui::Galley>,
    color: Color32,
    baseline: f32,
    center: f32,
}

fn row_line(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32) -> RowLine {
    let size = font.size;
    let galley = line_galley(ui, text, font, color, max_width);
    let baseline = galley
        .rows
        .first()
        .and_then(|row| {
            row.glyphs
                .iter()
                .find(|glyph| !glyph.chr.is_whitespace())
                .map(|glyph| row.pos.y + glyph.pos.y)
        })
        .unwrap_or(size * 0.8);
    let center = if galley.mesh_bounds.is_finite() {
        (galley.mesh_bounds.min.y + galley.mesh_bounds.max.y) / 2.0
    } else {
        baseline - size * 0.36
    };
    RowLine {
        galley,
        color,
        baseline,
        center,
    }
}

fn baseline_at(line: &RowLine, center_y: f32) -> f32 {
    center_y - line.center + line.baseline
}

fn paint_line_at(ui: &Ui, left: f32, baseline: f32, line: RowLine) {
    ui.painter().galley(
        Pos2::new(left, baseline - line.baseline),
        line.galley,
        line.color,
    );
}

fn paint_line_before(ui: &Ui, right: f32, baseline: f32, line: RowLine) -> f32 {
    let left = right - line.galley.size().x;
    ui.painter().galley(
        Pos2::new(left, baseline - line.baseline),
        line.galley.clone(),
        line.color,
    );
    left
}

fn room_color(status: &Status, joined: bool) -> Option<Color32> {
    match status {
        Status::Online { .. } => Some(theme::p().green),
        Status::Connecting => Some(theme::p().amber),
        _ if !joined => None,
        _ => Some(theme::p().red),
    }
}

fn theme_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::System => "跟随系统",
        ThemeMode::Light => "浅色",
        ThemeMode::Dark => "深色",
    }
}

fn language_label(language: Language) -> &'static str {
    match language {
        Language::System => "跟随系统",
        Language::Chinese => "简体中文",
        Language::English => "English",
    }
}

const SETTINGS_ROW_HEIGHT: f32 = 54.0;
const SETTINGS_SWITCH_HEIGHT: f32 = 56.0;

fn settings_group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 23.0), Sense::hover());
    paint_line(
        ui,
        Pos2::new(rect.left() + 18.0, rect.top() + 2.0),
        title,
        FontId::proportional(11.5),
        theme::p().accent,
        rect.width() - 36.0,
    );
    let dark = theme::p().dark;
    Frame::new()
        .fill(theme::with_alpha(
            theme::p().surface,
            if dark { 199 } else { 224 },
        ))
        .stroke(Stroke::new(
            1.0,
            theme::with_alpha(theme::p().glass_border, 128),
        ))
        .corner_radius(CornerRadius::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

fn settings_divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        (rect.left() + 16.0)..=(rect.right() - 16.0),
        rect.center().y,
        Stroke::new(1.0, theme::with_alpha(theme::p().border, 102)),
    );
}

fn settings_row(ui: &mut Ui, height: f32, clickable: bool) -> (Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        if clickable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if clickable && response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), theme::p().hover);
    }
    (rect, response)
}

fn settings_nav_row(ui: &mut Ui, title: &str, value: &str) -> bool {
    let (rect, response) = settings_row(ui, SETTINGS_ROW_HEIGHT, true);
    let center = rect.center().y;
    let chevron = Rect::from_center_size(Pos2::new(rect.right() - 25.0, center), Vec2::splat(18.0));
    icons::paint(ui.painter(), chevron, Icon::ChevronRight, theme::p().faint);
    let value = row_line(
        ui,
        value,
        FontId::proportional(13.0),
        theme::p().muted,
        200.0,
    );
    let value_left = chevron.left() - 2.0 - value.galley.size().x;
    let title = row_line(
        ui,
        title,
        FontId::proportional(15.0),
        theme::p().text,
        (value_left - 12.0 - rect.left() - 16.0).max(40.0),
    );
    let baseline = baseline_at(&title, center);
    paint_line_at(ui, rect.left() + 16.0, baseline, title);
    paint_line_at(ui, value_left, baseline, value);
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn settings_value_row(
    ui: &mut Ui,
    title: &str,
    value: &str,
    detail: Option<(&str, Color32)>,
    monospace: bool,
    value_color: Option<Color32>,
    link: bool,
) -> bool {
    let (rect, response) = settings_row(ui, SETTINGS_ROW_HEIGHT, link);
    let center = rect.center().y;
    let chevron = Rect::from_center_size(Pos2::new(rect.right() - 25.0, center), Vec2::splat(18.0));
    if link {
        icons::paint(ui.painter(), chevron, Icon::ChevronRight, theme::p().faint);
    }
    let color = value_color.unwrap_or(theme::p().muted);
    let value_font = if monospace {
        FontId::monospace(13.0)
    } else {
        FontId::proportional(13.0)
    };
    let right_edge = if link {
        chevron.left() - 10.0
    } else {
        rect.right() - 16.0
    };
    let value = row_line(ui, value, value_font, color, 200.0);
    let mut title_limit = right_edge - value.galley.size().x - 12.0;
    let detail = detail.map(|(detail, detail_color)| {
        row_line(ui, detail, FontId::proportional(11.5), detail_color, 120.0)
    });
    if let Some(detail) = &detail {
        title_limit = title_limit - detail.galley.size().x - 9.0;
    }
    let title = row_line(
        ui,
        title,
        FontId::proportional(15.0),
        theme::p().text,
        (title_limit - rect.left() - 16.0).max(40.0),
    );
    let baseline = baseline_at(&title, center);
    paint_line_at(ui, rect.left() + 16.0, baseline, title);
    let mut right = right_edge;
    right = paint_line_before(ui, right, baseline, value);
    if let Some(detail) = detail {
        paint_line_before(ui, right - 9.0, baseline, detail);
    }
    if link {
        response.on_hover_cursor(CursorIcon::PointingHand).clicked()
    } else {
        false
    }
}

fn settings_action_row(ui: &mut Ui, title: &str) -> bool {
    let (rect, response) = settings_row(ui, SETTINGS_ROW_HEIGHT, true);
    let center = rect.center().y;
    let chevron = Rect::from_center_size(Pos2::new(rect.right() - 25.0, center), Vec2::splat(18.0));
    icons::paint(ui.painter(), chevron, Icon::ChevronRight, theme::p().faint);
    let title = row_line(
        ui,
        title,
        FontId::proportional(15.0),
        theme::p().red,
        chevron.left() - 12.0 - rect.left() - 16.0,
    );
    paint_line_at(ui, rect.left() + 16.0, baseline_at(&title, center), title);
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn settings_switch_row(ui: &mut Ui, title: &str, summary: &str, checked: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), SETTINGS_SWITCH_HEIGHT),
        Sense::hover(),
    );
    let title_center = rect.top() + 20.0;
    let switch_id = title;
    let title = row_line(
        ui,
        title,
        FontId::proportional(15.0),
        theme::p().text,
        (rect.width() - 32.0 - 36.0 - 12.0).max(60.0),
    );
    paint_line_at(
        ui,
        rect.left() + 16.0,
        baseline_at(&title, title_center),
        title,
    );
    paint_line(
        ui,
        Pos2::new(rect.left() + 16.0, rect.top() + 33.0),
        summary,
        FontId::proportional(10.5),
        theme::p().muted,
        rect.width() - 32.0,
    );
    let track = Rect::from_min_size(
        Pos2::new(rect.right() - 16.0 - 36.0, title_center - 10.0),
        Vec2::new(36.0, 20.0),
    );
    let response = ui
        .interact(
            track.expand(4.0),
            Id::unique(("settings-switch", switch_id)),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    ui.painter().rect_filled(
        track,
        CornerRadius::same(10),
        if checked {
            theme::p().accent
        } else {
            theme::p().switch_off
        },
    );
    let knob_x = if checked {
        track.right() - 10.0
    } else {
        track.left() + 10.0
    };
    ui.painter().circle_filled(
        Pos2::new(knob_x, track.center().y),
        7.0,
        if checked {
            theme::p().accent_text
        } else {
            theme::p().muted
        },
    );
    response.clicked()
}

fn settings_update_row(ui: &mut Ui, state: &UpdateState) -> Option<SettingsAction> {
    let (rect, _) = settings_row(ui, SETTINGS_ROW_HEIGHT, false);
    let center = rect.center().y;
    let busy = matches!(
        state,
        UpdateState::Checking | UpdateState::Downloading { .. }
    );
    let asset = match state {
        UpdateState::Available { asset, .. } => asset.as_ref(),
        _ => None,
    };
    let refresh = Rect::from_center_size(
        Pos2::new(rect.right() - 16.0 - 15.0, center),
        Vec2::splat(30.0),
    );
    let download = Rect::from_center_size(
        Pos2::new(refresh.left() - 2.0 - 15.0, center),
        Vec2::splat(30.0),
    );
    let label_right = if asset.is_some() {
        download.left() - 6.0
    } else {
        refresh.left() - 6.0
    };
    let status = row_line(
        ui,
        &update_label(state),
        FontId::proportional(11.5),
        update_color(state),
        220.0,
    );
    let title = row_line(
        ui,
        "检查更新",
        FontId::proportional(15.0),
        theme::p().text,
        (label_right - status.galley.size().x - 12.0 - rect.left() - 16.0).max(60.0),
    );
    let baseline = baseline_at(&title, center);
    paint_line_at(ui, rect.left() + 16.0, baseline, title);
    paint_line_before(ui, label_right, baseline, status);

    let mut action = None;
    if asset.is_some()
        && settings_icon_action(ui, download, Icon::Download, !busy, "下载更新").clicked()
    {
        action = Some(SettingsAction::DownloadUpdate);
    }
    if settings_icon_action(ui, refresh, Icon::Refresh, !busy, "检查更新").clicked() {
        action = Some(SettingsAction::CheckUpdate);
    }
    if matches!(state, UpdateState::Available { asset: None, .. }) {
        settings_footnote(ui, "这台机器没有对应的安装包");
    }
    if matches!(state, UpdateState::Idle) {
        settings_footnote(ui, "安装包放进系统下载目录，装好重启就是新版本");
    }
    action
}

fn settings_icon_action(
    ui: &mut Ui,
    rect: Rect,
    icon: Icon,
    enabled: bool,
    tip: &str,
) -> egui::Response {
    let response = ui.interact(
        rect,
        Id::unique(("settings-icon", tip)),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), theme::p().hover);
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
        icon,
        if enabled {
            theme::p().muted
        } else {
            theme::p().faint
        },
    );
    let response = if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    };
    response.on_hover_text(tip)
}

fn settings_footnote(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    paint_line(
        ui,
        Pos2::new(rect.left() + 16.0, rect.top() + 1.0),
        text,
        FontId::proportional(10.5),
        theme::p().muted,
        rect.width() - 32.0,
    );
}

fn update_label(state: &UpdateState) -> String {
    match state {
        UpdateState::Idle => "还没检查过".to_string(),
        UpdateState::Checking => "正在检查…".to_string(),
        UpdateState::UpToDate(version) => format!("已是最新（{version}）"),
        UpdateState::Available { release, .. } => {
            format!("发现新版本 {}", release.version)
        }
        UpdateState::Downloading { asset, received } => format!(
            "下载中 {} / {}",
            human_size(*received),
            human_size(asset.size)
        ),
        UpdateState::Downloaded(name) => format!("已下载 {name}"),
        UpdateState::Failed(message) => format!("更新失败：{message}"),
    }
}

fn update_color(state: &UpdateState) -> Color32 {
    match state {
        UpdateState::Failed(_) => theme::p().red,
        UpdateState::Available { .. } => theme::p().amber,
        UpdateState::Downloaded(_) => theme::p().green,
        _ => theme::p().muted,
    }
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

fn selection_payload(items: &[Record], picked: &BTreeSet<String>) -> Option<String> {
    let parts: Vec<String> = items
        .iter()
        .filter(|record| picked.contains(&record.id))
        .map(|record| match &record.file {
            Some(file) => format!("[{}]", file.name),
            None => record.text.clone(),
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn load_texture(
    ctx: &egui::Context,
    path: &str,
    max_side: u32,
    name: String,
) -> Option<TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    let image = image.thumbnail(max_side, max_side);
    let rgba = image.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(name, color, TextureOptions::LINEAR))
}

fn human_left(ms: i64) -> String {
    if ms < 3_600_000 {
        format!("{} 分钟", ms / 60_000)
    } else if ms < 86_400_000 {
        format!("{} 小时", ms / 3_600_000)
    } else {
        format!("{} 天", ms / 86_400_000)
    }
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
    use super::{human_left, random_room_code, relative_time, sanitize_room, selection_payload};

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
    fn expiry_counts_down_in_the_same_units_as_compose() {
        assert_eq!(human_left(42_000), "0 分钟");
        assert_eq!(human_left(59 * 60_000), "59 分钟");
        assert_eq!(human_left(3_600_000), "1 小时");
        assert_eq!(human_left(72 * 3_600_000), "3 天");
    }

    fn file_record(id: &str, name: &str, text: &str) -> super::Record {
        super::Record {
            id: id.to_string(),
            kind: super::KIND_FILE.to_string(),
            ts: 0,
            sender_id: String::new(),
            sender_name: String::new(),
            text: text.to_string(),
            file: Some(super::FileInfo {
                name: name.to_string(),
                mime: "image/png".to_string(),
                size: 3,
                sha256: String::new(),
                link: String::new(),
                expires_at: 0,
                cached: false,
                local_path: None,
            }),
            pinned: false,
            order: 0,
            note: String::new(),
        }
    }

    #[test]
    fn multi_select_labels_files_with_their_name() {
        let mut text = super::Record {
            id: "t".to_string(),
            kind: super::KIND_TEXT.to_string(),
            ts: 0,
            sender_id: String::new(),
            sender_name: String::new(),
            text: "hello".to_string(),
            file: None,
            pinned: false,
            order: 0,
            note: String::new(),
        };
        text.text = "hello".to_string();
        let picked = super::BTreeSet::from(["t".to_string(), "f".to_string()]);
        let payload = selection_payload(&[text, file_record("f", "shot.png", "shot.png")], &picked);
        assert_eq!(payload.as_deref(), Some("hello\n\n[shot.png]"));
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

    #[test]
    fn list_viewport_clips_at_the_glass_header_top() {
        use clipsync_core::{engine, mqtt::BrokerLinks};

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();

        let dir = std::env::temp_dir().join(format!("clipsync-viewport-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut app = super::ClipSyncApp::new(
            engine::spawn(BrokerLinks),
            super::History::open(dir.join("history.json")),
            super::Settings::open(dir.join("settings.json"), "viewport-test"),
            super::Preferences::open(dir.join("preferences.json")),
            // 测试里不注册全局快捷键：那会占掉这台机器上的 Alt+A
            None,
        );
        for index in 0..24 {
            app.history.add(super::Record {
                id: format!("id{index}"),
                kind: super::KIND_TEXT.to_string(),
                ts: 1_730_000_000_000,
                sender_id: String::new(),
                sender_name: "test".to_string(),
                text: format!("第 {index} 条记录"),
                file: None,
                pinned: false,
                order: 0,
                note: String::new(),
            });
        }

        let ctx = egui::Context::default();
        super::theme::configure(&ctx);
        super::theme::apply(&ctx, super::ThemeMode::Dark);
        super::icons::install(&ctx);

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(920.0, 620.0));
        let mut first = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        first.events = vec![egui::Event::PointerMoved(egui::pos2(460.0, 400.0))];
        let mut output = ctx.run_ui(first, |ui| app.draw(ui));
        output.textures_delta.clear();

        let mut scroll = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        scroll.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -1_200.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: egui::Modifiers::default(),
        });
        let mut list_layer = egui::LayerId::background();
        let mut clips = Vec::new();
        let mut output = ctx.run_ui(scroll, |ui| {
            app.draw(ui);
            list_layer = egui::LayerId::background();
            ui.ctx().graphics(|layers| {
                if let Some(list) = layers.get(list_layer) {
                    clips = list.all_entries().map(|entry| entry.clip_rect).collect();
                }
            });
        });
        output.textures_delta.clear();

        let header_top = screen.top() + super::theme::TOP_INSET + super::theme::CONTENT_MARGIN;
        let top = screen.top();
        let scrolled: Vec<_> = clips.iter().filter(|rect| rect.min.y > top + 0.5).collect();
        assert!(
            !scrolled.is_empty(),
            "no clipped list shape below y={top}, header top at {header_top}, clips={clips:?}"
        );
        for rect in scrolled {
            assert!(
                (rect.min.y - header_top).abs() <= 0.5,
                "list viewport starts at {:.1}, expected the glass header top {header_top:.1}",
                rect.min.y,
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
