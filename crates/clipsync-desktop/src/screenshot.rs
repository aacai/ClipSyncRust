//! The global screenshot hotkey and the screen grab it triggers.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui::{Key, Modifiers, Pos2, Rect, Vec2};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::clipboard::image_hash;
use crate::overlay;

/// WeChat's default; the one users get without ever opening settings.
pub const DEFAULT_HOTKEY: &str = "Alt+A";

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
    let windows = xcap::Window::all()
        .map_err(|error| error.to_string())?
        .into_iter()
        // Our own windows never make good targets: the overlay is off screen but still in the
        // list, and picking it would offer a strip of last week's screenshot.
        .filter(|window| window.pid().ok() != Some(std::process::id()))
        .filter(|window| !window.is_minimized().unwrap_or(false))
        .filter_map(|window| {
            let left = window.x().ok()? as f32 - position.x;
            let top = window.y().ok()? as f32 - position.y;
            let right = left + window.width().ok()? as f32;
            let bottom = top + window.height().ok()? as f32;
            (right > left && bottom > top).then_some(Rect::from_min_max(
                Pos2::new(left, top),
                Pos2::new(right, bottom),
            ))
        })
        .collect();
    Ok(overlay::Freeze {
        image,
        position,
        size: Vec2::new(width, height),
        scale,
        windows,
    })
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
pub fn encode(frame: image::RgbaImage) -> Result<Shot, String> {
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

/// Without the grant macOS hands back wallpaper instead of an error, which users would
/// report as a broken screenshot rather than a missing permission.
#[cfg(target_os = "macos")]
pub fn permission_missing() -> bool {
    !unsafe { CGPreflightScreenCaptureAccess() }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

#[cfg(not(target_os = "macos"))]
pub fn permission_missing() -> bool {
    false
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

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    current: Option<HotKey>,
    pressed: Arc<AtomicBool>,
}

impl Hotkeys {
    /// macOS dispatches Carbon hotkeys on the main run loop, so the manager has to be
    /// built there — which is where eframe constructs the app.
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        let manager = GlobalHotKeyManager::new().ok()?;
        let pressed = Arc::new(AtomicBool::new(false));
        let flag = pressed.clone();
        let ctx = ctx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if matches!(event.state(), HotKeyState::Pressed) {
                flag.store(true, Ordering::Relaxed);
                // The idle loop only wakes once a second; a screenshot must feel instant.
                ctx.request_repaint();
            }
        }));
        Some(Self {
            manager,
            current: None,
            pressed,
        })
    }

    pub fn take_pressed(&self) -> bool {
        self.pressed.swap(false, Ordering::Relaxed)
    }

    pub fn apply(&mut self, spec: &str) -> Result<(), String> {
        let next = parse(spec).ok_or_else(|| format!("「{spec}」不是可用的快捷键"))?;
        if self.current == Some(next) {
            return Ok(());
        }
        // Register the new chord first: if it fails the old one stays live instead of
        // leaving the user with no hotkey at all.
        self.manager
            .register(next)
            .map_err(|_| format!("「{spec}」注册失败，换一个组合键试试"))?;
        if let Some(current) = self.current.replace(next) {
            let _ = self.manager.unregister(current);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, spec, token, DEFAULT_HOTKEY};
    use eframe::egui::{Key, Modifiers};

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
