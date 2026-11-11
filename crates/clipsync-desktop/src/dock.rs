//! macOS Dock：窗口藏进托盘后点 Dock 图标应把窗口抬起来。
//!
//! winit 0.30 的 `WinitApplicationDelegate` 不实现
//! `applicationShouldHandleReopen:hasVisibleWindows:`，Dock 点击会被静默丢掉。
//! 不能替换整个 delegate（会拆掉 winit 的 run loop），只给现有类补这一个 selector。

pub fn install(ctx: egui::Context) {
    imp::install(ctx);
}

pub fn take_reopen() -> bool {
    imp::take_reopen()
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn install(_ctx: egui::Context) {}

    pub fn take_reopen() -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{ffi, sel};

    static INSTALLED: AtomicBool = AtomicBool::new(false);
    static REOPEN: AtomicBool = AtomicBool::new(false);
    static CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

    pub fn install(ctx: egui::Context) {
        *CTX.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ctx);
        if INSTALLED.swap(true, Ordering::Relaxed) {
            return;
        }
        // SAFETY: 只给 winit 已有的 delegate 类补一个尚未实现的实例方法；签名与
        // AppKit 文档一致，实现里只写原子标志并 request_repaint，不会 panic 穿过 ObjC 边界。
        unsafe {
            let Some(cls) = AnyClass::get(c"WinitApplicationDelegate") else {
                return;
            };
            let sel = sel!(applicationShouldHandleReopen:hasVisibleWindows:);
            if cls.instance_method(sel).is_some() {
                return;
            }
            let types = c"B@:@B";
            let imp: unsafe extern "C-unwind" fn(
                *mut AnyObject,
                Sel,
                *mut AnyObject,
                Bool,
            ) -> Bool = should_handle_reopen;
            let _ = ffi::class_addMethod(
                cls as *const AnyClass as *mut AnyClass,
                sel,
                std::mem::transmute::<
                    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool,
                    Imp,
                >(imp),
                types.as_ptr(),
            );
        }
    }

    pub fn take_reopen() -> bool {
        REOPEN.swap(false, Ordering::Relaxed)
    }

    unsafe extern "C-unwind" fn should_handle_reopen(
        _this: *mut AnyObject,
        _cmd: Sel,
        _app: *mut AnyObject,
        _has_visible: Bool,
    ) -> Bool {
        REOPEN.store(true, Ordering::Relaxed);
        if let Some(ctx) = CTX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            // 窗口 Visible(false) 时 eframe 不跑 update，不叫醒就永远读不到 REOPEN。
            ctx.request_repaint();
        }
        Bool::YES
    }
}
