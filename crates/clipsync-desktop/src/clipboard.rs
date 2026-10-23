use std::collections::VecDeque;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use arboard::Clipboard;
#[cfg(not(target_os = "macos"))]
use arboard::ImageData;
use clipsync_core::crypto::sha256_hex;
use clipsync_core::files::MAX_FILE_BYTES;

pub const POLL_INTERVAL: Duration = Duration::from_millis(800);

/// How long a text we broadcast stays "ours". Long enough that the room's reply can no
/// longer overwrite a copy the user just made, short enough that copying the same text
/// again five minutes later is still a real clip.
const ECHO_WINDOW: Duration = Duration::from_secs(10);

fn meaningful(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

/// Decides which clipboard reads are real copies the local user made.
///
/// The same text can reach us three ways: the user copied it, we wrote it back after
/// an incoming clip, or we published it from the compose box. Only the first is worth
/// re-broadcasting; the others must be suppressed or the room loops on itself.
///
/// Outbound texts are remembered as a short window rather than as one slot: another
/// ClipSync on the same machine treats the clip we sent as a fresh copy and pushes it
/// back, and that echo has to be dropped without forgetting what the user copied since.
#[derive(Default)]
pub struct CopyFilter {
    captured: Option<String>,
    own_write: Option<String>,
    outbound: VecDeque<(String, Instant)>,
}

impl CopyFilter {
    /// Seed with whatever is already on the clipboard so startup does not publish it.
    pub fn new(baseline: Option<String>) -> Self {
        Self {
            captured: meaningful(baseline),
            ..Default::default()
        }
    }

    /// Some(text) when this is a new copy we have not already broadcast.
    pub fn observe(&mut self, text: Option<String>) -> Option<String> {
        let text = meaningful(text)?;
        if Some(&text) == self.captured.as_ref()
            || Some(&text) == self.own_write.as_ref()
            || self.echoed(&text)
        {
            return None;
        }
        self.captured = Some(text.clone());
        Some(text)
    }

    pub fn wrote(&mut self, text: &str) {
        self.own_write = Some(text.to_string());
    }

    pub fn published(&mut self, text: &str) {
        self.remember(text, Instant::now());
    }

    /// True while `text` is something this device put on the wire moments ago.
    pub fn echoed(&self, text: &str) -> bool {
        self.outbound
            .iter()
            .any(|(sent, at)| sent == text && at.elapsed() < ECHO_WINDOW)
    }

    fn remember(&mut self, text: &str, at: Instant) {
        self.outbound
            .retain(|(_, sent_at)| sent_at.elapsed() < ECHO_WINDOW);
        self.outbound.push_back((text.to_string(), at));
        while self.outbound.len() > 32 {
            self.outbound.pop_front();
        }
    }

    fn mark_seen(&mut self, text: Option<String>) {
        if let Some(text) = meaningful(text) {
            self.captured = Some(text);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipFile {
    pub path: PathBuf,
    pub name: String,
    pub size: i64,
}

impl ClipFile {
    pub fn mime(&self) -> &'static str {
        clipsync_core::files::mime_for_name(&self.name)
    }
}

#[derive(Debug, Clone)]
pub struct ClipImage {
    pub png: Vec<u8>,
    pub hash: String,
    pub name: String,
}

impl ClipImage {
    pub fn mime(&self) -> &'static str {
        "image/png"
    }
}

pub enum Captured {
    Text(String),
    Files(Vec<ClipFile>),
    Image(ClipImage),
}

pub struct ClipboardWatcher {
    clipboard: Option<Clipboard>,
    filter: CopyFilter,
    last_files: Option<String>,
    own_files: Option<String>,
    last_image: Option<String>,
    own_image: Option<String>,
}

impl ClipboardWatcher {
    pub fn new() -> Self {
        let mut clipboard = Clipboard::new().ok();
        let baseline = clipboard.as_mut().and_then(|clip| clip.get_text().ok());
        let last_files = clipboard.as_mut().and_then(file_signature);
        let last_image = clipboard.as_mut().and_then(image_fingerprint);
        Self {
            clipboard,
            filter: CopyFilter::new(baseline),
            last_files,
            own_files: None,
            last_image,
            own_image: None,
        }
    }

    /// Files first: Finder also puts a text path on the pasteboard, and treating that
    /// as text would sync the path instead of the file.
    pub fn poll(&mut self, watch_files: bool, watch_images: bool) -> Option<Captured> {
        let clipboard = self.clipboard.as_mut()?;

        if watch_files {
            let files = read_files(clipboard);
            if !files.is_empty() {
                let signature = signature_of(&files);
                if Some(&signature) == self.last_files.as_ref()
                    || Some(&signature) == self.own_files.as_ref()
                {
                    return None;
                }
                self.last_files = Some(signature);
                return Some(Captured::Files(files));
            }
        }

        let text = clipboard.get_text().ok();
        if text.as_ref().is_some_and(|text| !text.trim().is_empty()) {
            return self.filter.observe(text).map(Captured::Text);
        }
        self.filter.mark_seen(text);

        if watch_images {
            let image = read_image(clipboard)?;
            if Some(&image.hash) == self.last_image.as_ref()
                || Some(&image.hash) == self.own_image.as_ref()
            {
                return None;
            }
            self.last_image = Some(image.hash.clone());
            return Some(Captured::Image(image));
        }
        None
    }

    /// The current clipboard text, for an explicit "send clipboard" action.
    pub fn read_once(&mut self) -> Option<String> {
        let text = meaningful(Some(self.clipboard.as_mut()?.get_text().ok()?))?;
        self.filter.captured = Some(text.clone());
        Some(text)
    }

    /// Re-read the pasteboard as the new baseline, so turning watching back on does not
    /// replay whatever changed while nobody was looking.
    pub fn reseed(&mut self) {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return;
        };
        self.filter.mark_seen(clipboard.get_text().ok());
        self.last_files = file_signature(clipboard);
        self.last_image = image_fingerprint(clipboard);
        self.own_files = None;
        self.own_image = None;
    }

    pub fn write(&mut self, text: &str) {
        self.filter.wrote(text);
        if let Some(clipboard) = self.clipboard.as_mut() {
            let _ = clipboard.set_text(text);
        }
    }

    pub fn note_published(&mut self, text: &str) {
        self.filter.published(text);
    }

    /// A clip we already put on the wire or on the pasteboard: writing it back would only
    /// bury whatever the user copied since.
    pub fn is_echo(&self, text: &str) -> bool {
        self.filter.echoed(text) || self.filter.own_write.as_deref() == Some(text)
    }

    pub fn write_files(&mut self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return false;
        }
        let Some(clipboard) = self.clipboard.as_mut() else {
            return false;
        };
        if clipboard.set().file_list(paths).is_err() {
            return false;
        }
        let files = read_files(clipboard);
        self.own_files = Some(signature_of(&files));
        true
    }

    pub fn write_image(&mut self, png: &[u8]) -> Result<(), String> {
        let decoded = image::load_from_memory(png).map_err(|error| error.to_string())?;
        let rgba = decoded.to_rgba8();
        let (width, height) = rgba.dimensions();
        let bytes = rgba.into_raw();
        let hash = image_hash(&bytes, width, height);
        #[cfg(target_os = "macos")]
        {
            // Cursor / Electron 认 public.png；arboard 只写 NSImage→TIFF，粘贴会报不支持格式。
            // PNG + TIFF 都放上，两边都能吃。
            write_macos_image(png, &bytes, width as usize, height as usize)?;
        }
        #[cfg(not(target_os = "macos"))]
        {
            let clipboard = self.clipboard.as_mut().ok_or("剪贴板不可用")?;
            clipboard
                .set_image(ImageData {
                    width: width as usize,
                    height: height as usize,
                    bytes: bytes.into(),
                })
                .map_err(|error| error.to_string())?;
        }
        self.own_image = Some(hash);
        Ok(())
    }
}

