mod app;
mod clipboard;
mod icons;
mod jobs;
mod paths;
mod settings;
mod store;
mod text_tools;
mod theme;
mod transfer;
mod translate;

use std::net::TcpListener;

use app::ClipSyncApp;
use clipsync_core::engine;
use clipsync_core::mqtt::BrokerLinks;
use settings::Settings;
use store::History;

const SINGLE_INSTANCE_PORT: u16 = 48124;

fn main() -> eframe::Result {
    let Ok(_single_instance) = TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) else {
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let engine = engine::spawn(BrokerLinks);
    let history = History::open(paths::app_dir().join("history.json"));
    let settings = Settings::open(paths::app_dir().join("settings.json"), &paths::install_id());

    eframe::run_native(
        "ClipSync",
        native_options(),
        Box::new(move |ctx| {
            theme::configure(&ctx.egui_ctx);
            Ok(Box::new(ClipSyncApp::new(engine, history, settings)))
        }),
    )
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
