//! The global screenshot hotkey and the screen grab it triggers.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use eframe::egui::{Key, Modifiers, Pos2, Rect, Vec2};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::clipboard::image_hash;
use crate::overlay::{self, Mark};
use crate::theme;

/// WeChat's default; the one users get without ever opening settings.
pub const DEFAULT_HOTKEY: &str = "Alt+A";

/// 录屏默认键，避开截屏的 Alt+A。
pub const DEFAULT_RECORD_HOTKEY: &str = "Alt+Shift+A";

pub struct Shot {
    pub png: Vec<u8>,
    pub hash: String,
}

/// The frozen screen the overlay draws: the pixels, the monitor in points, and the windows that
/// were on it, front to back, already in the overlay's own coordinates.
pub fn freeze() -> Result<overlay::Freeze, String> {
    let monitor = primary_monitor()?;
    let image = monitor.capture_image().map_err(|error| error.to_string())?;
    let width = monitor.width().map_err(|error| error.to_string())? as f32;
    let height = monitor.height().map_err(|error| error.to_string())? as f32;
    let position = Pos2::new(
        monitor.x().map_err(|error| error.to_string())? as f32,
        monitor.y().map_err(|error| error.to_string())? as f32,
    );
    // Window bounds come in display points, so one point is however many pixels the grab came
    // back with — that ratio is what turns a selection into a crop.
    let scale = image.width() as f32 / width.max(1.0);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
    let windows = snap_targets(position, screen);
    Ok(overlay::Freeze {
        image,
        position,
        size: Vec2::new(width, height),
        scale,
        windows,
        top_inset: overlay::top_inset(),
    })
}

// 普通层窗口的全局 bounds（点，左上原点，顶层在前）。xcap 不暴露 window layer，
// 而程序坞是一块盖满整屏的 layer-20 窗口，用它排在最前会把所有点击都吃成整屏。
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn cs_snap_windows(out: *mut f64, max: i32) -> i32;
}

const MAX_SNAP_WINDOWS: usize = 256;