/// macOS：写入 `public.png`。arboard 只放 NSImage/TIFF，Cursor 等 Electron 会报不支持格式。
#[cfg(target_os = "macos")]
fn write_macos_image(
    png: &[u8],
    _rgba: &[u8],
    _width: usize,
    _height: usize,
) -> Result<(), String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
    use objc2_foundation::NSData;

    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    let png_data = NSData::with_bytes(png);
    if !unsafe { pasteboard.setData_forType(Some(&png_data), NSPasteboardTypePNG) } {
        return Err("写入 PNG 到剪贴板失败".into());
    }
    Ok(())
}

fn read_files(clipboard: &mut Clipboard) -> Vec<ClipFile> {
    let Ok(paths) = clipboard.get().file_list() else {
        return Vec::new();
    };
    paths
        .into_iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() || meta.len() == 0 {
                return None;
            }
            let name = path.file_name()?.to_string_lossy().to_string();
            Some(ClipFile {
                path,
                name,
                size: meta.len() as i64,
            })
        })
        .collect()
}

fn file_signature(clipboard: &mut Clipboard) -> Option<String> {
    let files = read_files(clipboard);
    (!files.is_empty()).then(|| signature_of(&files))
}

fn signature_of(files: &[ClipFile]) -> String {
    files
        .iter()
        .map(|file| format!("{}:{}", file.name, file.size))
        .collect::<Vec<_>>()
        .join("|")
}

