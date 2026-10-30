//! macOS 应用菜单里那条「About ClipSyncRust」：winit 默认把它接到系统的
//! `orderFrontStandardAboutPanel:`，弹的是系统面板。这里只改这一项的 action/target，
//! 让它去开我们自己的关于卡片，其余菜单项原样不动。改不到的话下一帧再试。
//!
//! `setTarget:` 不持有目标，所以换完得自己把那个对象留着。
//!
//! 非 macOS 上两个入口都是空实现，调用点不用写 `#[cfg]`。

pub fn install() -> bool {
    imp::install()
}

pub fn take_request() -> bool {
    imp::take_request()
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn install() -> bool {
        true
    }

    pub fn take_request() -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, Sel};
    use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::NSObjectProtocol;

    static REQUESTED: AtomicBool = AtomicBool::new(false);

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

    pub fn take_request() -> bool {
        REQUESTED.swap(false, Ordering::Relaxed)
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
        /// 「关于」这一项的落地对象，被点时只举一次旗。
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct AboutTarget;

        unsafe impl NSObjectProtocol for AboutTarget {}

        impl AboutTarget {
            #[unsafe(method(openClipSyncAbout:))]
            fn open_about(&self, _sender: Option<&AnyObject>) {
                REQUESTED.store(true, Ordering::Relaxed);
            }
        }
    );

    impl AboutTarget {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_the_standard_about_action_is_repointed() {
            assert!(is_about_action(Some(sel!(orderFrontStandardAboutPanel:))));
            assert!(!is_about_action(Some(sel!(terminate:))));
            assert!(!is_about_action(None));
        }

        #[test]
        fn a_request_is_handed_over_once() {
            REQUESTED.store(true, Ordering::Relaxed);
            assert!(take_request());
            assert!(!take_request());
        }
    }
}
