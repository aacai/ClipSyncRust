// console 子系统会让 Windows 在 GUI 旁边多开一个黑窗口；测试二进制还要读 stdout，所以只关正式产物。
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app;
mod app_menu;
mod autostart;
mod clipboard;
mod drag_out;
mod icons;
mod jobs;
mod meminfo;
mod overlay;
mod paths;
mod preview;
mod record;
mod screenshot;
mod settings;
mod startup;
mod store;
mod text_tools;
mod theme;
mod transfer;
mod translate;
mod tray;
mod update;
mod win_caption;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use app::ClipSyncApp;
use clipsync_core::engine;
use clipsync_core::mqtt::BrokerLinks;
use settings::{Preferences, Settings, ThemeMode};
use store::History;

const SINGLE_INSTANCE_PORT: u16 = 48124;

fn main() -> eframe::Result {
    startup::begin();
    startup::install_panic_hook();

    let listener = match TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(listener) => Some(listener),
        Err(error) => {
            if error.kind() == std::io::ErrorKind::AddrInUse && notify_existing_instance() {
                startup::note("已有实例在运行，这次启动只负责唤醒它");
                return Ok(());
            }
            startup::note(&format!(
                "单实例端口 {SINGLE_INSTANCE_PORT} 不可用（{error}），这次照常启动"
            ));
            None
        }
    };
    let (show_tx, show_rx) = mpsc::channel();
    if let Some(listener) = listener {
        start_instance_server(listener, show_tx);
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        // 异步这边只有 broker 连接和文件收发，默认按逻辑核数开 worker 白占栈。
        .worker_threads(2)
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let engine = engine::spawn(BrokerLinks);
    let history = History::open(paths::app_dir().join("history.json"));
    let settings = Settings::open(paths::app_dir().join("settings.json"), &paths::install_id());
    let preferences = Preferences::open(paths::app_dir().join("preferences.json"));

    let result = eframe::run_native(
        "ClipSyncRust",
        native_options(),
        Box::new(move |ctx| {
            let dark = configure_shell(&ctx.egui_ctx, preferences.theme());
            // Windows 那条原生标题条得跟着主题色走，放在主题定下来之后。
            win_caption::tint(dark);
            let tray = tray::Tray::try_install();
            let app = ClipSyncApp::new(
                engine,
                history,
                settings,
                preferences,
                screenshot::Hotkeys::new(&ctx.egui_ctx),
                show_rx,
                tray,
            );
            startup::mark_running();
            Ok(Box::new(app))
        }),
    );
    if let Err(error) = &result {
        startup::fail(&format!(
            "窗口创建失败：{error}\n多半是显卡驱动或 OpenGL 不可用"
        ));
    }
    result
}

fn configure_shell(ctx: &egui::Context, mode: ThemeMode) -> bool {
    theme::configure(ctx);
    let dark = theme::apply(ctx, mode);
    icons::install(ctx);
    dark
}

fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([920.0, 620.0])
            .with_min_inner_size([680.0, 460.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_icon(icons::app_icon()),
        ..Default::default()
    }
}

fn start_instance_server(listener: TcpListener, show_tx: mpsc::Sender<()>) {
    thread::Builder::new()
        .name("clipsync-instance".into())
        .spawn(move || {
            listener.set_nonblocking(false).ok();
            while let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1];
                let _ = stream.read(&mut buf);
                let _ = show_tx.send(());
            }
        })
        .ok();
}

fn notify_existing_instance() -> bool {
    match TcpStream::connect(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(mut stream) => stream.write_all(&[1]).is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A panic here kills the process outright: the startup closure runs inside winit's
    // `extern "C"` launch callback, so it aborts with an empty faulting stack.
    #[test]
    fn startup_style_renders_both_themes() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let ctx = egui::Context::default();
            configure_shell(&ctx, mode);
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.label("Aa09 中文");
                theme::frost(
                    ui.ctx(),
                    ui.max_rect(),
                    theme::GLASS_RADIUS,
                    theme::frost_layer(),
                );
            });
            output.textures_delta.clear();
        }
    }
}
