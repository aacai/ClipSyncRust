use std::io::Read;
use std::time::Duration;

use serde::Deserialize;

pub const ENDPOINT: &str = "https://api.github.com/repos/aacai/ClipSyncRust/releases/latest";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(900);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub size: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate(String),
    Available {
        release: Release,
        asset: Option<ReleaseAsset>,
    },
    Downloading {
        asset: ReleaseAsset,
        received: i64,
    },
    Downloaded(String),
    Failed(String),
}

pub enum UpdateEvent {
    Checked(Result<Release, String>),
    Progress(ReleaseAsset, i64),
    Downloaded(ReleaseAsset, Result<Vec<u8>, String>),
}

pub fn latest() -> Result<Release, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let response = agent
        .get(ENDPOINT)
        .set("User-Agent", "ClipSync")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| format!("连不上发布页：{error}"))?;
    let raw = response
        .into_string()
        .map_err(|error| format!("发布页返回的内容读不出来：{error}"))?;
    decode_release(&raw)
}

pub fn download(url: &str, mut on_progress: impl FnMut(i64)) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new().timeout(DOWNLOAD_TIMEOUT).build();
    let response = match agent.get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let detail = response
                .into_string()
                .map(|text| text.chars().take(200).collect::<String>())
                .unwrap_or_default();
            return Err(format!("下载被拒绝（HTTP {code}）{detail}"));
        }
        Err(ureq::Error::Transport(error)) => return Err(format!("下载失败：{error}")),
    };
    let mut reader = response.into_reader();
    let mut out = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    let mut received = 0i64;
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| format!("下载失败：{error}"))?;
        if read == 0 {
            break;
        }
        received += read as i64;
        out.extend_from_slice(&chunk[..read]);
        on_progress(received);
    }
    Ok(out)
}

fn decode_release(raw: &str) -> Result<Release, String> {
    let dto: ReleaseDto = serde_json::from_str(raw).map_err(|_| "发布信息读不懂".to_string())?;
    let version = dto
        .tag_name
        .unwrap_or_default()
        .trim_start_matches(['v', 'V'])
        .to_string();
    if version.is_empty() {
        return Err(dto
            .message
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "发布信息里没有版本号".to_string()));
    }
    Ok(Release {
        version,
        assets: dto
            .assets
            .into_iter()
            .filter(|asset| {
                asset.browser_download_url.starts_with("http") && !asset.name.is_empty()
            })
            .map(|asset| ReleaseAsset {
                name: asset.name,
                url: asset.browser_download_url,
                size: asset.size,
            })
            .collect(),
    })
}

#[derive(Deserialize)]
struct ReleaseDto {
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    assets: Vec<AssetDto>,
}

#[derive(Deserialize)]
struct AssetDto {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    size: i64,
}

pub fn is_behind(current: &str, latest: &str) -> bool {
    let mine = version_parts(current);
    let theirs = version_parts(latest);
    let width = mine.len().max(theirs.len());
    for index in 0..width {
        let a = mine.get(index).copied().unwrap_or(0);
        let b = theirs.get(index).copied().unwrap_or(0);
        if a != b {
            return a < b;
        }
    }
    false
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

pub fn asset_for<'a>(release: &'a Release, os: &str, arch: &str) -> Option<&'a ReleaseAsset> {
    let extension = installer_extension(os)?;
    let mut universal = None;
    let mut matching = None;
    for asset in &release.assets {
        if !asset
            .name
            .to_lowercase()
            .ends_with(&format!(".{extension}"))
        {
            continue;
        }
        match arch_tag(&asset.name) {
            Some(tag) if tag.eq_ignore_ascii_case(arch) && !arch.is_empty() => {
                matching = Some(asset);
            }
            Some(_) => {}
            None => universal = universal.or(Some(asset)),
        }
    }
    matching.or(universal)
}

fn installer_extension(os: &str) -> Option<&'static str> {
    let os = os.to_lowercase();
    if os.contains("mac") || os.contains("darwin") {
        Some("dmg")
    } else if os.contains("win") {
        Some("msi")
    } else if os.contains("linux") || os.contains("nux") {
        Some("deb")
    } else if os.contains("android") {
        Some("apk")
    } else {
        None
    }
}

