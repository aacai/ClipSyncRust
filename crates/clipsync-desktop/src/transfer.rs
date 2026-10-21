use std::io::{self, Cursor, Read};
use std::time::Duration;

use clipsync_core::files::MAX_FILE_BYTES;
use clipsync_core::protocol::now_millis;

pub const UPLOAD_URL: &str = "https://litterbox.catbox.moe/resources/internals/api.php";
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const PROGRESS_STEP: i64 = 64 * 1024;

pub struct TransferClient {
    agent: ureq::Agent,
    endpoint: String,
}

impl TransferClient {
    pub fn new() -> Self {
        Self::with_endpoint(UPLOAD_URL)
    }

    pub fn with_endpoint(endpoint: &str) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build();
        Self {
            agent,
            endpoint: endpoint.to_string(),
        }
    }

    /// Uploads already-encrypted bytes and returns the public link.
    pub fn upload(
        &self,
        file_name: &str,
        bytes: &[u8],
        mut on_progress: impl FnMut(i64, i64),
    ) -> Result<String, String> {
        let boundary = boundary();
        let body = multipart_body(&boundary, "72h", file_name, bytes);
        let total = body.len() as i64;
        let content_type = format!("multipart/form-data; boundary={boundary}");
        let reader = ProgressReader::new(body, total, |sent| on_progress(sent, total));
        let response = self
            .agent
            .post(&self.endpoint)
            .set("Content-Type", &content_type)
            .set("Content-Length", &total.to_string())
            .send(reader);
        match response {
            Ok(response) => {
                let link = response
                    .into_string()
                    .map_err(|error| format!("上传失败：{error}"))?
                    .trim()
                    .to_string();
                if !link.starts_with("http") {
                    return Err(format!("上传响应不是有效链接：{link}"));
                }
                Ok(link)
            }
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

fn multipart_body(boundary: &str, expires: &str, file_name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"reqtype\"\r\n\r\nfileupload\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"time\"\r\n\r\n{expires}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"fileToUpload\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
            file_name.replace(['"', '\r', '\n'], "_")
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
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

pub fn expiry_from(expires: &str) -> i64 {
    clipsync_core::files::expiry_from(expires, now_millis())
}

#[cfg(test)]
mod tests {
    use super::{boundary, multipart_body, ProgressReader};
    use std::io::Read;

    #[test]
    fn multipart_carries_the_litterbox_fields_and_bytes() {
        let body = multipart_body("BOUND", "72h", "cs-abc.bin", b"\x00\x01hello");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--BOUND\r\n"));
        assert!(text.contains("name=\"reqtype\"\r\n\r\nfileupload\r\n"));
        assert!(text.contains("name=\"time\"\r\n\r\n72h\r\n"));
        assert!(text.contains("filename=\"cs-abc.bin\""));
        assert!(body.ends_with(b"\r\n--BOUND--\r\n"));
        let start = body
            .windows(6)
            .position(|window| window == b"\r\n\r\n\x00\x01")
            .expect("binary payload starts after its headers");
        assert_eq!(&body[start + 4..start + 11], b"\x00\x01hello");
    }

    #[test]
    fn multipart_escapes_quotes_in_the_file_name() {
        let body = multipart_body("B", "72h", "a\"b\r\nc.bin", b"x");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("filename=\"a_b__c.bin\""));
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
