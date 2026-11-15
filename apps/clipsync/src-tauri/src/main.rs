#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Windows 上父进程盯着子进程，渲染崩了就带软件渲染重拉一次。
    if clipsync_tauri_lib::gpu::guard() {
        return;
    }
    clipsync_tauri_lib::run();
}
