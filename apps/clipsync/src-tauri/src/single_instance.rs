//! 单实例守卫：已经有实例在跑就把它叫醒，然后自己退出。

use std::net::TcpListener;
use std::sync::mpsc::Sender;

const PORT: u16 = 48124;

pub fn claim_or_notify() -> bool {
    match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(listener) => {
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stream.is_ok() {
                        break;
                    }
                }
            });
            true
        }
        Err(_) => {
            let _ = std::net::TcpStream::connect(("127.0.0.1", PORT));
            false
        }
    }
}

pub fn spawn_wakeup(app: tauri::AppHandle, sender: Sender<()>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let reachable = tokio::net::TcpStream::connect(("127.0.0.1", PORT)).await.is_ok();
            if reachable {
                let _ = sender.send(());
                let _ = crate::tray::show_window(&app);
            }
        }
    });
}
