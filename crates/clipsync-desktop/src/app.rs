use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
use clipsync_core::protocol::{clip_topic, room_hash};
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
use crate::tray::{Tray, TrayCmd};
use crate::update::{self, UpdateEvent, UpdateState};
use crate::{autostart, overlay, paths, preview, record, screenshot};

const TOAST_INFO: Duration = Duration::from_secs(4);
const TOAST_ERROR: Duration = Duration::from_secs(6);
const TOAST_UNDO: Duration = Duration::from_secs(9);
/// The overlay's record accent, so the picker and the recording bar read as the same action.
const RECORD_RED: Color32 = Color32::from_rgb(0xff, 0x45, 0x3a);
/// 一次录屏最长两小时，和 Compose 端一致。
const RECORD_LIMIT_SEC: u64 = 120 * 60;
/// 录制中的操作栏：和录屏工具条一样固定在顶部中间，显示计时和结束按钮。
const RECORD_BAR_WIDTH: f32 = 420.0;
const RECORD_BAR_HEIGHT: f32 = 56.0;
/// 操作栏离菜单栏的间距，和录屏工具条一致。
const RECORD_BAR_TOP_GAP: f32 = 8.0;
/// Time for the app window to actually leave the screen before the grab.
const CAPTURE_HIDE_DELAY: Duration = Duration::from_millis(140);
const THUMB_MAX_SIDE: u32 = 256;
/// 同时在库里留着的缩略图张数：一屏十几行，留出滚动余量就够，再多就是白占显存。
const THUMB_CACHE: usize = 48;
/// 视口上下各多准备这么高的行，快速滚动时不至于先看到空位。
const THUMB_SCAN: f32 = 640.0;
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
    /// 本帧末尾交给系统拖出（访达/资源管理器）。
    pending_drag_out: Option<Vec<PathBuf>>,
    thumbnails: HashMap<String, ThumbEntry>,
    thumb_tick: u64,
    zoom: Option<String>,
    zoom_texture: Option<(String, TextureHandle)>,
    zoom_failed: Option<String>,
    watch_clipboard: bool,
    auto_copy: bool,
    auto_paste_file: bool,
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
    /// 这次冻屏是为了选录制区域，而不是截一张图。
    record_pick: bool,
    record_phase: RecordPhase,
    /// 所有 ScreenCaptureKit 调用都排队给这条线程，主线程一次都不碰。
    record_worker: record::Worker,
    record_started: Option<Instant>,
    record_path: Option<PathBuf>,
    record_stop: Arc<AtomicBool>,
    /// 录制期间定时唤醒主窗口：主窗口已经藏进托盘，eframe 不会再自己刷新，
    /// 计时条和停止请求就都收不到消息了。
    record_pump: Arc<AtomicBool>,
    /// 录制条窗口出现后重刷一次内容过滤器，把它自己排除在录像之外。
    record_exclude_sent: bool,
    record_excluded: bool,
    /// 录制条露脸之后再刷一次：万一隐藏窗口不在第一份排除名单里，这一下也把它摘掉。
    record_rechecked: bool,
    /// 第二实例连上本机回环端口时发来的「请显示窗口」。
    show_rx: mpsc::Receiver<()>,
    tray: Option<Tray>,
    /// 设置页「退出」：允许真正关进程，普通点关闭只是藏窗。
    force_quit: bool,
    /// 主窗口此刻是不是露着。录屏时要先收起来，录完再按原来的样子还回去。
    window_visible: bool,
    /// 开始录屏前主窗口是露着的：停止后要重新抬出来；本来是收进托盘的就不动。
    record_restore_window: bool,
    /// 为了冻屏把主窗口藏起来了，选完区域/截完图要还原。
    capture_hidden_window: bool,
    /// 设置页里「麦克风设备」的候选，打开对话框时刷新。
    record_inputs: Vec<record::MicInput>,
    /// 录完自动弹出的预览窗口；分享/下载/删除都在这里收。
    record_preview: Option<preview::Preview>,
    /// 预览窗口「下载」在后台读文件、写下载目录，结果从这里回来。
    preview_save_tx: mpsc::Sender<Result<String, String>>,
    preview_save_rx: mpsc::Receiver<Result<String, String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Settings,
}

