//! 启动失败的日志与提示：Windows 的 GUI 子系统没有 stderr，错误得自己落盘再弹窗。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

const DIALOG_LIMIT: usize = 700;

static LOG: OnceLock<PathBuf> = OnceLock::new();
static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn begin() {
    let _ = std::fs::write(
        log(),
        format!("ClipSyncRust {}\n", env!("CARGO_PKG_VERSION")),
    );
}

pub fn note(message: &str) {
    append(message);
}

pub fn fail(message: &str) {
    append(message);
    show(message);
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let message = format!(
            "panic：{info}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        if RUNNING.load(Ordering::Relaxed) {
            append(&message);
        } else {
            fail(&message);
        }
    }));
}

pub fn mark_running() {
    RUNNING.store(true, Ordering::Relaxed);
}

fn append(message: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log())
    {
        let _ = writeln!(file, "{message}");
    }
}

fn show(message: &str) {
    let text = format!(
        "ClipSyncRust 启动失败\n\n{}\n\n日志：{}",
        short(message),
        log().display()
    );
    dialog(&text);
    #[cfg(not(windows))]
    eprintln!("{text}");
}

fn short(message: &str) -> String {
    if message.chars().count() <= DIALOG_LIMIT {
        return message.to_string();
    }
    let mut trimmed: String = message.chars().take(DIALOG_LIMIT).collect();
    trimmed.push('…');
    trimmed
}

fn log() -> &'static Path {
    LOG.get_or_init(|| {
        let preferred = crate::paths::app_dir().join("startup.log");
        if writable(&preferred) {
            return preferred;
        }
        let fallback = std::env::temp_dir().join("ClipSyncRust-startup.log");
        let _ = writable(&fallback);
        fallback
    })
}

fn writable(path: &Path) -> bool {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .is_ok()
}

fn dialog(text: &str) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;

        const MB_ICONERROR: u32 = 0x0000_0010;
        const MB_SETFOREGROUND: u32 = 0x0001_0000;

        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(
                owner: *mut c_void,
                text: *const u16,
                caption: *const u16,
                style: u32,
            ) -> i32;
        }

        let text = wide(text);
        let caption = wide("ClipSyncRust");
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_ICONERROR | MB_SETFOREGROUND,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = text;
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::short;

    #[test]
    fn only_long_dialog_messages_get_trimmed() {
        assert_eq!(short("boom"), "boom");
        let long = "x".repeat(1000);
        let trimmed = short(&long);
        assert_eq!(trimmed.chars().count(), 701);
        assert!(trimmed.ends_with('…'));
    }
}