fn snap_targets(position: Pos2, screen: Rect) -> Vec<Rect> {
    #[cfg(target_os = "macos")]
    {
        let mut buffer = [0f64; MAX_SNAP_WINDOWS * 4];
        let count = unsafe { cs_snap_windows(buffer.as_mut_ptr(), MAX_SNAP_WINDOWS as i32) };
        let count = count.clamp(0, MAX_SNAP_WINDOWS as i32) as usize;
        (0..count)
            .filter_map(|index| {
                local_window_rect(&buffer[index * 4..index * 4 + 4], position, screen)
            })
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        xcap::Window::all()
            .map(|windows| {
                windows
                    .into_iter()
                    .filter(|window| window.pid().ok() != Some(std::process::id()))
                    .filter(|window| !window.is_minimized().unwrap_or(false))
                    .filter_map(|window| {
                        let bounds = [
                            window.x().ok()? as f64,
                            window.y().ok()? as f64,
                            window.width().ok()? as f64,
                            window.height().ok()? as f64,
                        ];
                        local_window_rect(&bounds, position, screen)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 全局 bounds 转成 overlay 局部点，裁到本屏，碎条直接丢掉。
fn local_window_rect(bounds: &[f64], origin: Pos2, screen: Rect) -> Option<Rect> {
    let rect = Rect::from_min_max(
        Pos2::new(bounds[0] as f32 - origin.x, bounds[1] as f32 - origin.y),
        Pos2::new(
            (bounds[0] + bounds[2]) as f32 - origin.x,
            (bounds[1] + bounds[3]) as f32 - origin.y,
        ),
    );
    let visible = rect.intersect(screen);
    (visible.width() >= 40.0 && visible.height() >= 40.0).then_some(visible)
}

fn primary_monitor() -> Result<xcap::Monitor, String> {
    let monitors = xcap::Monitor::all().map_err(|error| error.to_string())?;
    let index = monitors
        .iter()
        .position(|monitor| monitor.is_primary().unwrap_or(false))
        .unwrap_or(0);
    monitors
        .into_iter()
        .nth(index)
        .ok_or_else(|| "没有可用的显示器".to_string())
}

/// PNG plus the content hash the clipboard watch has to know about, or a re-record follows.
/// `origin` is the crop's top-left in frozen-frame pixels; marks are in overlay points and get
/// scaled and shifted into the crop before they are baked in.
pub fn encode(
    mut frame: image::RgbaImage,
    marks: &[Mark],
    origin: (u32, u32),
    scale: f32,
) -> Result<Shot, String> {
    if !marks.is_empty() {
        bake(&mut frame, marks, origin, scale);
    }
    let (width, height) = frame.dimensions();
    let hash = image_hash(frame.as_raw(), width, height);
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(frame)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(Shot {
        png: png.into_inner(),
        hash,
    })
}

const MARK_RED: [u8; 4] = [0xff, 0x3b, 0x30, 0xff];

fn bake(canvas: &mut image::RgbaImage, marks: &[Mark], origin: (u32, u32), scale: f32) {
    let source = canvas.clone();
    let width = (3.0 * scale).max(1.0);
    let to_px = |p: Pos2| (p.x * scale - origin.0 as f32, p.y * scale - origin.1 as f32);
    let to_rect = |rect: Rect| {
        let (x0, y0) = to_px(rect.min);
        let (x1, y1) = to_px(rect.max);
        (x0, y0, x1, y1)
    };
    for mark in marks {
        match mark {
            Mark::Box { rect } => {
                let (x0, y0, x1, y1) = to_rect(*rect);
                draw_line(canvas, (x0, y0), (x1, y0), width);
                draw_line(canvas, (x1, y0), (x1, y1), width);
                draw_line(canvas, (x1, y1), (x0, y1), width);
                draw_line(canvas, (x0, y1), (x0, y0), width);
            }
            Mark::Arrow { from, to } => {
                let from = to_px(*from);
                let to = to_px(*to);
                draw_line(canvas, from, to, width);
                let angle = (to.1 - from.1).atan2(to.0 - from.0);
                let head = 14.0 * scale;
                let left = (
                    to.0 - head * (angle - std::f32::consts::FRAC_PI_6).cos(),
                    to.1 - head * (angle - std::f32::consts::FRAC_PI_6).sin(),
                );
                let right = (
                    to.0 - head * (angle + std::f32::consts::FRAC_PI_6).cos(),
                    to.1 - head * (angle + std::f32::consts::FRAC_PI_6).sin(),
                );
                fill_triangle(canvas, to, left, right);
            }
            Mark::Pen { points } => {
                for pair in points.windows(2) {
                    draw_line(canvas, to_px(pair[0]), to_px(pair[1]), width);
                }
            }
            Mark::Text { at, text } => draw_text(canvas, text, to_px(*at), scale),
            Mark::Mosaic { rect } => {
                let (x0, y0, x1, y1) = to_rect(*rect);
                mosaic(
                    canvas,
                    &source,
                    x0,
                    y0,
                    x1,
                    y1,
                    (12.0 * scale).max(1.0) as i32,
                );
            }
        }
    }
}

/// 一条圆头粗线：沿线段连续盖章，比 Bresenham 更接近 Java2D 的描边。
fn draw_line(canvas: &mut image::RgbaImage, from: (f32, f32), to: (f32, f32), width: f32) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let length = (dx * dx + dy * dy).sqrt();
    let steps = (length / (width * 0.25).max(0.35)).ceil().max(1.0) as usize;
    let radius = width / 2.0;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        stamp(canvas, (from.0 + dx * t, from.1 + dy * t), radius);
    }
}

fn stamp(canvas: &mut image::RgbaImage, center: (f32, f32), radius: f32) {
    let left = (center.0 - radius - 1.0).floor().max(0.0) as i32;
    let top = (center.1 - radius - 1.0).floor().max(0.0) as i32;
    let right = (center.0 + radius + 1.0).ceil() as i32;
    let bottom = (center.1 + radius + 1.0).ceil() as i32;
    for y in top..=bottom {
        for x in left..=right {
            if x < 0 || y < 0 || x >= canvas.width() as i32 || y >= canvas.height() as i32 {
                continue;
            }
            let dx = x as f32 + 0.5 - center.0;
            let dy = y as f32 + 0.5 - center.1;
            let coverage = (radius + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            blend(canvas, x as u32, y as u32, MARK_RED, coverage);
        }
    }
}

fn fill_triangle(canvas: &mut image::RgbaImage, a: (f32, f32), b: (f32, f32), c: (f32, f32)) {
    let min_x = a.0.min(b.0).min(c.0).floor().max(0.0) as i32;
    let min_y = a.1.min(b.1).min(c.1).floor().max(0.0) as i32;
    let max_x = a.0.max(b.0).max(c.0).ceil() as i32;
    let max_y = a.1.max(b.1).max(c.1).ceil() as i32;
    let edge = |p: (f32, f32), q: (f32, f32), x: f32, y: f32| {
        (x - p.0) * (q.1 - p.1) - (y - p.1) * (q.0 - p.0)
    };
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if x < 0 || y < 0 || x >= canvas.width() as i32 || y >= canvas.height() as i32 {
                continue;
            }
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge(b, c, px, py);
            let w1 = edge(c, a, px, py);
            let w2 = edge(a, b, px, py);
            if (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0) {
                blend(canvas, x as u32, y as u32, MARK_RED, 1.0);
            }
        }
    }
}

fn draw_text(canvas: &mut image::RgbaImage, text: &str, at: (f32, f32), scale: f32) {
    let Ok(font) = FontRef::try_from_slice(theme::PRIMARY_FACE) else {
        return;
    };
    let size = (18.0 * scale).max(12.0);
    let scaled = font.as_scaled(PxScale::from(size));
    let mut caret = point(at.0, at.1 + scaled.ascent());
    let mut previous = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(previous) = previous {
            caret.x += scaled.kern(previous, id);
        }
        let glyph = id.with_scale_and_position(PxScale::from(size), caret);
        if let Some(outline) = scaled.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|x, y, coverage| {
                let px = bounds.min.x as i32 + x as i32;
                let py = bounds.min.y as i32 + y as i32;
                if px >= 0 && py >= 0 && px < canvas.width() as i32 && py < canvas.height() as i32 {
                    blend(canvas, px as u32, py as u32, MARK_RED, coverage);
                }
            });
        }
        caret.x += scaled.h_advance(id);
        previous = Some(id);
    }
}

