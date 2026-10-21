use std::path::{Path, PathBuf};

pub fn app_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ClipSync");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn install_id() -> String {
    let file = app_dir().join("install-id");
    if let Ok(existing) = std::fs::read_to_string(&file) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("os random unavailable");
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let id = uuid_from_bytes(&bytes);
    std::fs::write(&file, &id).ok();
    id
}

fn uuid_from_bytes(bytes: &[u8; 16]) -> String {
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn device_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "Desktop".to_string())
}

pub fn device_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    }
}

pub fn downloads_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub use clipsync_core::files::{numbered_name, sanitize_file_name};

pub fn cache_dir() -> PathBuf {
    app_dir().join("cache")
}

pub fn write_cache(id: &str, name: &str, bytes: &[u8]) -> std::io::Result<String> {
    let safe = sanitize_file_name(name);
    let name = if safe.is_empty() {
        "file"
    } else {
        safe.as_str()
    };
    let dir = cache_dir().join(id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path.to_string_lossy().to_string())
}

pub fn delete_cache(path: Option<&str>) {
    let Some(path) = path else {
        return;
    };
    let _ = std::fs::remove_file(path);
    if let Some(parent) = Path::new(path).parent() {
        if parent.starts_with(cache_dir()) {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

pub fn open_path(path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    command.arg(path).spawn()?;
    Ok(())
}

pub fn save_to_downloads(name: &str, bytes: &[u8]) -> std::io::Result<String> {
    let safe = sanitize_file_name(name);
    if safe.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "没有可用的文件名",
        ));
    }
    let dir = downloads_dir();
    std::fs::create_dir_all(&dir)?;
    let written = numbered_name(&safe, |candidate| dir.join(candidate).exists());
    std::fs::write(dir.join(&written), bytes)?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::{numbered_name, sanitize_file_name};

    #[test]
    fn file_names_cannot_escape_or_use_windows_reserved_words() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("C:\\tmp\\a:b?.txt"), "a_b_.txt");
        assert_eq!(sanitize_file_name("con.txt"), "_con.txt");
        assert_eq!(sanitize_file_name("  report.pdf...  "), "report.pdf");
    }

    #[test]
    fn duplicate_downloads_get_a_numbered_suffix() {
        let existing = ["a.txt", "a (1).txt"];
        assert_eq!(
            numbered_name("a.txt", |candidate| existing.contains(&candidate)),
            "a (2).txt"
        );
        assert_eq!(numbered_name("b", |candidate| candidate == "b (1)"), "b");
    }
}
