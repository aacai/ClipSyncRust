//! 录完自动弹出的预览窗口。
//!
//! macOS 上视频画面是 AVFoundation 的 `AVPlayerLayer`，挂到预览窗口 contentView 的一个子视图上；
//! 窗口本身和播放/分享/下载/删除控件仍然是 egui。其它平台没有系统级 MP4 解码，退回成一张
//! 黑色画面，分享/下载/删除照常可用。
//!
//! 和 Compose 端 `RecordPreview.jvm.kt` 对齐：打开就预滚首帧、内嵌播放/暂停、时间轴、删除确认。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontFamily, FontId, Frame, Id, Layout, Margin, Pos2,
    Rect, RichText, Sense, Shape, Stroke, Ui, Vec2, ViewportCommand, ViewportId,
};

use crate::icons::{self, Icon};

const TITLE: &str = "制作MP4";
const WIDTH: f32 = 920.0;
const HEIGHT: f32 = 640.0;
const MIN_WIDTH: f32 = 680.0;
const MIN_HEIGHT: f32 = 480.0;

const BG: Color32 = Color32::from_rgb(0x10, 0x12, 0x16);
const PANEL: Color32 = Color32::from_rgb(0x17, 0x1a, 0x20);
const LINE: Color32 = Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 0x1f);
const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa1, 0xad);
const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x8d, 0xff);
const DANGER: Color32 = Color32::from_rgb(0xff, 0x45, 0x3a);

/// 预览窗口上的动作，交给 App 处理；播放/拖动直接在预览线程里完成。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Close,
    Share,
    Download,
    Delete,
}

/// 播放器状态；只放普通值，预览窗口的绘制闭包可以安全共享。
#[derive(Clone, Debug)]
struct Playback {
    ready: bool,
    failed: bool,
    playing: bool,
    duration: f64,
    position: f64,
    scrubbing: bool,
    scrub: f64,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            ready: false,
            failed: false,
            playing: false,
            duration: 0.0,
            position: 0.0,
            scrubbing: false,
            scrub: 0.0,
        }
    }
}

struct Shared {
    path: String,
    name: String,
    playback: Playback,
    action: Option<Action>,
    /// 视频画面在窗口内容里的位置，用来摆放 AVPlayerLayer。
    video: Rect,
    /// 窗口内容尺寸，用来把 egui 的 y 翻成 AppKit 的坐标。
    content: Vec2,
    confirm: bool,
}

/// 一次录屏的预览会话。App 只保存这个结构，真正的播放器在下面的线程局部变量里。
pub struct Preview {
    path: String,
    name: String,
    clip_id: Option<String>,
    shared: Arc<Mutex<Shared>>,
    visible: bool,
}

