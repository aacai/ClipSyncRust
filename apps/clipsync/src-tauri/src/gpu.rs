//! Windows 渲染看门狗：WebView2 被显卡驱动带崩时，自动退回软件渲染再拉一次。
//!
//! Tauri 不用 wgpu，没有 Vulkan/DX12 那套后端；Windows 上真正会崩的是 WebView2 的
//! 渲染进程。它的浏览器进程一挂，整个 webview 跟着走，所以只能在进程层面兜：
//! 父进程拉起子进程盯着退出码，判定是驱动级崩溃就带着 `--disable-gpu` 再拉一次。

use std::path::Path;
use std::process::{Command, ExitStatus};

const CHILD_FLAG: &str = "CLIPSYNC_RENDER_CHILD";
const FALLBACK_ENV: &str = "CLIPSYNC_GPU_FALLBACK";
const FALLBACK_FILE: &str = "gpu-backend";
const SOFTWARE: &str = "software";
const WEBVIEW_ARGS_ENV: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
const SOFTWARE_ARGS: &str = "--disable-gpu --disable-gpu-compositing";
const RENDER_FAILED: i32 = 88;
const NTSTATUS_FAULT: u32 = 0xC000_0000;

pub const BOTH_FAILED: &str = "渲染进程反复崩溃 / The render process keeps crashing\n\
     已退回软件渲染仍然起不来 / Software rendering did not help either\n\
     请升级显卡驱动 / Please update the GPU driver";

/// 父进程该接手时返回 true：已经拉起子进程，本进程可以退出。
pub fn guard() -> bool {
    if !cfg!(windows)
        || std::env::var_os(CHILD_FLAG).is_some()
        || std::env::var_os(FALLBACK_ENV).is_some()
    {
        return false;
    }
    let Some(exe) = std::env::current_exe().ok() else {
        return false;
    };
    let fallback = preferred_fallback();
    let Some(status) = spawn(&exe, fallback.as_deref()) else {
        return false;
    };
    if !crashed(status) {
        return true;
    }
    if fallback.as_deref() == Some(SOFTWARE) {
        crate::startup::fail(BOTH_FAILED);
        return true;
    }
    remember(SOFTWARE);
    if spawn(&exe, Some(SOFTWARE)).is_some_and(crashed) {
        crate::startup::fail(BOTH_FAILED);
    }
    true
}

/// 子进程：把退路告诉 WebView2，必须在窗口创建之前生效。
pub fn apply_env() {
    if std::env::var_os(FALLBACK_ENV).is_some() {
        std::env::set_var(WEBVIEW_ARGS_ENV, SOFTWARE_ARGS);
    }
}

pub fn using_fallback() -> bool {
    cfg!(windows)
        && std::env::var_os(CHILD_FLAG).is_some()
        && std::env::var_os(FALLBACK_ENV).is_some()
}

pub fn can_retry() -> bool {
    cfg!(windows)
        && std::env::var_os(CHILD_FLAG).is_some()
        && std::env::var_os(FALLBACK_ENV).is_none()
}

#[cfg(windows)]
pub fn disable_error_dialogs() {
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
    }

    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    }
}

#[cfg(not(windows))]
pub fn disable_error_dialogs() {}

fn preferred_fallback() -> Option<String> {
    let value = std::fs::read_to_string(crate::paths::app_dir().join(FALLBACK_FILE)).ok()?;
    let value = value.trim().to_string();
    (value == SOFTWARE).then_some(value)
}

fn remember(backend: &str) {
    let _ = std::fs::write(crate::paths::app_dir().join(FALLBACK_FILE), backend);
}

fn spawn(exe: &Path, fallback: Option<&str>) -> Option<ExitStatus> {
    let mut command = Command::new(exe);
    command.env(CHILD_FLAG, "1");
    if let Some(fallback) = fallback {
        command.env(FALLBACK_ENV, fallback);
    }
    command.spawn().ok()?.wait().ok()
}

fn crashed(status: ExitStatus) -> bool {
    fault_code(status.success(), status.code())
}

fn fault_code(success: bool, code: Option<i32>) -> bool {
    if success {
        return false;
    }
    match code {
        Some(code) => code == RENDER_FAILED || (code as u32) >= NTSTATUS_FAULT,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{fault_code, RENDER_FAILED};

    #[test]
    fn driver_faults_trigger_fallback() {
        assert!(fault_code(false, Some(0xC000_0005u32 as i32)));
        assert!(fault_code(false, Some(0xC000_0409u32 as i32)));
        assert!(fault_code(false, Some(RENDER_FAILED)));
        assert!(fault_code(false, None));
    }

    #[test]
    fn normal_exit_and_application_errors_do_not_restart() {
        assert!(!fault_code(true, Some(0)));
        assert!(!fault_code(false, Some(1)));
        assert!(!fault_code(false, Some(101)));
    }
}