/// 录屏的四态：开录和收尾都在工作线程上跑，主线程只等回信。
#[derive(Clone, Copy, PartialEq, Eq)]
enum RecordPhase {
    Idle,
    /// 指令已发出，ScreenCaptureKit 还没回「开始」。
    Preparing,
    Recording,
    /// 已请求停止，正在等 moov 落盘并把收尾结论带回来。
    Stopping,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsChoice {
    Theme,
    Language,
    Retention,
    RecordMode,
    RecordSize,
    RecordFps,
    RecordMicDevice,
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
    AutoPasteFile(bool),
    WatchImages(bool),
    WatchFiles(bool),
    DeleteExpired(bool),
    ScreenshotKeepWindow(bool),
    RecordHotkey,
    RecordMode,
    RecordSize,
    RecordFps,
    RecordMicDevice,
    RecordMic(bool),
    RecordSystem(bool),
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
    used: u64,
}

#[derive(Clone)]
enum Overlay {
    Share,
    Help,
    About,
    Safety,
    /// 同屋设备列表（顶栏设备按钮）；与安全码对话框分开。
    Devices,
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
        record: bool,
        error: Option<String>,
    },
    /// 点「发送文件」后的框：只有拖进这块区域才上传。
    SendFile,
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
    SetRecordHotkey(String),
    SetRecordRegion(bool),
    SetRecordLongEdge(i32),
    SetRecordFps(u32),
    SetRecordMicDevice(String),
    SaveNickname(String),
    PickFiles,
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
    /// 从该条（或当前多选）拖出到访达/桌面。
    DragOut(String),
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
    /// 该行是否给缩略图留位置（由记录决定）。
    slot: bool,
    thumb: Option<TextureHandle>,
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
        show_rx: mpsc::Receiver<()>,
        tray: Option<Tray>,
    ) -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let watch_clipboard = preferences.watch_clipboard();
        let auto_copy = preferences.auto_copy();
        let auto_paste_file = preferences.auto_paste_file();
        let watch_files = preferences.watch_files();
        let watch_images = preferences.watch_images();
        let theme_mode = preferences.theme();
        let language = preferences.language();
        let autostart_available = autostart::is_available();
        let (preview_save_tx, preview_save_rx) = mpsc::channel();
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
            pending_drag_out: None,
            thumbnails: HashMap::new(),
            thumb_tick: 0,
            zoom: None,
            zoom_texture: None,
            zoom_failed: None,
            watch_clipboard,
            auto_copy,
            auto_paste_file,
            watch_files,
            watch_images,
            update: UpdateState::Idle,
            update_rx: None,
            autostart_available,
            hotkeys,
            shot: None,
            shot_visible: false,
            capture_at: None,
            record_pick: false,
            record_phase: RecordPhase::Idle,
            record_worker: record::Worker::spawn(),
            record_started: None,
            record_path: None,
            record_stop: Arc::new(AtomicBool::new(false)),
            record_pump: Arc::new(AtomicBool::new(false)),
            record_exclude_sent: false,
            record_excluded: false,
            record_rechecked: false,
            show_rx,
            tray,
            force_quit: false,
            window_visible: true,
            record_restore_window: false,
            capture_hidden_window: false,
            record_inputs: Vec::new(),
            record_preview: None,
            preview_save_tx,
            preview_save_rx,
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

    fn reveal_window(&mut self, ctx: &egui::Context) {
        self.window_visible = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    }

    /// 藏主窗口。录屏/冻屏都要先把它从屏幕上摘掉，否则录进去、冻进去的都是自己。
    fn hide_window(&mut self, ctx: &egui::Context) {
        self.window_visible = false;
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }

    /// 录屏结束：恢复主窗口透明度；本来收在托盘里的继续收着。
    fn restore_record_window(&mut self, ctx: &egui::Context) {
        crate::overlay::set_main_window_hidden(false);
        if self.record_restore_window {
            self.record_restore_window = false;
            self.reveal_window(ctx);
        } else {
            self.hide_window(ctx);
        }
    }

    fn drain_shell_signals(&mut self, ctx: &egui::Context) {
        let mut show = false;
        while self.show_rx.try_recv().is_ok() {
            show = true;
        }
        if let Some(tray) = self.tray.as_ref() {
            while let Some(cmd) = tray.poll() {
                match cmd {
                    TrayCmd::Show => show = true,
                    TrayCmd::Quit => {
                        self.force_quit = true;
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                        return;
                    }
                }
            }
        }
        if show {
            self.reveal_window(ctx);
        }
        // 关窗收进托盘；设置里「退出」或托盘退出才真正结束进程。
        if ctx.input(|input| input.viewport().close_requested()) {
            if self.force_quit {
                return;
            }
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window(ctx);
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
                JobEvent::DownloadFailed { id, message } => {
                    self.transfer = None;
                    self.pending_downloads.remove(&id);
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
                // 录屏上传完成才有 clip id：预览里的「删除」要靠它把列表记录一起摘掉。
                if let Some(preview) = self.record_preview.as_mut() {
                    if preview.name() == done.name {
                        preview.set_clip_id(done.id.clone());
                    }
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
            // 收完就替用户放进剪贴板：回窗口 Cmd/Ctrl+V 直接粘走，不用再来列表点复制。
            DownloadAfter::Auto => self.paste_incoming_file(&name, &done.path),
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

    /// 房间另一头发来的文件下载完了直接进剪贴板；`write_files` 会记下签名，
    /// 轮询到自家写入时不再二次广播。
    fn paste_incoming_file(&mut self, name: &str, path: &str) {
        if !self.auto_paste_file {
            return;
        }
        if self.clipboard.write_files(&[PathBuf::from(path)]) {
            self.notify(format!("{name} 已放进剪贴板，可直接粘贴"));
        } else {
            self.notify_error("写入剪贴板失败");
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
                        cached: None,
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
                cached: None,
            }]);
        } else {
            self.record_local_file(&name, image.mime(), &image.png);
        }
    }

    fn record_local_file(&mut self, name: &str, mime: &str, bytes: &[u8]) {
        let id = new_id();
        let path = crate::paths::write_cache(&id, name, bytes).ok();
        let sha256 = clipsync_core::crypto::sha256_hex(bytes);
        self.record_file_entry(id, name, mime, bytes.len() as i64, sha256, path);
    }

    fn record_file_entry(
        &mut self,
        id: String,
        name: &str,
        mime: &str,
        size: i64,
        sha256: String,
        local_path: Option<String>,
    ) {
        let ts = now_millis();
        let sender_name = self.sender_name();
        self.history.add(Record {
            id,
            kind: KIND_FILE.to_string(),
            ts,
            sender_id: crate::paths::install_id(),
            sender_name,
            text: name.to_string(),
            file: Some(FileInfo {
                name: name.to_string(),
                mime: mime.to_string(),
                size,
                sha256,
                link: String::new(),
                expires_at: 0,
                cached: local_path.is_some(),
                local_path,
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
            clip_topic: clip_topic(&self.room_hash),
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
        // 麦克风列表只在进设置页/开对话框时取一次：设备枚举不算便宜，别每帧都跑。
        self.record_inputs = record::inputs();
    }

    fn close_settings(&mut self) {
        self.screen = Screen::Main;
        self.overlay = None;
    }

    /// 设置页上显示的输入设备名。存着的设备拔掉了就退回「系统默认」。
    fn record_mic_label(&self) -> String {
        let selected = self.preferences.record_mic_device();
        if selected.is_empty() {
            return "系统默认".to_string();
        }
        self.record_inputs
            .iter()
            .find(|input| input.id == selected)
            .map(|input| input.name.clone())
            .unwrap_or_else(|| "系统默认".to_string())
    }

    fn set_theme(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.theme_mode = mode;
        self.preferences.set_theme(mode);
        let dark = theme::apply(ctx, mode);
        crate::win_caption::tint(dark);
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

    fn set_auto_paste_file(&mut self, enabled: bool) {
        self.auto_paste_file = enabled;
        self.preferences.set_auto_paste_file(enabled);
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
        let record = self.preferences.record_hotkey().to_string();
        let mut errors = Vec::new();
        if let Some(hotkeys) = self.hotkeys.as_mut() {
            if let Err(error) = hotkeys.apply(&spec) {
                errors.push(error);
            }
            if let Err(error) = hotkeys.apply_record(&record) {
                errors.push(error);
            }
        }
        for error in errors {
            self.notify_error(error);
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

    fn set_record_hotkey(&mut self, spec: &str) -> Result<(), String> {
        let result = match self.hotkeys.as_mut() {
            Some(hotkeys) => hotkeys.apply_record(spec),
            None => Ok(()),
        };
        result?;
        self.preferences.set_record_hotkey(spec);
        self.notify(format!("录屏快捷键已改为 {spec}"));
        Ok(())
    }

    /// Freeze the screen, then open the WeChat overlay on top of everything: the frame goes grey
    /// and the user picks a window with a click or draws a region before anything is cut.
    fn start_capture(&mut self, ctx: &egui::Context) {
        if self.shot.is_some() || self.capture_at.is_some() {
            return;
        }
        if !screenshot::request_access() {
            self.notify_error(
                "缺少屏幕录制权限：系统设置 → 隐私与安全性 → 屏幕与系统音频录制，勾上 ClipSyncRust；勾完要退出重开一次才生效",
            );
            return;
        }
        // 对齐 Compose：截图默认保留本窗，关掉「截图时保留本窗口」才先藏起再抓。
        // 录屏框选则一律先藏：冻屏只是框选时的背景，但用户不该在这层背景里又看见 ClipSync。
        // 透明 GL 窗被抓到也会是黑洞，所以藏窗时要留一帧时间让系统真正摘掉它。
        if !self.record_pick && self.preferences.screenshot_keep_window() {
            self.capture_at = Some(Instant::now());
            ctx.request_repaint();
        } else {
            self.capture_at = Some(Instant::now() + CAPTURE_HIDE_DELAY);
            self.capture_hidden_window = self.window_visible;
            self.hide_window(ctx);
            ctx.request_repaint_after(CAPTURE_HIDE_DELAY);
        }
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
                let record = self.record_pick;
                let options = record_options(&self.preferences);
                self.shot = Some(Arc::new(Mutex::new(overlay::Session::with_options(
                    freeze, record, options,
                ))));
            }
            Err(error) => {
                if self.capture_hidden_window {
                    self.capture_hidden_window = false;
                    self.reveal_window(ctx);
                }
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
            // 无边框、无边距：否则 egui 默认面板会在全屏上画出「额外的线」和边角控件感。
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let Some(session) = &session else { return };
                    if let Ok(mut session) = session.lock() {
                        session.draw(ui);
                    }
                });
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

        let record_pick = self.record_pick;
        // None until the user is done.
        let picked = session.lock().ok().and_then(|mut guard| {
            let outcome = guard.take_outcome()?;
            Some(match outcome {
                overlay::Outcome::Cancel => None,
                overlay::Outcome::StartRecording { rect, options } if record_pick => {
                    Some(Err((rect, options)))
                }
                overlay::Outcome::StartRecording { .. } => None,
                overlay::Outcome::Crop { rect, marks } => {
                    let image = guard.take_image()?;
                    let size = Vec2::new(image.width() as f32, image.height() as f32);
                    let (x, y, width, height) = overlay::crop_px(rect, size, guard.scale());
                    let cropped = image::imageops::crop_imm(&image, x, y, width, height).to_image();
                    screenshot::encode(cropped, &marks, (x, y), guard.scale())
                        .ok()
                        .map(Ok)
                }
            })
        });
        let Some(picked) = picked else { return };
        overlay::reveal(ctx, false);
        self.shot_visible = false;
        self.shot = None;
        self.record_pick = false;
        // 录屏不要把主窗抬回来：用户按下快捷键时它可能还在托盘里，抬出来就是「录屏时窗口又蹦出来」。
        // 截屏/取消才还原；录屏开始前先把窗口藏好，停止后再按原来的样子还回去。
        match picked {
            Some(Ok(shot)) => {
                if self.capture_hidden_window {
                    self.capture_hidden_window = false;
                    self.reveal_window(ctx);
                }
                self.commit_shot(shot);
            }
            Some(Err((rect, options))) => {
                // 不要在这里先藏窗：begin_recording 要靠 window_visible 记住录前是不是露着的，
                // 提前藏掉会让录完的窗口再也回不来。
                self.apply_record_options(&options);
                self.begin_recording(
                    ctx,
                    rect.min.x,
                    rect.min.y,
                    rect.width(),
                    rect.height(),
                    options,
                );
            }
            None => {
                if self.capture_hidden_window {
                    self.capture_hidden_window = false;
                    self.reveal_window(ctx);
                }
            }
        }
    }

    fn toggle_recording(&mut self, ctx: &egui::Context) {
        match self.record_phase {
            RecordPhase::Recording => {
                self.stop_recording(ctx);
                return;
            }
            RecordPhase::Preparing | RecordPhase::Stopping => {
                self.notify("上一条录屏指令还在处理，稍等一下");
                return;
            }
            RecordPhase::Idle => {}
        }
        if self.shot.is_some() || self.capture_at.is_some() {
            return;
        }
        if !screenshot::request_access() {
            self.notify_error(
                "缺少屏幕录制权限：系统设置 → 隐私与安全性 → 屏幕与系统音频录制，勾上 ClipSyncRust；勾完要退出重开一次才生效",
            );
            return;
        }
        if self.preferences.record_region() {
            self.record_pick = true;
            self.start_capture(ctx);
        } else {
            let options = record_options(&self.preferences);
            self.begin_recording(ctx, 0.0, 0.0, 0.0, 0.0, options);
        }
    }

    fn begin_recording(
        &mut self,
        ctx: &egui::Context,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        options: overlay::RecordOptions,
    ) {
        if self.record_phase != RecordPhase::Idle {
            return;
        }
        let dir = std::env::temp_dir().join("clipsync-recordings");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("recording-{}.mp4", new_id()));
        self.record_phase = RecordPhase::Preparing;
        self.record_started = None;
        self.record_path = Some(path.clone());
        self.record_exclude_sent = false;
        self.record_excluded = false;
        self.record_rechecked = false;
        self.record_stop.store(false, Ordering::Relaxed);
        // 主窗口马上要藏起来，eframe 不会再主动跑 tick；开一个轻量泵定时唤醒根窗口，
        // 让 Started 回信、计时和停止请求都能被处理。停止/失败时把开关放下。
        self.record_pump.store(true, Ordering::Relaxed);
        let pump = self.record_pump.clone();
        let pump_ctx = ctx.clone();
        std::thread::spawn(move || {
            while pump.load(Ordering::Relaxed) {
                pump_ctx.request_repaint_of(egui::ViewportId::ROOT);
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        // 录像里不该有我们自己的窗口，也不该在开录瞬间蹦出来：先藏好，记下原来是不是露着的，
        // 停止后再决定要不要抬回来（本来收在托盘里就继续收着）。
        self.record_restore_window = self.window_visible || self.capture_hidden_window;
        self.capture_hidden_window = false;
        // 录制操作栏是主窗口的 deferred 子窗口：主窗口一藏，它也会跟着消失。
        // 录屏时把主窗口调成透明并让鼠标穿透，用户看不见也点不到，子窗口还能正常显示。
        self.window_visible = false;
        crate::overlay::set_main_window_hidden(true);
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        self.record_worker
            .send(record::Job::Start(record::StartSpec {
                x: x as f64,
                y: y as f64,
                w: w as f64,
                h: h as f64,
                mic: options.mic,
                system_audio: options.system_audio,
                fps: options.quality.fps() as i32,
                long_edge: options.quality.long_edge(),
                mic_device: options.mic_device,
                path,
            }));
        ctx.request_repaint_after_for(Duration::from_millis(80), egui::ViewportId::ROOT);
    }

    /// 工具条选过的画质和声音来源写回设置，下次打开沿用。
    fn apply_record_options(&mut self, options: &overlay::RecordOptions) {
        self.preferences
            .set_record_long_edge(options.quality.long_edge());
        self.preferences.set_record_fps(options.quality.fps());
        self.preferences
            .set_record_system_audio(options.system_audio);
        self.preferences.set_record_mic(options.mic);
        self.preferences.set_record_mic_device(&options.mic_device);
    }

    fn on_record_started(&mut self, ctx: &egui::Context, result: Result<(), String>) {
        if self.record_phase != RecordPhase::Preparing {
            return;
        }
        match result {
            Ok(()) => {
                self.record_phase = RecordPhase::Recording;
                self.record_started = Some(Instant::now());
                let hotkey = self.preferences.record_hotkey().to_string();
                self.notify(format!(
                    "录屏已开始：再按一次 {hotkey}，或点顶部计时条上的「结束录制」"
                ));
            }
            Err(error) => {
                self.record_phase = RecordPhase::Idle;
                self.record_pump.store(false, Ordering::Relaxed);
                if let Some(path) = self.record_path.take() {
                    let _ = std::fs::remove_file(path);
                }
                // 开录失败时把窗口还回去，不然用户连错误提示都看不到。
                self.restore_record_window(ctx);
                self.notify_error(error);
            }
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    fn stop_recording(&mut self, ctx: &egui::Context) {
        if self.record_phase != RecordPhase::Recording {
            return;
        }
        let Some(path) = self.record_path.clone() else {
            self.record_phase = RecordPhase::Idle;
            self.record_pump.store(false, Ordering::Relaxed);
            return;
        };
        self.record_phase = RecordPhase::Stopping;
        self.record_started = None;
        self.record_worker.send(record::Job::Stop(path));
        ctx.request_repaint_after(Duration::from_millis(120));
    }

    fn on_record_stopped(
        &mut self,
        ctx: &egui::Context,
        stop_error: Option<String>,
        temp: PathBuf,
        size: i64,
        finished: bool,
    ) {
        if self.record_phase != RecordPhase::Stopping {
            return;
        }
        self.record_phase = RecordPhase::Idle;
        self.record_pump.store(false, Ordering::Relaxed);
        self.record_exclude_sent = false;
        self.record_excluded = false;
        self.record_rechecked = false;
        // 开录前窗口是露着的才抬回来；本来收在托盘里就继续收着。
        self.restore_record_window(ctx);
        self.record_path = None;
        // 没收尾的 mp4 没有 moov，播放器打不开；宁可丢掉也不能当成功写进列表。
        if !finished {
            let _ = std::fs::remove_file(&temp);
            let reason = stop_error.unwrap_or_else(|| "录像没有正常收尾".to_string());
            self.notify_error(format!("{reason}，文件不完整，已丢弃"));
            return;
        }
        let id = new_id();
        let name = format!("录屏-{}.mp4", &id[..8]);
        // 字节已经在盘上了，搬不进缓存也守着临时文件继续用，别再抄一份。
        let stored = crate::paths::adopt_cache(&id, &name, &temp)
            .map(PathBuf::from)
            .unwrap_or_else(|_| temp.clone());
        let _ = self.clipboard.write_files(std::slice::from_ref(&stored));
        self.record_preview = Some(preview::Preview::open(stored.clone(), name.clone()));
        let syncable = matches!(self.status, Status::Online { .. }) && self.room_crypto.is_some();
        if !syncable || size > MAX_FILE_BYTES {
            let clip_id = id.clone();
            let sha256 = clipsync_core::crypto::sha256_hex_file(&stored).unwrap_or_default();
            let local_path = Some(stored.to_string_lossy().into_owned());
            self.record_file_entry(id, &name, "video/mp4", size, sha256, local_path);
            if let Some(preview) = self.record_preview.as_mut() {
                preview.set_clip_id(clip_id);
            }
            self.notify(if size > MAX_FILE_BYTES {
                format!(
                    "录像已留在本机，超过 {}MB 无法同步",
                    clipsync_core::files::max_mb()
                )
            } else {
                "录像已留在本机列表，也放进了剪贴板".to_string()
            });
            return;
        }
        // 只有真要同步才把整段读进来：加密上传绕不开这一份。
        match std::fs::read(&stored) {
            Ok(bytes) => {
                self.start_uploads(vec![UploadRequest {
                    name,
                    mime: "video/mp4".to_string(),
                    bytes,
                    target: UploadTarget::Publish { cache: true },
                    cached: Some(stored.to_string_lossy().into_owned()),
                }]);
                self.notify("录像已存进列表，也放进了剪贴板");
            }
            Err(error) => self.notify_error(format!("录像读不出来：{error}")),
        }
    }

    /// 收录屏工作线程的回信，推进状态机。
    fn drain_record(&mut self, ctx: &egui::Context) {
        while let Some(event) = self.record_worker.poll() {
            match event {
                record::Event::Started(result) => self.on_record_started(ctx, result),
                record::Event::Excluded(result) => {
                    if self.record_phase == RecordPhase::Recording {
                        self.record_excluded = result.is_ok();
                        ctx.request_repaint();
                    }
                }
                record::Event::Stopped {
                    error,
                    path,
                    size,
                    finished,
                } => self.on_record_stopped(ctx, error, path, size, finished),
            }
        }
    }

    fn service_recording(&mut self, ctx: &egui::Context) {
        let id = egui::ViewportId::from_hash_of("record-bar");
        if self.record_stop.swap(false, Ordering::Relaxed) {
            self.stop_recording(ctx);
        }
        if self.record_phase == RecordPhase::Idle {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(false));
            return;
        }
        if self.record_phase == RecordPhase::Preparing {
            // 录制条等真的开录了再露面：它出现在排除快照之前，提前建就会录进片头。
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(false));
            ctx.request_repaint_after(Duration::from_millis(60));
            return;
        }
        let stopping = self.record_phase == RecordPhase::Stopping;
        let mut secs = 0u64;
        let mut visible = true;
        if !stopping {
            let Some(started) = self.record_started else {
                ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(false));
                return;
            };
            let elapsed = started.elapsed();
            if elapsed >= Duration::from_secs(RECORD_LIMIT_SEC) {
                self.stop_recording(ctx);
                self.notify("已录满 120 分钟，自动停了");
                return;
            }
            // 录制条是开录后才建的窗口，开录时的排除快照里没有它：先建好但藏着，重刷过滤器把它
            // 摘出去，再让它见人。拿不到就退回到看得见为准，不能让录制条永远不出现。
            if !self.record_exclude_sent && elapsed >= Duration::from_millis(150) {
                self.record_exclude_sent = true;
                self.record_worker.send(record::Job::Exclude);
            }
            if self.record_excluded
                && !self.record_rechecked
                && elapsed >= Duration::from_millis(700)
            {
                self.record_rechecked = true;
                self.record_worker.send(record::Job::Exclude);
            }
            secs = elapsed.as_secs();
            visible = self.record_excluded || elapsed >= Duration::from_secs(1);
        }
        let stop = self.record_stop.clone();
        let label = if stopping {
            "正在保存…".to_string()
        } else {
            format!(
                "{} / {}",
                record_clock(secs),
                record_clock(RECORD_LIMIT_SEC)
            )
        };
        ctx.show_viewport_deferred(
            id,
            egui::ViewportBuilder::default()
                .with_title("ClipSyncRust 录制")
                .with_decorations(false)
                .with_always_on_top()
                .with_resizable(false)
                .with_inner_size([RECORD_BAR_WIDTH, RECORD_BAR_HEIGHT])
                .with_taskbar(false)
                .with_visible(visible),
            move |ui, _| {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::NONE
                            .fill(Color32::from_black_alpha(240))
                            .corner_radius(28)
                            .inner_margin(Margin::symmetric(18, 10))
                            .stroke(Stroke::new(
                                1.0,
                                Color32::from_rgba_unmultiplied(0xff, 0x45, 0x3a, 170),
                            )),
                    )
                    .show_inside(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            let (dot, _) =
                                ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter().circle_filled(dot.center(), 5.0, RECORD_RED);
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(label.clone())
                                    .color(Color32::WHITE)
                                    .size(14.0)
                                    .monospace(),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if !stopping {
                                    let button = egui::Button::new(
                                        RichText::new("结束录制").color(Color32::WHITE).size(13.0),
                                    )
                                    .fill(RECORD_RED)
                                    .corner_radius(18)
                                    .min_size(Vec2::new(88.0, 36.0));
                                    if ui.add(button).clicked() {
                                        stop.store(true, Ordering::Relaxed);
                                        ui.ctx().request_repaint();
                                    }
                                }
                            });
                        });
                    });
            },
        );
        let monitor = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(Vec2::new(1440.0, 900.0));
        // 和录屏工具条同一个位置：顶部居中、菜单栏下面。录制过程中它一直留在那儿。
        let position = Pos2::new(
            ((monitor.x - RECORD_BAR_WIDTH) / 2.0).max(0.0),
            (crate::overlay::top_inset() + RECORD_BAR_TOP_GAP).max(0.0),
        );
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::OuterPosition(position));
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(visible));
        ctx.request_repaint_after(if visible {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(100)
        });
    }

    /// 录后预览窗口：保持 viewport 注册、收回动作、把下载结果变成提示。
    fn service_preview(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.preview_save_rx.try_recv() {
            match result {
                Ok(written) => self.notify(format!("已存到下载目录：{written}")),
                Err(error) => self.notify_error(error),
            }
        }
        let Some(mut preview) = self.record_preview.take() else {
            return;
        };
        preview.show(ctx);
        match preview.take_action() {
            None => {}
            Some(preview::Action::Close) => {
                preview.close(ctx);
                return;
            }
            Some(preview::Action::Share) => {
                let path = PathBuf::from(preview.path());
                if !path.is_file() {
                    self.notify_error("文件已经不在，无法分享");
                } else if self.clipboard.write_files(&[path]) {
                    self.notify("录像已放进剪贴板，可直接粘贴");
                } else {
                    self.notify_error("写入剪贴板失败");
                }
            }
            Some(preview::Action::Download) => {
                let path = PathBuf::from(preview.path());
                let name = preview.name().to_string();
                let tx = self.preview_save_tx.clone();
                std::thread::spawn(move || {
                    let result = match std::fs::read(&path) {
                        Ok(bytes) => crate::paths::save_to_downloads(&name, &bytes)
                            .map_err(|error| format!("保存失败：{error}")),
                        Err(error) => Err(format!("读取录像失败：{error}")),
                    };
                    let _ = tx.send(result);
                });
            }
            Some(preview::Action::Delete) => {
                let clip_id = preview.clip_id().map(str::to_string);
                let path = preview.path().to_string();
                if let Some(id) = clip_id {
                    self.history.remove(&[id]);
                    self.history.flush();
                }
                // 记录不在列表里（还在上传）时，remove 不会碰文件，这里补一刀。
                crate::paths::delete_cache(Some(&path));
                preview.close(ctx);
                self.notify("已删除这段录像");
                return;
            }
        }
        self.record_preview = Some(preview);
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
                    // 标题栏固定在滚动区外：内容从栏下开始滚，不会滑到「设置」玻璃下面。
                    ui.add_space(bar_bottom - screen.top() + 10.0);
                    egui::ScrollArea::vertical()
                        .id_salt("settings-scroll")
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                        )
                        .show(ui, |ui| {
                            ui.set_width(width);
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
                            ui.add_space(14.0);

                            // 录屏/截屏放最前面：窗口不高，放到底下要滚很久，用户会以为没有这些设置。
                            let record_hotkey = self.preferences.record_hotkey().to_string();
                            let region = self.preferences.record_region();
                            let mic = self.preferences.record_mic();
                            let system_audio = self.preferences.record_system_audio();
                            let long_edge = self.preferences.record_long_edge();
                            let fps = self.preferences.record_fps();
                            let mic_device = self.record_mic_label();
                            let size_label = match long_edge {
                                1920 => "1080p",
                                1280 => "720p",
                                _ => "原始",
                            };
                            settings_group(ui, "录屏", |ui| {
                                if self.hotkeys.is_some() {
                                    if settings_nav_row(ui, "录屏快捷键", &record_hotkey) {
                                        action = Some(SettingsAction::RecordHotkey);
                                    }
                                    settings_footnote(
                                        ui,
                                        "点这一行，然后按下要使用的组合键。再按一次或点停止条结束，最长 60 秒",
                                    );
                                    settings_divider(ui);
                                }
                                if settings_nav_row(
                                    ui,
                                    "录制范围",
                                    if region { "区域" } else { "整屏" },
                                ) {
                                    action = Some(SettingsAction::RecordMode);
                                }
                                settings_divider(ui);
                                if settings_nav_row(ui, "画面大小", size_label) {
                                    action = Some(SettingsAction::RecordSize);
                                }
                                settings_divider(ui);
                                if settings_nav_row(ui, "帧率", &format!("{fps} fps")) {
                                    action = Some(SettingsAction::RecordFps);
                                }
                                settings_divider(ui);
                                if settings_switch_row(
                                    ui,
                                    "录麦克风",
                                    "说话声混进录像。关掉就只有画面和系统声（若系统声也开着）",
                                    mic,
                                ) {
                                    action = Some(SettingsAction::RecordMic(!mic));
                                }
                                if mic {
                                    settings_divider(ui);
                                    if settings_nav_row(ui, "麦克风设备", &mic_device) {
                                        action = Some(SettingsAction::RecordMicDevice);
                                    }
                                    settings_footnote(
                                        ui,
                                        "用哪个输入设备。选「系统默认」就跟着系统当前输入走",
                                    );
                                }
                                settings_divider(ui);
                                if settings_switch_row(
                                    ui,
                                    "录系统声音",
                                    "电脑正在播放的声音。和麦克风可以同时开",
                                    system_audio,
                                ) {
                                    action = Some(SettingsAction::RecordSystem(!system_audio));
                                }
                            });
                            ui.add_space(14.0);

                            let hotkey = self.preferences.screenshot_hotkey().to_string();
                            let keep_window = self.preferences.screenshot_keep_window();
                            settings_group(ui, "截屏", |ui| {
                                if self.hotkeys.is_some() {
                                    if settings_nav_row(ui, "截屏快捷键", &hotkey) {
                                        action = Some(SettingsAction::Hotkey);
                                    }
                                    settings_footnote(
                                        ui,
                                        "点这一行，然后按下要使用的组合键。按一下快捷键打开框选：图进剪贴板，也存进列表",
                                    );
                                    settings_divider(ui);
                                }
                                if settings_switch_row(
                                    ui,
                                    "截图时保留本窗口",
                                    "开着时冻屏里还能看见 ClipSyncRust；关掉会先藏起本窗口再截，画面里就不会出现自己",
                                    keep_window,
                                ) {
                                    action = Some(SettingsAction::ScreenshotKeepWindow(
                                        !keep_window,
                                    ));
                                }
                            });
                            ui.add_space(14.0);

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
                            ui.add_space(14.0);

                            let watch = self.watch_clipboard;
                            let auto_copy = self.auto_copy;
                            let auto_paste_file = self.auto_paste_file;
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
                                settings_divider(ui);
                                if settings_switch_row(
                                    ui,
                                    "收到文件即进剪贴板",
                                    "同伴发来的文件自动下完就直接放进剪贴板，回窗口 Cmd/Ctrl+V 就能粘走；关掉则要点条目里的复制才写入",
                                    auto_paste_file,
                                ) {
                                    action =
                                        Some(SettingsAction::AutoPasteFile(!auto_paste_file));
                                }
                            });
                            ui.add_space(14.0);

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
                    .corner_radius(CornerRadius::same(20))
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
        theme::frost(ctx, response.response.rect, 20, theme::frost_layer());
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
            SettingsAction::Hotkey => self.open(Overlay::Hotkey {
                record: false,
                error: None,
            }),
            SettingsAction::RecordHotkey => self.open(Overlay::Hotkey {
                record: true,
                error: None,
            }),
            SettingsAction::RecordMode => self.open(Overlay::Choice(SettingsChoice::RecordMode)),
            SettingsAction::RecordSize => self.open(Overlay::Choice(SettingsChoice::RecordSize)),
            SettingsAction::RecordFps => self.open(Overlay::Choice(SettingsChoice::RecordFps)),
            SettingsAction::RecordMicDevice => {
                self.record_inputs = record::inputs();
                self.open(Overlay::Choice(SettingsChoice::RecordMicDevice));
            }
            SettingsAction::RecordMic(enabled) => self.preferences.set_record_mic(enabled),
            SettingsAction::RecordSystem(enabled) => {
                self.preferences.set_record_system_audio(enabled);
            }
            SettingsAction::Nickname => self.open(Overlay::Nickname {
                input: self.preferences.nickname().to_string(),
            }),
            SettingsAction::RememberRoom(enabled) => self.set_remember_room(enabled),
            SettingsAction::LaunchAtLogin(enabled) => self.set_launch_at_login(enabled),
            SettingsAction::WatchClipboard(enabled) => self.set_watch_clipboard(enabled),
            SettingsAction::AutoCopy(enabled) => self.set_auto_copy(enabled),
            SettingsAction::AutoPasteFile(enabled) => self.set_auto_paste_file(enabled),
            SettingsAction::WatchImages(enabled) => self.set_watch_images(enabled),
            SettingsAction::WatchFiles(enabled) => self.set_watch_files(enabled),
            SettingsAction::DeleteExpired(enabled) => self.set_delete_expired(enabled),
            SettingsAction::ScreenshotKeepWindow(enabled) => {
                self.preferences.set_screenshot_keep_window(enabled);
            }
            SettingsAction::EditRoom => self.open(Overlay::Share),
            SettingsAction::About => self.open(Overlay::About),
            SettingsAction::LeaveRoom => self.open(Overlay::LeaveRoom),
            SettingsAction::Quit => {
                self.force_quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
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
        let Some(file) = self.history.file(id) else {
            return;
        };
        if self.pending_downloads.contains(id) {
            // 任务还在飞就再点一次：得给句话，不然看着就是按钮坏了
            if after != DownloadAfter::Auto {
                self.notify(format!("{} 正在下载", sanitize_file_name(&file.name)));
            }
            return;
        }
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
            cached: None,
        }]);
    }

    fn pick_files(&mut self) {
        let Some(paths) = rfd::FileDialog::new().pick_files() else {
            return;
        };
        self.ingest_paths(paths);
    }

    /// 选择器与窗口拖入共用：未进房明确报错；目录跳过；普通文件走现有上传。
    fn ingest_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if !matches!(self.status, Status::Online { .. }) || self.room_crypto.is_none() {
            self.notify_error("进房间后才能发送文件");
            return;
        }
        let mut uploads = Vec::new();
        for path in paths {
            if path.is_dir() {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                self.notify_error(format!("跳过了文件夹：{name}"));
                continue;
            }
            if !path.is_file() {
                continue;
            }
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
                cached: None,
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

    /// 条目没了就把纹理放掉，别让显存里留着已删除记录的图。
    fn prune_thumbnails(&mut self) {
        self.thumbnails
            .retain(|id, _| self.history.items.iter().any(|record| &record.id == id));
    }

    /// 行上要不要留缩略图的位置：看记录本身，不看纹理到没到，否则图晚一帧就会让行跳。
    fn has_thumb_slot(record: &Record) -> bool {
        record
            .file
            .as_ref()
            .is_some_and(|file| file.cached && file.local_path.is_some() && is_image(&file.mime))
    }

    fn thumb_key(record: &Record) -> Option<(String, String)> {
        if !Self::has_thumb_slot(record) {
            return None;
        }
        let file = record.file.as_ref()?;
        Some((record.id.clone(), file.local_path.clone()?))
    }

    /// 缩略图按需解码：一次全屏解码就是几十 MB，500 条历史全部预解码能把峰值顶到 200MB。
    /// 缓存只留最近的 THUMB_CACHE 张，划过去的旧图腾地方。
    fn load_thumb(
        &mut self,
        ctx: &egui::Context,
        id: String,
        path: String,
    ) -> Option<TextureHandle> {
        self.thumb_tick += 1;
        let used = self.thumb_tick;
        let known = self
            .thumbnails
            .get(&id)
            .filter(|entry| entry.path == path)
            .map(|entry| (entry.attempted, entry.texture.clone()));
        if let Some((attempted, texture)) = known {
            if attempted {
                if let Some(entry) = self.thumbnails.get_mut(&id) {
                    entry.used = used;
                }
                return texture;
            }
        } else {
            self.thumbnails.insert(
                id.clone(),
                ThumbEntry {
                    path: path.clone(),
                    texture: None,
                    attempted: false,
                    used,
                },
            );
        }
        if self.thumbnails.len() > THUMB_CACHE {
            let laziest = self
                .thumbnails
                .iter()
                .filter(|(held, _)| *held != &id)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(held, _)| held.clone());
            if let Some(laziest) = laziest {
                self.thumbnails.remove(&laziest);
            }
        }
        let texture = load_texture(ctx, &path, THUMB_MAX_SIDE, format!("thumb-{id}"));
        if let Some(entry) = self.thumbnails.get_mut(&id) {
            entry.attempted = true;
            entry.used = used;
            entry.texture = texture.clone();
        }
        texture
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

    fn visible_count(&self) -> usize {
        visible_count(&self.history, &self.query, self.regex, self.case_sensitive)
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
            AppEvent::DragOut(id) => self.queue_drag_out(&id),
            AppEvent::Menu(id, action) => self.handle_menu(id, action),
            AppEvent::Share => self.open(Overlay::Share),
            AppEvent::Retry => {
                self.engine.retry_now();
                self.status = Status::Connecting;
            }
        }
    }

    /// 多选时拖其中一条则拖整批；文本→临时 txt，已缓存文件走本机路径。
    fn queue_drag_out(&mut self, id: &str) {
        let ids: Vec<String> = if self.pick_mode && self.picked.contains(id) {
            self.picked.iter().cloned().collect()
        } else {
            vec![id.to_string()]
        };
        let records: Vec<&Record> = ids.iter().filter_map(|rid| self.record(rid)).collect();
        let paths = crate::drag_out::prepare_paths(&records);
        if paths.is_empty() {
            self.notify_error("没有可拖出的本机文件（文本会生成 txt，文件需已下载）");
            return;
        }
        self.pending_drag_out = Some(paths);
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
        self.drain_shell_signals(ctx);
        self.drain_events();
        self.drain_jobs();
        self.drain_updates();
        self.drain_record(ctx);
        if self
            .hotkeys
            .as_ref()
            .is_some_and(|hotkeys| hotkeys.take_record_pressed())
        {
            self.toggle_recording(ctx);
        }
        if self
            .hotkeys
            .as_ref()
            .is_some_and(|hotkeys| hotkeys.take_pressed())
            && self.record_phase == RecordPhase::Idle
        {
            self.record_pick = false;
            self.start_capture(ctx);
        }
        self.take_capture(ctx);
        self.service_shot(ctx);
        self.service_recording(ctx);
        self.service_preview(ctx);
        self.poll_file_drops(ctx);
        self.poll_clipboard();
        self.poll_translation();
        self.expire_toast();
        let update_busy = matches!(
            self.update,
            UpdateState::Checking | UpdateState::Downloading { .. }
        );
        let drop_hover = self.file_drop_hovering(ctx);
        let wait = if self.toast.is_some()
            || self.translate_rx.is_some()
            || self.transfer.is_some()
            || !self.pending_downloads.is_empty()
            || update_busy
            || drop_hover
        {
            Duration::from_millis(120)
        } else {
            POLL_INTERVAL
        };
        ctx.request_repaint_after(wait);
    }

    /// 主屏且没有对话框/截屏遮罩时才接拖入。
    fn accepts_file_drops(&self) -> bool {
        self.screen == Screen::Main
            && self.overlay.is_none()
            && self.shot.is_none()
            && !self.shot_visible
    }

    fn file_drop_hovering(&self, ctx: &egui::Context) -> bool {
        self.accepts_file_drops() && ctx.input(|i| !i.raw.hovered_files.is_empty())
    }

    fn poll_file_drops(&mut self, ctx: &egui::Context) {
        let send_file = matches!(self.overlay, Some(Overlay::SendFile));
        if !self.accepts_file_drops() && !send_file {
            return;
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        if send_file && !self.drop_inside_send_file(ctx) {
            return;
        }
        if send_file {
            self.overlay = None;
        }
        self.ingest_paths(dropped);
    }

    /// 松手点在发送文件框里，或上一帧光标还在框里（松手当帧坐标有时已经空了）。
    fn drop_inside_send_file(&self, ctx: &egui::Context) -> bool {
        let zone = ctx.data(|data| data.get_temp::<Rect>(Id::new("send-file-zone")));
        let hot = ctx
            .data(|data| data.get_temp::<bool>(Id::new("send-file-hot")))
            .unwrap_or(false);
        let pos = ctx.input(|input| input.pointer.hover_pos());
        match (zone, pos) {
            (Some(rect), Some(pos)) => rect.contains(pos) || hot,
            (Some(_), None) => hot,
            _ => false,
        }
    }

    fn paint_drop_overlay(&self, ctx: &egui::Context) {
        if !self.file_drop_hovering(ctx) {
            return;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            Order::Foreground,
            Id::new("file-drop-overlay"),
        ));
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(110));
        let label = "松开以发送文件";
        let font = FontId::proportional(16.0);
        let galley =
            ctx.fonts_mut(|fonts| fonts.layout_no_wrap(label.to_string(), font, theme::p().text));
        let pad = Vec2::new(28.0, 18.0);
        let size = galley.size() + pad * 2.0;
        let card = Rect::from_center_size(screen.center(), size);
        painter.rect(
            card,
            CornerRadius::same(18),
            theme::with_alpha(theme::p().surface, 235),
            Stroke::new(2.0, theme::with_alpha(theme::p().accent, 220)),
            StrokeKind::Inside,
        );
        painter.galley(card.min + pad, galley, theme::p().text);
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
        self.prune_thumbnails();
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
                                let area = ui.clip_rect();
                                for (position, index) in visible.iter().enumerate() {
                                    let row_top = ui.cursor().top();
                                    // 视口附近才解码，远处的行等滚过来再补。
                                    let key = if row_top + THUMB_SCAN >= area.top()
                                        && row_top <= area.bottom() + THUMB_SCAN
                                    {
                                        ClipSyncApp::thumb_key(&self.history.items[*index])
                                    } else {
                                        None
                                    };
                                    let thumb = key
                                        .and_then(|(id, path)| self.load_thumb(ui.ctx(), id, path));
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
                                        slot: ClipSyncApp::has_thumb_slot(record),
                                        thumb,
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
        self.paint_drop_overlay(&ctx);
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

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.draw(ui);
        if let Some(paths) = self.pending_drag_out.take() {
            crate::drag_out::begin(frame, paths);
        }
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
                    self.open(Overlay::Devices);
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
                self.open(Overlay::SendFile);
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
            // 条数跟着当前搜索走：搜着的是命中数，清空搜索就是全部。
            let total = self.visible_count();
            if total > 0 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("共 {total} 条记录"))
                            .font(FontId::proportional(11.5))
                            .color(theme::p().muted),
                    );
                });
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
                "同屋的 {} 发来的东西本机解不开：多半是某一端的房间密码打错了，两边收不到彼此的剪贴板",
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
            Overlay::Devices => 400.0,
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
                        // 对齐 Compose HelpDialog：序号顶对齐、固定列宽，正文 wrap，避免圆点垂直居中把字顶歪。
                        egui::ScrollArea::vertical()
                            .max_height(360.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 12.0;
                                for (index, step) in HELP_STEPS.iter().enumerate() {
                                    ui.horizontal_top(|ui| {
                                        ui.add_sized(
                                            [16.0, 16.0],
                                            egui::Label::new(
                                                RichText::new((index + 1).to_string())
                                                    .monospace()
                                                    .size(12.0)
                                                    .color(theme::p().accent),
                                            ),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(*step)
                                                    .size(12.5)
                                                    .color(theme::p().text),
                                            )
                                            .wrap(),
                                        );
                                    });
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
                        if self.peers.is_empty() && self.mismatch.is_empty() {
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
                                            let (label, color) = if connected {
                                                ("一致", theme::p().green)
                                            } else {
                                                ("未知", theme::p().faint)
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
                            for label in &self.mismatch {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(label)
                                            .size(12.5)
                                            .color(theme::p().text),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new("解不开")
                                                    .size(11.0)
                                                    .strong()
                                                    .color(theme::p().red),
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
                    Overlay::Devices => {
                        dialog_header(ui, Icon::Devices, "同屋设备");
                        ui.add_space(12.0);
                        let mine = self.sender_name();
                        let mine_os = paths::device_os();
                        egui::ScrollArea::vertical()
                            .max_height(360.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                device_row(ui, &mine, mine_os, true);
                                ui.add_space(6.0);
                                if self.peers.is_empty() {
                                    hint(ui, "暂时没有其他设备在线。");
                                } else {
                                    for peer in &self.peers {
                                        device_row(ui, &peer.name, &peer.os, false);
                                        ui.add_space(6.0);
                                    }
                                }
                            });
                        ui.add_space(14.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary_button(ui, "关闭").clicked() {
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
                            SettingsChoice::RecordMode => (
                                "录制范围",
                                Icon::Select,
                                vec!["区域".to_string(), "整屏".to_string()],
                                if self.preferences.record_region() { 0 } else { 1 },
                            ),
                            SettingsChoice::RecordSize => (
                                "画面大小",
                                Icon::Select,
                                vec!["原始".to_string(), "1080p".to_string(), "720p".to_string()],
                                match self.preferences.record_long_edge() {
                                    1920 => 1,
                                    1280 => 2,
                                    _ => 0,
                                },
                            ),
                            SettingsChoice::RecordFps => (
                                "帧率",
                                Icon::Select,
                                vec!["60 fps".to_string(), "30 fps".to_string(), "15 fps".to_string()],
                                match self.preferences.record_fps() {
                                    60 => 0,
                                    15 => 2,
                                    _ => 1,
                                },
                            ),
                            SettingsChoice::RecordMicDevice => {
                                let selected = self.preferences.record_mic_device();
                                let mut labels = vec!["系统默认".to_string()];
                                labels.extend(
                                    self.record_inputs
                                        .iter()
                                        .map(|input| input.name.clone()),
                                );
                                let index = self
                                    .record_inputs
                                    .iter()
                                    .position(|input| input.id == selected)
                                    .map(|at| at + 1)
                                    .unwrap_or(0);
                                ("麦克风设备", Icon::Select, labels, index)
                            }
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
                                    SettingsChoice::RecordMode => {
                                        DialogAction::SetRecordRegion(index == 0)
                                    }
                                    SettingsChoice::RecordSize => DialogAction::SetRecordLongEdge(
                                        match index {
                                            1 => 1920,
                                            2 => 1280,
                                            _ => 0,
                                        },
                                    ),
                                    SettingsChoice::RecordFps => DialogAction::SetRecordFps(
                                        match index {
                                            0 => 60,
                                            2 => 15,
                                            _ => 30,
                                        },
                                    ),
                                    SettingsChoice::RecordMicDevice => {
                                        let id = if index == 0 {
                                            String::new()
                                        } else {
                                            self.record_inputs
                                                .get(index - 1)
                                                .map(|input| input.id.clone())
                                                .unwrap_or_default()
                                        };
                                        DialogAction::SetRecordMicDevice(id)
                                    }
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
                    Overlay::Hotkey { record, error } => {
                        let hotkey = if *record {
                            self.preferences.record_hotkey().to_string()
                        } else {
                            hotkey.clone()
                        };
                        dialog_header(
                            ui,
                            Icon::Select,
                            if *record { "录屏快捷键" } else { "截屏快捷键" },
                        );
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
                                "现在按下要使用的组合键，需要带上 Command、Control 或 Alt",
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
                                action = if *record {
                                    DialogAction::SetRecordHotkey(spec)
                                } else {
                                    DialogAction::SetHotkey(spec)
                                };
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
                            if let Some(texture) = Self::thumb_key(&record)
                                .and_then(|(thumb_id, path)| self.load_thumb(ui.ctx(), thumb_id, path))
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
                    Overlay::SendFile => {
                        dialog_header(ui, Icon::Attach, "发送文件");
                        ui.add_space(12.0);
                        let width = ui.available_width();
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(width, 112.0), Sense::hover());
                        let hovering = ui.input(|input| {
                            !input.raw.hovered_files.is_empty()
                                && input
                                    .pointer
                                    .hover_pos()
                                    .is_some_and(|pos| rect.contains(pos))
                        });
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(Id::new("send-file-zone"), rect);
                            data.insert_temp(Id::new("send-file-hot"), hovering);
                        });
                        let stroke = if hovering {
                            Stroke::new(2.0, theme::p().accent)
                        } else {
                            Stroke::new(1.5, theme::p().accent.gamma_multiply(0.55))
                        };
                        ui.painter().rect(
                            rect,
                            CornerRadius::same(14),
                            if hovering {
                                theme::p().accent.gamma_multiply(0.12)
                            } else {
                                Color32::from_white_alpha(8)
                            },
                            stroke,
                            StrokeKind::Inside,
                        );
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            "把文件拖到这个框里就能上传",
                            FontId::proportional(14.0),
                            theme::p().text,
                        );
                        ui.add_space(14.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if accent_button(ui, "从电脑选择").clicked() {
                                action = DialogAction::PickFiles;
                            }
                            ui.add_space(8.0);
                            if secondary_button(ui, "取消").clicked() {
                                close = true;
                            }
                        });
                    }
                }
            });
        // 磨砂衬在对话框底下（对齐 Compose Popup + GlassCard）
        theme::frost(
            ctx,
            response.response.rect,
            theme::DIALOG_RADIUS,
            theme::frost_layer(),
        );
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
                    overlay = Overlay::Hotkey {
                        record: false,
                        error: Some(error),
                    };
                }
            },
            DialogAction::SetRecordHotkey(spec) => match self.set_record_hotkey(&spec) {
                Ok(()) => keep = false,
                Err(error) => {
                    overlay = Overlay::Hotkey {
                        record: true,
                        error: Some(error),
                    };
                }
            },
            DialogAction::SetRecordRegion(region) => {
                self.preferences.set_record_region(region);
                keep = false;
            }
            DialogAction::SetRecordLongEdge(edge) => {
                self.preferences.set_record_long_edge(edge);
                keep = false;
            }
            DialogAction::SetRecordFps(fps) => {
                self.preferences.set_record_fps(fps);
                keep = false;
            }
            DialogAction::SetRecordMicDevice(id) => {
                self.preferences.set_record_mic_device(&id);
                keep = false;
            }
            DialogAction::PickFiles => {
                self.pick_files();
                keep = false;
            }
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
        slot,
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
    let thumb_gap = if slot { 54.0 } else { 8.0 };
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
    let (top_pad, thumb_top) = if slot {
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
    // click_and_drag：点按仍激活；拖动手势开始则拖出到访达。
    let card_response = ui.interact(card, id.with("card"), Sense::click_and_drag());
    let text_y = card.top() + top_pad;
    let line_y = text_y + line_center;
    if scroll_to {
        ui.scroll_to_rect(card, Some(Align::Center));
    }

    let painter = ui.painter().clone();
    let hovered = card_response.contains_pointer();
    let revealed = hovered || cursor;
    // 对齐 Compose ClipRow：选中/光标用 primary(accent) 透明叠色；浅色空闲行用实心 surface。
    let accent = theme::p().accent;
    let fill = if picked {
        theme::with_alpha(accent, 41)
    } else if cursor {
        theme::with_alpha(accent, 20)
    } else if theme::p().dark {
        theme::with_alpha(theme::p().card, 128)
    } else {
        theme::p().surface
    };
    let border = if picked {
        theme::with_alpha(accent, 140)
    } else if cursor {
        theme::with_alpha(accent, 82)
    } else {
        theme::with_alpha(theme::p().border, 115)
    };
    painter.rect(
        card,
        CornerRadius::same(18),
        fill,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    // 顶沿高光（Compose rimHighlight）
    painter.hline(
        (card.left() + 14.0)..=(card.right() - 14.0),
        card.top() + 1.0,
        Stroke::new(
            1.0,
            Color32::from_white_alpha(if theme::p().dark { 36 } else { 140 }),
        ),
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
    if let Some(texture) = &thumb {
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
    } else if card_response.drag_started() {
        events.push(AppEvent::DragOut(record.id.clone()));
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
    // 对齐 Compose InlineMessage：图标 / 正文 / 操作同一中线，别再写死 center.y-7。
    let avail = ui.available_width();
    let width = (avail - theme::ROW_INSET * 2.0).max(240.0);
    let action_width = if action.is_some() { 78.0 } else { 0.0 };
    let text_width = (width - 40.0 - action_width - 16.0).max(80.0);
    let font = FontId::proportional(12.0);
    let text_color = if error {
        theme::p().on_error_container
    } else {
        theme::p().text
    };
    let galley = {
        let mut job = egui::text::LayoutJob {
            wrap: TextWrapping {
                max_width: text_width,
                max_rows: 2,
                overflow_character: Some('…'),
                ..Default::default()
            },
            ..Default::default()
        };
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: text_color,
                ..Default::default()
            },
        );
        ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
    };
    let card_h = (galley.size().y + 22.0).max(44.0);
    let (outer, _) = ui.allocate_exact_size(Vec2::new(avail, card_h + 8.0), Sense::hover());
    let card = Rect::from_min_max(
        Pos2::new(outer.left() + theme::ROW_INSET, outer.top() + 4.0),
        Pos2::new(
            outer.left() + theme::ROW_INSET + width,
            outer.top() + 4.0 + card_h,
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
    ui.painter().rect(
        card,
        CornerRadius::same(12),
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    let mid = card.center().y;
    icons::paint(
        ui.painter(),
        Rect::from_center_size(Pos2::new(card.left() + 20.0, mid), Vec2::splat(16.0)),
        icon,
        color,
    );
    ui.painter().galley(
        Pos2::new(card.left() + 40.0, mid - galley.size().y * 0.5),
        galley,
        text_color,
    );
    let Some(label) = action else {
        return false;
    };
    let pill = Rect::from_center_size(
        Pos2::new(card.right() - 12.0 - 36.0, mid),
        Vec2::new(72.0, 28.0),
    );
    let response = ui.interact(pill, Id::unique(("banner-action", text)), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(pill, CornerRadius::same(8), Color32::from_white_alpha(12));
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(pill.left() + 14.0, pill.center().y),
            Vec2::splat(14.0),
        ),
        Icon::Key,
        text_color,
    );
    ui.painter().text(
        Pos2::new(pill.left() + 26.0, pill.center().y),
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
        ui.set_height(24.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), Vec2::splat(20.0)),
            icon,
            theme::p().accent,
        );
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.add_space(1.0);
            ui.label(
                RichText::new(title)
                    .size(17.0)
                    .strong()
                    .color(theme::p().text),
            );
        });
    });
}

fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(theme::p().muted));
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(10.5).color(theme::p().faint));
}

fn device_row(ui: &mut Ui, name: &str, os: &str, mine: bool) {
    Frame::new()
        .fill(theme::with_alpha(theme::p().card, 180))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                icons::paint(ui.painter(), icon_rect, Icon::Devices, theme::p().muted);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - if mine { 48.0 } else { 0.0 });
                    ui.label(RichText::new(name).size(12.5).color(theme::p().text));
                    ui.label(RichText::new(os).size(10.5).color(theme::p().faint));
                });
                if mine {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("本机")
                                .size(11.0)
                                .strong()
                                .color(theme::p().accent),
                        );
                    });
                }
            });
        });
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

/// 工具栏上「共 N 条记录」的那个 N：走的是和列表同一套搜索，搜着就只数命中的。
fn visible_count(history: &History, query: &str, regex: bool, case_sensitive: bool) -> usize {
    history.search(query, regex, case_sensitive).len()
}

const SETTINGS_ROW_HEIGHT: f32 = 48.0;
const SETTINGS_SWITCH_HEIGHT: f32 = 52.0;
const SETTINGS_PAD: f32 = 14.0;
const SETTINGS_GROUP_RADIUS: u8 = 18;

fn settings_group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
    paint_line(
        ui,
        Pos2::new(rect.left() + SETTINGS_PAD, rect.top() + 1.0),
        title,
        FontId::proportional(11.5),
        theme::p().muted,
        rect.width() - SETTINGS_PAD * 2.0,
    );
    // 不用 frost Mesh：同层会盖住条目；半透明 surface 才看得见文字。
    let dark = theme::p().dark;
    Frame::new()
        .fill(theme::with_alpha(
            theme::p().surface,
            if dark { 199 } else { 224 },
        ))
        .stroke(Stroke::new(1.0, theme::p().glass_border))
        .corner_radius(CornerRadius::same(SETTINGS_GROUP_RADIUS))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