impl Preview {
    pub fn open(path: PathBuf, name: String) -> Self {
        let path = path.to_string_lossy().to_string();
        load(Path::new(&path));
        Self {
            shared: Arc::new(Mutex::new(Shared {
                path: path.clone(),
                name: name.clone(),
                playback: Playback::default(),
                action: None,
                video: Rect::NOTHING,
                content: Vec2::ZERO,
                confirm: false,
            })),
            path,
            name,
            clip_id: None,
            visible: false,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn clip_id(&self) -> Option<&str> {
        self.clip_id.as_deref()
    }

    pub fn set_clip_id(&mut self, id: String) {
        self.clip_id = Some(id);
    }

    /// 注册/刷新预览 viewport；窗口第一次出现时居中并拿到焦点。
    pub fn show(&mut self, ctx: &egui::Context) {
        let shared = self.shared.clone();
        ctx.show_viewport_deferred(
            viewport_id(),
            egui::ViewportBuilder::default()
                .with_title(TITLE)
                .with_inner_size([WIDTH, HEIGHT])
                .with_min_inner_size([MIN_WIDTH, MIN_HEIGHT])
                .with_resizable(true)
                .with_visible(true),
            move |ui, _| paint(ui, &shared),
        );
        if !self.visible {
            self.visible = true;
            let monitor = ctx
                .input(|i| i.viewport().monitor_size)
                .unwrap_or(Vec2::new(1440.0, 900.0));
            let position = Pos2::new(
                ((monitor.x - WIDTH) / 2.0).max(0.0),
                ((monitor.y - HEIGHT) / 2.0).max(0.0),
            );
            ctx.send_viewport_cmd_to(viewport_id(), ViewportCommand::OuterPosition(position));
            ctx.send_viewport_cmd_to(viewport_id(), ViewportCommand::Focus);
        }
    }

    pub fn take_action(&self) -> Option<Action> {
        let mut shared = lock(&self.shared);
        shared.action.take()
    }

    pub fn close(&mut self, ctx: &egui::Context) {
        if self.visible {
            self.visible = false;
            ctx.send_viewport_cmd_to(viewport_id(), ViewportCommand::Visible(false));
        }
        unload();
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        unload();
    }
}

fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("record-preview")
}

fn lock(shared: &Arc<Mutex<Shared>>) -> std::sync::MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn paint(ui: &mut Ui, shared: &Arc<Mutex<Shared>>) {
    let mut state = lock(shared);
    tick_player(&mut state.playback);

    if ui.input(|i| i.viewport().close_requested()) {
        // 和截屏遮罩一样：只藏窗口，别真的销毁原生窗，否则下一次预览会再叠一层。
        ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
        state.action = Some(Action::Close);
    }

    body(ui, &mut state);

    let exists = Path::new(&state.path).is_file();
    if exists && state.playback.ready && !state.playback.failed {
        place_player(state.video, state.content);
    } else {
        hide_player();
    }

    if state.action.is_some() {
        ui.ctx().request_repaint_of(ViewportId::ROOT);
    }
    ui.ctx().request_repaint_after(if state.playback.playing {
        Duration::from_millis(80)
    } else {
        Duration::from_millis(250)
    });
}

fn body(ui: &mut Ui, state: &mut Shared) {
    let exists = Path::new(&state.path).is_file();
    state.content = ui.max_rect().size();

    egui::CentralPanel::default()
        .frame(Frame::NONE.fill(BG).inner_margin(Margin::symmetric(20, 12)))
        .show_inside(ui, |ui| {
            header(ui, state, exists);
            ui.add_space(10.0);

            let controls_height = 58.0;
            let video_height = (ui.available_height() - controls_height - 10.0).max(80.0);
            let (video, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), video_height),
                Sense::hover(),
            );
            state.video = video;
            ui.painter().rect_filled(video, 10.0, Color32::BLACK);
            let message = if !exists {
                Some("文件已经不在")
            } else if !cfg!(target_os = "macos") {
                Some("当前平台不支持内嵌预览，可下载后查看")
            } else if state.playback.failed {
                Some("无法播放这段录像")
            } else if !state.playback.ready {
                Some("正在载入…")
            } else {
                None
            };
            if let Some(message) = message {
                ui.painter().text(
                    video.center(),
                    Align2::CENTER_CENTER,
                    message,
                    FontId::proportional(13.0),
                    MUTED,
                );
            }

            ui.add_space(10.0);
            let (row, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), controls_height),
                Sense::hover(),
            );
            controls(ui, state, row, exists);
        });

    if state.confirm {
        confirm_delete(ui, state);
    }
}

