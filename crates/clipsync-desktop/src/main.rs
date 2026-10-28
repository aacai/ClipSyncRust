// console 子系统会让 Windows 在 GUI 旁边多开一个黑窗口；测试二进制还要读 stdout，所以只关正式产物。
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app;
mod autostart;
mod clipboard;
mod drag_out;
mod icons;
mod jobs;
mod overlay;
mod paths;
mod preview;
mod record;
mod screenshot;
mod settings;
mod store;
mod text_tools;
mod theme;
mod transfer;
mod translate;
mod tray;
mod update;

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
    let listener = match TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(listener) => listener,
        Err(_) => {
            // 已有实例：敲一下回环端口，请它把窗口抬起来
            notify_existing_instance();
            return Ok(());
        }
    };
    let (show_tx, show_rx) = mpsc::channel();
    start_instance_server(listener, show_tx);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let engine = engine::spawn(BrokerLinks);
    let history = History::open(paths::app_dir().join("history.json"));
    let settings = Settings::open(paths::app_dir().join("settings.json"), &paths::install_id());
    let preferences = Preferences::open(paths::app_dir().join("preferences.json"));
    let tray = tray::Tray::try_install();

    eframe::run_native(
        "ClipSyncRust",
        native_options(),
        Box::new(move |ctx| {
            configure_shell(&ctx.egui_ctx, preferences.theme());
            Ok(Box::new(ClipSyncApp::new(
                engine,
                history,
                settings,
                preferences,
                screenshot::Hotkeys::new(&ctx.egui_ctx),
                show_rx,
                tray,
            )))
        }),
    )
}

fn configure_shell(ctx: &egui::Context, mode: ThemeMode) {
    theme::configure(ctx);
    theme::apply(ctx, mode);
    icons::install(ctx);
}

fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([920.0, 620.0])
            .with_min_inner_size([680.0, 460.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
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

fn notify_existing_instance() {
    if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        let _ = stream.write_all(&[1]);
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
