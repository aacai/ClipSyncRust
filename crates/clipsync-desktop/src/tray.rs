//! 菜单栏 / 系统托盘：关窗后进程继续守剪贴板，点托盘再把窗口抬起来。

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui::Color32;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::icons;
use crate::theme;

pub enum TrayCmd {
    Show,
    Quit,
}

pub struct Tray {
    _icon: TrayIcon,
    cmd_rx: Receiver<TrayCmd>,
}

impl Tray {
    pub fn try_install(dark: bool) -> Option<Self> {
        let icon = tray_icon(dark).ok()?;
        let show_item = MenuItem::new("显示窗口", true, None);
        let quit_item = MenuItem::new("退出", true, None);
        let menu = Menu::new();
        menu.append(&show_item).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit_item).ok()?;

        let show_id = show_item.id().clone();
        let quit_id = quit_item.id().clone();

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("ClipSyncRust")
            .with_icon(icon.clone());
        // `with_icon_templated` 只在 macOS 上存在，其他平台被 cfg 掉了：菜单栏把图标
        // 当模板看，只认 alpha 并按外观上色；Windows / Linux 没有这套机制。
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon);

        let tray = builder.build().ok()?;

        let (cmd_tx, cmd_rx) = mpsc::channel();
        spawn_menu_pump(cmd_tx.clone(), show_id, quit_id);
        spawn_click_pump(cmd_tx);

        Some(Self {
            _icon: tray,
            cmd_rx,
        })
    }

    pub fn poll(&self) -> Option<TrayCmd> {
        self.cmd_rx.try_recv().ok()
    }
}

fn spawn_menu_pump(
    tx: Sender<TrayCmd>,
    show_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
) {
    thread::Builder::new()
        .name("clipsync-tray-menu".into())
        .spawn(move || {
            while let Ok(event) = MenuEvent::receiver().recv() {
                if event.id == show_id {
                    let _ = tx.send(TrayCmd::Show);
                } else if event.id == quit_id {
                    let _ = tx.send(TrayCmd::Quit);
                }
            }
        })
        .ok();
}

fn spawn_click_pump(tx: Sender<TrayCmd>) {
    thread::Builder::new()
        .name("clipsync-tray-click".into())
        .spawn(move || {
            while let Ok(event) = TrayIconEvent::receiver().recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let _ = tx.send(TrayCmd::Show);
                }
            }
        })
        .ok();
}

/// 托盘用应用自己的剪贴板轮廓，和界面里 `Icon::Clipboard` 同一份 path 数据。
/// macOS 交模板图给系统上色；其他平台压在品牌色底上，免得单色轮廓在深色任务栏里整颗消失。
fn tray_icon(dark: bool) -> Result<Icon, tray_icon::BadIcon> {
    let data = if cfg!(target_os = "macos") {
        let tint = if dark {
            Color32::WHITE
        } else {
            Color32::from_rgb(0x1a, 0x1a, 0x1a)
        };
        icons::raster(icons::Icon::Clipboard, tint)
    } else {
        icons::plate(icons::Icon::Clipboard, theme::palette(dark).accent)
    };
    Icon::from_rgba(data.rgba, data.width, data.height)
}
