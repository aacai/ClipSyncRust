//! The WeChat capture overlay: the screen is frozen under a dim mask, hovering previews the
//! window a click would take, dragging draws a region, and eight handles adjust it before it is
//! cut out of the frame.
//!
//! 指针状态机与 Compose `ScreenshotOverlay.kt` 对齐：双击确认、Esc/右键取消、标注工具、撤销重做。
//! 录屏模式在顶部固定一条工具条；截屏模式在选区下方放完整工具栏，标注在确认时烘焙进 PNG。

use std::time::Instant;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontFamily, FontId, Key, PointerButton, Pos2, Rect, Sense,
    Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand,
};
#[cfg(target_os = "macos")]
use objc2::{rc::Retained, MainThreadMarker};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSWindow};

use crate::record::{self, MicInput};

/// Everything but the region the user aims at goes under this much black.
const MASK: Color32 = Color32::from_black_alpha(128);
/// 工具条 / 尺寸徽标的底色，对齐 Compose `Ink`（0xE61A1A1A）。
const INK: Color32 = Color32::from_rgba_premultiplied(0x1a, 0x1a, 0x1a, 0xe6);
/// 选区、按钮和手柄的强调色，对齐 Compose `Accent`（0xFF2B7FFF）。
const ACCENT: Color32 = Color32::from_rgb(0x2b, 0x7f, 0xff);
/// 录屏读数的红点，和截屏的蓝框区分开。
const RECORD: Color32 = Color32::from_rgb(0xff, 0x45, 0x3a);
/// 标注统一红色，和示例一致。
const MARK_RED: Color32 = Color32::from_rgb(0xff, 0x3b, 0x30);
const MUTED: Color32 = Color32::from_rgb(0xb8, 0xb8, 0xb8);
const CONFIRM: Color32 = Color32::from_rgb(0x3f, 0xb9, 0x50);
/// Width reserved for the painted record dot in the readout.
const DOT: f32 = 8.0;
/// A handle is grabbable from this far out, so the pointer never has to land on one exactly.
const HANDLE_SLOP: f32 = 9.0;
const HANDLE_RADIUS: f32 = 5.0;
/// Thinner than this and it was a click, not a region.
const MIN_SIZE: f32 = 6.0;
const TOOLBAR_GAP: f32 = 8.0;
const DOUBLE_TAP_MS: u128 = 400;
const DOUBLE_TAP_SLOP: f32 = 24.0;

/// 截图工具栏：六个工具、撤销/重做、取消/确认。
const SHOT_BAR_HEIGHT: f32 = 45.0;
const SHOT_BAR_PAD: f32 = 8.0;
const SHOT_TOOL: f32 = 32.0;
const SHOT_SMALL: f32 = 30.0;
const SHOT_GAP: f32 = 2.0;
const SHOT_DIVIDER: f32 = 13.0;
const SHOT_SPACER: f32 = 4.0;
const SHOT_CONFIRM: f32 = 32.0;
const SHOT_BAR_WIDTH: f32 = SHOT_BAR_PAD * 2.0
    + SHOT_TOOL * 6.0
    + SHOT_DIVIDER * 2.0
    + SHOT_SMALL * 3.0
    + SHOT_SPACER
    + SHOT_CONFIRM
    + SHOT_GAP * 12.0;

/// 录屏工具条：选择格式、画质、系统声音、麦克风，然后开始/退出。
const RECORD_BAR_HEIGHT: f32 = 54.0;
const RECORD_BAR_PAD: f32 = 12.0;
const RECORD_BAR_GAP: f32 = 8.0;
const RECORD_BUTTON_HEIGHT: f32 = 34.0;
const RECORD_BAR_WIDTH: f32 = 657.0;
const RECORD_MENU_GAP: f32 = 6.0;
const MENU_ITEM_HEIGHT: f32 = 34.0;
const MENU_QUALITY_ITEM: f32 = 46.0;
const MENU_PAD: f32 = 6.0;

/// 录屏画质预设：工具条上选一次，同时决定长边和帧率。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordQuality {
    Native,
    Hd,
    Smooth,
}

impl RecordQuality {
    pub const ALL: [RecordQuality; 3] = [
        RecordQuality::Native,
        RecordQuality::Hd,
        RecordQuality::Smooth,
    ];

    pub fn long_edge(self) -> i32 {
        match self {
            RecordQuality::Native => 0,
            RecordQuality::Hd => 1920,
            RecordQuality::Smooth => 1280,
        }
    }

    pub fn fps(self) -> u32 {
        match self {
            RecordQuality::Native => 60,
            RecordQuality::Hd => 30,
            RecordQuality::Smooth => 15,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RecordQuality::Native => "原画",
            RecordQuality::Hd => "高清",
            RecordQuality::Smooth => "流畅",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            RecordQuality::Native => "屏幕原生分辨率 · 60fps",
            RecordQuality::Hd => "长边 1920 · 30fps",
            RecordQuality::Smooth => "长边 1280 · 15fps",
        }
    }

    pub fn of(long_edge: i32) -> Self {
        match long_edge {
            1280 => RecordQuality::Smooth,
            1920 => RecordQuality::Hd,
            _ => RecordQuality::Native,
        }
    }
}

/// 录屏工具条这一次选的东西；开始录制时原样交给 App。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RecordOptions {
    pub quality: RecordQuality,
    pub system_audio: bool,
    pub mic: bool,
    /// 空串表示系统默认输入设备。
    pub mic_device: String,
}

impl Default for RecordOptions {
    fn default() -> Self {
        Self {
            quality: RecordQuality::Native,
            system_audio: true,
            mic: true,
            mic_device: String::new(),
        }
    }
}

/// 截图工具栏上的动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShotAction {
    Tool(ShotTool),
    Undo,
    Redo,
    Cancel,
    Confirm,
}

/// 截图工具栏上的工具。Select 只负责搬动/缩放选区。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotTool {
    Select,
    Box,
    Arrow,
    Pen,
    Text,
    Mosaic,
}

/// 画在截图上的一个标注。坐标都是遮罩点坐标。
#[derive(Clone, PartialEq, Debug)]
pub enum Mark {
    Box { rect: Rect },
    Arrow { from: Pos2, to: Pos2 },
    Pen { points: Vec<Pos2> },
    Text { at: Pos2, text: String },
    Mosaic { rect: Rect },
}

impl Mark {
    fn worth_keeping(&self) -> bool {
        match self {
            Mark::Box { rect } | Mark::Mosaic { rect } => {
                rect.width() >= MIN_SIZE && rect.height() >= MIN_SIZE
            }
            Mark::Arrow { from, to } => (*to - *from).length() >= MIN_SIZE,
            Mark::Pen { points } => points.len() >= 2,
            Mark::Text { text, .. } => !text.trim().is_empty(),
        }
    }
}

/// 标注列表加上撤销栈；撤销过的还能重做，新画一笔就把重做栈清空。
#[derive(Default)]
struct MarkLayer {
    items: Vec<Mark>,
    undone: Vec<Mark>,
}

impl MarkLayer {
    fn add(&mut self, mark: Mark) {
        self.items.push(mark);
        self.undone.clear();
    }

    fn undo(&mut self) -> bool {
        let Some(last) = self.items.pop() else {
            return false;
        };
        self.undone.push(last);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(mark) = self.undone.pop() else {
            return false;
        };
        self.items.push(mark);
        true
    }

    fn clear(&mut self) {
        self.items.clear();
        self.undone.clear();
    }
}

/// 正在输入的一段文字；还没落到 [MarkLayer] 里。
struct TextDraft {
    at: Pos2,
    text: String,
}

/// The id of the overlay window; one at a time, since only one screen can be frozen.
pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("screenshot-overlay")
}

/// The overlay window itself: borderless, above every other app, and hidden until a capture is
/// pinned over the screen. There is one of them for the whole process because closing a window
/// only closes its egui side — the native window stays behind and the next capture stacks on it.
pub fn viewport() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_app_id("clipsync-screenshot")
        .with_title(TITLE)
        .with_decorations(false)
        .with_titlebar_shown(false)
        .with_titlebar_buttons_shown(false)
        .with_close_button(false)
        .with_minimize_button(false)
        .with_maximize_button(false)
        .with_resizable(false)
        .with_taskbar(false)
        .with_visible(false)
        .with_always_on_top()
        .with_has_shadow(false)
}

/// The window title, which is also how the native macOS layer finds the overlay window.
const TITLE: &str = "ClipSyncRust 截图";

/// One frozen screen plus the windows that were on it, all in overlay-local points.
pub struct Freeze {
    /// The screen in physical pixels.
    pub image: image::RgbaImage,
    /// Where the monitor sits in screen points, so the overlay window can land on it exactly.
    pub position: Pos2,
    /// The monitor in points; the overlay window is exactly this size.
    pub size: Vec2,
    /// Image pixels per point, so a selection in points can be cut back out in pixels.
    pub scale: f32,
    /// On-screen window bounds in overlay-local points, front to back.
    pub windows: Vec<Rect>,
    /// Menu-bar height in points. macOS does not deliver clicks in that strip, even to a
    /// window drawn on top of it, so the toolbar must sit entirely below it.
    pub top_inset: f32,
}