fn mosaic(
    canvas: &mut image::RgbaImage,
    source: &image::RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    block: i32,
) {
    let left = (x0.round() as i32).clamp(0, source.width() as i32 - 1);
    let top = (y0.round() as i32).clamp(0, source.height() as i32 - 1);
    let right = (x1.round() as i32).clamp(left + 1, source.width() as i32);
    let bottom = (y1.round() as i32).clamp(top + 1, source.height() as i32);
    let mut y = top;
    while y < bottom {
        let mut x = left;
        while x < right {
            let sample_x = (x + block / 2).clamp(0, source.width() as i32 - 1) as u32;
            let sample_y = (y + block / 2).clamp(0, source.height() as i32 - 1) as u32;
            let pixel = *source.get_pixel(sample_x, sample_y);
            for py in y..(y + block).min(bottom) {
                for px in x..(x + block).min(right) {
                    canvas.put_pixel(px as u32, py as u32, pixel);
                }
            }
            x += block;
        }
        y += block;
    }
}

fn blend(canvas: &mut image::RgbaImage, x: u32, y: u32, color: [u8; 4], coverage: f32) {
    if coverage <= 0.0 {
        return;
    }
    let alpha = (color[3] as f32 / 255.0) * coverage.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let pixel = canvas.get_pixel_mut(x, y);
    for channel in 0..3 {
        pixel[channel] = (color[channel] as f32 * alpha + pixel[channel] as f32 * (1.0 - alpha))
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    pixel[3] = ((pixel[3] as f32 / 255.0) + alpha)
        .min(1.0)
        .mul_add(255.0, 0.5) as u8;
}

/// Without the grant macOS hands back wallpaper instead of an error, which users would
/// report as a broken screenshot rather than a missing permission.
#[cfg(target_os = "macos")]
pub fn permission_missing() -> bool {
    !unsafe { CGPreflightScreenCaptureAccess() }
}

/// Preflight alone never prompts and never registers the app in System Settings → Screen
/// Recording. Mirror Compose: when missing, call `CGRequestScreenCaptureAccess` so the
/// system sheet appears and ClipSyncRust shows up in the list. A freshly granted grant
/// still needs a process restart before capture works.
#[cfg(target_os = "macos")]
pub fn request_access() -> bool {
    if unsafe { CGPreflightScreenCaptureAccess() } {
        return true;
    }
    unsafe { CGRequestScreenCaptureAccess() }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(not(target_os = "macos"))]
pub fn permission_missing() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn request_access() -> bool {
    true
}

pub fn parse(spec: &str) -> Option<HotKey> {
    let hotkey = spec.parse::<HotKey>().ok()?;
    (!hotkey.mods.is_empty()).then_some(hotkey)
}

/// The shortcut string for the key the user just pressed, or `None` when it cannot be
/// one. Bare keys are rejected: a global hotkey without a modifier would swallow typing.
pub fn spec(key: Key, modifiers: Modifiers) -> Option<String> {
    let token = token(key)?;
    let mut prefix = String::new();
    // `command` mirrors ctrl off macOS, so the CMD slot reads the mac-only field and
    // Ctrl+S stays a single modifier on every platform.
    if modifiers.mac_cmd {
        prefix.push_str("Command+");
    }
    if modifiers.ctrl {
        prefix.push_str("Control+");
    }
    if modifiers.alt {
        prefix.push_str("Alt+");
    }
    if modifiers.shift {
        prefix.push_str("Shift+");
    }
    if prefix.is_empty() {
        return None;
    }
    prefix.push_str(token);
    Some(prefix)
}

/// Key names `global_hotkey` parses, spelled the way settings show them.
fn token(key: Key) -> Option<&'static str> {
    let token = match key {
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::Space => "Space",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::ArrowUp => "ArrowUp",
        Key::ArrowDown => "ArrowDown",
        Key::ArrowLeft => "ArrowLeft",
        Key::ArrowRight => "ArrowRight",
        Key::Minus => "Minus",
        Key::Equals => "Equal",
        Key::OpenBracket => "BracketLeft",
        Key::CloseBracket => "BracketRight",
        Key::Backslash => "Backslash",
        Key::Semicolon => "Semicolon",
        Key::Quote => "Quote",
        Key::Comma => "Comma",
        Key::Period => "Period",
        Key::Slash => "Slash",
        Key::Backtick => "Backquote",
        _ => return None,
    };
    Some(token)
}