fn read_image(clipboard: &mut Clipboard) -> Option<ClipImage> {
    let image = clipboard.get_image().ok()?;
    let width = image.width;
    let height = image.height;
    if width == 0 || height == 0 {
        return None;
    }
    let rgba = image.bytes.into_owned();
    let hash = image_hash(&rgba, width as u32, height as u32);
    let dynamic = image::RgbaImage::from_raw(width as u32, height as u32, rgba.clone())?;
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(dynamic)
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    let png = png.into_inner();
    if png.len() as i64 > MAX_FILE_BYTES {
        return None;
    }
    Some(ClipImage {
        png,
        name: format!("image-{}.png", &hash[..8]),
        hash,
    })
}

fn image_fingerprint(clipboard: &mut Clipboard) -> Option<String> {
    read_image(clipboard).map(|image| image.hash)
}

pub fn image_hash(rgba: &[u8], width: u32, height: u32) -> String {
    let mut bytes = Vec::with_capacity(rgba.len() + 8);
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(rgba);
    sha256_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::{image_hash, signature_of, ClipFile, CopyFilter, ECHO_WINDOW};
    use std::path::PathBuf;
    use std::time::Instant;

    #[test]
    fn startup_baseline_is_not_a_copy() {
        let mut filter = CopyFilter::new(Some("already here".to_string()));
        assert_eq!(filter.observe(Some("already here".to_string())), None);
    }

    #[test]
    fn a_new_copy_is_broadcast_once_then_suppressed() {
        let mut filter = CopyFilter::new(None);
        assert_eq!(
            filter.observe(Some("fresh".to_string())),
            Some("fresh".to_string())
        );
        assert_eq!(filter.observe(Some("fresh".to_string())), None);
    }

    #[test]
    fn blank_and_absent_reads_are_ignored() {
        let mut filter = CopyFilter::new(None);
        assert_eq!(filter.observe(None), None);
        assert_eq!(filter.observe(Some("   ".to_string())), None);
    }

    #[test]
    fn our_own_write_is_not_re_published() {
        let mut filter = CopyFilter::new(None);
        filter.wrote("incoming");
        assert_eq!(filter.observe(Some("incoming".to_string())), None);
        assert_eq!(
            filter.observe(Some("something else".to_string())),
            Some("something else".to_string())
        );
    }

    #[test]
    fn text_we_published_is_not_re_read() {
        let mut filter = CopyFilter::new(None);
        filter.published("sent");
        assert_eq!(filter.observe(Some("sent".to_string())), None);
    }

    #[test]
    fn a_recent_send_is_an_echo_but_an_ancient_one_is_a_fresh_clip() {
        let mut filter = CopyFilter::new(None);
        filter.published("sent");
        assert!(filter.echoed("sent"));
        assert!(!filter.echoed("something else"));
        if let Some(ancient) = Instant::now().checked_sub(ECHO_WINDOW * 2) {
            filter.remember("stale", ancient);
            assert!(!filter.echoed("stale"));
        }
    }

    #[test]
    fn a_later_copy_does_not_erase_what_we_sent() {
        let mut filter = CopyFilter::new(None);
        filter.published("first");
        filter.published("second");
        assert!(filter.echoed("first"));
        assert!(filter.echoed("second"));
    }

    #[test]
    fn file_signature_changes_with_name_or_size() {
        let files = vec![
            ClipFile {
                path: PathBuf::from("/tmp/a.png"),
                name: "a.png".into(),
                size: 10,
            },
            ClipFile {
                path: PathBuf::from("/tmp/b.pdf"),
                name: "b.pdf".into(),
                size: 20,
            },
        ];
        assert_eq!(signature_of(&files), "a.png:10|b.pdf:20");
        let mut changed = files.clone();
        changed[1].size = 21;
        assert_ne!(signature_of(&files), signature_of(&changed));
    }

    #[test]
    fn image_hash_covers_dimensions_and_pixels() {
        let a = image_hash(&[1, 2, 3, 4], 1, 1);
        assert_eq!(a, image_hash(&[1, 2, 3, 4], 1, 1));
        assert_ne!(a, image_hash(&[1, 2, 3, 5], 1, 1));
        assert_ne!(a, image_hash(&[1, 2, 3, 4], 2, 1));
    }
}