/// What the overlay ended up doing with the frozen frame.
#[derive(Clone, Debug)]
pub enum Outcome {
    Crop { rect: Rect, marks: Vec<Mark> },
    StartRecording { rect: Rect, options: RecordOptions },
    Cancel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

const HANDLES: [Handle; 8] = [
    Handle::TopLeft,
    Handle::Top,
    Handle::TopRight,
    Handle::Right,
    Handle::BottomRight,
    Handle::Bottom,
    Handle::BottomLeft,
    Handle::Left,
];

#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Drawing a region from scratch.
    Fresh { anchor: Pos2 },
    /// Dragging the whole region by its interior.
    Move { grab: Vec2 },
    /// Dragging one edge or corner.
    Handle(Handle),
    /// Drawing a mark inside the selection.
    Paint { tool: ShotTool, start: Pos2 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuKind {
    Quality,
    System,
    Mic,
}

pub struct Session {
    freeze: Freeze,
    texture: Option<TextureHandle>,
    /// The region being adjusted; `None` while still aiming.
    selection: Option<Rect>,
    /// The region a click would take right now.
    hover: Option<Rect>,
    drag: Option<Drag>,
    outcome: Option<Outcome>,
    last_up: Option<Pos2>,
    last_up_at: Instant,
    /// The frozen screen has reached the glass, which is when the app window may come back.
    raised: bool,
    /// The overlay holds the keyboard, so Esc and Enter are its to hear.
    focused: bool,
    /// Picking a region for a recording instead of a screenshot: top toolbar, 录屏 readout.
    record: bool,
    /// 录屏工具条这一次的选择。
    options: RecordOptions,
    /// 可选的麦克风，打开遮罩时枚举一次。
    devices: Vec<MicInput>,
    /// 打开的下拉菜单。
    menu: Option<MenuKind>,
    /// 截图标注。
    tool: ShotTool,
    marks: MarkLayer,
    pending: Option<Mark>,
    draft: Option<TextDraft>,
    draft_focus: bool,
}

impl Session {
    pub fn new(freeze: Freeze, record: bool) -> Self {
        Self::with_options(freeze, record, RecordOptions::default())
    }

    pub fn with_options(freeze: Freeze, record: bool, options: RecordOptions) -> Self {
        Self {
            freeze,
            texture: None,
            selection: None,
            hover: None,
            drag: None,
            outcome: None,
            last_up: None,
            last_up_at: Instant::now(),
            raised: false,
            focused: false,
            record,
            options,
            devices: if record { record::inputs() } else { Vec::new() },
            menu: None,
            tool: ShotTool::Select,
            marks: MarkLayer::default(),
            pending: None,
            draft: None,
            draft_focus: false,
        }
    }

    /// Where the overlay window has to sit for this frame to fill the screen.
    pub fn screen(&self) -> (Pos2, Vec2) {
        (self.freeze.position, self.freeze.size)
    }

    /// Hands the frozen frame over to the caller once the session is done with it.
    pub fn take_image(&mut self) -> Option<image::RgbaImage> {
        if self.freeze.image.width() == 0 {
            return None;
        }
        Some(std::mem::replace(
            &mut self.freeze.image,
            image::RgbaImage::new(0, 0),
        ))
    }

    pub fn scale(&self) -> f32 {
        self.freeze.scale
    }

    pub fn take_outcome(&mut self) -> Option<Outcome> {
        self.outcome.take()
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        // 状态机永远在冻屏坐标系里（对齐 Compose session.frame）；
        // egui 的 max_rect 可能和 NSScreen 不完全一致，指针要换算过去，画再换算回来。
        let logical = Rect::from_min_size(Pos2::ZERO, self.freeze.size);
        let surface = ui.max_rect();
        // 首帧清成黑，避免透明/白屏闪一下
        ui.painter().rect_filled(surface, 0.0, Color32::BLACK);
        self.paint_frame(ui, surface);
        let record_bar = self
            .record
            .then(|| record_bar_at(logical, self.freeze.top_inset));
        let shot_bar = self.shot_bar_rect(logical);
        let position = self.track_pointer(ui, logical, surface, record_bar, shot_bar);
        self.paint_scene(ui, logical, surface);
        if let Some(bar) = shot_bar {
            self.paint_shot_bar(ui, bar, surface, logical);
        }
        if let Some(bar) = record_bar {
            self.paint_record_bar(ui, bar, surface, logical);
            if let Some(kind) = self.menu {
                self.paint_menu(ui, kind, bar, surface, logical);
            }
        }
        self.paint_text_editor(ui, logical, surface);
        self.paint_cursor(ui, position, record_bar, shot_bar);
        self.raise(ui.ctx());
    }

    fn to_logical(pos: Pos2, surface: Rect, logical: Rect) -> Pos2 {
        let nx = (pos.x - surface.min.x) / surface.width().max(1.0);
        let ny = (pos.y - surface.min.y) / surface.height().max(1.0);
        Pos2::new(
            logical.min.x + nx * logical.width(),
            logical.min.y + ny * logical.height(),
        )
    }

    fn to_surface(rect: Rect, surface: Rect, logical: Rect) -> Rect {
        let map = |p: Pos2| {
            let nx = (p.x - logical.min.x) / logical.width().max(1.0);
            let ny = (p.y - logical.min.y) / logical.height().max(1.0);
            Pos2::new(
                surface.min.x + nx * surface.width(),
                surface.min.y + ny * surface.height(),
            )
        };
        Rect::from_min_max(map(rect.min), map(rect.max))
    }

    /// 主窗必须一直藏着：中途 Visible(true) 会叠在冻屏遮罩上，再带毛玻璃就又糊又闪。
    /// 键盘交给 overlay 自己的 key window（下一帧 focus），对齐 Compose「遮罩结束再 restore」。
    fn raise(&mut self, ctx: &egui::Context) {
        if !self.raised {
            self.raised = true;
            ctx.request_repaint();
        } else if !self.focused {
            self.focused = true;
            focus();
        }
    }

    fn paint_frame(&mut self, ui: &Ui, frame: Rect) {
        let texture = match &self.texture {
            Some(texture) => texture.clone(),
            None => {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [
                        self.freeze.image.width() as usize,
                        self.freeze.image.height() as usize,
                    ],
                    self.freeze.image.as_raw(),
                );
                let texture =
                    ui.ctx()
                        .load_texture("screenshot-freeze", color, TextureOptions::LINEAR);
                self.texture = Some(texture.clone());
                texture
            }
        };
        ui.painter().image(
            texture.id(),
            frame,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    /// 对齐 Compose `onEscape` / `onRightClick` / `onDown` / `onUp` / `onHover`。
    /// 返回的是**逻辑坐标**（冻屏坐标系），给光标命中用。
    fn track_pointer(
        &mut self,
        ui: &mut Ui,
        logical: Rect,
        surface: Rect,
        record_bar: Option<Rect>,
        shot_bar: Option<Rect>,
    ) -> Option<Pos2> {
        if self.outcome.is_some() {
            return ui.input(|i| {
                i.pointer
                    .hover_pos()
                    .or(i.pointer.latest_pos())
                    .map(|p| Self::to_logical(p, surface, logical))
            });
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.on_escape();
            return None;
        }
        let pointer = ui.input(|i| i.pointer.clone());
        // latest_pos：松开瞬间 hover_pos 有时是 None，会导致点叉没登记抬起。
        let raw = pointer.hover_pos().or(pointer.latest_pos());
        let position = raw.map(|p| Self::to_logical(p, surface, logical));
        if pointer.button_pressed(PointerButton::Secondary) {
            if self.menu.is_some() {
                self.menu = None;
            } else {
                self.on_right_click();
            }
            return None;
        }
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            self.on_enter();
            return position;
        }

        if pointer.button_pressed(PointerButton::Primary) {
            if let Some(position) = position {
                // 菜单打开时，点菜单外只负责关掉它；这一下不能穿透成新的框选。
                if let Some(kind) = self.menu {
                    if let Some(bar) = record_bar {
                        if let Some(choice) =
                            menu_choice(kind, bar, logical, self.devices.len(), position)
                        {
                            self.apply_menu_choice(kind, choice);
                        } else {
                            self.menu = None;
                        }
                    } else {
                        self.menu = None;
                    }
                    return Some(position);
                }
                if let Some(bar) = record_bar {
                    if bar.contains(position) {
                        self.record_bar_click(position, bar);
                        return Some(position);
                    }
                }
                if let Some(bar) = shot_bar {
                    if bar.contains(position) {
                        self.shot_bar_click(position, bar);
                        return Some(position);
                    }
                }
                if self
                    .draft
                    .as_ref()
                    .is_some_and(|draft| text_rect(draft.at).contains(position))
                {
                    // 文字框自己收点击，别把它当成开始画新区域。
                    return Some(position);
                }
                self.on_down(position, shot_bar);
            }
        } else if let (Some(drag), Some(position)) = (self.drag, position) {
            if pointer.button_down(PointerButton::Primary) {
                self.apply_drag(drag, position, logical);
            }
        }
        if pointer.button_released(PointerButton::Primary) {
            if let Some(position) = position {
                self.on_up(position, logical);
            } else {
                self.drag = None;
            }
        }
        if self.selection.is_none() && self.drag.is_none() {
            if let Some(position) = position {
                self.hover = Some(hit_window(&self.freeze.windows, position, logical));
            }
        }
        position
    }

    fn on_down(&mut self, position: Pos2, shot_bar: Option<Rect>) {
        if self.outcome.is_some() {
            return;
        }
        // 文字还没提交就点别处：先把这段字收下，和 Compose `onDown` 一致。
        if self.draft.is_some() {
            self.commit_text();
        }
        // 双击确认：对齐 Compose DOUBLE_TAP_*，第二下按下时直接确认，别开新 drag。
        if self.tool == ShotTool::Select {
            if let (Some(_), Some(previous)) = (self.selection, self.last_up) {
                let dt = self.last_up_at.elapsed().as_millis();
                if dt <= DOUBLE_TAP_MS && (position - previous).length() <= DOUBLE_TAP_SLOP {
                    self.on_enter();
                    return;
                }
            }
        }
        self.begin_drag(position, shot_bar);
    }

    fn on_up(&mut self, position: Pos2, frame: Rect) {
        if self.outcome.is_some() {
            return;
        }
        self.last_up = Some(position);
        self.last_up_at = Instant::now();
        if matches!(self.drag, Some(Drag::Paint { .. })) {
            let mark = self.pending.take();
            self.drag = None;
            if let Some(mark) = mark.filter(Mark::worth_keeping) {
                self.marks.add(mark);
            }
            return;
        }
        self.finish_drag(position, frame);
    }

    fn begin_drag(&mut self, position: Pos2, shot_bar: Option<Rect>) {
        if let Some(selection) = self.selection {
            if let Some(bar) = shot_bar {
                if bar.contains(position) {
                    // 工具栏点击已经由 `track_pointer` 处理；这里兜底，别擦掉选区。
                    return;
                }
            }
            if let Some(handle) = handle_at(selection, position, HANDLE_SLOP) {
                self.drag = Some(Drag::Handle(handle));
                return;
            }
            if !self.record && self.tool != ShotTool::Select && selection.contains(position) {
                if self.tool == ShotTool::Text {
                    self.draft = Some(TextDraft {
                        at: position,
                        text: String::new(),
                    });
                    self.draft_focus = true;
                    return;
                }
                self.drag = Some(Drag::Paint {
                    tool: self.tool,
                    start: position,
                });
                self.pending = mark_of(self.tool, position, position);
                return;
            }
            if selection.contains(position) {
                self.drag = Some(Drag::Move {
                    grab: position - selection.min,
                });
                return;
            }
        }
        self.drag = Some(Drag::Fresh { anchor: position });
        self.selection = None;
        self.hover = None;
        self.pending = None;
        // 重新框一块就是一张新截图，旧标注不能跟着跑到新区域里。
        if !self.record {
            self.marks.clear();
        }
    }

    fn apply_drag(&mut self, drag: Drag, position: Pos2, frame: Rect) {
        match drag {
            Drag::Fresh { anchor } => {
                self.selection = Some(between(anchor, position).intersect(frame))
            }
            Drag::Move { grab } => {
                if let Some(selection) = self.selection {
                    self.selection = Some(translate(selection, position - grab, frame));
                }
            }
            Drag::Handle(handle) => {
                if let Some(selection) = self.selection {
                    self.selection = Some(resize(selection, handle, position, frame));
                }
            }
            Drag::Paint { tool, start } => {
                self.pending = Some(match tool {
                    ShotTool::Pen => {
                        let mut points = match self.pending.take() {
                            Some(Mark::Pen { points }) => points,
                            _ => vec![start],
                        };
                        points.push(position);
                        Mark::Pen { points }
                    }
                    ShotTool::Box => Mark::Box {
                        rect: between(start, position),
                    },
                    ShotTool::Mosaic => Mark::Mosaic {
                        rect: between(start, position),
                    },
                    ShotTool::Arrow => Mark::Arrow {
                        from: start,
                        to: position,
                    },
                    ShotTool::Select | ShotTool::Text => {
                        self.pending.take().unwrap_or(Mark::Arrow {
                            from: start,
                            to: position,
                        })
                    }
                });
            }
        }
    }

    fn finish_drag(&mut self, position: Pos2, frame: Rect) {
        let Some(Drag::Fresh { anchor }) = self.drag.take() else {
            self.drag = None;
            return;
        };
        let dragged = between(anchor, position).intersect(frame);
        let picked = if is_region(dragged, frame) {
            dragged
        } else {
            hit_window(&self.freeze.windows, position, frame)
        };
        self.selection = Some(picked);
        self.hover = None;
    }

    fn choose_tool(&mut self, tool: ShotTool) {
        self.tool = tool;
        if tool != ShotTool::Text {
            self.draft = None;
        }
    }

    fn undo(&mut self) -> bool {
        self.marks.undo()
    }

    fn redo(&mut self) -> bool {
        self.marks.redo()
    }

    fn commit_text(&mut self) {
        let Some(draft) = self.draft.take() else {
            return;
        };
        let text = draft.text.trim().to_string();
        if !text.is_empty() {
            self.marks.add(Mark::Text { at: draft.at, text });
        }
        self.draft_focus = false;
    }

    fn cancel_text(&mut self) {
        self.draft = None;
        self.draft_focus = false;
    }

    fn on_right_click(&mut self) {
        if self.draft.is_some() {
            self.cancel_text();
            return;
        }
        self.outcome = Some(Outcome::Cancel);
    }

    fn on_escape(&mut self) {
        if self.draft.is_some() {
            self.cancel_text();
            return;
        }
        self.outcome = Some(Outcome::Cancel);
    }

    /// 回车：正在输入就先收下这段字；否则有选区就切图/开录。
    fn on_enter(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        if self.draft.is_some() {
            self.commit_text();
            return;
        }
        // 录屏不强迫先拖选区：没选就录整块屏幕，按钮从打开遮罩起就是可点的。
        let rect = match self.selection {
            Some(rect) => rect,
            None if self.record => Rect::from_min_size(Pos2::ZERO, self.freeze.size),
            None => return,
        };
        self.outcome = Some(if self.record {
            Outcome::StartRecording {
                rect,
                options: self.options.clone(),
            }
        } else {
            Outcome::Crop {
                rect,
                marks: self.marks.items.clone(),
            }
        });
    }

    fn record_bar_click(&mut self, position: Pos2, bar: Rect) {
        let layout = record_bar_layout(bar);
        if layout.quality.contains(position) {
            self.menu = (self.menu != Some(MenuKind::Quality)).then_some(MenuKind::Quality);
            return;
        }
        if layout.system.contains(position) {
            self.menu = (self.menu != Some(MenuKind::System)).then_some(MenuKind::System);
            return;
        }
        if layout.mic.contains(position) {
            // 每次打开都重新枚举：用户可能刚插上耳机。
            self.devices = record::inputs();
            self.menu = (self.menu != Some(MenuKind::Mic)).then_some(MenuKind::Mic);
            return;
        }
        if layout.start.contains(position) {
            self.on_enter();
            return;
        }
        if layout.exit.contains(position) {
            self.on_escape();
        }
    }

    fn apply_menu_choice(&mut self, kind: MenuKind, index: usize) {
        match kind {
            MenuKind::Quality => {
                if let Some(quality) = RecordQuality::ALL.get(index) {
                    self.options.quality = *quality;
                }
            }
            MenuKind::System => self.options.system_audio = index == 1,
            MenuKind::Mic => {
                if index == 0 {
                    self.options.mic = false;
                } else if index == 1 {
                    self.options.mic = true;
                    self.options.mic_device.clear();
                } else if let Some(device) = self.devices.get(index - 2) {
                    self.options.mic = true;
                    self.options.mic_device = device.id.clone();
                }
            }
        }
        self.menu = None;
    }

    fn shot_bar_click(&mut self, position: Pos2, bar: Rect) {
        for (action, rect) in shot_bar_buttons(bar) {
            if !rect.contains(position) {
                continue;
            }
            match action {
                ShotAction::Tool(tool) => self.choose_tool(tool),
                ShotAction::Undo => {
                    self.undo();
                }
                ShotAction::Redo => {
                    self.redo();
                }
                ShotAction::Cancel => self.on_escape(),
                ShotAction::Confirm => self.on_enter(),
            }
            return;
        }
    }

    fn paint_scene(&self, ui: &Ui, logical: Rect, surface: Rect) {
        let hole = self.selection.or(self.hover);
        for rect in mask_rects(logical, hole) {
            if !rect.is_negative() {
                ui.painter()
                    .rect_filled(Self::to_surface(rect, surface, logical), 0.0, MASK);
            }
        }
        let Some(rect) = hole else { return };
        if self.selection.is_some() && !self.record {
            for mark in self.marks.items.iter().chain(self.pending.iter()) {
                self.paint_mark(ui, mark, logical, surface);
            }
        }
        let drawn = Self::to_surface(rect, surface, logical);
        ui.painter()
            .rect_stroke(drawn, 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Middle);
        if self.selection.is_some() && self.drag.is_none() {
            for handle in HANDLES {
                let center = Self::to_surface(
                    Rect::from_center_size(handle_point(rect, handle), Vec2::ZERO),
                    surface,
                    logical,
                )
                .center();
                ui.painter()
                    .circle_filled(center, HANDLE_RADIUS, Color32::WHITE);
                ui.painter()
                    .circle_stroke(center, HANDLE_RADIUS, Stroke::new(1.5, ACCENT));
            }
        }
        self.paint_size(ui, rect, logical, surface);
    }

    fn paint_mark(&self, ui: &Ui, mark: &Mark, logical: Rect, surface: Rect) {
        let map = |p: Pos2| {
            Self::to_surface(Rect::from_min_size(p, Vec2::ZERO), surface, logical).center()
        };
        let stroke = Stroke::new(3.0, MARK_RED);
        match mark {
            Mark::Box { rect } => {
                let rect = Self::to_surface(*rect, surface, logical);
                ui.painter()
                    .rect_stroke(rect, 0.0, stroke, StrokeKind::Middle);
            }
            Mark::Arrow { from, to } => {
                let from = map(*from);
                let to = map(*to);
                ui.painter().line_segment([from, to], stroke);
                let angle = (to.y - from.y).atan2(to.x - from.x);
                let head = 14.0;
                let left = Pos2::new(
                    to.x - head * (angle - std::f32::consts::FRAC_PI_6).cos(),
                    to.y - head * (angle - std::f32::consts::FRAC_PI_6).sin(),
                );
                let right = Pos2::new(
                    to.x - head * (angle + std::f32::consts::FRAC_PI_6).cos(),
                    to.y - head * (angle + std::f32::consts::FRAC_PI_6).sin(),
                );
                ui.painter().add(Shape::convex_polygon(
                    vec![to, left, right],
                    MARK_RED,
                    Stroke::NONE,
                ));
            }
            Mark::Pen { points } => {
                if points.len() >= 2 {
                    let points: Vec<Pos2> = points.iter().map(|p| map(*p)).collect();
                    ui.painter().add(Shape::line(points, stroke));
                }
            }
            Mark::Text { at, text } => {
                let at = map(*at);
                ui.painter().text(
                    at,
                    Align2::LEFT_TOP,
                    text,
                    FontId::new(18.0, FontFamily::Proportional),
                    MARK_RED,
                );
            }
            Mark::Mosaic { rect } => self.paint_mosaic(ui, *rect, logical, surface),
        }
    }

    /// 预览里的马赛克：按 12 点的块从冻屏像素里取色，画成一格一格。
    fn paint_mosaic(&self, ui: &Ui, rect: Rect, logical: Rect, surface: Rect) {
        let image = &self.freeze.image;
        if image.width() == 0 || image.height() == 0 {
            return;
        }
        let block: f32 = 12.0;
        let mut y = rect.min.y;
        while y < rect.max.y {
            let mut x = rect.min.x;
            while x < rect.max.x {
                let px = ((x * self.freeze.scale).round() as i64).clamp(0, image.width() as i64 - 1)
                    as u32;
                let py = ((y * self.freeze.scale).round() as i64)
                    .clamp(0, image.height() as i64 - 1) as u32;
                let pixel = image.get_pixel(px, py);
                let color = Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3]);
                let cell = Rect::from_min_size(
                    Pos2::new(x, y),
                    Vec2::new(block.min(rect.max.x - x), block.min(rect.max.y - y)),
                );
                ui.painter()
                    .rect_filled(Self::to_surface(cell, surface, logical), 0.0, color);
                x += block;
            }
            y += block;
        }
    }