struct HotkeySlots {
    shot_id: std::sync::atomic::AtomicU32,
    record_id: std::sync::atomic::AtomicU32,
    shot: AtomicBool,
    record: AtomicBool,
}

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    shot: Option<HotKey>,
    record: Option<HotKey>,
    slots: Arc<HotkeySlots>,
}

impl Hotkeys {
    /// macOS dispatches Carbon hotkeys on the main run loop, so the manager has to be
    /// built there — which is where eframe constructs the app.
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        let manager = GlobalHotKeyManager::new().ok()?;
        let slots = Arc::new(HotkeySlots {
            shot_id: std::sync::atomic::AtomicU32::new(0),
            record_id: std::sync::atomic::AtomicU32::new(0),
            shot: AtomicBool::new(false),
            record: AtomicBool::new(false),
        });
        let flags = slots.clone();
        let ctx = ctx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if !matches!(event.state(), HotKeyState::Pressed) {
                return;
            }
            let id = event.id();
            if id == flags.record_id.load(Ordering::Relaxed) {
                flags.record.store(true, Ordering::Relaxed);
            } else if id == flags.shot_id.load(Ordering::Relaxed) {
                flags.shot.store(true, Ordering::Relaxed);
            }
            // The idle loop only wakes once a second; a screenshot must feel instant.
            ctx.request_repaint();
        }));
        Some(Self {
            manager,
            shot: None,
            record: None,
            slots,
        })
    }

    pub fn take_pressed(&self) -> bool {
        self.slots.shot.swap(false, Ordering::Relaxed)
    }

    pub fn take_record_pressed(&self) -> bool {
        self.slots.record.swap(false, Ordering::Relaxed)
    }

    pub fn apply(&mut self, spec: &str) -> Result<(), String> {
        self.bind(spec, false)
    }

    pub fn apply_record(&mut self, spec: &str) -> Result<(), String> {
        self.bind(spec, true)
    }

    fn bind(&mut self, spec: &str, record: bool) -> Result<(), String> {
        let next = parse(spec).ok_or_else(|| format!("「{spec}」不是可用的快捷键"))?;
        let current = if record { &self.record } else { &self.shot };
        if *current == Some(next) {
            return Ok(());
        }
        // Register the new chord first: if it fails the old one stays live instead of
        // leaving the user with no hotkey at all.
        self.manager
            .register(next)
            .map_err(|_| format!("「{spec}」注册失败，换一个组合键试试"))?;
        let id_slot = if record {
            &self.slots.record_id
        } else {
            &self.slots.shot_id
        };
        id_slot.store(next.id(), Ordering::Relaxed);
        let slot = if record {
            &mut self.record
        } else {
            &mut self.shot
        };
        if let Some(previous) = slot.replace(next) {
            let _ = self.manager.unregister(previous);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{local_window_rect, parse, spec, token, DEFAULT_HOTKEY};
    use eframe::egui::{Key, Modifiers, Pos2, Rect, Vec2};

    /// Needs a live desktop; run with `cargo test -p clipsync-desktop live_freeze -- --ignored`.
    #[test]
    #[ignore]
    fn a_live_freeze_finds_windows_on_this_screen() {
        let freeze = super::freeze().expect("freeze the screen");
        assert!(freeze.image.width() > 0);
        assert!(!freeze.windows.is_empty(), "no windows snapped");
        eprintln!(
            "screen {:?}: {:?}",
            freeze.size,
            &freeze.windows[..freeze.windows.len().min(6)]
        );
    }

    #[test]
    fn a_window_is_moved_into_overlay_points_clipped_to_the_screen() {
        let clock_screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
        // Second display sitting to the right of the frozen one.
        let origin = Pos2::new(1440.0, 0.0);
        assert_eq!(
            local_window_rect(&[1500.0, 100.0, 400.0, 300.0], origin, clock_screen),
            Some(Rect::from_min_max(
                Pos2::new(60.0, 100.0),
                Pos2::new(460.0, 400.0)
            ))
        );
        // Half off the frozen screen: only the visible half is clickable.
        assert_eq!(
            local_window_rect(&[100.0, 0.0, 1440.0, 600.0], Pos2::ZERO, clock_screen),
            Some(Rect::from_min_max(
                Pos2::new(100.0, 0.0),
                Pos2::new(1440.0, 600.0)
            ))
        );
        // Menubar strips and off-screen windows are not targets.
        assert_eq!(
            local_window_rect(&[0.0, 0.0, 1440.0, 24.0], Pos2::ZERO, clock_screen),
            None
        );
        assert_eq!(
            local_window_rect(&[2000.0, 0.0, 800.0, 600.0], Pos2::ZERO, clock_screen),
            None
        );
    }

    #[test]
    fn every_advertised_key_parses_back() {
        for key in [
            Key::A,
            Key::Z,
            Key::Num1,
            Key::Num0,
            Key::F12,
            Key::Space,
            Key::Enter,
            Key::ArrowUp,
            Key::Comma,
            Key::Backtick,
        ] {
            let shortcut = spec(
                key,
                Modifiers {
                    alt: true,
                    ..Default::default()
                },
            )
            .expect("a key with Alt");
            assert!(parse(&shortcut).is_some(), "{shortcut} should register");
        }
    }

    #[test]
    fn modifiers_come_first_and_at_least_one_is_required() {
        let both = spec(
            Key::S,
            Modifiers {
                mac_cmd: true,
                shift: true,
                ..Default::default()
            },
        )
        .expect("Command+Shift+S");
        assert_eq!(both, "Command+Shift+S");
        assert_eq!(token(Key::S), Some("S"));
        assert_eq!(
            spec(
                Key::S,
                Modifiers {
                    alt: false,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(parse("A"), None);
        assert_eq!(parse("Shift+"), None);
        assert_eq!(parse("A+Shift"), None);
    }

    #[test]
    fn modifier_keys_and_unmapped_keys_are_not_shortcuts() {
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        assert_eq!(spec(Key::ShiftLeft, alt), None);
        assert_eq!(spec(Key::Escape, alt), None);
    }

    #[test]
    fn the_default_matches_wechat() {
        assert_eq!(DEFAULT_HOTKEY, "Alt+A");
        assert!(parse(DEFAULT_HOTKEY).is_some());
    }

    /// 键名单和 Compose 端的 `ScreenshotHotkey.keys` 是同一份：两端各存各的设置，
    /// 但同一条「Alt+A」必须注册到同一个物理键上，所以这里把每个 token 都过一遍解析。
    #[test]
    fn every_advertised_token_name_registers() {
        let names = [
            "Space",
            "Enter",
            "Tab",
            "Backspace",
            "Delete",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "Minus",
            "Equal",
            "BracketLeft",
            "BracketRight",
            "Backslash",
            "Semicolon",
            "Quote",
            "Comma",
            "Period",
            "Slash",
            "Backquote",
        ];
        for name in names {
            let shortcut = format!("Alt+{name}");
            assert!(parse(&shortcut).is_some(), "{shortcut} should register");
        }
        for byte in b'A'..=b'Z' {
            let shortcut = format!("Alt+{}", char::from(byte));
            assert!(parse(&shortcut).is_some(), "{shortcut} should register");
        }
        for digit in '0'..='9' {
            let shortcut = format!("Alt+{digit}");
            assert!(parse(&shortcut).is_some(), "{shortcut} should register");
        }
        for number in 1..=12 {
            let shortcut = format!("Alt+F{number}");
            assert!(parse(&shortcut).is_some(), "{shortcut} should register");
        }
    }
}
