//! 录屏。macOS 走 `native/record.m` 的 ScreenCaptureKit，其它平台先给个明白话。

use std::ffi::{CStr, CString};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn cs_record_start(
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        mic: i32,
        system_audio: i32,
        fps: i32,
        long_edge: i32,
        mic_device: *const i8,
        path: *const i8,
    ) -> i32;
    fn cs_record_stop() -> i32;
    fn cs_record_exclude_self() -> i32;
    fn cs_record_inputs() -> *const i8;
    fn cs_record_last_error() -> *const i8;
}

/// 一块可选的麦克风。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicInput {
    pub id: String,
    pub name: String,
}

pub const AUDIO_SUPPORTED: bool = cfg!(target_os = "macos");

/// 开录参数：坐标是主屏左上角为原点的点，`w`/`h` 小于等于 1 时录整个主屏。
#[derive(Clone, Debug)]
pub struct StartSpec {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub mic: bool,
    pub system_audio: bool,
    pub fps: i32,
    pub long_edge: i32,
    pub mic_device: String,
    pub path: PathBuf,
}

/// 开录。`w`/`h` 小于等于 1 时录整个主屏。
pub fn start(spec: &StartSpec) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let text = spec.path.to_string_lossy();
        let c = CString::new(text.as_ref()).map_err(|_| "路径里有空字节".to_string())?;
        let device = if spec.mic
            && !spec.mic_device.is_empty()
            && !inputs().iter().any(|item| item.id == spec.mic_device)
        {
            String::new()
        } else {
            spec.mic_device.clone()
        };
        let device = CString::new(device).map_err(|_| "设备名里有空字节".to_string())?;
        let code = unsafe {
            cs_record_start(
                spec.x,
                spec.y,
                spec.w,
                spec.h,
                i32::from(spec.mic),
                i32::from(spec.system_audio),
                spec.fps,
                spec.long_edge,
                device.as_ptr(),
                c.as_ptr(),
            )
        };
        if code == 0 {
            Ok(())
        } else {
            Err(last_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        crate::record_portable::start(crate::record_portable::PortableSpec {
            x: spec.x,
            y: spec.y,
            w: spec.w,
            h: spec.h,
            fps: spec.fps,
            long_edge: spec.long_edge,
            path: spec.path.clone(),
        })
    }
}

/// 收尾，文件留在盘上。
pub fn stop() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let code = unsafe { cs_record_stop() };
        if code == 0 {
            Ok(())
        } else {
            Err(last_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        crate::record_portable::stop()
    }
}

/// 重新排除自己的窗口。
pub fn exclude_self() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if unsafe { cs_record_exclude_self() } == 0 {
            Ok(())
        } else {
            Err(last_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// 本机可用的麦克风。
pub fn inputs() -> Vec<MicInput> {
    #[cfg(target_os = "macos")]
    {
        let ptr = unsafe { cs_record_inputs() };
        if ptr.is_null() {
            return Vec::new();
        }
        let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
        let mut out = Vec::new();
        if let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            for item in list {
                out.push(MicInput {
                    id: item
                        .get("id")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    name: item
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }
        out
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// MP4 顶层 box 里必须有 moov 才算收尾完成。
pub fn is_finished(path: &Path) -> bool {
    std::fs::File::open(path).is_ok_and(|mut file| has_moov(&mut file))
}

fn has_moov<R: Read + Seek>(reader: &mut R) -> bool {
    let len = match reader.seek(SeekFrom::End(0)) {
        Ok(len) => len,
        Err(_) => return false,
    };
    let mut offset: u64 = 0;
    let mut head = [0u8; 8];
    while offset + 8 <= len {
        if reader.seek(SeekFrom::Start(offset)).is_err() || reader.read_exact(&mut head).is_err() {
            return false;
        }
        let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as u64;
        if &head[4..8] == b"moov" {
            return true;
        }
        let step = match size {
            0 => return false,
            1 => {
                let mut large = [0u8; 8];
                if reader.read_exact(&mut large).is_err() {
                    return false;
                }
                u64::from_be_bytes(large)
            }
            size => size,
        };
        if step < 8 {
            return false;
        }
        match offset.checked_add(step) {
            Some(next) => offset = next,
            None => return false,
        }
    }
    false
}

fn last_error() -> String {
    #[cfg(target_os = "macos")]
    {
        let ptr = unsafe { cs_record_last_error() };
        if ptr.is_null() {
            return "录屏失败".to_string();
        }
        let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
        if text.is_empty() {
            "录屏失败".to_string()
        } else {
            text.into_owned()
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        "这个平台的录屏还没接上".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{has_moov, is_finished};
    use std::io::Cursor;
    use std::path::PathBuf;

    fn box_of(kind: &[u8; 4], payload: usize) -> Vec<u8> {
        let size = 8 + payload;
        let mut out = (size as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend(std::iter::repeat_n(0u8, payload));
        out
    }

    #[test]
    fn a_moov_box_means_the_file_is_finished() {
        let mut bytes = box_of(b"ftyp", 8);
        bytes.extend(box_of(b"mdat", 32));
        bytes.extend(box_of(b"moov", 64));
        assert!(has_moov(&mut Cursor::new(bytes)));
    }

    #[test]
    fn a_file_without_moov_is_only_a_draft() {
        let bytes = box_of(b"ftyp", 8);
        assert!(!has_moov(&mut Cursor::new(bytes)));
    }

    #[test]
    fn a_nested_moov_tag_is_not_a_top_level_box() {
        let mut bytes = box_of(b"mdat", 16);
        bytes.extend_from_slice(b"moov");
        assert!(!has_moov(&mut Cursor::new(bytes)));
    }

    #[test]
    fn a_missing_file_is_not_finished() {
        let path = PathBuf::from("/tmp/clipsync-does-not-exist.mp4");
        assert!(!is_finished(&path));
    }
}