    /// The `宽 * 高` readout, in the pixels the crop will actually be. A recording leads with a
    /// red `● 录屏` so the same frozen screen can't be mistaken for a screenshot.
    fn paint_size(&self, ui: &Ui, rect: Rect, logical: Rect, surface: Rect) {
        let (tag, size) = readout(self.record, rect, self.freeze.scale);
        let font = FontId::proportional(12.0);
        let pad = Vec2::new(7.0, 4.0);
        let gap = 5.0;
        let mut parts = Vec::new();
        if let Some(tag) = tag {
            parts.push((
                ui.painter()
                    .layout_no_wrap(tag.to_string(), font.clone(), RECORD),
                RECORD,
            ));
        }
        parts.push((
            ui.painter().layout_no_wrap(size, font, Color32::WHITE),
            Color32::WHITE,
        ));
        let dot = if self.record { DOT + gap } else { 0.0 };
        let text = Vec2::new(
            dot + parts.iter().map(|(g, _)| g.size().x).sum::<f32>()
                + gap * (parts.len() as f32 - 1.0),
            parts.iter().map(|(g, _)| g.size().y).fold(0.0, f32::max),
        );
        let size = text + pad * 2.0;
        let min = Pos2::new(
            (rect.min.x + 8.0).clamp(0.0, (logical.max.x - size.x).max(0.0)),
            (rect.min.y + 8.0).clamp(0.0, (logical.max.y - size.y).max(0.0)),
        );
        let badge = Self::to_surface(Rect::from_min_size(min, size), surface, logical);
        ui.painter().rect_filled(badge, 4.0, INK);
        let mut x = badge.min.x + pad.x;
        if self.record {
            ui.painter()
                .circle_filled(Pos2::new(x + DOT / 2.0, badge.center().y), 3.5, RECORD);
            x += DOT + gap;
        }
        for (galley, color) in parts {
            let width = galley.size().x;
            ui.painter()
                .galley(Pos2::new(x, badge.min.y + pad.y), galley, color);
            x += width + gap;
        }
    }

    /// The screenshot toolbar under the region, or above it at the bottom edge.
    fn shot_bar_rect(&self, frame: Rect) -> Option<Rect> {
        if self.record {
            return None;
        }
        let selection = self.selection?;
        Some(shot_bar_at(selection, frame, self.freeze.top_inset))
    }

