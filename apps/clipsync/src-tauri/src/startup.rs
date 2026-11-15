//! 启动日志与 panic 兜底。Windows 的 GUI 子系统没有 stderr，失败得自己落盘再弹窗。

use std::io::Write;

const DIALOG_LIMIT: usize = 700;

pub fn log_path() -> Option<std::path::PathBuf> {
    Some(crate::paths::app_dir().join("startup.log"))
}

pub fn append(line: &str) {
    let Some(path) = log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// 启动失败：落盘 + 弹窗。Windows 上没有控制台，只能靠 MessageBoxW 告诉用户出了什么事。
pub fn fail(message: &str) {
    append(message);
    let text = format!(
        "ClipSync 启动失败 / Startup failed\n\n{}\n\n日志 / Log：{}",
        short(message),
        log_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "（未知）".to_string())
    );
    dialog(&text);
    #[cfg(not(windows))]
    eprintln!("{text}");
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = match info.payload().downcast_ref::<&str>() {
            Some(text) => (*text).to_string(),
            None => match info.payload().downcast_ref::<String>() {
                Some(text) => text.clone(),
                None => "未知 panic".to_string(),
            },
        };
        append(&format!("panic: {message}"));
        previous(info);
    }));
}

fn short(message: &str) -> String {
    if message.chars().count() <= DIALOG_LIMIT {
        return message.to_string();
    }
    let mut trimmed: String = message.chars().take(DIALOG_LIMIT).collect();
    trimmed.push('…');
    trimmed
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
        let caption = wide("ClipSync");
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
