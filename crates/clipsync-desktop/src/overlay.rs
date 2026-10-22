//! The WeChat capture overlay: the screen is frozen under a dim mask, hovering previews the
//! window a click would take, dragging draws a region, and eight handles adjust it before it is
//! cut out of the frame.

use eframe::egui::{
    self, Color32, CursorIcon, Frame, Id, Key, Order, PointerButton, Pos2, Rect, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand, ViewportId,
};
#[cfg(target_os = "macos")]
use objc2::{rc::Retained, MainThreadMarker};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSWindow};

use crate::icons;
use crate::theme;

/// Everything but the region the user aims at goes under this much black.
const MASK: Color32 = Color32::from_black_alpha(128);
/// A handle is grabbable from this far out, so the pointer never has to land on one exactly.
const HANDLE_SLOP: f32 = 9.0;
const HANDLE_SIZE: f32 = 8.0;
/// Thinner than this and it was a click, not a region.
const MIN_SIZE: f32 = 6.0;
const TOOLBAR_GAP: f32 = 8.0;
const TOOLBAR_HEIGHT: f32 = 45.0;
/// Two 33pt buttons, 6pt of margin either side, 4pt between them.
const TOOLBAR_WIDTH: f32 = 2.0 * 33.0 + 2.0 * 6.0 + 4.0;

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
}

/// What the overlay ended up doing with the frozen frame.
#[derive(Clone, Copy, Debug)]
pub enum Outcome {
    Crop(Rect),
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
    /// The frozen screen has reached the glass, which is when the app window may come back.
    raised: bool,
    /// The overlay holds the keyboard, so Esc and Enter are its to hear.
    focused: bool,
}

impl Session {
    pub fn new(freeze: Freeze) -> Self {
        Self {
            freeze,
            texture: None,
            selection: None,
            hover: None,
            drag: None,
            outcome: None,
            raised: false,
            focused: false,
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
        let frame = ui.max_rect();
        self.paint_frame(ui, frame);
        // Where the pill is going to sit, so a click on it is not mistaken for a new region.
        let pill = self.toolbar_rect(frame);
        let position = self.track_pointer(ui, frame, pill);
        self.paint_marks(ui, frame);
        let toolbar = self.paint_toolbar(ui, pill);
        self.paint_cursor(ui, position, toolbar);
        self.raise(ui.ctx());
    }

    /// Once the frozen screen is on glass the app window may come back behind it. Showing it also
    /// hands it the keyboard, so the overlay has to take the keys one pass later rather than in
    /// the same flush — otherwise Esc goes to a window nobody can see.
    fn raise(&mut self, ctx: &egui::Context) {
        if !self.raised {
            self.raised = true;
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Visible(true));
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

    /// The whole pointer state machine for this frame; returns where the pointer is.
    fn track_pointer(&mut self, ui: &mut Ui, frame: Rect, pill: Option<Rect>) -> Option<Pos2> {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.outcome = Some(Outcome::Cancel);
            return None;
        }
        let pointer = ui.input(|i| i.pointer.clone());
        let position = pointer.hover_pos();
        if pointer.button_pressed(PointerButton::Secondary) {
            self.outcome = Some(Outcome::Cancel);
            return None;
        }
        // The second click of a double-click is also a press, so finish on it before the press
        // below can start a new drag and throw the region away.
        if pointer.button_double_clicked(PointerButton::Primary) {
            if let Some(selection) = self.selection {
                self.outcome = Some(Outcome::Crop(selection));
                return Some(position.unwrap_or(selection.min));
            }
        }
        if pointer.button_pressed(PointerButton::Primary) {
            self.begin_drag(position, pill);
        } else if let (Some(drag), Some(position)) = (self.drag, position) {
            if pointer.button_down(PointerButton::Primary) {
                self.apply_drag(drag, position, frame);
            }
        }
        if pointer.button_released(PointerButton::Primary) {
            self.finish_drag(position, frame);
        }
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Some(selection) = self.selection {
                self.outcome = Some(Outcome::Crop(selection));
            }
        }
        if let (None, Some(position)) = (self.selection, position) {
            // Nothing is being adjusted yet, so preview whatever a click would take.
            self.hover = Some(hit_window(&self.freeze.windows, position, frame));
        }
        position
    }