    fn paint_shot_bar(&self, ui: &mut Ui, bar: Rect, surface: Rect, logical: Rect) {
        let bar = Self::to_surface(bar, surface, logical);
        let hover = ui
            .input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos()))
            .map(|p| Self::to_logical(p, surface, logical));
        ui.painter().rect_filled(bar, 12.0, INK);
        ui.painter().rect_stroke(
            bar,
            12.0,
            Stroke::new(1.0, Color32::from_white_alpha(40)),
            StrokeKind::Inside,
        );
        for (index, (action, rect_logical)) in shot_bar_buttons(bar_logical(bar, surface, logical))
            .into_iter()
            .enumerate()
        {
            let rect = Self::to_surface(rect_logical, surface, logical);
            let selected = matches!(action, ShotAction::Tool(tool) if tool == self.tool);
            let enabled = match action {
                ShotAction::Undo => !self.marks.items.is_empty(),
                ShotAction::Redo => !self.marks.undone.is_empty(),
                _ => true,
            };
            if selected {
                ui.painter().rect_filled(
                    rect,
                    7.0,
                    Color32::from_rgba_unmultiplied(0x2b, 0x7f, 0xff, 90),
                );
            } else if enabled && hover.is_some_and(|p| rect_logical.contains(p)) {
                ui.painter()
                    .rect_filled(rect, 7.0, Color32::from_white_alpha(18));
            }
            let color = if enabled {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(70)
            };
            paint_shot_icon(ui.painter(), action, rect, color);
            let id = ui.scope_id().with(("shot-action", index));
            ui.interact(rect, id, Sense::hover());
        }
    }

    fn mic_label(&self) -> String {
        if !self.options.mic {
            "不录制麦克风".to_string()
        } else if self.options.mic_device.is_empty() {
            "系统默认输入".to_string()
        } else {
            self.devices
                .iter()
                .find(|device| device.id == self.options.mic_device)
                .map(|device| device.name.clone())
                .unwrap_or_else(|| "系统默认输入".to_string())
        }
    }

    fn paint_record_bar(&self, ui: &mut Ui, bar: Rect, surface: Rect, logical: Rect) {
        let layout = record_bar_layout(bar);
        let bar = Self::to_surface(bar, surface, logical);
        let hover = ui
            .input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos()))
            .map(|p| Self::to_logical(p, surface, logical));
        ui.painter().rect_filled(bar, 14.0, INK);
        ui.painter().rect_stroke(
            bar,
            14.0,
            Stroke::new(1.0, Color32::from_white_alpha(50)),
            StrokeKind::Inside,
        );
        let text = |rect: Rect, value: &str, size: f32, color: Color32| {
            let rect = Self::to_surface(rect, surface, logical);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                value,
                FontId::proportional(size),
                color,
            );
        };
        text(layout.label, "选择格式", 12.0, MUTED);
        let format = Self::to_surface(layout.format, surface, logical);
        ui.painter().rect_filled(format, 6.0, ACCENT);
        ui.painter().text(
            format.center(),
            Align2::CENTER_CENTER,
            "MP4",
            FontId::proportional(12.0),
            Color32::WHITE,
        );
        for divider in [layout.divider1, layout.divider2, layout.divider3] {
            ui.painter().rect_filled(
                Self::to_surface(divider, surface, logical),
                0.0,
                Color32::from_white_alpha(50),
            );
        }
        for (rect, value) in [
            (
                layout.quality,
                format!("{} ▾", self.options.quality.label()),
            ),
            (
                layout.system,
                format!(
                    "{} ▾",
                    fit_label(
                        ui,
                        if self.options.system_audio {
                            "录制电脑声音"
                        } else {
                            "不录制电脑声音"
                        },
                        layout.system.width() - 20.0,
                    )
                ),
            ),
            (
                layout.mic,
                format!(
                    "{} ▾",
                    fit_label(ui, &self.mic_label(), layout.mic.width() - 20.0)
                ),
            ),
        ] {
            if hover.is_some_and(|p| rect.contains(p)) {
                ui.painter().rect_filled(
                    Self::to_surface(rect, surface, logical),
                    7.0,
                    Color32::from_white_alpha(16),
                );
            }
            text(rect, &value, 13.0, Color32::WHITE);
        }
        // 录屏没选区域时录整屏，所以开始按钮始终是蓝色可点的。
        let start = Self::to_surface(layout.start, surface, logical);
        ui.painter().rect_filled(start, 8.0, ACCENT);
        ui.painter().text(
            start.center(),
            Align2::CENTER_CENTER,
            "开始录制",
            FontId::proportional(13.0),
            Color32::WHITE,
        );
        let exit = Self::to_surface(layout.exit, surface, logical);
        if hover.is_some_and(|p| layout.exit.contains(p)) {
            ui.painter()
                .rect_filled(exit, 8.0, Color32::from_white_alpha(14));
        }
        ui.painter().rect_stroke(
            exit,
            8.0,
            Stroke::new(1.0, Color32::from_white_alpha(100)),
            StrokeKind::Inside,
        );
        ui.painter().text(
            exit.center(),
            Align2::CENTER_CENTER,
            "退出录制",
            FontId::proportional(13.0),
            Color32::WHITE,
        );
    }

    fn paint_menu(&self, ui: &mut Ui, kind: MenuKind, bar: Rect, surface: Rect, logical: Rect) {
        let rect = menu_rect(kind, bar, logical, self.devices.len());
        let rect = Self::to_surface(rect, surface, logical);
        ui.painter().rect_filled(rect, 10.0, INK);
        ui.painter().rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0, Color32::from_white_alpha(40)),
            StrokeKind::Inside,
        );
        let hover = ui
            .input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos()))
            .map(|p| Self::to_logical(p, surface, logical));
        let items = menu_items(kind, bar, logical, self.devices.len());
        for (index, item) in items.iter().enumerate() {
            let drawn = Self::to_surface(*item, surface, logical);
            if hover.is_some_and(|p| item.contains(p)) {
                ui.painter()
                    .rect_filled(drawn, 6.0, Color32::from_white_alpha(16));
            }
            let (label, hint, checked) = menu_item(kind, index, &self.options, &self.devices);
            let left = drawn.min.x + 10.0;
            if hint.is_empty() {
                ui.painter().text(
                    Pos2::new(left, drawn.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(13.0),
                    Color32::WHITE,
                );
            } else {
                ui.painter().text(
                    Pos2::new(left, drawn.min.y + 9.0),
                    Align2::LEFT_TOP,
                    label,
                    FontId::proportional(13.0),
                    Color32::WHITE,
                );
                ui.painter().text(
                    Pos2::new(left, drawn.max.y - 9.0),
                    Align2::LEFT_BOTTOM,
                    hint,
                    FontId::proportional(11.0),
                    MUTED,
                );
            }
            if checked {
                ui.painter().text(
                    Pos2::new(drawn.max.x - 10.0, drawn.center().y),
                    Align2::RIGHT_CENTER,
                    "✓",
                    FontId::proportional(13.0),
                    ACCENT,
                );
            }
        }
    }

    fn paint_text_editor(&mut self, ui: &mut Ui, logical: Rect, surface: Rect) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let at = draft.at;
        let pos = Self::to_surface(
            Rect::from_min_size(at, Vec2::new(200.0, 30.0)),
            surface,
            logical,
        )
        .min;
        let mut focus = self.draft_focus;
        egui::Area::new(egui::Id::new("shot-text-draft"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                ui.set_width(200.0);
                let frame = egui::Frame::NONE
                    .fill(Color32::from_black_alpha(217))
                    .stroke(Stroke::new(1.0, ACCENT))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(6, 5));
                frame.show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut draft.text)
                            .font(FontId::new(15.0, FontFamily::Proportional))
                            .text_color(MARK_RED)
                            .hint_text("输入文字")
                            .desired_width(188.0),
                    );
                    if focus {
                        response.request_focus();
                        focus = false;
                    }
                });
            });
        self.draft_focus = focus;
    }

    fn paint_cursor(
        &self,
        ui: &mut Ui,
        position: Option<Pos2>,
        record_bar: Option<Rect>,
        shot_bar: Option<Rect>,
    ) {
        let cursor = if self.menu.is_some() {
            CursorIcon::Default
        } else if let Some(position) = position {
            if record_bar.is_some_and(|rect| rect.contains(position))
                || shot_bar.is_some_and(|rect| rect.contains(position))
                || self
                    .draft
                    .as_ref()
                    .is_some_and(|draft| text_rect(draft.at).contains(position))
            {
                CursorIcon::Default
            } else if let Some(drag) = self.drag {
                match drag {
                    Drag::Handle(handle) => resize_cursor(handle),
                    Drag::Move { .. } => CursorIcon::Move,
                    Drag::Fresh { .. } | Drag::Paint { .. } => CursorIcon::Crosshair,
                }
            } else if let Some(selection) = self.selection {
                match handle_at(selection, position, HANDLE_SLOP) {
                    Some(handle) => resize_cursor(handle),
                    None if self.tool != ShotTool::Select && selection.contains(position) => {
                        CursorIcon::Crosshair
                    }
                    None if selection.contains(position) => CursorIcon::Move,
                    None => CursorIcon::Crosshair,
                }
            } else {
                CursorIcon::Crosshair
            }
        } else {
            CursorIcon::Crosshair
        };
        ui.output_mut(|o| o.cursor_icon = cursor);
    }
}

/// The rectangle between two pointer positions, whichever way the drag went.
fn between(anchor: Pos2, position: Pos2) -> Rect {
    Rect::from_two_pos(anchor, position)
}

/// A region the user drew is only real once it is big enough to be worth pixels.
fn is_region(rect: Rect, frame: Rect) -> bool {
    rect.intersect(frame).width() >= MIN_SIZE && rect.intersect(frame).height() >= MIN_SIZE
}

/// The window the front-most hit at `position`, or the whole screen when the desktop is showing.
fn hit_window(windows: &[Rect], position: Pos2, frame: Rect) -> Rect {
    windows
        .iter()
        .find(|rect| rect.contains(position))
        .copied()
        .unwrap_or(frame)
}

/// The readout split so a recording can lead with a red dot and tag; the size is always in
/// output pixels. The dot is painted, not a `●` glyph, so it never depends on font coverage.
fn readout(record: bool, rect: Rect, scale: f32) -> (Option<&'static str>, String) {
    let width = (rect.width() * scale).round().max(1.0) as i64;
    let height = (rect.height() * scale).round().max(1.0) as i64;
    (record.then_some("录屏"), format!("{width} * {height}"))
}

/// 工具条上的下拉标签按实际字宽截断，设备名再长也不会压到旁边的按钮。
fn fit_label(ui: &Ui, text: &str, width: f32) -> String {
    let font = FontId::proportional(13.0);
    let measure = |value: &str| {
        ui.ctx().fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(value.to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
    };
    if measure(text) <= width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate = format!("{}…", chars.iter().collect::<String>());
        if measure(&candidate) <= width {
            return candidate;
        }
    }
    "…".to_string()
}

fn text_rect(at: Pos2) -> Rect {
    Rect::from_min_size(at, Vec2::new(200.0, 30.0))
}

/// The first draft of a mark, before the pointer has moved.
fn mark_of(tool: ShotTool, from: Pos2, to: Pos2) -> Option<Mark> {
    match tool {
        ShotTool::Box => Some(Mark::Box {
            rect: between(from, to),
        }),
        ShotTool::Mosaic => Some(Mark::Mosaic {
            rect: between(from, to),
        }),
        ShotTool::Arrow => Some(Mark::Arrow { from, to }),
        ShotTool::Pen => Some(Mark::Pen {
            points: vec![from, to],
        }),
        ShotTool::Select | ShotTool::Text => None,
    }
}

