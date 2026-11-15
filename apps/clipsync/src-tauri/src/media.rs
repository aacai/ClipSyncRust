//! 图片缩略图、大图预览与文件下载。

use base64::Engine;

use crate::paths;

fn scaled(image: &image::DynamicImage, max_edge: u32) -> image::DynamicImage {
    let (width, height) = (image.width(), image.height());
    let longest = width.max(height);
    if longest <= max_edge {
        return image.clone();
    }
    let ratio = max_edge as f32 / longest as f32;
    image.resize(
        max_edge,
        (height as f32 * ratio).round() as u32,
        image::imageops::FilterType::Lanczos3,
    )
}

fn encode_png_data_url(image: &image::DynamicImage) -> Result<String, String> {
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(format!("data:image/png;base64,{encoded}"))
}

#[tauri::command]
pub fn image_thumbnail_by_hash(hash: String, name: String, max_edge: Option<u32>) -> Result<String, String> {
    let path = crate::paths::cache_path(&hash, &name).ok_or_else(|| "本机没有这张图片的缓存".to_string())?;
    image_thumbnail(path.to_string_lossy().to_string(), max_edge)
}

#[tauri::command]
pub fn image_thumbnail(path: String, max_edge: Option<u32>) -> Result<String, String> {
    let image = image::open(&path).map_err(|err| err.to_string())?;
    let max = max_edge.unwrap_or(256);
    Ok(encode_png_data_url(&scaled(&image, max))?)
}

#[tauri::command]
pub fn image_preview(path: String, max_edge: Option<u32>) -> Result<String, String> {
    let image = image::open(&path).map_err(|err| err.to_string())?;
    let max = max_edge.unwrap_or(2048);
    Ok(encode_png_data_url(&scaled(&image, max))?)
}

#[tauri::command]
pub fn download_file(link: String, name: String) -> Result<String, String> {
    if !link.starts_with("http") {
        return Err("没有可用的下载链接".to_string());
    }
    let response = ureq::get(&link).call().map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    let safe = clipsync_core::files::sanitize_file_name(&name);
    let path = paths::write_cache("download", &safe, &bytes).map_err(|err| err.to_string())?;
    Ok(path)
}
