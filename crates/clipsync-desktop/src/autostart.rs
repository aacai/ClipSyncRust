use std::path::{Path, PathBuf};

pub const HIDDEN_ARGUMENT: &str = "--hidden";

pub fn is_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        app_bundle().is_some()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::current_exe()
            .map(|path| !path.to_string_lossy().contains("/target/"))
            .unwrap_or(false)
    }
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let bundle = app_bundle().ok_or_else(|| "启动器不在 .app 包里".to_string())?;
        if enabled {
            write_launch_agent(&bundle)
        } else {
            remove_launch_agent(&bundle)
        }
    }
    #[cfg(target_os = "windows")]
    {
        registry(enabled)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        autostart_entry(enabled)
    }
}

#[cfg(target_os = "macos")]
fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn launch_agent_file(bundle: &Path) -> Option<PathBuf> {
    let label = agent_label(bundle)?;
    Some(
        dirs::home_dir()?
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist")),
    )
}

#[cfg(target_os = "macos")]
fn write_launch_agent(bundle: &Path) -> Result<(), String> {
    let label = agent_label(bundle).ok_or_else(|| "认不出 .app 的名字".to_string())?;
    let agent = launch_agent_file(bundle).ok_or_else(|| "找不到用户目录".to_string())?;
    let text = launch_agent_xml(&label, &bundle.to_string_lossy());
    if let Some(parent) = agent.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if std::fs::read_to_string(&agent).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&agent, text).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_launch_agent(bundle: &Path) -> Result<(), String> {
    let Some(agent) = launch_agent_file(bundle) else {
        return Ok(());
    };
    if agent.exists() {
        std::fs::remove_file(&agent).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn agent_label(bundle: &Path) -> Option<String> {
    let name = bundle.file_name()?.to_string_lossy();
    let label: String = name
        .trim_end_matches(".app")
        .chars()
        .filter(|ch| ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        .collect();
    (!label.is_empty()).then_some(label)
}

pub fn launch_agent_xml(label: &str, bundle_path: &str) -> String {
    let arguments = [
        "/usr/bin/open",
        "-a",
        bundle_path,
        "--args",
        HIDDEN_ARGUMENT,
    ]
    .iter()
    .map(|argument| format!("\t\t<string>{}</string>", escape_xml(argument)))
    .collect::<Vec<_>>()
    .join("\n");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{}</string>
	<key>ProgramArguments</key>
	<array>
{}
	</array>
	<key>RunAtLoad</key>
	<true/>
</dict>
</plist>
"#,
        escape_xml(label),
        arguments
    )
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(target_os = "windows")]
fn registry(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let name = exe
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "ClipSync".to_string());
    let key = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run";
    let status = if enabled {
        std::process::Command::new("reg")
            .args([
                "add",
                key,
                "/v",
                &name,
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{}\" {HIDDEN_ARGUMENT}", exe.to_string_lossy()),
                "/f",
            ])
            .status()
    } else {
        std::process::Command::new("reg")
            .args(["delete", key, "/v", &name, "/f"])
            .status()
    };
    match status {
        Ok(status) if !enabled || status.success() => Ok(()),
        Ok(status) => Err(format!("reg 退出码 {status}")),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn autostart_entry(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let name = exe
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "ClipSync".to_string());
    let directory = dirs::home_dir()
        .ok_or_else(|| "找不到用户目录".to_string())?
        .join(".config/autostart");
    let entry = directory.join(format!("{}.desktop", name.to_lowercase()));
    if !enabled {
        if entry.exists() {
            std::fs::remove_file(&entry).map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    std::fs::write(&entry, desktop_entry(&name, &exe.to_string_lossy()))
        .map_err(|error| error.to_string())
}

pub fn desktop_entry(name: &str, exec_path: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={} {HIDDEN_ARGUMENT}\nTerminal=false\n",
        name,
        quote_exec(exec_path)
    )
}

fn quote_exec(path: &str) -> String {
    if path
        .chars()
        .any(|ch| ch.is_whitespace() || ch == '"' || ch == '\\')
    {
        format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{agent_label, desktop_entry, launch_agent_xml, HIDDEN_ARGUMENT};
    use std::path::Path;

    #[test]
    fn the_launch_agent_opens_the_bundle_hidden() {
        let xml = launch_agent_xml("ClipSync", "/Applications/ClipSync.app");
        assert!(xml.contains("<string>ClipSync</string>"));
        assert!(xml.contains("<string>/usr/bin/open</string>"));
        assert!(xml.contains("<string>-a</string>"));
        assert!(xml.contains("<string>/Applications/ClipSync.app</string>"));
        assert!(xml.contains(&format!("<string>{HIDDEN_ARGUMENT}</string>")));
        assert!(xml.contains("<key>RunAtLoad</key>\n\t<true/>"));
    }

    #[test]
    fn labels_and_paths_are_sanitised_and_escaped() {
        assert_eq!(
            agent_label(Path::new("/Applications/Clip Sync.app")).as_deref(),
            Some("ClipSync")
        );
        assert_eq!(agent_label(Path::new("/")), None);
        let xml = launch_agent_xml("A&B", "/tmp/<app>.app");
        assert!(xml.contains("<string>A&amp;B</string>"));
        assert!(xml.contains("/tmp/&lt;app&gt;.app"));
    }

    #[test]
    fn the_linux_entry_quotes_paths_with_spaces() {
        let entry = desktop_entry("ClipSync", "/opt/Clip Sync/clipsync");
        assert!(entry.contains("Exec=\"/opt/Clip Sync/clipsync\" --hidden"));
        assert!(desktop_entry("ClipSync", "/usr/bin/clipsync")
            .contains("Exec=/usr/bin/clipsync --hidden"));
    }
}