/// The screenshot toolbar: centred under the region, flipped above at the bottom edge.
fn shot_bar_at(selection: Rect, frame: Rect, top_inset: f32) -> Rect {
    let x = (selection.center().x - SHOT_BAR_WIDTH / 2.0)
        .clamp(0.0, (frame.max.x - SHOT_BAR_WIDTH).max(0.0));
    let min_y = (top_inset + TOOLBAR_GAP).max(0.0);
    let below = selection.max.y + TOOLBAR_GAP + SHOT_BAR_HEIGHT;
    let y = if below > frame.max.y {
        (selection.min.y - TOOLBAR_GAP - SHOT_BAR_HEIGHT).max(min_y)
    } else {
        selection.max.y + TOOLBAR_GAP
    };
    // 全屏框选时上面也放不下，至少别钻到菜单栏里；下沿同样别越界。
    let y = y.min((frame.max.y - SHOT_BAR_HEIGHT).max(min_y));
    Rect::from_min_size(Pos2::new(x, y), Vec2::new(SHOT_BAR_WIDTH, SHOT_BAR_HEIGHT))
}

/// Toolbar button rects, in the same order they are painted.
fn shot_bar_buttons(bar: Rect) -> Vec<(ShotAction, Rect)> {
    let top = bar.min.y + (SHOT_BAR_HEIGHT - SHOT_TOOL) / 2.0;
    let small_top = bar.min.y + (SHOT_BAR_HEIGHT - SHOT_SMALL) / 2.0;
    let mut x = bar.min.x + SHOT_BAR_PAD;
    let mut take = |width: f32, top: f32| {
        let rect = Rect::from_min_size(Pos2::new(x, top), Vec2::new(width, SHOT_TOOL));
        x += width + SHOT_GAP;
        rect
    };
    let mut buttons = Vec::new();
    for tool in [
        ShotTool::Select,
        ShotTool::Box,
        ShotTool::Arrow,
        ShotTool::Pen,
        ShotTool::Text,
        ShotTool::Mosaic,
    ] {
        let rect = take(SHOT_TOOL, top);
        buttons.push((ShotAction::Tool(tool), rect));
    }
    x += SHOT_DIVIDER + SHOT_GAP;
    for action in [ShotAction::Undo, ShotAction::Redo] {
        let rect = Rect::from_min_size(Pos2::new(x, small_top), Vec2::new(SHOT_SMALL, SHOT_SMALL));
        x += SHOT_SMALL + SHOT_GAP;
        buttons.push((action, rect));
    }
    x += SHOT_DIVIDER + SHOT_GAP;
    let cancel = Rect::from_min_size(Pos2::new(x, small_top), Vec2::new(SHOT_SMALL, SHOT_SMALL));
    x += SHOT_SMALL + SHOT_GAP;
    buttons.push((ShotAction::Cancel, cancel));
    x += SHOT_SPACER + SHOT_GAP;
    let confirm = Rect::from_min_size(
        Pos2::new(x, bar.min.y + (SHOT_BAR_HEIGHT - SHOT_CONFIRM) / 2.0),
        Vec2::new(SHOT_CONFIRM, SHOT_CONFIRM),
    );
    buttons.push((ShotAction::Confirm, confirm));
    buttons
}

/// The toolbar is laid out in logical points but painted in surface points; recover the
/// logical rect so hit-testing and painting agree.
fn bar_logical(drawn: Rect, surface: Rect, logical: Rect) -> Rect {
    let map = |p: Pos2| {
        let nx = (p.x - surface.min.x) / surface.width().max(1.0);
        let ny = (p.y - surface.min.y) / surface.height().max(1.0);
        Pos2::new(
            logical.min.x + nx * logical.width(),
            logical.min.y + ny * logical.height(),
        )
    };
    Rect::from_min_max(map(drawn.min), map(drawn.max))
}

/// Points at the top of the main screen that the menu bar occupies. Clicks there never
/// reach the overlay, so the toolbar uses this as its minimum y.
#[cfg(target_os = "macos")]
pub fn top_inset() -> f32 {
    use objc2_app_kit::NSScreen;

    let Some(mtm) = MainThreadMarker::new() else {
        return 28.0;
    };
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        return 28.0;
    };
    let frame = screen.frame();
    let visible = screen.visibleFrame();
    let top = (frame.origin.y + frame.size.height) - (visible.origin.y + visible.size.height);
    (top as f32).max(0.0)
}

#[cfg(not(target_os = "macos"))]
pub fn top_inset() -> f32 {
    0.0
}

/// 录屏时主窗口不能真的藏起来：录制操作栏是它的 deferred 子窗口，父窗口一藏，
/// 操作栏也会跟着消失。这里把主窗口整窗调成透明并让鼠标穿透，用户看不见也点不到，
/// 但子窗口仍然能显示。
#[cfg(target_os = "macos")]
pub fn set_main_window_hidden(hidden: bool) {
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    for window in app.windows().iter() {
        if window.title().to_string() == "ClipSyncRust" {
            window.setAlphaValue(if hidden { 0.0 } else { 1.0 });
            window.setIgnoresMouseEvents(hidden);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_main_window_hidden(_hidden: bool) {}

/// 录屏工具条固定在屏幕顶部居中，菜单栏下面，避免点击被系统吃掉。
fn record_bar_at(frame: Rect, top_inset: f32) -> Rect {
    let x = ((frame.width() - RECORD_BAR_WIDTH) / 2.0).max(0.0);
    let y = (top_inset + 8.0).max(24.0);
    let y = y.min((frame.max.y - RECORD_BAR_HEIGHT).max(0.0));
    Rect::from_min_size(
        Pos2::new(frame.min.x + x, frame.min.y + y),
        Vec2::new(RECORD_BAR_WIDTH, RECORD_BAR_HEIGHT),
    )
}

#[derive(Clone, Copy, Debug)]
struct RecordBarLayout {
    label: Rect,
    format: Rect,
    quality: Rect,
    system: Rect,
    mic: Rect,
    start: Rect,
    exit: Rect,
    divider1: Rect,
    divider2: Rect,
    divider3: Rect,
}

fn record_bar_layout(bar: Rect) -> RecordBarLayout {
    let mid = bar.center().y;
    let button_top = mid - RECORD_BUTTON_HEIGHT / 2.0;
    let mut x = bar.min.x + RECORD_BAR_PAD;
    let label = Rect::from_min_size(Pos2::new(x, mid - 9.0), Vec2::new(48.0, 18.0));
    x += 48.0 + RECORD_BAR_GAP;
    let format = Rect::from_min_size(Pos2::new(x, mid - 12.0), Vec2::new(46.0, 24.0));
    x += 46.0;
    let divider1 = Rect::from_min_size(Pos2::new(x + 6.0, mid - 11.0), Vec2::new(1.0, 22.0));
    x += 13.0;
    let quality = Rect::from_min_size(
        Pos2::new(x, button_top),
        Vec2::new(76.0, RECORD_BUTTON_HEIGHT),
    );
    x += 76.0;
    let divider2 = Rect::from_min_size(Pos2::new(x + 6.0, mid - 11.0), Vec2::new(1.0, 22.0));
    x += 13.0;
    let system = Rect::from_min_size(
        Pos2::new(x, button_top),
        Vec2::new(112.0, RECORD_BUTTON_HEIGHT),
    );
    x += 112.0;
    let divider3 = Rect::from_min_size(Pos2::new(x + 6.0, mid - 11.0), Vec2::new(1.0, 22.0));
    x += 13.0;
    let mic = Rect::from_min_size(
        Pos2::new(x, button_top),
        Vec2::new(120.0, RECORD_BUTTON_HEIGHT),
    );
    x += 120.0 + RECORD_BAR_GAP;
    let start = Rect::from_min_size(
        Pos2::new(x, button_top),
        Vec2::new(84.0, RECORD_BUTTON_HEIGHT),
    );
    x += 84.0 + RECORD_BAR_GAP;
    let exit = Rect::from_min_size(
        Pos2::new(x, button_top),
        Vec2::new(84.0, RECORD_BUTTON_HEIGHT),
    );
    RecordBarLayout {
        label,
        format,
        quality,
        system,
        mic,
        start,
        exit,
        divider1,
        divider2,
        divider3,
    }
}

fn menu_count(kind: MenuKind, devices: usize) -> usize {
    match kind {
        MenuKind::Quality => 3,
        MenuKind::System => 2,
        // 关闭麦克风、系统默认输入，然后最多六个设备。
        MenuKind::Mic => 2 + devices.min(6),
    }
}

fn menu_rect(kind: MenuKind, bar: Rect, frame: Rect, devices: usize) -> Rect {
    let layout = record_bar_layout(bar);
    let anchor = match kind {
        MenuKind::Quality => layout.quality,
        MenuKind::System => layout.system,
        MenuKind::Mic => layout.mic,
    };
    let width = match kind {
        MenuKind::Quality => 200.0,
        MenuKind::System => 190.0,
        MenuKind::Mic => 220.0,
    };
    let item = match kind {
        MenuKind::Quality => MENU_QUALITY_ITEM,
        _ => MENU_ITEM_HEIGHT,
    };
    let height = menu_count(kind, devices) as f32 * item + MENU_PAD * 2.0;
    let x = anchor.min.x.clamp(0.0, (frame.max.x - width).max(0.0));
    let below = bar.max.y + RECORD_MENU_GAP;
    let y = if below + height > frame.max.y {
        (bar.min.y - RECORD_MENU_GAP - height).max(0.0)
    } else {
        below
    };
    Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height))
}

fn menu_items(kind: MenuKind, bar: Rect, frame: Rect, devices: usize) -> Vec<Rect> {
    let rect = menu_rect(kind, bar, frame, devices);
    let item = match kind {
        MenuKind::Quality => MENU_QUALITY_ITEM,
        _ => MENU_ITEM_HEIGHT,
    };
    (0..menu_count(kind, devices))
        .map(|index| {
            Rect::from_min_size(
                Pos2::new(
                    rect.min.x + MENU_PAD,
                    rect.min.y + MENU_PAD + index as f32 * item,
                ),
                Vec2::new(rect.width() - MENU_PAD * 2.0, item),
            )
        })
        .collect()
}

/// 命中菜单返回第几项；点菜单外返回 None，由调用方关掉菜单。
fn menu_choice(
    kind: MenuKind,
    bar: Rect,
    frame: Rect,
    devices: usize,
    position: Pos2,
) -> Option<usize> {
    menu_items(kind, bar, frame, devices)
        .iter()
        .position(|item| item.contains(position))
}

fn menu_item<'a>(
    kind: MenuKind,
    index: usize,
    options: &'a RecordOptions,
    devices: &'a [MicInput],
) -> (&'a str, &'a str, bool) {
    match kind {
        MenuKind::Quality => {
            let quality = RecordQuality::ALL[index];
            (quality.label(), quality.hint(), options.quality == quality)
        }
        MenuKind::System => {
            if index == 0 {
                ("不录制电脑声音", "", !options.system_audio)
            } else {
                ("录制电脑声音", "", options.system_audio)
            }
        }
        MenuKind::Mic => {
            if index == 0 {
                ("关闭麦克风", "", !options.mic)
            } else if index == 1 {
                (
                    "系统默认输入",
                    "",
                    options.mic && options.mic_device.is_empty(),
                )
            } else {
                let device = &devices[index - 2];
                (
                    device.name.as_str(),
                    "",
                    options.mic && options.mic_device == device.id,
                )
            }
        }
    }
}