fn header(ui: &mut Ui, state: &mut Shared, exists: bool) {
    let (header, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
    let name = if state.name.chars().count() > 64 {
        let mut short: String = state.name.chars().take(63).collect();
        short.push('…');
        short
    } else {
        state.name.clone()
    };
    ui.painter().text(
        Pos2::new(header.min.x, header.min.y + 3.0),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(13.0),
        Color32::WHITE,
    );
    ui.painter().text(
        Pos2::new(header.min.x, header.min.y + 23.0),
        Align2::LEFT_TOP,
        if exists { "MP4" } else { "文件已经不在" },
        FontId::proportional(11.0),
        MUTED,
    );

    let close = Rect::from_center_size(
        Pos2::new(header.max.x - 15.0, header.center().y),
        Vec2::splat(30.0),
    );
    let response = ui.interact(close, Id::new("preview-close"), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(close, 8.0, Color32::from_white_alpha(12));
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(close.center(), Vec2::splat(18.0)),
        Icon::Close,
        MUTED,
    );
    if response.clicked() {
        state.action = Some(Action::Close);
    }
}

fn controls(ui: &mut Ui, state: &mut Shared, row: Rect, exists: bool) {
    let center_y = row.center().y;
    let enabled = exists && state.playback.ready && !state.playback.failed;

    let play = Rect::from_center_size(Pos2::new(row.min.x + 21.0, center_y), Vec2::splat(42.0));
    if play_button(ui, play, state.playback.playing, enabled).clicked() {
        toggle_player(&mut state.playback);
    }

    let at = if state.playback.scrubbing {
        state.playback.scrub
    } else {
        state.playback.position
    };
    let time = format!("{} / {}", clock(at), clock(state.playback.duration));
    ui.painter().text(
        Pos2::new(row.min.x + 56.0, center_y),
        Align2::LEFT_CENTER,
        time,
        FontId::new(12.0, FontFamily::Monospace),
        MUTED,
    );

    let mut right = row.max.x;
    let delete = Rect::from_min_size(
        Pos2::new(right - 66.0, center_y - 16.0),
        Vec2::new(66.0, 32.0),
    );
    right -= 74.0;
    let download = Rect::from_min_size(
        Pos2::new(right - 72.0, center_y - 16.0),
        Vec2::new(72.0, 32.0),
    );
    right -= 80.0;
    let share = Rect::from_min_size(
        Pos2::new(right - 66.0, center_y - 16.0),
        Vec2::new(66.0, 32.0),
    );

    if action_button(ui, share, Icon::Export, "分享", Color32::WHITE).clicked() {
        state.action = Some(Action::Share);
    }
    if action_button(ui, download, Icon::Download, "下载", Color32::WHITE).clicked() {
        state.action = Some(Action::Download);
    }
    if action_button(ui, delete, Icon::Trash, "删除", DANGER).clicked() {
        state.confirm = true;
    }

    let rail_left = row.min.x + 160.0;
    let rail_right = (share.min.x - 16.0).max(rail_left + 40.0);
    let rail = Rect::from_min_max(
        Pos2::new(rail_left, center_y - 2.0),
        Pos2::new(rail_right, center_y + 2.0),
    );
    ui.painter()
        .rect_filled(rail, 2.0, Color32::from_white_alpha(50));

    let max = state.playback.duration.max(0.1);
    let ratio = (at / max).clamp(0.0, 1.0) as f32;
    let filled = Rect::from_min_max(
        rail.min,
        Pos2::new(rail.min.x + rail.width() * ratio, rail.max.y),
    );
    ui.painter().rect_filled(filled, 2.0, ACCENT);
    ui.painter()
        .circle_filled(Pos2::new(filled.max.x, center_y), 6.0, Color32::WHITE);

    let hit = Rect::from_min_max(
        Pos2::new(rail_left, center_y - 10.0),
        Pos2::new(rail_right, center_y + 10.0),
    );
    let response = ui.interact(
        hit,
        Id::new("preview-seek"),
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    if enabled {
        if response.dragged() {
            if let Some(position) = response.interact_pointer_pos() {
                let ratio = ((position.x - rail.min.x) / rail.width().max(1.0)).clamp(0.0, 1.0);
                state.playback.scrubbing = true;
                state.playback.scrub = ratio as f64 * max;
            }
        }
        if response.drag_stopped() || response.clicked() {
            if response.clicked() {
                if let Some(position) = response.interact_pointer_pos() {
                    let ratio = ((position.x - rail.min.x) / rail.width().max(1.0)).clamp(0.0, 1.0);
                    state.playback.scrub = ratio as f64 * max;
                }
            }
            seek_player(state.playback.scrub);
            state.playback.position = state.playback.scrub;
            state.playback.scrubbing = false;
        }
    }
    if enabled && response.hovered() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::PointingHand);
    }
}

