//! Windows 渲染进程看门狗：显卡驱动 native 崩溃后自动降级到 DX12 重启。

use std::path::Path;
use std::process::{Command, ExitStatus};

const CHILD_FLAG: &str = "CLIPSYNC_RENDER_CHILD";
const BACKEND_ENV: &str = "WGPU_BACKEND";
const BACKEND_FILE: &str = "gpu-backend";
const DX12: &str = "dx12";
const NTSTATUS_FAULT: u32 = 0xC000_0000;
const BOTH_FAILED: &str = "图形后端启动失败 / Failed to start the graphics backend\n\
     Vulkan 与 DX12 都不可用，请升级显卡驱动 / \
     Both Vulkan and DX12 failed; update the GPU driver";

pub const RENDER_FAILED: i32 = 88;

pub fn guard() -> bool {
    if !cfg!(windows)
        || std::env::var_os(CHILD_FLAG).is_some()
        || std::env::var_os(BACKEND_ENV).is_some()
    {
        return false;
    }
    let Some(exe) = std::env::current_exe().ok() else {
        return false;
    };
    let backend = preferred_backend();
    let Some(status) = spawn(&exe, backend.as_deref()) else {
        return false;
    };
    if !crashed(status) {
        return true;
    }
    if backend.as_deref() == Some(DX12) {
        crate::startup::fail(BOTH_FAILED);
        return true;
    }
    remember(DX12);
    if spawn(&exe, Some(DX12)).is_some_and(crashed) {
        crate::startup::fail(BOTH_FAILED);
    }
    true
}

pub fn can_retry() -> bool {
    if !cfg!(windows) || std::env::var_os(CHILD_FLAG).is_none() {
        return false;
    }
    !matches!(std::env::var(BACKEND_ENV).as_deref(), Ok("dx12"))
}

pub fn using_fallback() -> bool {
    cfg!(windows)
        && std::env::var_os(CHILD_FLAG).is_some()
        && matches!(std::env::var(BACKEND_ENV).as_deref(), Ok("dx12"))
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

fn preferred_backend() -> Option<String> {
    let value = std::fs::read_to_string(crate::paths::app_dir().join(BACKEND_FILE)).ok()?;
    let value = value.trim().to_string();
    (value == DX12).then_some(value)
}

fn remember(backend: &str) {
    let _ = std::fs::write(crate::paths::app_dir().join(BACKEND_FILE), backend);
}

fn spawn(exe: &Path, backend: Option<&str>) -> Option<ExitStatus> {
    let mut command = Command::new(exe);
    command.env(CHILD_FLAG, "1");
    if let Some(backend) = backend {
        command.env(BACKEND_ENV, backend);
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
