use std::time::Duration;

use arboard::Clipboard;

pub const POLL_INTERVAL: Duration = Duration::from_millis(800);

fn meaningful(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

/// Decides which clipboard reads are real copies the local user made.
///
/// The same text can reach us three ways: the user copied it, we wrote it back after
/// an incoming clip, or we published it from the compose box. Only the first is worth
/// re-broadcasting; the others must be suppressed or the room loops on itself.
#[derive(Default)]
pub struct CopyFilter {
    captured: Option<String>,
    own_write: Option<String>,
    published: Option<String>,
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
            || Some(&text) == self.published.as_ref()
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
        self.published = Some(text.to_string());
    }
}

pub struct ClipboardWatcher {
    clipboard: Option<Clipboard>,
    filter: CopyFilter,
}

impl ClipboardWatcher {
    pub fn new() -> Self {
        let mut clipboard = Clipboard::new().ok();
        let baseline = clipboard.as_mut().and_then(|clip| clip.get_text().ok());
        Self {
            clipboard,
            filter: CopyFilter::new(baseline),
        }
    }

    /// Some(text) only when the local user copied something we have not already broadcast.
    pub fn poll(&mut self) -> Option<String> {
        let text = self.clipboard.as_mut()?.get_text().ok();
        self.filter.observe(text)
    }

    /// The current clipboard text, for an explicit "send clipboard" action.
    pub fn read_once(&mut self) -> Option<String> {
        let text = meaningful(Some(self.clipboard.as_mut()?.get_text().ok()?))?;
        self.filter.captured = Some(text.clone());
        Some(text)
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
}

#[cfg(test)]
mod tests {
    use super::CopyFilter;

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
}
