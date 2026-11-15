//! Windows 窗口边框染色：Win11 给无边框窗口也画一圈系统边框，把它换成应用配色。
//!
//! 只用 extern 声明，不引 `windows` crate：DWMWA 的这几个属性就是「设一个 u32」，
//! 包一层类型换不来什么安全，倒省下依赖。
//! `DWMWA_CAPTION_COLOR` / `DWMWA_BORDER_COLOR` 是 Windows 11 才有，Win10 返回错误码，
//! 忽略即可——那边维持系统原样，总比画错颜色好。

/// `#RRGGBB` → COLORREF。COLORREF 是 0x00BBGGRR，小端内存里正好按 B、G、R 排三个字节。
pub fn colorref(hex: &str) -> Option<u32> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(u32::from_le_bytes([red, green, blue, 0]))
}

/// 边框色跟着主题走。
#[tauri::command]
pub fn window_tint(app: tauri::AppHandle, color: String, dark: bool) -> Result<(), String> {
    use tauri::Manager;

    let value = colorref(&color).ok_or_else(|| "颜色格式不对".to_string())?;
    let Some(window) = app.get_webview_window("main") else {
        return Err("找不到主窗口".to_string());
    };
    imp::tint(&window, value, dark);
    Ok(())
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_CAPTION_COLOR: u32 = 35;
    const U32: u32 = std::mem::size_of::<u32>() as u32;
    const I32: u32 = std::mem::size_of::<i32>() as u32;

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            window: *mut c_void,
            attribute: u32,
            value: *const c_void,
            value_size: u32,
        ) -> i32;
    }

    pub fn tint(window: &tauri::WebviewWindow, color: u32, dark: bool) {
        let Ok(hwnd) = window.hwnd() else {
            return;
        };
        let handle = hwnd.0 as *mut c_void;
        let immersive = i32::from(dark);
        unsafe {
            DwmSetWindowAttribute(
                handle,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &immersive as *const i32 as *const c_void,
                I32,
            );
            DwmSetWindowAttribute(
                handle,
                DWMWA_BORDER_COLOR,
                &color as *const u32 as *const c_void,
                U32,
            );
            DwmSetWindowAttribute(
                handle,
                DWMWA_CAPTION_COLOR,
                &color as *const u32 as *const c_void,
                U32,
            );
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn tint(_window: &tauri::WebviewWindow, _color: u32, _dark: bool) {}
}

#[cfg(test)]
mod tests {
    use super::colorref;

    #[test]
    fn colorref_swaps_red_and_blue() {
        assert_eq!(colorref("#123456"), Some(0x0056_3412));
        assert_eq!(colorref("123456"), Some(0x0056_3412));
        assert_eq!(colorref("#000000"), Some(0));
        assert_eq!(colorref("#ffffff"), Some(0x00ff_ffff));
        assert_eq!(colorref("#fff"), None);
        assert_eq!(colorref("zzz"), None);
    }
}