/// The grabbable handle under the pointer, if any; corners win because they are checked first.
fn handle_point(rect: Rect, handle: Handle) -> Pos2 {
    let x = match handle {
        Handle::TopLeft | Handle::BottomLeft | Handle::Left => rect.min.x,
        Handle::TopRight | Handle::BottomRight | Handle::Right => rect.max.x,
        Handle::Top | Handle::Bottom => (rect.min.x + rect.max.x) / 2.0,
    };
    let y = match handle {
        Handle::TopLeft | Handle::Top | Handle::TopRight => rect.min.y,
        Handle::BottomLeft | Handle::Bottom | Handle::BottomRight => rect.max.y,
        Handle::Left | Handle::Right => (rect.min.y + rect.max.y) / 2.0,
    };
    Pos2::new(x, y)
}

fn square_at(center: Pos2, size: f32) -> Rect {
    Rect::from_center_size(center, Vec2::splat(size))
}

/// The grabbable handle under the pointer, if any; corners win because they are checked first.
fn handle_at(rect: Rect, position: Pos2, slop: f32) -> Option<Handle> {
    HANDLES
        .into_iter()
        .find(|handle| square_at(handle_point(rect, *handle), slop * 2.0).contains(position))
}

/// Sliding the whole region to put its top-left at `target`, kept inside the screen.
fn translate(rect: Rect, target: Pos2, bounds: Rect) -> Rect {
    let size = rect.size();
    let min = Pos2::new(
        target
            .x
            .clamp(bounds.min.x, (bounds.max.x - size.x).max(bounds.min.x)),
        target
            .y
            .clamp(bounds.min.y, (bounds.max.y - size.y).max(bounds.min.y)),
    );
    Rect::from_min_size(min, size)
}

/// Which edge of an axis follows the pointer; the opposite edge is the anchor.
#[derive(Clone, Copy, PartialEq)]
enum Mover {
    Min,
    Max,
    Pinned,
}

/// Pulling one edge or corner to the pointer, never off the screen.
fn resize(rect: Rect, handle: Handle, position: Pos2, bounds: Rect) -> Rect {
    let position = position.clamp(bounds.min, bounds.max);
    let horizontal = match handle {
        Handle::TopLeft | Handle::Left | Handle::BottomLeft => Mover::Min,
        Handle::TopRight | Handle::Right | Handle::BottomRight => Mover::Max,
        Handle::Top | Handle::Bottom => Mover::Pinned,
    };
    let vertical = match handle {
        Handle::TopLeft | Handle::Top | Handle::TopRight => Mover::Min,
        Handle::BottomLeft | Handle::Bottom | Handle::BottomRight => Mover::Max,
        Handle::Left | Handle::Right => Mover::Pinned,
    };
    let (min_x, max_x) = axis(
        rect.min.x,
        rect.max.x,
        horizontal,
        position.x,
        bounds.min.x,
        bounds.max.x,
    );
    let (min_y, max_y) = axis(
        rect.min.y,
        rect.max.y,
        vertical,
        position.y,
        bounds.min.y,
        bounds.max.y,
    );
    Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
}

/// One axis of a resize: the moving edge chases the pointer and the anchor holds, so a handle
/// dragged past its opposite collapses to the smallest region instead of flipping it inside out.
fn axis(
    min: f32,
    max: f32,
    mover: Mover,
    position: f32,
    bound_min: f32,
    bound_max: f32,
) -> (f32, f32) {
    let mut min = min;
    let mut max = max;
    if mover == Mover::Min {
        min = position;
    } else if mover == Mover::Max {
        max = position;
    }
    if max - min < MIN_SIZE {
        match mover {
            Mover::Min => min = (max - MIN_SIZE).max(bound_min),
            Mover::Max => max = (min + MIN_SIZE).min(bound_max),
            Mover::Pinned => {}
        }
    }
    // An anchor already flush against the far edge leaves no room, so it gives a little.
    if max - min < MIN_SIZE {
        match mover {
            Mover::Min => max = (min + MIN_SIZE).min(bound_max),
            Mover::Max => min = (max - MIN_SIZE).max(bound_min),
            Mover::Pinned => {}
        }
    }
    (min, max)
}

/// The four dim bands around a hole; the hole itself stays at full brightness.
fn mask_rects(outer: Rect, hole: Option<Rect>) -> [Rect; 4] {
    let Some(hole) = hole.filter(|hole| !hole.is_negative()) else {
        return [outer, Rect::NOTHING, Rect::NOTHING, Rect::NOTHING];
    };
    let hole = hole.intersect(outer);
    [
        Rect::from_min_max(outer.min, Pos2::new(hole.min.x, outer.max.y)),
        Rect::from_min_max(Pos2::new(hole.max.x, outer.min.y), outer.max),
        Rect::from_min_max(
            Pos2::new(hole.min.x, outer.min.y),
            Pos2::new(hole.max.x, hole.min.y),
        ),
        Rect::from_min_max(
            Pos2::new(hole.min.x, hole.max.y),
            Pos2::new(hole.max.x, outer.max.y),
        ),
    ]
}

fn resize_cursor(handle: Handle) -> CursorIcon {
    match handle {
        Handle::TopLeft | Handle::BottomRight => CursorIcon::ResizeNorthWest,
        Handle::TopRight | Handle::BottomLeft => CursorIcon::ResizeNorthEast,
        Handle::Top | Handle::Bottom => CursorIcon::ResizeVertical,
        Handle::Left | Handle::Right => CursorIcon::ResizeHorizontal,
    }
}

/// 工具栏图标：18×18 点的小线条，和 Compose `draw*Icon` 一一对应。
fn paint_shot_icon(painter: &egui::Painter, action: ShotAction, rect: Rect, color: Color32) {
    let center = rect.center();
    let scale = 18.0 / 18.0;
    let p = |x: f32, y: f32| Pos2::new(center.x - 9.0 * scale + x, center.y - 9.0 * scale + y);
    let stroke = Stroke::new(1.6, color);
    match action {
        ShotAction::Tool(ShotTool::Select) => {
            let points = vec![
                p(4.0, 3.0),
                p(4.0, 14.0),
                p(7.5, 11.0),
                p(10.0, 16.0),
                p(12.0, 15.0),
                p(9.5, 10.0),
                p(14.0, 9.5),
            ];
            painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
        }
        ShotAction::Tool(ShotTool::Box) => {
            painter.rect_stroke(
                Rect::from_min_max(p(2.0, 3.0), p(16.0, 15.0)),
                0.0,
                stroke,
                StrokeKind::Middle,
            );
        }
        ShotAction::Tool(ShotTool::Arrow) => {
            painter.line_segment([p(3.0, 15.0), p(15.0, 3.0)], stroke);
            painter.line_segment([p(15.0, 3.0), p(9.0, 4.0)], stroke);
            painter.line_segment([p(15.0, 3.0), p(14.0, 9.0)], stroke);
        }
        ShotAction::Tool(ShotTool::Pen) => {
            painter.line_segment([p(3.0, 15.0), p(13.0, 5.0)], stroke);
            painter.line_segment([p(13.0, 5.0), p(15.0, 3.0)], stroke);
            painter.line_segment([p(3.0, 15.0), p(6.0, 14.0)], stroke);
        }
        ShotAction::Tool(ShotTool::Text) => {
            painter.line_segment([p(3.0, 4.0), p(15.0, 4.0)], stroke);
            painter.line_segment([p(9.0, 4.0), p(9.0, 15.0)], stroke);
            painter.line_segment([p(6.0, 15.0), p(12.0, 15.0)], stroke);
        }
        ShotAction::Tool(ShotTool::Mosaic) => {
            for row in 0..3 {
                for col in 0..3 {
                    let color = if (row + col) % 2 == 0 {
                        color
                    } else {
                        Color32::from_white_alpha(102)
                    };
                    painter.rect_filled(
                        Rect::from_min_size(
                            p(3.0 + col as f32 * 4.4, 3.0 + row as f32 * 4.4),
                            Vec2::splat(3.4),
                        ),
                        0.0,
                        color,
                    );
                }
            }
        }
        ShotAction::Undo => paint_undo(painter, rect, color, false),
        ShotAction::Redo => paint_undo(painter, rect, color, true),
        ShotAction::Cancel => {
            painter.line_segment([p(4.0, 4.0), p(14.0, 14.0)], stroke);
            painter.line_segment([p(14.0, 4.0), p(4.0, 14.0)], stroke);
        }
        ShotAction::Confirm => {
            painter.line_segment([p(3.5, 9.0), p(7.5, 13.0)], Stroke::new(2.0, color));
            painter.line_segment([p(7.5, 13.0), p(14.5, 5.0)], Stroke::new(2.0, color));
        }
    }
}

fn paint_undo(painter: &egui::Painter, rect: Rect, color: Color32, redo: bool) {
    let center = Pos2::new(rect.center().x, rect.center().y + 1.0);
    let radius = 6.0;
    let start = if redo {
        210.0_f32.to_radians()
    } else {
        150.0_f32.to_radians()
    };
    let sweep = if redo {
        240.0_f32.to_radians()
    } else {
        -240.0_f32.to_radians()
    };
    let mut points = Vec::new();
    for step in 0..=16 {
        let angle = start + sweep * step as f32 / 16.0;
        points.push(Pos2::new(
            center.x + radius * angle.cos(),
            center.y - radius * angle.sin(),
        ));
    }
    painter.add(Shape::line(points.clone(), Stroke::new(1.6, color)));
    let tip = points.last().copied().unwrap_or(center);
    let angle = start + sweep;
    let tangent = angle + std::f32::consts::FRAC_PI_2 * sweep.signum();
    painter.line_segment(
        [
            tip,
            Pos2::new(
                tip.x + 3.5 * (tangent + 0.6).cos(),
                tip.y - 3.5 * (tangent + 0.6).sin(),
            ),
        ],
        Stroke::new(1.6, color),
    );
    painter.line_segment(
        [
            tip,
            Pos2::new(
                tip.x + 3.5 * (tangent - 0.6).cos(),
                tip.y - 3.5 * (tangent - 0.6).sin(),
            ),
        ],
        Stroke::new(1.6, color),
    );
}

/// Put the overlay window on screen, or take it off again. It is never closed: closing tears down
/// the egui side but leaves the native window behind, so one window per capture would pile up.
pub fn reveal(ctx: &egui::Context, visible: bool) {
    ctx.send_viewport_cmd_to(viewport_id(), ViewportCommand::Visible(visible));
    // egui 只藏 viewport；若原生层叠了同名窗，点叉仍会打在下面那层上。
    #[cfg(target_os = "macos")]
    if !visible {
        hide_overlay_windows();
    }
}