fn play_button(ui: &mut Ui, rect: Rect, playing: bool, enabled: bool) -> egui::Response {
    let response = ui.interact(
        rect,
        Id::new("preview-play"),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let center = rect.center();
    ui.painter().circle_filled(
        center,
        rect.width() / 2.0,
        if enabled { ACCENT } else { PANEL },
    );
    if playing {
        let bar = 4.0;
        for x in [-6.0, 2.0] {
            ui.painter().rect_filled(
                Rect::from_min_size(
                    Pos2::new(center.x + x, center.y - 7.0),
                    Vec2::new(bar, 14.0),
                ),
                1.0,
                Color32::WHITE,
            );
        }
    } else {
        ui.painter().add(Shape::convex_polygon(
            vec![
                Pos2::new(center.x - 5.0, center.y - 8.0),
                Pos2::new(center.x + 8.0, center.y),
                Pos2::new(center.x - 5.0, center.y + 8.0),
            ],
            Color32::WHITE,
            Stroke::NONE,
        ));
    }
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

fn action_button(
    ui: &mut Ui,
    rect: Rect,
    icon: Icon,
    label: &str,
    tint: Color32,
) -> egui::Response {
    let response = ui.interact(rect, Id::new(("preview-action", label)), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 8.0, Color32::from_white_alpha(14));
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.min.x + 15.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        icon,
        tint,
    );
    ui.painter().text(
        Pos2::new(rect.min.x + 29.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        tint,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn confirm_delete(ui: &mut Ui, state: &mut Shared) {
    let mut confirmed = false;
    let mut dismissed = false;
    let response = egui::Modal::new(Id::new("preview-delete-modal"))
        .frame(
            Frame::NONE
                .fill(PANEL)
                .stroke(Stroke::new(1.0, LINE))
                .corner_radius(14)
                .inner_margin(Margin::same(20)),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.label(
                RichText::new("删除这段录像？")
                    .color(Color32::WHITE)
                    .size(16.0)
                    .strong(),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("本地文件和这次记录会一起删掉；已经同步到别的设备的副本不受影响。")
                    .color(MUTED)
                    .size(12.0),
            );
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let delete =
                    egui::Button::new(RichText::new("删除").color(Color32::WHITE).size(13.0))
                        .fill(DANGER)
                        .corner_radius(8)
                        .min_size(Vec2::new(72.0, 32.0));
                if ui.add(delete).clicked() {
                    confirmed = true;
                }
                let cancel = egui::Button::new(RichText::new("取消").color(MUTED).size(13.0))
                    .fill(Color32::TRANSPARENT)
                    .corner_radius(8)
                    .min_size(Vec2::new(72.0, 32.0));
                if ui.add(cancel).clicked() {
                    dismissed = true;
                }
            });
        });
    if response.should_close() {
        dismissed = true;
    }
    if confirmed {
        state.confirm = false;
        state.action = Some(Action::Delete);
    } else if dismissed {
        state.confirm = false;
    }
}

fn clock(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

// ------------------------------------------------------------------------------------
// macOS 播放器
// ------------------------------------------------------------------------------------

#[cfg(target_os = "macos")]
thread_local! {
    static PLAYER: std::cell::RefCell<Option<MacPlayer>> = const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "macos")]
enum Warm {
    Waiting,
    Playing(std::time::Instant),
    Done,
}

#[cfg(target_os = "macos")]
struct MacPlayer {
    player: objc2::rc::Retained<objc2_av_foundation::AVPlayer>,
    view: objc2::rc::Retained<objc2_app_kit::NSView>,
    window: Option<objc2::rc::Retained<objc2_app_kit::NSWindow>>,
    warm: Warm,
    ended: bool,
}

#[cfg(target_os = "macos")]
fn load(path: &Path) {
    use objc2::MainThreadOnly;
    use objc2_app_kit::NSView;
    use objc2_av_foundation::{AVLayerVideoGravityResizeAspect, AVPlayer, AVPlayerLayer};
    use objc2_core_graphics::CGColor;
    use objc2_foundation::{NSString, NSURL};

    PLAYER.with(|slot| {
        *slot.borrow_mut() = None;
        if !path.is_file() {
            return;
        }
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        let Some(path) = path.to_str() else {
            return;
        };
        let path = NSString::from_str(path);
        let url = NSURL::fileURLWithPath(&path);
        let player = unsafe { AVPlayer::playerWithURL(&url, mtm) };
        unsafe { player.setVolume(1.0) };
        let layer = unsafe { AVPlayerLayer::playerLayerWithPlayer(Some(&player)) };
        if let Some(gravity) = unsafe { AVLayerVideoGravityResizeAspect } {
            unsafe { layer.setVideoGravity(gravity) };
        }
        layer.setBackgroundColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, 1.0)));
        layer.setMasksToBounds(true);
        layer.setCornerRadius(10.0);
        let view = NSView::initWithFrame(NSView::alloc(mtm), objc2_foundation::NSRect::ZERO);
        view.setWantsLayer(true);
        view.setLayer(Some(&*layer));
        view.setHidden(true);
        *slot.borrow_mut() = Some(MacPlayer {
            player,
            view,
            window: None,
            warm: Warm::Waiting,
            ended: false,
        });
    });
}

