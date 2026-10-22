//! The global screenshot hotkey and the screen grab it triggers.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui::{self, Key, Modifiers};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::clipboard::image_hash;

/// WeChat's default; the one users get without ever opening settings.
pub const DEFAULT_HOTKEY: &str = "Alt+A";

pub struct Shot {
    pub png: Vec<u8>,
    pub hash: String,
}

pub fn capture() -> Result<Shot, String> {
    let monitors = xcap::Monitor::all().map_err(|error| error.to_string())?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or("没有可用的显示器")?;
    let frame = monitor.capture_image().map_err(|error| error.to_string())?;
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
        if let Some(current) = self.current.take() {
            let _ = self.manager.unregister(current);
        }
        self.manager
            .register(next)
            .map_err(|error| error.to_string())?;
        self.current = Some(next);
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
}
