//! 菜单栏 / 系统托盘：关窗后进程继续守剪贴板，点托盘再把窗口抬起来。

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayCmd {
    Show,
    Quit,
}

pub struct Tray {
    _icon: TrayIcon,
    cmd_rx: Receiver<TrayCmd>,
}

impl Tray {
    pub fn try_install() -> Option<Self> {
        let icon = tray_rgba().ok()?;
        let show_item = MenuItem::new("显示窗口", true, None);
        let quit_item = MenuItem::new("退出", true, None);
        let menu = Menu::new();
        menu.append(&show_item).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit_item).ok()?;

        let show_id = show_item.id().clone();
        let quit_id = quit_item.id().clone();

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("ClipSyncRust")
            .with_icon(icon)
            .build()
            .ok()?;

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

/// 没有打包图标时现场画一颗小圆点，够托盘辨认即可。
fn tray_rgba() -> Result<Icon, tray_icon::BadIcon> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let cx = (SIZE as f32 - 1.0) * 0.5;
    let cy = cx;
    let r_outer = 12.0f32;
    let r_inner = 5.0f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let i = ((y * SIZE + x) * 4) as usize;
            if d <= r_inner {
                rgba[i] = 0xff;
                rgba[i + 1] = 0xff;
                rgba[i + 2] = 0xff;
                rgba[i + 3] = 0xff;
            } else if d <= r_outer {
                rgba[i] = 0x25;
                rgba[i + 1] = 0x63;
                rgba[i + 2] = 0xeb;
                rgba[i + 3] = 0xff;
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE)
}
