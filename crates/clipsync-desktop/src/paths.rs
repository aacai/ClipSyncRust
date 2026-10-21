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

pub fn sanitize_file_name(raw: &str) -> String {
    let base = raw.rsplit('/').next().unwrap_or("");
    let base = base.rsplit('\\').next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|ch| {
            if (ch as u32) < 0x20 || "\"*/:<>?\\|".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .chars()
        .take(120)
        .collect();
    let stem = cleaned
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(cleaned.as_str());
    const RESERVED: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if RESERVED.contains(&stem.to_lowercase().as_str()) {
        format!("_{cleaned}")
    } else {
        cleaned
    }
}

pub fn numbered_name(name: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    for index in 1..1000 {
        let candidate = match ext {
            Some(ext) => format!("{stem} ({index}).{ext}"),
            None => format!("{stem} ({index})"),
        };
        if !exists(&candidate) {
            return candidate;
        }
    }
    name.to_string()
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

pub fn downloads_path(name: &str) -> PathBuf {
    downloads_dir().join(name)
}

pub fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
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
