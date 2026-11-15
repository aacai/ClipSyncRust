//! macOS 窗口枚举：框选时把选区吸附到别的窗口上。

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn cs_snap_windows(out: *mut f64, max: i32) -> i32;
    fn cs_screen_capture_ok() -> i32;
}

/// 主显示器左上角在全局坐标里的位置（点）。
#[cfg(target_os = "macos")]
fn primary_origin() -> (f64, f64) {
    match crate::screenshot::primary_monitor() {
        Ok(monitor) => {
            let scale = monitor.scale_factor().unwrap_or(1.0).max(1.0) as f64;
            (
                monitor.x().unwrap_or(0) as f64 / scale,
                monitor.y().unwrap_or(0) as f64 / scale,
            )
        }
        Err(_) => (0.0, 0.0),
    }
}

#[cfg(not(target_os = "macos"))]
fn primary_origin() -> (f64, f64) {
    (0.0, 0.0)
}

/// 屏幕上别的应用窗口的位置（点，左上原点）。
#[cfg(target_os = "macos")]
pub fn snap() -> Vec<crate::picker::WindowRect> {
    const MAX: usize = 64;
    let mut buffer = [0.0f64; MAX * 4];
    let count = unsafe { cs_snap_windows(buffer.as_mut_ptr(), MAX as i32) };
    if count <= 0 {
        return Vec::new();
    }
    // 统一到主显示器原点，遮罩窗正好铺在主屏上，坐标才对得上。
    let (origin_x, origin_y) = primary_origin();
    (0..count as usize)
        .map(|index| crate::picker::WindowRect {
            x: buffer[index * 4] - origin_x,
            y: buffer[index * 4 + 1] - origin_y,
            w: buffer[index * 4 + 2],
            h: buffer[index * 4 + 3],
        })
        .collect()
}

/// 能不能读到别的窗口标题：有就说明屏幕录制权限已经给了。
#[cfg(target_os = "macos")]
pub fn screen_capture_ok() -> bool {
    unsafe { cs_screen_capture_ok() == 1 }
}

#[cfg(not(target_os = "macos"))]
pub fn snap() -> Vec<crate::picker::WindowRect> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
pub fn screen_capture_ok() -> bool {
    true
}
