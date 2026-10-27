use std::sync::mpsc::Sender;
use std::sync::Arc;

use clipsync_core::files::{
    decrypt_file, encrypt_file, max_mb, sanitize_file_name, upload_file_name,
    validate_outgoing_file, RejectReason, MAX_FILE_BYTES,
};
use clipsync_core::protocol::{now_millis, random_mid};
use clipsync_core::RoomCrypto;

use crate::paths;
use crate::transfer::TransferClient;

pub struct JobContext {
    pub crypto: Arc<RoomCrypto>,
    pub client: Arc<TransferClient>,
    pub clip_topic: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadTarget {
    Publish { cache: bool },
    Resend { id: String, ts: i64 },
}

pub struct UploadRequest {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub target: UploadTarget,
    /// The payload already sits in the cache at this path — don't write a second copy.
    pub cached: Option<String>,
}

#[derive(Debug)]
pub struct UploadDone {
    pub id: String,
    pub ts: i64,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
    pub link: String,
    pub expires_at: i64,
    pub local_path: Option<String>,
    pub target: UploadTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadAfter {
    Auto,
    Cache,
    Open,
    Save,
    CopyImage,
}

pub struct DownloadRequest {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
    pub link: String,
    pub after: DownloadAfter,
}

#[derive(Debug)]
pub struct DownloadDone {
    pub id: String,
    pub name: String,
    pub path: String,
    pub bytes: Option<Vec<u8>>,
    pub after: DownloadAfter,
}

pub enum JobEvent {
    Progress {
        upload: bool,
        name: String,
        done: i64,
        total: i64,
    },
    Uploaded(UploadDone),
    Downloaded(DownloadDone),
    Failed(String),
}

pub fn spawn_uploads(context: JobContext, requests: Vec<UploadRequest>, sender: Sender<JobEvent>) {
    std::thread::spawn(move || {
        for request in requests {
            let name = request.name.clone();
            let progress_sender = sender.clone();
            let outcome = perform_upload(&context, request, |done, total| {
                let _ = progress_sender.send(JobEvent::Progress {
                    upload: true,
                    name: name.clone(),
                    done,
                    total,
                });
            });
            match outcome {
                Ok(done) => {
                    if sender.send(JobEvent::Uploaded(done)).is_err() {
                        return;
                    }
                }
                Err(message) => {
                    if sender.send(JobEvent::Failed(message)).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

pub fn spawn_download(context: JobContext, request: DownloadRequest, sender: Sender<JobEvent>) {
    std::thread::spawn(move || {
        let name = request.name.clone();
        let progress_sender = sender.clone();
        let outcome = perform_download(&context, request, |done, total| {
            let _ = progress_sender.send(JobEvent::Progress {
                upload: false,
                name: name.clone(),
                done,
                total,
            });
        });
        let event = match outcome {
            Ok(done) => JobEvent::Downloaded(done),
            Err(message) => JobEvent::Failed(message),
        };
        let _ = sender.send(event);
    });
}

pub fn perform_upload(
    context: &JobContext,
    request: UploadRequest,
    on_progress: impl FnMut(i64, i64),
) -> Result<UploadDone, String> {
    let safe = sanitize_file_name(&request.name);
    let size = request.bytes.len() as i64;
    if let Some(reason) = validate_outgoing_file(&safe, size) {
        return Err(reject_message(reason, &safe));
    }
    let encrypted = encrypt_file(&context.crypto, &context.clip_topic, &safe, &request.mime, &request.bytes)
        .map_err(|error| format!("加密失败：{error}"))?;
    let link = context
        .client
        .upload(&upload_file_name(), &encrypted.cipher, on_progress)?;
    let (id, ts) = match &request.target {
        UploadTarget::Publish { .. } => (random_mid(), now_millis()),
        UploadTarget::Resend { id, ts } => (id.clone(), *ts),
    };
    let local_path = if matches!(&request.target, UploadTarget::Publish { cache: true }) {
        request
            .cached
            .clone()
            .or_else(|| paths::write_cache(&id, &safe, &request.bytes).ok())
    } else {
        None
    };
    Ok(UploadDone {
        id,
        ts,
        name: safe,
        mime: request.mime,
        size,
        sha256: encrypted.sha256,
        link: link.url,
        expires_at: link.expires_at,
        local_path,
        target: request.target,
    })
}

pub fn perform_download(
    context: &JobContext,
    request: DownloadRequest,
    on_progress: impl FnMut(i64, i64),
) -> Result<DownloadDone, String> {
    if request.link.trim().is_empty() {
        return Err(format!(
            "下载失败：{} 还没有上传链接",
            display_name(&request.name)
        ));
    }
    if request.size > MAX_FILE_BYTES {
        return Err(format!(
            "收到超过 {}MB 的文件，已忽略：{}",
            max_mb(),
            display_name(&request.name)
        ));
    }
    let cipher = context.client.download(&request.link, on_progress)?;
    let plain = decrypt_file(
        &context.crypto,
        &context.clip_topic,
        &request.name,
        &request.mime,
        request.size,
        &request.sha256,
        &cipher,
    )
    .map_err(|error| format!("文件校验/解密失败：{error}"))?;
    let path = paths::write_cache(&request.id, &request.name, &plain)
        .map_err(|error| format!("下载失败：无法写入缓存（{error}）"))?;
    let bytes = matches!(
        request.after,
        DownloadAfter::Save | DownloadAfter::CopyImage
    )
    .then_some(plain);
    Ok(DownloadDone {
        id: request.id,
        name: request.name,
        path,
        bytes,
        after: request.after,
    })
}

fn display_name(raw: &str) -> String {
    let safe = sanitize_file_name(raw);
    if safe.is_empty() {
        "文件".to_string()
    } else {
        safe
    }
}

fn reject_message(reason: RejectReason, name: &str) -> String {
    let name = display_name(name);
    match reason {
        RejectReason::TooLarge => format!("文件超过 {}MB，暂不支持：{name}", max_mb()),
        RejectReason::Empty => format!("空文件无法分享：{name}"),
        RejectReason::InvalidName => format!("文件名不合法：{name}"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use clipsync_core::files::encrypt_file;
    use clipsync_core::RoomCrypto;

    use super::{
        perform_download, DownloadAfter, DownloadRequest, JobContext, JobEvent, UploadRequest,
        UploadTarget,
    };
    use crate::transfer::TransferClient;

    fn topic() -> String {
        clipsync_core::protocol::clip_topic("testhash")
    }

    fn context() -> JobContext {
        JobContext {
            crypto: std::sync::Arc::new(RoomCrypto::open("TESTROOM", "pw").unwrap()),
            client: std::sync::Arc::new(TransferClient::with_hosts(
                "http://127.0.0.1:1/",
                "http://127.0.0.1:1/",
            )),
            clip_topic: topic(),
        }
    }

    fn serve_once(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        });
        format!("http://{address}/file.bin")
    }

    #[test]
    fn a_download_is_decrypted_cached_and_reported() {
        let room = RoomCrypto::open("TESTROOM", "pw").unwrap();
        let plain = b"hello file bytes".to_vec();
        let encrypted = encrypt_file(&room, &topic(), "photo.png", "image/png", &plain).unwrap();
        let link = serve_once(encrypted.cipher.clone());
        let request = DownloadRequest {
            id: "download-test".to_string(),
            name: "photo.png".to_string(),
            mime: "image/png".to_string(),
            size: plain.len() as i64,
            sha256: encrypted.sha256.clone(),
            link,
            after: DownloadAfter::CopyImage,
        };
        let mut progress = Vec::new();
        let done = perform_download(&context(), request, |done, total| {
            progress.push((done, total));
        })
        .expect("download");
        assert_eq!(done.bytes.as_deref(), Some(plain.as_slice()));
        assert_eq!(std::fs::read(&done.path).unwrap(), plain);
        assert!(!progress.is_empty());
        assert_eq!(progress.last().unwrap().0, encrypted.cipher.len() as i64);
        crate::paths::delete_cache(Some(&done.path));
    }

    #[test]
    fn a_tampered_download_is_rejected_before_caching() {
        let room = RoomCrypto::open("TESTROOM", "pw").unwrap();
        let plain = b"content".to_vec();
        let mut encrypted = encrypt_file(&room, &topic(), "a.txt", "text/plain", &plain).unwrap();
        let last = encrypted.cipher.len() - 1;
        encrypted.cipher[last] ^= 0xff;
        let link = serve_once(encrypted.cipher);
        let request = DownloadRequest {
            id: "tampered-test".to_string(),
            name: "a.txt".to_string(),
            mime: "text/plain".to_string(),
            size: plain.len() as i64,
            sha256: encrypted.sha256,
            link,
            after: DownloadAfter::Cache,
        };
        let error = perform_download(&context(), request, |_, _| {}).unwrap_err();
        assert!(error.starts_with("文件校验/解密失败"), "{error}");
    }

    #[test]
    fn offline_files_without_a_link_fail_loudly() {
        let request = DownloadRequest {
            id: "cold".to_string(),
            name: "cold.txt".to_string(),
            mime: "text/plain".to_string(),
            size: 5,
            sha256: "ab".to_string(),
            link: String::new(),
            after: DownloadAfter::Cache,
        };
        let error = perform_download(&context(), request, |_, _| {}).unwrap_err();
        assert!(error.contains("还没有上传链接"), "{error}");
    }

    #[test]
    fn upload_requests_are_rejected_before_touching_the_network() {
        let request = UploadRequest {
            name: "empty.txt".to_string(),
            mime: "text/plain".to_string(),
            bytes: Vec::new(),
            target: UploadTarget::Publish { cache: false },
            cached: None,
        };
        let error = super::perform_upload(&context(), request, |_, _| {}).unwrap_err();
        assert_eq!(error, "空文件无法分享：empty.txt");
    }

    #[test]
    fn progress_events_reach_the_channel() {
        let (sender, receiver) = mpsc::channel();
        let _ = sender.send(JobEvent::Failed("x".to_string()));
        assert!(matches!(receiver.try_recv(), Ok(JobEvent::Failed(_))));
    }
}
