//! 截屏：抓主显示器的画面，编码成 PNG。

use image::ImageFormat;

/// 抓主显示器整屏，返回 PNG 字节。
pub fn capture_primary() -> Result<Vec<u8>, String> {
    let monitor = primary_monitor()?;
    let frame = monitor.capture_image().map_err(|error| error.to_string())?;
    let mut buffer = Vec::new();
    frame
        .write_to(&mut std::io::Cursor::new(&mut buffer), ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(buffer)
}

/// 主显示器句柄，录屏那边也要用。
pub fn primary_monitor() -> Result<xcap::Monitor, String> {
    let monitors = xcap::Monitor::all().map_err(|error| error.to_string())?;
    let index = monitors
        .iter()
        .position(|monitor| monitor.is_primary().unwrap_or(false))
        .unwrap_or(0);
    monitors
        .into_iter()
        .nth(index)
        .ok_or_else(|| "没有可用的显示器".to_string())
}