    fn begin_drag(&mut self, position: Option<Pos2>, pill: Option<Rect>) {
        let Some(position) = position else { return };
        if let Some(selection) = self.selection {
            if let Some(handle) = handle_at(selection, position, HANDLE_SLOP) {
                self.drag = Some(Drag::Handle(handle));
                return;
            }
            if selection.contains(position) {
                self.drag = Some(Drag::Move {
                    grab: position - selection.min,
                });
                return;
            }
            // A click on the pill under the region belongs to its buttons; starting a fresh
            // region here would erase the one the buttons are attached to.
            if pill.is_some_and(|rect| rect.contains(position)) {
                return;
            }
        }
        self.drag = Some(Drag::Fresh { anchor: position });
        self.selection = None;
        self.hover = None;
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
        }
    }

    fn finish_drag(&mut self, position: Option<Pos2>, frame: Rect) {
        let Some(Drag::Fresh { anchor }) = self.drag.take() else {
            return;
        };
        // A drag that ended off the edges still has to leave a region inside them: the pill and the
        // handles are placed from this rect, and off-screen means unreachable.
        let dragged = position.map(|position| between(anchor, position).intersect(frame));
        let picked = dragged
            .filter(|rect| is_region(*rect, frame))
            .unwrap_or_else(|| {
                // No real drag: a click takes the window under the cursor, the desktop if none.
                hit_window(&self.freeze.windows, position.unwrap_or(anchor), frame)
            });
        self.selection = Some(picked);
        self.hover = None;
    }

    fn paint_marks(&self, ui: &Ui, frame: Rect) {
        let hole = self.selection.or(self.hover);
        for rect in mask_rects(frame, hole) {
            if !rect.is_negative() {
                ui.painter().rect_filled(rect, 0, MASK);
            }
        }
        let Some(rect) = hole else { return };
        let accent = theme::p().accent;
        ui.painter()
            .rect_stroke(rect, 0, Stroke::new(1.5, accent), StrokeKind::Middle);
        if self.selection.is_some() && self.drag.is_none() {
            for handle in HANDLES {
                let square = square_at(handle_point(rect, handle), HANDLE_SIZE);
                ui.painter()
                    .rect_filled(square, 1, Color32::from_white_alpha(230));
                ui.painter()
                    .rect_stroke(square, 1, Stroke::new(1.0, accent), StrokeKind::Inside);
            }
        }
        self.paint_size(ui, rect);
    }

    /// The `宽 × 高` readout, in the pixels the crop will actually be.
    fn paint_size(&self, ui: &Ui, rect: Rect) {
        let width = (rect.width() * self.freeze.scale).round().max(1.0) as i64;
        let height = (rect.height() * self.freeze.scale).round().max(1.0) as i64;
        let label = format!("{width} × {height}");
        let font = egui::FontId::proportional(12.0);
        let galley = ui.painter().layout_no_wrap(label, font, Color32::WHITE);
        let pad = Vec2::new(7.0, 4.0);
        let size = galley.size() + pad * 2.0;
        let below = rect.max.y + TOOLBAR_GAP + size.y;
        let min = Pos2::new(
            (rect.max.x - size.x).clamp(0.0, ui.max_rect().max.x - size.x),
            if below > ui.max_rect().max.y {
                (rect.min.y - TOOLBAR_GAP - size.y).max(0.0)
            } else {
                below
            },
        );
        let pill = Rect::from_min_size(min, size);
        ui.painter()
            .rect_filled(pill, 4, Color32::from_black_alpha(200));
        ui.painter().galley(pill.min + pad, galley, Color32::WHITE);
    }

    /// The pill under the region, or above it when the region reaches the bottom of the screen.
    fn toolbar_rect(&self, frame: Rect) -> Option<Rect> {
        let selection = self.selection?;
        if self.drag.is_some() {
            return None;
        }
        Some(pill_at(selection, frame))
    }

    /// The pill's cancel and confirm buttons, with room to grow into the annotation tools.
    fn paint_toolbar(&mut self, ui: &mut Ui, pill: Option<Rect>) -> Option<Rect> {
        let selection = self.selection?;
        let min = pill?.min;
        let mut cancel = false;
        let mut confirm = false;
        egui::Area::new(Id::unique("screenshot-toolbar"))
            .order(Order::Foreground)
            .fixed_pos(min)
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(Color32::from_black_alpha(220))
                    .stroke(Stroke::new(1.0, Color32::from_white_alpha(40)))
                    .corner_radius(10)
                    .inner_margin(6)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::splat(4.0);
                            if icons::icon_button_tinted(
                                ui,
                                icons::Icon::Close,
                                Color32::WHITE,
                                true,
                                "取消（Esc）",
                            )
                            .clicked()
                            {
                                cancel = true;
                            }
                            if icons::icon_button_tinted(
                                ui,
                                icons::Icon::Check,
                                theme::p().green,
                                true,
                                "完成（Enter）",
                            )
                            .clicked()
                            {
                                confirm = true;
                            }
                        });
                    });
            });
        if cancel {
            self.outcome = Some(Outcome::Cancel);
        } else if confirm {
            self.outcome = Some(Outcome::Crop(selection));
        }
        pill
    }

    fn paint_cursor(&self, ui: &mut Ui, position: Option<Pos2>, toolbar: Option<Rect>) {
        let cursor = if let Some(drag) = self.drag {
            match drag {
                Drag::Handle(handle) => resize_cursor(handle),
                Drag::Move { .. } => CursorIcon::Move,
                Drag::Fresh { .. } => CursorIcon::Crosshair,
            }
        } else if let Some(position) = position {
            if toolbar.is_some_and(|rect| rect.contains(position)) {
                CursorIcon::Default
            } else if let Some(selection) = self.selection {
                match handle_at(selection, position, HANDLE_SLOP) {
                    Some(handle) => resize_cursor(handle),
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

/// Put the overlay window on screen, or take it off again. It is never closed: closing tears down
/// the egui side but leaves the native window behind, so one window per capture would pile up.
pub fn reveal(ctx: &egui::Context, visible: bool) {
    ctx.send_viewport_cmd_to(viewport_id(), ViewportCommand::Visible(visible));
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

/// A region the user drew is only real once it is big enough to be worth pixels.
fn is_region(rect: Rect, frame: Rect) -> bool {
    rect.intersect(frame).width() >= MIN_SIZE && rect.intersect(frame).height() >= MIN_SIZE
}

/// The rectangle between two pointer positions, whichever way the drag went.
fn between(anchor: Pos2, position: Pos2) -> Rect {
    Rect::from_two_pos(anchor, position)
}

/// The window the front-most hit at `position`, or the whole screen when the desktop is showing.
fn hit_window(windows: &[Rect], position: Pos2, frame: Rect) -> Rect {
    windows
        .iter()
        .find(|rect| rect.contains(position))
        .copied()
        .unwrap_or(frame)
}

/// Where the button pill sits: under the region's bottom-right, flipped above it at the edge.
fn pill_at(selection: Rect, frame: Rect) -> Rect {
    let min = Pos2::new(
        (selection.max.x - TOOLBAR_WIDTH).clamp(0.0, frame.max.x - TOOLBAR_WIDTH),
        if selection.max.y + TOOLBAR_GAP + TOOLBAR_HEIGHT > frame.max.y {
            (selection.min.y - TOOLBAR_GAP - TOOLBAR_HEIGHT).max(0.0)
        } else {
            selection.max.y + TOOLBAR_GAP
        },
    );
    Rect::from_min_size(min, Vec2::new(TOOLBAR_WIDTH, TOOLBAR_HEIGHT))
}

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
        between, crop_px, handle_at, handle_point, hit_window, is_region, mask_rects, pill_at,
        resize, square_at, translate, Drag, Freeze, Handle, Session, MIN_SIZE, TOOLBAR_GAP,
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
    fn the_pill_sits_under_the_region_and_flips_above_it_at_the_bottom_edge() {
        let pill = pill_at(rect(100.0, 100.0, 300.0, 200.0), SCREEN);
        assert_eq!(pill.max.x, 300.0);
        assert_eq!(pill.min.y, 200.0 + TOOLBAR_GAP);

        let flipped = pill_at(rect(100.0, 100.0, 300.0, 590.0), SCREEN);
        assert_eq!(flipped.max.y, 100.0 - TOOLBAR_GAP);

        let edge = pill_at(rect(900.0, 100.0, 1000.0, 200.0), SCREEN);
        assert_eq!(edge.max.x, SCREEN.max.x);
    }

    #[test]
    fn a_click_on_the_pill_leaves_the_region_alone() {
        let selection = rect(100.0, 100.0, 300.0, 200.0);
        let pill = pill_at(selection, SCREEN);
        let mut session = session();
        session.selection = Some(selection);

        session.begin_drag(Some(pill.center()), Some(pill));
        assert!(session.drag.is_none());
        assert_eq!(session.selection, Some(selection));

        // Handles still win over the pill, so the bottom edge stays adjustable.
        session.begin_drag(Some(Pos2::new(200.0, 204.0)), Some(pill));
        assert!(matches!(session.drag, Some(Drag::Handle(Handle::Bottom))));

        // Anywhere else starts a fresh region.
        session.begin_drag(Some(Pos2::new(600.0, 400.0)), Some(pill));
        assert!(matches!(session.drag, Some(Drag::Fresh { .. })));
        assert_eq!(session.selection, None);
    }

    fn session() -> Session {
        Session::new(Freeze {
            image: image::RgbaImage::new(1, 1),
            position: Pos2::ZERO,
            size: Vec2::new(1000.0, 600.0),
            scale: 1.0,
            windows: Vec::new(),
        })
    }
}