fn arch_tag(name: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    if name.contains("-arm64") {
        Some("arm64")
    } else if name.contains("-x64") {
        Some("x64")
    } else {
        None
    }
}

pub fn arch() -> String {
    let arch = std::env::consts::ARCH.to_lowercase();
    if arch.contains("aarch64") || arch.contains("arm64") {
        "arm64".to_string()
    } else if arch.contains("amd64") || arch.contains("x86_64") || arch.contains("x64") {
        "x64".to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{asset_for, decode_release, installer_extension, is_behind, Release, ReleaseAsset};

    fn release(assets: &[(&str, &str, i64)]) -> Release {
        Release {
            version: "0.5.0".to_string(),
            assets: assets
                .iter()
                .map(|(name, url, size)| ReleaseAsset {
                    name: name.to_string(),
                    url: url.to_string(),
                    size: *size,
                })
                .collect(),
        }
    }

    #[test]
    fn versions_compare_segment_by_segment() {
        assert!(is_behind("0.9", "0.10"));
        assert!(is_behind("0.5", "0.5.1"));
        assert!(!is_behind("0.5.1", "0.5"));
        assert!(!is_behind("1.0", "1.0.0"));
        assert!(is_behind("v0.4.9", "V0.5"));
    }

    #[test]
    fn the_latest_release_parses_and_ignores_unknown_fields() {
        let raw = r#"{
            "tag_name": "v0.5.0",
            "message": null,
            "draft": false,
            "assets": [
                {"name":"ClipSync-0.5.0-arm64.dmg","browser_download_url":"https://example.test/a.dmg","size":10,"extra":1},
                {"name":"notes.txt","browser_download_url":"ftp://nope","size":1}
            ]
        }"#;
        let parsed = decode_release(raw).expect("release");
        assert_eq!(parsed.version, "0.5.0");
        assert_eq!(parsed.assets.len(), 1);
        assert_eq!(parsed.assets[0].name, "ClipSync-0.5.0-arm64.dmg");
    }

    #[test]
    fn a_bad_release_reports_what_went_wrong() {
        assert_eq!(decode_release("not json").unwrap_err(), "发布信息读不懂");
        assert_eq!(
            decode_release(r#"{"tag_name":""}"#).unwrap_err(),
            "发布信息里没有版本号"
        );
        assert_eq!(
            decode_release(r#"{"tag_name":"","message":"API rate limit exceeded"}"#).unwrap_err(),
            "API rate limit exceeded"
        );
    }

    #[test]
    fn installers_are_matched_by_os_and_arch() {
        let mac = release(&[
            ("ClipSync-0.5.0-x64.dmg", "https://e/a.dmg", 1),
            ("ClipSync-0.5.0-arm64.dmg", "https://e/b.dmg", 1),
            ("ClipSync-0.5.0.dmg", "https://e/c.dmg", 1),
        ]);
        assert_eq!(
            asset_for(&mac, "macOS", "arm64").unwrap().url,
            "https://e/b.dmg"
        );
        assert_eq!(
            asset_for(&mac, "macOS", "x64").unwrap().url,
            "https://e/a.dmg"
        );
        assert_eq!(asset_for(&mac, "macOS", "").unwrap().url, "https://e/c.dmg");
        assert_eq!(asset_for(&mac, "Windows", "x64"), None);
        assert_eq!(installer_extension("Windows"), Some("msi"));
        assert_eq!(installer_extension("Linux"), Some("deb"));
    }

    #[test]
    fn an_universal_package_is_the_fallback() {
        let mixed = release(&[
            ("ClipSync-x64.dmg", "https://e/x.dmg", 1),
            ("ClipSync.dmg", "https://e/u.dmg", 1),
        ]);
        assert_eq!(
            asset_for(&mixed, "darwin", "arm64").unwrap().url,
            "https://e/u.dmg"
        );
        let only_tagged = release(&[("ClipSync-x64.dmg", "https://e/x.dmg", 1)]);
        assert_eq!(asset_for(&only_tagged, "macOS", "arm64"), None);
    }
}
