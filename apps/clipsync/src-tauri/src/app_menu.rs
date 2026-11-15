//! macOS 应用菜单里那条「About ClipSync」：系统默认会弹自己的关于面板，
//! 这里把这一项改指向自绘的关于卡片，其余菜单项原样不动。
//!
//! 非 macOS 上是空实现，调用点不用写 `#[cfg]`。

use std::sync::OnceLock;

static HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn install(app: &tauri::AppHandle) -> bool {
    let _ = HANDLE.set(app.clone());
    imp::install()
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn install() -> bool {
        true
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, Sel};
    use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::NSObjectProtocol;

    thread_local! {
        static TARGET: RefCell<Option<Retained<AboutTarget>>> = const { RefCell::new(None) };
    }

    pub fn install() -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return true;
        };
        let app = NSApplication::sharedApplication(mtm);
        let Some(item) = app
            .mainMenu()
            .and_then(|menu| menu.itemAtIndex(0))
            .and_then(|entry| entry.submenu())
            .and_then(|menu| find_about(&menu))
        else {
            return false;
        };
        let target = AboutTarget::new(mtm);
        unsafe {
            item.setAction(Some(sel!(openClipSyncAbout:)));
            item.setTarget(Some(&target));
        }
        TARGET.with(|slot| *slot.borrow_mut() = Some(target));
        true
    }

    fn find_about(menu: &NSMenu) -> Option<Retained<NSMenuItem>> {
        (0..menu.numberOfItems()).find_map(|index| {
            let item = menu.itemAtIndex(index)?;
            is_about_action(item.action()).then_some(item)
        })
    }

    fn is_about_action(action: Option<Sel>) -> bool {
        action == Some(sel!(orderFrontStandardAboutPanel:))
    }

    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct AboutTarget;

        unsafe impl NSObjectProtocol for AboutTarget {}

        impl AboutTarget {
            #[unsafe(method(openClipSyncAbout:))]
            fn open_about(&self, _sender: Option<&AnyObject>) {
                if let Some(handle) = super::HANDLE.get() {
                    use tauri::Emitter;
                    let _ = handle.emit("clipsync://about", ());
                }
            }
        }
    );

    impl AboutTarget {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }
}
