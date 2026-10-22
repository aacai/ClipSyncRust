use std::io::{self, Cursor, Read};
use std::time::Duration;

use clipsync_core::files::MAX_FILE_BYTES;
use clipsync_core::protocol::now_millis;

pub const UUGU_UPLOAD_URL: &str = "https://uguu.se/upload.php";
pub const X0_AT_UPLOAD_URL: &str = "https://x0.at/";
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const PROGRESS_STEP: i64 = 64 * 1024;

// 托管方各自承诺的存活时长。x0.at 只说 3 天起步，按最短的那头算，
// 免得给界面一个兑现不了的剩余时间；uguu 快但只有 3 小时。
const UUGU_TTL_MS: i64 = 3 * 3_600_000;
const X0_AT_TTL_MS: i64 = 72 * 3_600_000;

pub struct UploadedLink {
    pub url: String,
    pub expires_at: i64,
}

impl UploadedLink {
    fn with_ttl(ttl_ms: i64, url: String) -> Self {
        Self {
            url,
            expires_at: now_millis() + ttl_ms,
        }
    }
}

pub struct TransferClient {
    agent: ureq::Agent,
    uugu_url: String,
    x0at_url: String,
}

impl TransferClient {
    pub fn new() -> Self {
        Self::with_hosts(UUGU_UPLOAD_URL, X0_AT_UPLOAD_URL)
    }

    pub fn with_hosts(uugu_url: &str, x0at_url: &str) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build();
        Self {
            agent,
            uugu_url: uugu_url.to_string(),
            x0at_url: x0at_url.to_string(),
        }
    }

    /// Uploads already-encrypted bytes and returns the public link.
    ///
    /// uguu 主用（两头都快），它拒收才退回 x0.at；两家都失败时报主用的那个错。
    pub fn upload(
        &self,
        file_name: &str,
        bytes: &[u8],
        mut on_progress: impl FnMut(i64, i64),
    ) -> Result<UploadedLink, String> {
        let primary = self
            .post(
                &self.uugu_url,
                "files[]",
                &[],
                file_name,
                bytes,
                &mut on_progress,
            )
            .and_then(|body| {
                parse_uugu_link(&body).map(|url| UploadedLink::with_ttl(UUGU_TTL_MS, url))
            });
        match primary {
            Ok(link) => Ok(link),
            Err(primary) => match self
                .post(
                    &self.x0at_url,
                    "file",
                    &[("keep_name", "1")],
                    file_name,
                    bytes,
                    &mut on_progress,
                )
                .and_then(|body| {
                    parse_plain_link(&body).map(|url| UploadedLink::with_ttl(X0_AT_TTL_MS, url))
                }) {
                Ok(link) => Ok(link),
                Err(_) => Err(primary),
            },
        }
    }

    fn post(
        &self,
        url: &str,
        file_field: &str,
        fields: &[(&str, &str)],
        file_name: &str,
        bytes: &[u8],
        mut on_progress: impl FnMut(i64, i64),
    ) -> Result<String, String> {
        let boundary = boundary();
        let body = multipart_body(&boundary, fields, file_field, file_name, bytes);
        let total = body.len() as i64;
        let content_type = format!("multipart/form-data; boundary={boundary}");
        let reader = ProgressReader::new(body, total, |sent| on_progress(sent, total));
        let response = self
            .agent
            .post(url)
            .set("Content-Type", &content_type)
            .set("Content-Length", &total.to_string())
            .send(reader);
        match response {
            Ok(response) => response
                .into_string()
                .map_err(|error| format!("上传失败：{error}")),
            Err(ureq::Error::Status(code, response)) => {
                let detail = response
                    .into_string()
                    .map(|text| text.chars().take(200).collect::<String>())
                    .unwrap_or_default();
                Err(format!("上传被拒绝（HTTP {code}）{detail}"))
            }
            Err(ureq::Error::Transport(error)) => Err(format!("上传失败：{error}")),
        }
    }

    pub fn download(
        &self,
        link: &str,
        mut on_progress: impl FnMut(i64, i64),
    ) -> Result<Vec<u8>, String> {
        let response = match self.agent.get(link).call() {
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
        let total = response
            .header("Content-Length")
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(-1);
        let mut reader = response.into_reader();
        let mut out = Vec::with_capacity(total.max(0) as usize);
        let mut chunk = [0u8; 64 * 1024];
        let mut received = 0i64;
        let mut reported = 0i64;
        loop {
            let read = reader
                .read(&mut chunk)
                .map_err(|error| format!("下载失败：{error}"))?;
            if read == 0 {
                break;
            }
            received += read as i64;
            if received > MAX_FILE_BYTES + 64 {
                return Err("下载失败：文件超过大小上限".to_string());
            }
            out.extend_from_slice(&chunk[..read]);
            if received - reported >= PROGRESS_STEP {
                reported = received;
                on_progress(received, total);
            }
        }
        on_progress(received, total.max(received));
        Ok(out)
    }
}

fn boundary() -> String {
    let mut raw = [0u8; 12];
    getrandom::fill(&mut raw).expect("os random unavailable");
    format!("----ClipSync{}", hex::encode(raw))
}