#[cfg(not(target_os = "macos"))]
fn load(_path: &Path) {}

#[cfg(target_os = "macos")]
fn unload() {
    PLAYER.with(|slot| {
        if let Some(mut player) = slot.borrow_mut().take() {
            player.hide();
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn unload() {}

#[cfg(target_os = "macos")]
fn tick_player(state: &mut Playback) {
    PLAYER.with(|slot| {
        if let Some(player) = slot.borrow_mut().as_mut() {
            player.tick(state);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn tick_player(_state: &mut Playback) {}

#[cfg(target_os = "macos")]
fn toggle_player(state: &mut Playback) {
    PLAYER.with(|slot| {
        if let Some(player) = slot.borrow_mut().as_mut() {
            player.toggle(state);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn toggle_player(_state: &mut Playback) {}

#[cfg(target_os = "macos")]
fn seek_player(seconds: f64) {
    PLAYER.with(|slot| {
        if let Some(player) = slot.borrow_mut().as_mut() {
            player.seek(seconds);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn seek_player(_seconds: f64) {}

#[cfg(target_os = "macos")]
fn place_player(video: Rect, content: Vec2) {
    PLAYER.with(|slot| {
        if let Some(player) = slot.borrow_mut().as_mut() {
            player.place(video, content);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn place_player(_video: Rect, _content: Vec2) {}

#[cfg(target_os = "macos")]
fn hide_player() {
    PLAYER.with(|slot| {
        if let Some(player) = slot.borrow_mut().as_mut() {
            player.view.setHidden(true);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn hide_player() {}

#[cfg(target_os = "macos")]
impl MacPlayer {
    fn tick(&mut self, state: &mut Playback) {
        use objc2_av_foundation::AVPlayerItemStatus;
        use objc2_core_media::CMTime;

        if let Some(item) = unsafe { self.player.currentItem() } {
            match unsafe { item.status() } {
                AVPlayerItemStatus::ReadyToPlay => {
                    state.ready = true;
                    state.failed = false;
                    let duration = unsafe { item.duration().seconds() };
                    if duration.is_finite() && duration > 0.0 {
                        state.duration = duration;
                    }
                }
                AVPlayerItemStatus::Failed => {
                    state.ready = false;
                    state.failed = true;
                }
                _ => {}
            }
        }
        let position = unsafe { self.player.currentTime().seconds() };
        if position.is_finite() && position >= 0.0 {
            state.position = position;
        }
        state.playing = unsafe { self.player.rate() } > 0.0;

        match self.warm {
            Warm::Waiting if state.ready => {
                // 先静音滚一小段再回到开头：暂停状态也能看到第一帧，而不是一块黑。
                unsafe { self.player.setVolume(0.0) };
                unsafe { self.player.play() };
                self.warm = Warm::Playing(std::time::Instant::now());
                state.playing = false;
            }
            Warm::Playing(start) => {
                state.playing = false;
                state.position = 0.0;
                if start.elapsed() >= Duration::from_millis(180) {
                    unsafe { self.player.pause() };
                    unsafe { self.player.seekToTime(CMTime::with_seconds(0.0, 600)) };
                    unsafe { self.player.setVolume(1.0) };
                    self.warm = Warm::Done;
                }
            }
            _ => {}
        }

        if matches!(self.warm, Warm::Done)
            && state.ready
            && !state.playing
            && state.duration > 0.0
            && state.position >= state.duration - 0.05
        {
            if !self.ended {
                self.ended = true;
                unsafe { self.player.seekToTime(CMTime::with_seconds(0.0, 600)) };
                state.position = 0.0;
            }
        } else if state.duration <= 0.0 || state.position < state.duration - 0.1 {
            self.ended = false;
        }
    }

    fn toggle(&mut self, state: &Playback) {
        use objc2_core_media::CMTime;

        if !matches!(self.warm, Warm::Done) {
            unsafe { self.player.pause() };
            unsafe { self.player.seekToTime(CMTime::with_seconds(0.0, 600)) };
            unsafe { self.player.setVolume(1.0) };
            self.warm = Warm::Done;
        }
        if state.playing {
            unsafe { self.player.pause() };
        } else {
            if state.duration > 0.0 && state.position >= state.duration - 0.05 {
                unsafe { self.player.seekToTime(CMTime::with_seconds(0.0, 600)) };
            }
            unsafe { self.player.play() };
        }
    }

    fn seek(&mut self, seconds: f64) {
        use objc2_core_media::CMTime;

        self.warm = Warm::Done;
        self.ended = false;
        unsafe { self.player.setVolume(1.0) };
        unsafe {
            self.player
                .seekToTime(CMTime::with_seconds(seconds.max(0.0), 600))
        };
    }

    fn attach(&mut self) {
        use objc2_app_kit::NSApplication;

        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        let Some(window) = NSApplication::sharedApplication(mtm)
            .windows()
            .iter()
            .find(|window| window.title().to_string() == TITLE)
        else {
            return;
        };
        let same = self.window.as_ref().is_some_and(|current| {
            std::ptr::eq(
                &**current as *const objc2_app_kit::NSWindow,
                &*window as *const objc2_app_kit::NSWindow,
            )
        });
        if same && unsafe { self.view.superview() }.is_some() {
            return;
        }
        if unsafe { self.view.superview() }.is_some() {
            unsafe { self.view.removeFromSuperview() };
        }
        let Some(content) = window.contentView() else {
            return;
        };
        content.addSubview(&self.view);
        self.window = Some(window);
    }

    fn place(&mut self, video: Rect, content: Vec2) {
        use objc2_foundation::{NSPoint, NSRect, NSSize};

        self.attach();
        let flipped = self
            .window
            .as_ref()
            .and_then(|window| window.contentView())
            .map(|view| view.isFlipped())
            .unwrap_or(true);
        let y = if flipped {
            video.min.y
        } else {
            content.y - video.max.y
        };
        let width = video.width().max(1.0) as f64;
        let height = video.height().max(1.0) as f64;
        let frame = NSRect::new(
            NSPoint::new(video.min.x as f64, y as f64),
            NSSize::new(width, height),
        );
        self.view.setHidden(false);
        self.view.setFrame(frame);
    }

    fn hide(&mut self) {
        unsafe { self.player.pause() };
        self.view.setHidden(true);
        if unsafe { self.view.superview() }.is_some() {
            unsafe { self.view.removeFromSuperview() };
        }
        self.window = None;
    }
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn the_clock_drops_the_hour_part_below_an_hour() {
        assert_eq!(clock(0.0), "00:00");
        assert_eq!(clock(9.9), "00:09");
        assert_eq!(clock(65.0), "01:05");
        assert_eq!(clock(3600.0), "1:00:00");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(f64::NAN), "00:00");
    }
}