/// Stretch the overlay window over every pixel of the screen, menu bar and dock included, and
/// report whether it is in place. Done while the window is still hidden so nobody watches it move.
pub fn cover(ctx: &egui::Context, position: Pos2, size: Vec2) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = (ctx, position, size);
        pin_screen()
    }
    #[cfg(not(target_os = "macos"))]
    {
        ctx.send_viewport_cmd_to(viewport_id(), ViewportCommand::OuterPosition(position));
        ctx.send_viewport_cmd_to(viewport_id(), ViewportCommand::InnerSize(size));
        true
    }
}

/// AppKit keeps even a floating window below the menu bar (level 24) and the dock (level 20), so
/// only a screen-saver level window is allowed to dim the whole screen.
#[cfg(target_os = "macos")]
fn pin_screen() -> bool {
    use objc2_app_kit::{NSScreen, NSScreenSaverWindowLevel};

    let Some(window) = overlay_window() else {
        return false;
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        return false;
    };
    window.setLevel(NSScreenSaverWindowLevel);
    window.setFrame_display(screen.frame(), false);
    true
}

/// The overlay window, found by title because egui hands out no native handle for it.
#[cfg(target_os = "macos")]
fn overlay_window() -> Option<Retained<NSWindow>> {
    let mtm = MainThreadMarker::new()?;
    NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .find(|window| window.title().to_string() == TITLE)
}

#[cfg(target_os = "macos")]
fn hide_overlay_windows() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    for window in NSApplication::sharedApplication(mtm).windows().iter() {
        if window.title().to_string() == TITLE {
            window.orderOut(None);
        }
    }
}

/// Clicks already land on the overlay, but only the key window is told about Esc.
#[cfg(target_os = "macos")]
fn focus() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(window) = overlay_window() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.activate();
    window.makeKeyAndOrderFront(None);
}

#[cfg(not(target_os = "macos"))]
fn focus() {}

