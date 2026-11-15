//! 二维码生成。

use base64::Engine;

const QR_MAX_BYTES: usize = 2048;

fn qr_payload_for(text: &str, file_link: Option<&str>) -> Result<String, String> {
    if let Some(link) = file_link {
        let link = link.trim();
        if link.starts_with("http") {
            Ok(link.to_string())
        } else {
            Err("文件还没有下载链接（可能尚未上传完成）".to_string())
        }
    } else if text.trim().is_empty() {
        Err("没有可编码的内容".to_string())
    } else {
        Ok(text.to_string())
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn qr_code_png_for(text: String, file_link: Option<String>) -> Result<String, String> {
    let payload = qr_payload_for(&text, file_link.as_deref())?;
    qr_code_png(payload)
}

#[tauri::command]
pub fn qr_code_png(payload: String) -> Result<String, String> {
    if payload.as_bytes().len() > QR_MAX_BYTES {
        return Err("内容过长，无法生成可扫码的二维码".to_string());
    }
    let code = qrcode::QrCode::new(payload.as_bytes()).map_err(|err| err.to_string())?;
    let quiet = 2usize;
    let modules = code.width();
    let size = modules + quiet * 2;
    let scale = (240 / size).max(2);
    let px = size * scale;
    let mut pixels = vec![255u8; px * px * 3];
    for y in 0..modules {
        for x in 0..modules {
            if code[(x, y)] == qrcode::Color::Dark {
                let ox = (x + quiet) * scale;
                let oy = (y + quiet) * scale;
                for dy in 0..scale {
                    for dx in 0..scale {
                        let index = ((oy + dy) * px + (ox + dx)) * 3;
                        pixels[index] = 24;
                        pixels[index + 1] = 24;
                        pixels[index + 2] = 27;
                    }
                }
            }
        }
    }
    let mut png = Vec::new();
    image::RgbImage::from_raw(px as u32, px as u32, pixels)
        .ok_or_else(|| "生成二维码位图失败".to_string())?
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(format!("data:image/png;base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_png_data_url() {
        let data_url = qr_code_png("hello clipsync".to_string()).unwrap();
        assert!(data_url.starts_with("data:image/png;base64,"));
        let raw = base64::engine::general_purpose::STANDARD
            .decode(data_url.trim_start_matches("data:image/png;base64,"))
            .unwrap();
        assert_eq!(&raw[1..4], b"PNG");
        assert!(raw.len() > 100);
    }

    #[test]
    fn file_requires_http_link() {
        let err = qr_payload_for("ignored", Some("  ")).unwrap_err();
        assert_eq!(err, "文件还没有下载链接（可能尚未上传完成）");
        let err = qr_payload_for("ignored", Some("ftp://x")).unwrap_err();
        assert_eq!(err, "文件还没有下载链接（可能尚未上传完成）");
        let ok = qr_payload_for("ignored", Some("https://x.test/a.zip")).unwrap();
        assert_eq!(ok, "https://x.test/a.zip");
    }

    #[test]
    fn empty_text_is_rejected() {
        let err = qr_payload_for("   ", None).unwrap_err();
        assert_eq!(err, "没有可编码的内容");
        assert_eq!(qr_payload_for("hello", None).unwrap(), "hello");
    }

    #[test]
    fn rejects_oversized_payload() {
        let payload = "x".repeat(QR_MAX_BYTES + 1);
        assert!(qr_code_png(payload).is_err());
    }
}
