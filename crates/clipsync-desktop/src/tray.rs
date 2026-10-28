//! 菜单栏 / 系统托盘：关窗后进程继续守剪贴板，点托盘再把窗口抬起来。

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::icons;

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
        // 直接用应用 logo：自己画的剪贴板底板和装完看到的图标不是同一个东西。
        // macOS 也不当模板图用——logo 是实心的，交给系统只会糊成一块方斑。
        let data = icons::tray_icon();
        let icon = Icon::from_rgba(data.rgba, data.width, data.height).ok()?;
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