fn settings_divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        (rect.left() + SETTINGS_PAD)..=(rect.right() - SETTINGS_PAD),
        rect.center().y,
        Stroke::new(1.0, theme::with_alpha(theme::p().border, 71)),
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
        (value_left - 12.0 - rect.left() - SETTINGS_PAD).max(40.0),
    );
    let baseline = baseline_at(&title, center);
    paint_line_at(ui, rect.left() + SETTINGS_PAD, baseline, title);
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
        rect.right() - SETTINGS_PAD
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
        (title_limit - rect.left() - SETTINGS_PAD).max(40.0),
    );
    let baseline = baseline_at(&title, center);
    paint_line_at(ui, rect.left() + SETTINGS_PAD, baseline, title);
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
        chevron.left() - 12.0 - rect.left() - SETTINGS_PAD,
    );
    paint_line_at(
        ui,
        rect.left() + SETTINGS_PAD,
        baseline_at(&title, center),
        title,
    );
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn settings_switch_row(ui: &mut Ui, title: &str, summary: &str, checked: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), SETTINGS_SWITCH_HEIGHT),
        Sense::hover(),
    );
    let title_center = rect.top() + 18.0;
    let switch_id = title;
    let title = row_line(
        ui,
        title,
        FontId::proportional(15.0),
        theme::p().text,
        (rect.width() - SETTINGS_PAD * 2.0 - 36.0 - 12.0).max(60.0),
    );
    paint_line_at(
        ui,
        rect.left() + SETTINGS_PAD,
        baseline_at(&title, title_center),
        title,
    );
    paint_line(
        ui,
        Pos2::new(rect.left() + SETTINGS_PAD, rect.top() + 31.0),
        summary,
        FontId::proportional(10.5),
        theme::with_alpha(theme::p().muted, 230),
        rect.width() - SETTINGS_PAD * 2.0,
    );
    let track = Rect::from_min_size(
        Pos2::new(rect.right() - SETTINGS_PAD - 36.0, title_center - 10.0),
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
        Pos2::new(rect.right() - SETTINGS_PAD - 15.0, center),
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
        (label_right - status.galley.size().x - 12.0 - rect.left() - SETTINGS_PAD).max(60.0),
    );
    let baseline = baseline_at(&title, center);
    paint_line_at(ui, rect.left() + SETTINGS_PAD, baseline, title);
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
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    paint_line(
        ui,
        Pos2::new(rect.left() + SETTINGS_PAD, rect.top() + 1.0),
        text,
        FontId::proportional(10.5),
        theme::p().muted,
        rect.width() - SETTINGS_PAD * 2.0,
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

/// 设置页存的是长边 + 帧率；工具条用三档预设，打开时按长边映射回去。
fn record_options(preferences: &Preferences) -> overlay::RecordOptions {
    overlay::RecordOptions {
        quality: overlay::RecordQuality::of(preferences.record_long_edge()),
        system_audio: preferences.record_system_audio(),
        mic: preferences.record_mic(),
        mic_device: preferences.record_mic_device().to_string(),
    }
}

/// 录制条上的时长：分钟不封顶，120 分钟就是 120:00。
fn record_clock(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
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
    use std::sync::Mutex;

    /// `ClipSyncApp::new` 抓一次真剪贴板，而 NSPasteboard 的类型缓存不容两个线程同时动：
    /// 并发构造直接 SIGSEGV 在 `objc_msgSend`。真程序只有一条 UI 线程，所以只有测试需要这把锁。
    static PASTEBOARD: Mutex<()> = Mutex::new(());

    fn app_lock() -> std::sync::MutexGuard<'static, ()> {
        PASTEBOARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
    fn the_toolbar_count_follows_the_search() {
        let dir = std::env::temp_dir().join(format!("clipsync-count-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut history = super::History::open(dir.join("history.json"));
        for (index, text) in ["alpha one", "beta two", "alpha three"].iter().enumerate() {
            history.add(super::Record {
                id: format!("id{index}"),
                kind: super::KIND_TEXT.to_string(),
                ts: 1_730_000_000_000,
                sender_id: String::new(),
                sender_name: "test".to_string(),
                text: text.to_string(),
                file: None,
                pinned: false,
                order: 0,
                note: String::new(),
            });
        }

        assert_eq!(
            super::visible_count(&history, "", false, false),
            3,
            "没搜索时算全部"
        );
        assert_eq!(
            super::visible_count(&history, "alpha", false, false),
            2,
            "搜索时算命中"
        );
        assert_eq!(
            super::visible_count(&history, "ALPHA", false, false),
            2,
            "不分大小写"
        );
        assert_eq!(
            super::visible_count(&history, "ALPHA", false, true),
            0,
            "分大小写时不算"
        );
        assert_eq!(super::visible_count(&history, "nothing", false, false), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_viewport_clips_at_the_glass_header_top() {
        use clipsync_core::{engine, mqtt::BrokerLinks};

        let _pasteboard = app_lock();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();

        let dir = std::env::temp_dir().join(format!("clipsync-viewport-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let (_show_tx, show_rx) = std::sync::mpsc::channel();
        let mut app = super::ClipSyncApp::new(
            engine::spawn(BrokerLinks),
            super::History::open(dir.join("history.json")),
            super::Settings::open(dir.join("settings.json"), "viewport-test"),
            super::Preferences::open(dir.join("preferences.json")),
            // 测试里不注册全局快捷键：那会占掉这台机器上的 Alt+A
            None,
            show_rx,
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

    #[test]
    fn thumbnails_only_load_the_rows_near_the_viewport() {
        use clipsync_core::{engine, mqtt::BrokerLinks};

        let _pasteboard = app_lock();

        fn png() -> Vec<u8> {
            let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                64,
                64,
                image::Rgba([9, 200, 60, 255]),
            ));
            let mut out = std::io::Cursor::new(Vec::new());
            image
                .write_to(&mut out, image::ImageFormat::Png)
                .expect("png");
            out.into_inner()
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();

        let dir = std::env::temp_dir().join(format!("clipsync-thumbs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let (_show_tx, show_rx) = std::sync::mpsc::channel();
        let mut app = super::ClipSyncApp::new(
            engine::spawn(BrokerLinks),
            super::History::open(dir.join("history.json")),
            super::Settings::open(dir.join("settings.json"), "thumbs-test"),
            super::Preferences::open(dir.join("preferences.json")),
            None,
            show_rx,
            None,
        );
        for index in 0..70 {
            let path = dir.join(format!("clip{index}.png"));
            std::fs::write(&path, png()).unwrap();
            app.history.add(super::Record {
                id: format!("id{index}"),
                kind: super::KIND_FILE.to_string(),
                ts: 1_730_000_000_000,
                sender_id: String::new(),
                sender_name: "test".to_string(),
                text: format!("图 {index}"),
                file: Some(super::FileInfo {
                    name: format!("clip{index}.png"),
                    mime: "image/png".to_string(),
                    size: 64,
                    sha256: String::new(),
                    link: String::new(),
                    expires_at: 0,
                    cached: true,
                    local_path: Some(path.to_string_lossy().into_owned()),
                }),
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
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events = vec![egui::Event::PointerMoved(egui::pos2(460.0, 400.0))];
        let mut output = ctx.run_ui(input, |ui| app.draw(ui));
        output.textures_delta.clear();

        let loaded = app.thumbnails.len();
        assert!(loaded > 0, "视口内的行该有缩略图");
        assert!(
            loaded < 70,
            "只解码视口附近的行，不该把 70 张全部解码（实际 {loaded}）"
        );
        assert!(loaded <= super::THUMB_CACHE, "缓存越过了上限 {loaded}");
        assert!(!app.thumbnails.contains_key("id69"), "屏幕外的旧行不该解码");

        // 位置留给缩略图的槽位由记录决定，纹理晚一帧也不会让行高跳动。
        let with_file = app
            .history
            .items
            .iter()
            .filter(|record| super::ClipSyncApp::has_thumb_slot(record))
            .count();
        assert_eq!(with_file, 70, "每条都该给缩略图留位置");

        // 挨张点名：缓存只留最近用的 THUMB_CACHE 张，最早的要给新行腾地方。
        let keys: Vec<(String, String)> = app
            .history
            .items
            .iter()
            .filter_map(super::ClipSyncApp::thumb_key)
            .collect();
        assert_eq!(keys.len(), 70);
        for (id, path) in &keys {
            app.load_thumb(&ctx, id.clone(), path.clone());
        }
        assert_eq!(app.thumbnails.len(), super::THUMB_CACHE);
        for (index, _) in keys.iter().take(70 - super::THUMB_CACHE) {
            let id = format!("id{index}");
            assert!(!app.thumbnails.contains_key(&id), "{id} 早该被踢掉");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 停止回信只带路径：几百 MB 的录像不该为了写一条列表整段读进 RAM。
    #[test]
    fn a_stopped_recording_is_listed_from_its_path() {
        use clipsync_core::{engine, mqtt::BrokerLinks};

        fn top_box(kind: &[u8; 4], payload: usize) -> Vec<u8> {
            let mut out = ((8 + payload) as u32).to_be_bytes().to_vec();
            out.extend_from_slice(kind);
            out.extend(std::iter::repeat_n(0u8, payload));
            out
        }

        let _pasteboard = app_lock();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();

        let dir = std::env::temp_dir().join(format!("clipsync-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let (_show_tx, show_rx) = std::sync::mpsc::channel();
        let mut app = super::ClipSyncApp::new(
            engine::spawn(BrokerLinks),
            super::History::open(dir.join("history.json")),
            super::Settings::open(dir.join("settings.json"), "stop-test"),
            super::Preferences::open(dir.join("preferences.json")),
            None,
            show_rx,
            None,
        );
        let ctx = egui::Context::default();

        // mdat 撑到 200KB，顶层扫描必须跳过它才看得见后面的 moov。
        let mut clip = top_box(b"ftyp", 16);
        clip.extend(top_box(b"mdat", 200_000));
        clip.extend(top_box(b"moov", 240));
        let temp = dir.join("take.mp4");
        std::fs::write(&temp, &clip).unwrap();
        let size = clip.len() as i64;
        app.record_phase = super::RecordPhase::Stopping;
        app.record_path = Some(temp.clone());
        app.on_record_stopped(&ctx, None, temp.clone(), size, true);

        let file = app.history.items[0]
            .file
            .as_ref()
            .expect("录完的像该进列表");
        assert_eq!(file.size, size, "大小取自磁盘");
        assert_eq!(
            file.sha256,
            clipsync_core::crypto::sha256_hex(&clip),
            "流式哈希要和整段读一致"
        );
        assert_eq!(file.mime, "video/mp4");
        assert!(file.cached);
        let stored = file.local_path.clone().expect("本机留着");
        assert!(
            std::path::Path::new(&stored).starts_with(crate::paths::cache_dir()),
            "{stored} 该搬进缓存"
        );
        assert!(!temp.exists(), "临时文件是搬走的，不是复制一份再留原件");
        assert_eq!(std::fs::read(&stored).unwrap().len(), size as usize);
        assert_eq!(
            app.record_preview.as_ref().expect("就地预览").path(),
            stored,
            "预览和列表要同一条路径"
        );
        assert!(
            !app.toast
                .as_ref()
                .is_some_and(|toast| toast.kind == super::ToastKind::Error),
            "本机留下就该是提示，不是报错"
        );
        crate::paths::delete_cache(Some(&stored));

        // 半成品：当场删掉，列表里不该多一条。
        let broken = dir.join("broken.mp4");
        let mut half = top_box(b"ftyp", 16);
        half.extend(top_box(b"mdat", 32));
        std::fs::write(&broken, &half).unwrap();
        app.record_phase = super::RecordPhase::Stopping;
        app.record_path = Some(broken.clone());
        app.on_record_stopped(&ctx, None, broken.clone(), half.len() as i64, false);
        assert!(!broken.exists(), "没有 moov 的录像要当场删掉");
        assert_eq!(app.history.items.len(), 1, "半成品进不了列表");
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|toast| toast.kind == super::ToastKind::Error));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 下载失败必须解除 `pending_downloads` 占位：那个集合是同一行的去重闸门，
    /// 失败时不清空就等于这条再也点不动——点下去既没进度条也没提示。
    #[test]
    fn a_failed_download_frees_the_row_for_another_try() {
        use clipsync_core::{engine, mqtt::BrokerLinks};

        let _pasteboard = app_lock();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();

        let dir = std::env::temp_dir().join(format!("clipsync-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let (_show_tx, show_rx) = std::sync::mpsc::channel();
        let mut app = super::ClipSyncApp::new(
            engine::spawn(BrokerLinks),
            super::History::open(dir.join("history.json")),
            super::Settings::open(dir.join("settings.json"), "retry-test"),
            super::Preferences::open(dir.join("preferences.json")),
            None,
            show_rx,
            None,
        );
        app.history.add(file_record("clip1", "a.png", "图一"));

        app.pending_downloads.insert("clip1".to_string());
        app.job_tx
            .send(super::JobEvent::DownloadFailed {
                id: "clip1".to_string(),
                message: "链接过期".to_string(),
            })
            .unwrap();
        app.drain_jobs();
        assert!(app.pending_downloads.is_empty(), "失败后这条要能重新点下载");
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|toast| toast.text == "链接过期"));

        app.request_download("clip1", super::DownloadAfter::Cache);
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|toast| toast.text.contains("还没有上传链接")));

        // 任务真的在飞时重复点，也要有话，不能装作按钮坏了
        app.pending_downloads.insert("clip1".to_string());
        app.request_download("clip1", super::DownloadAfter::Cache);
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|toast| toast.text.contains("正在下载")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 收到文件自动下完后要替用户放进剪贴板；开关关掉则一个字都不该说。
    /// 断言只到「有没有尝试写剪贴板」为止——CI 的 ubuntu 没有 X，真粘还得肉眼验。
    #[test]
    fn an_incoming_file_is_offered_for_paste_when_the_switch_is_on() {
        use clipsync_core::{engine, mqtt::BrokerLinks};

        let _pasteboard = app_lock();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();

        let dir = std::env::temp_dir().join(format!("clipsync-paste-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cached = dir.join("a.png");
        std::fs::write(&cached, b"not really a png").unwrap();

        let (_show_tx, show_rx) = std::sync::mpsc::channel();
        let mut app = super::ClipSyncApp::new(
            engine::spawn(BrokerLinks),
            super::History::open(dir.join("history.json")),
            super::Settings::open(dir.join("settings.json"), "paste-test"),
            super::Preferences::open(dir.join("preferences.json")),
            None,
            show_rx,
            None,
        );
        app.history.add(file_record("clip1", "a.png", "图一"));
        app.history.add(file_record("clip2", "b.png", "图二"));

        let done = |id: &str, after: super::DownloadAfter| {
            super::JobEvent::Downloaded(super::jobs::DownloadDone {
                id: id.to_string(),
                name: "a.png".to_string(),
                path: cached.to_string_lossy().into_owned(),
                bytes: None,
                after,
            })
        };

        app.drain_jobs();
        app.toast = None;
        app.auto_paste_file = true;
        app.job_tx
            .send(done("clip1", super::DownloadAfter::Auto))
            .unwrap();
        app.drain_jobs();
        assert!(
            app.history.file("clip1").is_some_and(|file| file.cached),
            "自动下载完要标记已缓存"
        );
        assert!(
            app.toast
                .as_ref()
                .is_some_and(|toast| toast.text.contains("剪贴板")),
            "开关开着就该尝试写剪贴板"
        );

        app.toast = None;
        app.auto_paste_file = false;
        app.job_tx
            .send(done("clip2", super::DownloadAfter::Auto))
            .unwrap();
        app.drain_jobs();
        assert!(app.toast.is_none(), "开关关掉就不该碰剪贴板");
        assert!(app.history.file("clip2").is_some_and(|file| file.cached));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