fn multipart_body(
    boundary: &str,
    fields: &[(&str, &str)],
    file_field: &str,
    file_name: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 512);
    for (key, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
            file_name.replace(['"', '\r', '\n'], "_")
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// uguu 回 JSON，且会打乱我们送的文件名，只认 `files[0].url`。
fn parse_uugu_link(body: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| format!("上传响应读不懂：{}", clip(body, 200)))?;
    let link = value
        .get("files")
        .and_then(|files| files.get(0))
        .and_then(|file| file.get("url"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if value.get("success").and_then(serde_json::Value::as_bool) != Some(true)
        || !link.starts_with("http")
    {
        return Err(format!("上传被拒绝：{}", clip(body, 200)));
    }
    Ok(link.to_string())
}

fn parse_plain_link(body: &str) -> Result<String, String> {
    let link = body.trim();
    if !link.starts_with("http") {
        return Err(format!("上传响应不是有效链接：{}", clip(link, 200)));
    }
    Ok(link.to_string())
}

fn clip(raw: &str, limit: usize) -> String {
    raw.chars().take(limit).collect()
}

struct ProgressReader<F: FnMut(i64)> {
    inner: Cursor<Vec<u8>>,
    total: i64,
    reported: i64,
    on_progress: F,
}

impl<F: FnMut(i64)> ProgressReader<F> {
    fn new(body: Vec<u8>, total: i64, on_progress: F) -> Self {
        Self {
            inner: Cursor::new(body),
            total,
            reported: 0,
            on_progress,
        }
    }
}

impl<F: FnMut(i64)> Read for ProgressReader<F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        if read > 0 {
            let sent = self.inner.position() as i64;
            if sent - self.reported >= PROGRESS_STEP || sent == self.total {
                self.reported = sent;
                (self.on_progress)(sent);
            }
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        boundary, multipart_body, parse_plain_link, parse_uugu_link, ProgressReader, UploadedLink,
        X0_AT_TTL_MS,
    };
    use std::io::Read;

    const UUGU_OK: &str = r#"{"success":true,"files":[{"url":"https://n.uguu.se/Av2xQ9.png","size":"1024","name":"zzz"}]}"#;

    #[test]
    fn multipart_carries_the_extra_fields_and_the_binary_payload() {
        let body = multipart_body(
            "BOUND",
            &[("keep_name", "1")],
            "file",
            "cs-abc.bin",
            b"\x00\x01hello",
        );
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--BOUND\r\n"));
        assert!(text.contains("name=\"keep_name\"\r\n\r\n1\r\n"));
        assert!(text.contains("name=\"file\"; filename=\"cs-abc.bin\""));
        assert!(body.ends_with(b"\r\n--BOUND--\r\n"));
        let start = body
            .windows(6)
            .position(|window| window == b"\r\n\r\n\x00\x01")
            .expect("binary payload starts after its headers");
        assert_eq!(&body[start + 4..start + 11], b"\x00\x01hello");
    }

    #[test]
    fn multipart_drops_extra_fields_when_the_host_takes_none() {
        let body = multipart_body("BOUND", &[], "files[]", "a.bin", b"x");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--BOUND\r\nContent-Disposition: form-data; name=\"files[]\""));
    }

    #[test]
    fn multipart_escapes_quotes_in_the_file_name() {
        let body = multipart_body("B", &[], "files[]", "a\"b\r\nc.bin", b"x");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("filename=\"a_b__c.bin\""));
    }

    #[test]
    fn the_uguu_link_comes_from_the_first_file_entry() {
        assert_eq!(
            parse_uugu_link(UUGU_OK).unwrap(),
            "https://n.uguu.se/Av2xQ9.png"
        );
    }

    #[test]
    fn a_rejected_or_unreadable_uguu_response_is_an_error() {
        assert!(parse_uugu_link(r#"{"success":false,"files":[]}"#)
            .unwrap_err()
            .starts_with("上传被拒绝："));
        assert!(parse_uugu_link("<html>502</html>")
            .unwrap_err()
            .starts_with("上传响应读不懂："));
    }

    #[test]
    fn x0_at_returns_the_link_as_plain_text() {
        assert_eq!(
            parse_plain_link(" https://x0.at/a.bin\n").unwrap(),
            "https://x0.at/a.bin"
        );
    }

    #[test]
    fn a_non_link_from_x0_at_is_rejected() {
        assert!(parse_plain_link("<html>blocked</html>")
            .unwrap_err()
            .starts_with("上传响应不是有效链接："));
    }

    #[test]
    fn the_expiry_is_the_host_ttl_counted_from_now() {
        let before = clipsync_core::protocol::now_millis();
        let link = UploadedLink::with_ttl(X0_AT_TTL_MS, "https://x0.at/a.bin".to_string());
        assert_eq!(link.url, "https://x0.at/a.bin");
        assert!(link.expires_at >= before + X0_AT_TTL_MS);
    }

    #[test]
    fn boundaries_are_unique() {
        assert_ne!(boundary(), boundary());
    }

    #[test]
    fn progress_reader_reports_the_full_length() {
        let mut seen = Vec::new();
        let body = vec![7u8; 200 * 1024];
        let total = body.len() as i64;
        let mut reader = ProgressReader::new(body, total, |sent| seen.push(sent));
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), 200 * 1024);
        assert_eq!(seen.last().copied(), Some(total));
    }
}
