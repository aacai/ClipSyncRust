//! Windows 原生标题条染色：最小化/最大化/关闭留给系统，只把那条底色换成应用配色。
//!
//! 只用三个函数，直接 `extern` 声明，不引 `windows` crate 的 feature：DWM 的这两个属性
//! 就是「设一个 u32」，包一层类型换不来什么安全，倒省下依赖。
//! `DWMWA_CAPTION_COLOR` / `DWMWA_BORDER_COLOR` 是 Windows 11 才有，Win10 返回错误码，
//! 忽略即可——那边维持系统原样，总比画错颜色好。
//!
//! 非 Windows 上 `tint` 是空函数，模块照常参与编译：调用点不用写 `#[cfg]`，
//! 也就不会出现「本机全绿、别的平台编译不过」那种事。

use egui::Color32;

/// COLORREF 是 0x00BBGGRR，小端内存里正好按 B、G、R 排三个字节。
pub fn colorref(color: Color32) -> u32 {
    let [red, green, blue, _] = color.to_array();
    u32::from_le_bytes([red, green, blue, 0])
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use crate::win_caption::colorref;

    /// 主窗口标题，和 `main.rs` 传给 `run_native` 的名字一致；截图/录屏/预览窗各有
    /// 自己的标题，所以按标题找句柄只会命中主窗。
    const TITLE: &str = "ClipSyncRust";
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_CAPTION_COLOR: u32 = 35;
    const U32: u32 = std::mem::size_of::<u32>() as u32;
    const I32: u32 = std::mem::size_of::<i32>() as u32;

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(class_name: *const u16, window_name: *const u16) -> *mut c_void;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            window: *mut c_void,
            attribute: u32,
            value: *const c_void,
            value_size: u32,
        ) -> i32;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn tint(dark: bool) {
        let background = colorref(crate::theme::palette(dark).bg);
        let immersive_dark = i32::from(dark);
        unsafe {
            let title = wide(TITLE);
            let window = FindWindowW(std::ptr::null(), title.as_ptr());
            if window.is_null() {
                return;
            }
            let dark_ptr = &immersive_dark as *const i32 as *const c_void;
            let color_ptr = &background as *const u32 as *const c_void;
            DwmSetWindowAttribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE, dark_ptr, I32);
            DwmSetWindowAttribute(window, DWMWA_CAPTION_COLOR, color_ptr, U32);
            DwmSetWindowAttribute(window, DWMWA_BORDER_COLOR, color_ptr, U32);
        }
    }
}

#[cfg(windows)]
pub fn tint(dark: bool) {
    imp::tint(dark);
}

#[cfg(not(windows))]
pub fn tint(_dark: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_swaps_red_and_blue() {
        assert_eq!(colorref(Color32::from_rgb(0x12, 0x34, 0x56)), 0x0056_3412);
        assert_eq!(colorref(Color32::BLACK), 0);
        assert_eq!(colorref(Color32::WHITE) & 0xff_ff_ff, 0x00ff_ffff);
    }
}