/// The pixels behind a selection, in the frozen frame's own coordinates.
pub fn crop_px(rect: Rect, image: Vec2, scale: f32) -> (u32, u32, u32, u32) {
    let corner = Pos2::new(image.x, image.y);
    let min = (rect.min * scale).clamp(Pos2::ZERO, corner).round();
    let max = (rect.max * scale).clamp(Pos2::ZERO, corner).round();
    (
        min.x as u32,
        min.y as u32,
        (max.x - min.x).max(1.0) as u32,
        (max.y - min.y).max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        between, crop_px, handle_at, handle_point, hit_window, is_region, mask_rects, menu_choice,
        menu_items, menu_rect, readout, record_bar_at, record_bar_layout, resize, shot_bar_at,
        shot_bar_buttons, square_at, translate, Drag, Freeze, Handle, Mark, MenuKind, Outcome,
        RecordOptions, RecordQuality, Session, ShotAction, ShotTool, MIN_SIZE, TOOLBAR_GAP,
    };
    use eframe::egui::{Pos2, Rect, Vec2};

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
    }

    const SCREEN: Rect = Rect {
        min: Pos2 { x: 0.0, y: 0.0 },
        max: Pos2 {
            x: 1000.0,
            y: 600.0,
        },
    };

    #[test]
    fn a_drag_in_any_direction_becomes_a_positive_region() {
        assert_eq!(
            between(Pos2::new(300.0, 300.0), Pos2::new(100.0, 100.0)),
            rect(100.0, 100.0, 300.0, 300.0)
        );
        assert_eq!(
            between(Pos2::new(100.0, 100.0), Pos2::new(300.0, 300.0)),
            rect(100.0, 100.0, 300.0, 300.0)
        );
    }

    #[test]
    fn a_click_selects_the_front_window_and_the_desktop_falls_back_to_the_screen() {
        let windows = [
            rect(400.0, 200.0, 900.0, 500.0),
            rect(0.0, 0.0, 500.0, 300.0),
        ];
        // Front to back: the window listed first wins where they overlap.
        assert_eq!(
            hit_window(&windows, Pos2::new(450.0, 250.0), SCREEN),
            rect(400.0, 200.0, 900.0, 500.0)
        );
        assert_eq!(
            hit_window(&windows, Pos2::new(60.0, 60.0), SCREEN),
            rect(0.0, 0.0, 500.0, 300.0)
        );
        assert_eq!(
            hit_window(&windows, Pos2::new(950.0, 550.0), SCREEN),
            SCREEN
        );
    }

    #[test]
    fn a_jitter_click_is_not_a_region() {
        assert!(!is_region(rect(100.0, 100.0, 103.0, 300.0), SCREEN));
        assert!(is_region(rect(100.0, 100.0, 200.0, 300.0), SCREEN));
    }

    #[test]
    fn only_the_eight_handles_are_grabbable_and_corners_come_first() {
        let selection = rect(100.0, 100.0, 300.0, 200.0);
        assert_eq!(
            handle_at(selection, Pos2::new(100.0, 100.0), 9.0),
            Some(Handle::TopLeft)
        );
        assert_eq!(
            handle_at(selection, Pos2::new(200.0, 100.0), 9.0),
            Some(Handle::Top)
        );
        assert_eq!(
            handle_at(selection, Pos2::new(300.0, 200.0), 9.0),
            Some(Handle::BottomRight)
        );
        assert_eq!(handle_at(selection, Pos2::new(150.0, 150.0), 9.0), None);
        assert_eq!(
            handle_point(selection, Handle::Right),
            Pos2::new(300.0, 150.0)
        );
    }

    #[test]
    fn dragging_a_corner_moves_only_that_corner() {
        let selection = rect(100.0, 100.0, 300.0, 200.0);
        assert_eq!(
            resize(
                selection,
                Handle::BottomRight,
                Pos2::new(400.0, 260.0),
                SCREEN
            ),
            rect(100.0, 100.0, 400.0, 260.0)
        );
        assert_eq!(
            resize(selection, Handle::Top, Pos2::new(200.0, 60.0), SCREEN),
            rect(100.0, 60.0, 300.0, 200.0)
        );
    }

    #[test]
    fn a_handle_stays_on_the_screen_and_never_inverts() {
        let selection = rect(100.0, 100.0, 300.0, 200.0);
        assert_eq!(
            resize(
                selection,
                Handle::BottomRight,
                Pos2::new(5000.0, 5000.0),
                SCREEN
            ),
            rect(100.0, 100.0, 1000.0, 600.0)
        );
        // Pulled past the opposite corner: the anchor holds and the region collapses to the
        // minimum instead of flipping inside out.
        assert_eq!(
            resize(
                selection,
                Handle::BottomRight,
                Pos2::new(20.0, 20.0),
                SCREEN
            ),
            rect(100.0, 100.0, 100.0 + MIN_SIZE, 100.0 + MIN_SIZE)
        );
        assert_eq!(
            resize(selection, Handle::TopLeft, Pos2::new(500.0, 500.0), SCREEN),
            rect(300.0 - MIN_SIZE, 200.0 - MIN_SIZE, 300.0, 200.0)
        );
        let flipped = resize(selection, Handle::Left, Pos2::new(400.0, 150.0), SCREEN);
        assert!(flipped.width() >= MIN_SIZE && flipped.height() == 100.0);
        assert_eq!(flipped, rect(294.0, 100.0, 300.0, 200.0));
        assert!(SCREEN.contains(flipped.min) && SCREEN.contains(flipped.max));
    }

    #[test]
    fn moving_the_interior_slides_the_region_and_stops_at_the_edges() {
        let selection = rect(100.0, 100.0, 300.0, 200.0);
        assert_eq!(
            translate(selection, Pos2::new(400.0, 300.0), SCREEN),
            rect(400.0, 300.0, 600.0, 400.0)
        );
        assert_eq!(
            translate(selection, Pos2::new(-500.0, -500.0), SCREEN),
            rect(0.0, 0.0, 200.0, 100.0)
        );
        assert_eq!(
            translate(selection, Pos2::new(2000.0, 2000.0), SCREEN),
            rect(800.0, 500.0, 1000.0, 600.0)
        );
    }

    #[test]
    fn the_mask_dims_everything_but_the_hole_exactly_once() {
        let hole = rect(100.0, 100.0, 300.0, 200.0);
        let bands = mask_rects(SCREEN, Some(hole));
        let covered: Vec<Rect> = bands
            .iter()
            .filter(|band| !band.is_negative())
            .copied()
            .collect();
        assert_eq!(covered.len(), 4);
        let area: f32 = covered.iter().map(|band| band.area()).sum();
        assert_eq!(area, SCREEN.area() - hole.area());
        for band in &covered {
            assert_eq!(
                band.intersect(hole).area(),
                0.0,
                "{band:?} eats into the hole"
            );
        }
        assert!(covered.iter().all(|band| band.intersect(SCREEN) == *band));
        // No hole means the whole screen is dimmed, and nothing is painted twice.
        let all = mask_rects(SCREEN, None);
        assert_eq!(all[0], SCREEN);
        assert!(all[1..].iter().all(|band| band.is_negative()));
    }

    #[test]
    fn a_selection_becomes_whole_pixels_at_the_frame_scale() {
        // A 2× frame: points double into pixels.
        assert_eq!(
            crop_px(
                rect(100.5, 50.0, 300.5, 250.0),
                Vec2::new(2000.0, 1200.0),
                2.0
            ),
            (201, 100, 400, 400)
        );
        assert_eq!(
            crop_px(SCREEN, Vec2::new(1000.0, 600.0), 1.0),
            (0, 0, 1000, 600)
        );
        // Off-screen selections are clipped rather than wrapped.
        assert_eq!(
            crop_px(
                rect(990.0, 590.0, 1400.0, 900.0),
                Vec2::new(1000.0, 600.0),
                1.0
            ),
            (990, 590, 10, 10)
        );
        // A one-point sliver still yields a pixel, so the crop can never come back empty.
        assert_eq!(
            crop_px(rect(10.0, 10.0, 10.4, 10.4), Vec2::new(1000.0, 600.0), 1.0),
            (10, 10, 1, 1)
        );
    }

    #[test]
    fn handle_squares_are_centred_on_the_edge() {
        let selection = rect(100.0, 100.0, 300.0, 200.0);
        assert_eq!(
            square_at(handle_point(selection, Handle::Left), 8.0),
            rect(96.0, 146.0, 104.0, 154.0)
        );
    }

    #[test]
    fn the_shot_bar_sits_under_the_region_and_flips_above_it_at_the_bottom_edge() {
        let bar = shot_bar_at(rect(100.0, 100.0, 300.0, 200.0), SCREEN, 0.0);
        assert_eq!(bar.center().x, 200.0);
        assert_eq!(bar.min.y, 200.0 + TOOLBAR_GAP);

        let flipped = shot_bar_at(rect(100.0, 100.0, 300.0, 590.0), SCREEN, 0.0);
        assert_eq!(flipped.max.y, 100.0 - TOOLBAR_GAP);

        let edge = shot_bar_at(rect(900.0, 100.0, 1000.0, 200.0), SCREEN, 0.0);
        assert_eq!(edge.max.x, SCREEN.max.x);
    }

    #[test]
    fn a_full_screen_shot_bar_stays_below_the_menu_bar() {
        let bar = shot_bar_at(SCREEN, SCREEN, 28.0);
        assert!(bar.min.y >= 28.0 + TOOLBAR_GAP);
        assert!(bar.max.y <= SCREEN.max.y);
        assert!(bar.min.x >= SCREEN.min.x);
        assert!(bar.max.x <= SCREEN.max.x);
    }

    #[test]
    fn the_shot_bar_has_six_tools_and_the_expected_actions() {
        let bar = shot_bar_at(rect(100.0, 100.0, 300.0, 200.0), SCREEN, 0.0);
        let buttons = shot_bar_buttons(bar);
        assert_eq!(buttons.len(), 10);
        assert_eq!(buttons[0].0, ShotAction::Tool(ShotTool::Select));
        assert_eq!(buttons[5].0, ShotAction::Tool(ShotTool::Mosaic));
        assert_eq!(buttons[6].0, ShotAction::Undo);
        assert_eq!(buttons[7].0, ShotAction::Redo);
        assert_eq!(buttons[8].0, ShotAction::Cancel);
        assert_eq!(buttons[9].0, ShotAction::Confirm);
        assert!(buttons.iter().all(|(_, rect)| bar.contains(rect.min)));
        assert!(buttons.iter().all(|(_, rect)| bar.contains(rect.max)));
    }

    #[test]
    fn a_click_on_the_toolbar_leaves_the_region_alone() {
        let selection = rect(100.0, 100.0, 300.0, 200.0);
        let bar = shot_bar_at(selection, SCREEN, 0.0);
        let mut session = session();
        session.selection = Some(selection);

        // 点叉：直接取消，不擦选区、不开 Fresh。
        let cancel = shot_bar_buttons(bar)[8].1;
        session.shot_bar_click(cancel.center(), bar);
        assert!(session.drag.is_none());
        assert_eq!(session.selection, Some(selection));
        assert!(matches!(session.outcome, Some(Outcome::Cancel)));

        session.outcome = None;
        // 点工具：只换工具，不动选区。
        let box_tool = shot_bar_buttons(bar)[1].1;
        session.shot_bar_click(box_tool.center(), bar);
        assert_eq!(session.tool, ShotTool::Box);
        assert!(session.drag.is_none());
        assert_eq!(session.selection, Some(selection));

        // 点在条上但不是按钮：不动选区。
        session.begin_drag(Pos2::new(bar.center().x, bar.min.y + 1.0), Some(bar));
        assert!(session.drag.is_none());
        assert_eq!(session.selection, Some(selection));

        // 底边手柄（避开按钮）仍可拖。
        session.begin_drag(Pos2::new(200.0, 200.0), Some(bar));
        assert!(matches!(session.drag, Some(Drag::Handle(Handle::Bottom))));

        session.drag = None;
        session.selection = Some(selection);
        // Anywhere else starts a fresh region.
        session.begin_drag(Pos2::new(600.0, 400.0), Some(bar));
        assert!(matches!(session.drag, Some(Drag::Fresh { .. })));
        assert_eq!(session.selection, None);
    }

    #[test]
    fn pressing_cancel_cancels_without_a_crop() {
        let selection = rect(100.0, 100.0, 300.0, 200.0);
        let bar = shot_bar_at(selection, SCREEN, 0.0);
        let cancel = shot_bar_buttons(bar)[8].1;
        let mut session = session();
        session.selection = Some(selection);
        session.shot_bar_click(cancel.center(), bar);
        assert!(matches!(session.outcome, Some(Outcome::Cancel)));
    }

    #[test]
    fn coordinate_mapping_round_trips_corners() {
        let logical = SCREEN;
        let surface = rect(10.0, 20.0, 510.0, 320.0);
        let corners = [
            Pos2::new(0.0, 0.0),
            Pos2::new(1000.0, 0.0),
            Pos2::new(0.0, 600.0),
            Pos2::new(1000.0, 600.0),
            Pos2::new(250.0, 150.0),
        ];
        for logical_pos in corners {
            let on_surface = Session::to_surface(
                Rect::from_min_size(logical_pos, Vec2::splat(1.0)),
                surface,
                logical,
            )
            .min;
            let back = Session::to_logical(on_surface, surface, logical);
            assert!(
                (back.x - logical_pos.x).abs() < 0.01 && (back.y - logical_pos.y).abs() < 0.01,
                "round-trip failed: {logical_pos:?} -> {on_surface:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn a_recording_readout_says_so_and_a_screenshot_does_not() {
        let (tag, size) = readout(true, rect(0.0, 0.0, 320.0, 180.0), 2.0);
        assert_eq!(tag, Some("录屏"));
        assert_eq!(size, "640 * 360");
        let (tag, size) = readout(false, rect(0.0, 0.0, 320.0, 180.0), 2.0);
        assert_eq!(tag, None);
        assert_eq!(size, "640 * 360");
    }

    #[test]
    fn the_record_bar_sits_below_the_menu_bar_and_has_the_example_controls() {
        let bar = record_bar_at(SCREEN, 32.0);
        assert_eq!(bar.min.y, 40.0);
        assert_eq!(bar.center().x, SCREEN.center().x);
        let layout = record_bar_layout(bar);
        assert!(layout.start.min.x > layout.mic.max.x);
        assert!(layout.exit.min.x > layout.start.max.x);
        assert!(bar.contains(layout.exit.max));
        assert_eq!(record_bar_at(SCREEN, 0.0).min.y, 24.0);
    }

    #[test]
    fn the_record_menus_list_all_qualities_and_audio_choices() {
        let bar = record_bar_at(SCREEN, 32.0);
        let items = menu_items(MenuKind::Quality, bar, SCREEN, 0);
        assert_eq!(items.len(), 3);
        let rect = menu_rect(MenuKind::Quality, bar, SCREEN, 0);
        assert!(rect.min.y > bar.max.y);
        assert_eq!(
            menu_choice(MenuKind::Quality, bar, SCREEN, 0, items[1].center()),
            Some(1)
        );
        let system = menu_items(MenuKind::System, bar, SCREEN, 0);
        assert_eq!(system.len(), 2);
        let mic = menu_items(MenuKind::Mic, bar, SCREEN, 2);
        assert_eq!(mic.len(), 4);
    }

    #[test]
    fn quality_presets_match_the_settings_page() {
        assert_eq!(RecordQuality::Native.long_edge(), 0);
        assert_eq!(RecordQuality::Native.fps(), 60);
        assert_eq!(RecordQuality::Hd.long_edge(), 1920);
        assert_eq!(RecordQuality::Hd.fps(), 30);
        assert_eq!(RecordQuality::Smooth.long_edge(), 1280);
        assert_eq!(RecordQuality::Smooth.fps(), 15);
        assert_eq!(RecordQuality::of(0), RecordQuality::Native);
        assert_eq!(RecordQuality::of(1920), RecordQuality::Hd);
        assert_eq!(RecordQuality::of(1280), RecordQuality::Smooth);
    }

    #[test]
    fn a_recording_outcome_carries_the_toolbar_options() {
        let mut session = session();
        session.record = true;
        session.selection = Some(rect(10.0, 10.0, 200.0, 200.0));
        session.options = RecordOptions {
            quality: RecordQuality::Hd,
            system_audio: false,
            mic: true,
            mic_device: "mic-1".to_string(),
        };
        session.on_enter();
        match session.outcome {
            Some(Outcome::StartRecording { rect: got, options }) => {
                assert_eq!(got, rect(10.0, 10.0, 200.0, 200.0));
                assert_eq!(options.quality, RecordQuality::Hd);
                assert!(!options.system_audio);
                assert_eq!(options.mic_device, "mic-1");
            }
            other => panic!("expected StartRecording, got {other:?}"),
        }
    }

    #[test]
    fn starting_a_recording_without_a_selection_takes_the_whole_screen() {
        let mut session = session();
        session.record = true;
        session.on_enter();
        match session.outcome {
            Some(Outcome::StartRecording { rect: got, .. }) => {
                assert_eq!(
                    got,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 600.0))
                );
            }
            other => panic!("expected StartRecording, got {other:?}"),
        }
    }

    #[test]
    fn marks_undo_redo_and_new_strokes_clear_the_redo_stack() {
        let mut session = session();
        session.selection = Some(rect(10.0, 10.0, 300.0, 300.0));
        session.choose_tool(ShotTool::Box);
        session.begin_drag(Pos2::new(20.0, 20.0), None);
        session.apply_drag(
            Drag::Paint {
                tool: ShotTool::Box,
                start: Pos2::new(20.0, 20.0),
            },
            Pos2::new(80.0, 80.0),
            SCREEN,
        );
        session.on_up(Pos2::new(80.0, 80.0), SCREEN);
        assert_eq!(session.marks.items.len(), 1);
        assert!(session.undo());
        assert!(session.marks.items.is_empty());
        assert!(session.redo());
        assert_eq!(session.marks.items.len(), 1);

        session.undo();
        session.choose_tool(ShotTool::Arrow);
        session.begin_drag(Pos2::new(30.0, 30.0), None);
        session.apply_drag(
            Drag::Paint {
                tool: ShotTool::Arrow,
                start: Pos2::new(30.0, 30.0),
            },
            Pos2::new(90.0, 90.0),
            SCREEN,
        );
        session.on_up(Pos2::new(90.0, 90.0), SCREEN);
        assert!(session.marks.undone.is_empty());
    }

    #[test]
    fn text_tool_opens_a_draft_and_commits_it() {
        let mut session = session();
        session.selection = Some(rect(10.0, 10.0, 300.0, 300.0));
        session.choose_tool(ShotTool::Text);
        session.begin_drag(Pos2::new(50.0, 60.0), None);
        let draft = session.draft.as_mut().expect("draft");
        draft.text = "  你好  ".to_string();
        session.on_enter();
        assert!(session.draft.is_none());
        assert!(matches!(
            session.marks.items.first(),
            Some(Mark::Text { at, text }) if *at == Pos2::new(50.0, 60.0) && text == "你好"
        ));
    }

    #[test]
    fn a_fresh_selection_drops_old_marks_but_moving_keeps_them() {
        let mut session = session();
        session.selection = Some(rect(10.0, 10.0, 300.0, 300.0));
        session.marks.add(Mark::Box {
            rect: rect(20.0, 20.0, 60.0, 60.0),
        });
        session.choose_tool(ShotTool::Select);
        session.begin_drag(Pos2::new(100.0, 100.0), None);
        assert!(matches!(session.drag, Some(Drag::Move { .. })));
        assert_eq!(session.marks.items.len(), 1);

        session.drag = None;
        session.begin_drag(Pos2::new(500.0, 400.0), None);
        assert!(matches!(session.drag, Some(Drag::Fresh { .. })));
        assert!(session.marks.items.is_empty());
    }

    #[test]
    fn a_second_press_on_the_selection_starts_recording() {
        let mut session = session();
        session.record = true;
        session.selection = Some(rect(10.0, 10.0, 200.0, 200.0));
        session.last_up = Some(Pos2::new(50.0, 50.0));
        session.last_up_at = std::time::Instant::now();
        session.on_down(Pos2::new(52.0, 51.0), None);
        assert!(matches!(
            session.outcome,
            Some(Outcome::StartRecording { .. })
        ));
    }

    fn session() -> Session {
        Session::new(
            Freeze {
                image: image::RgbaImage::new(1, 1),
                position: Pos2::ZERO,
                size: Vec2::new(1000.0, 600.0),
                scale: 1.0,
                windows: Vec::new(),
                top_inset: 0.0,
            },
            false,
        )
    }
}
