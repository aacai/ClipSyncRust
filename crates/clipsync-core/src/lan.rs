use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_channel::Sender;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::protocol::{clip_topic, ROOT};

pub const LAN_PORT: u16 = 43871;
pub const LAN_DISCOVERY_PORT: u16 = 43879;
pub const BLOB_SEGMENT: &str = "blob";
pub const LAN_LINK_TTL_MS: i64 = 24 * 3_600_000;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024 + 4096;
const BEACON_INTERVAL: Duration = Duration::from_millis(1500);
const BEACON_MAX_AGE: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanInfo {
    pub port: u16,
    pub addrs: Vec<String>,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanBeacon {
    pub id: String,
    pub name: String,
    pub os: String,
    pub port: u16,
    pub addrs: Vec<String>,
    pub room_hash: String,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanPeer {
    pub id: String,
    pub name: String,
    pub os: String,
    pub addrs: Vec<String>,
    pub port: u16,
}

impl LanPeer {
    pub fn base(&self) -> Option<String> {
        self.addrs
            .first()
            .map(|host| format!("http://{host}:{}", self.port))
    }
}

#[derive(Debug, Clone)]
pub struct LanIdentity {
    pub id: String,
    pub name: String,
    pub os: String,
}

#[derive(Debug, Clone)]
pub struct LanConfig {
    pub port: u16,
    pub blob_dir: PathBuf,
}

impl Default for LanConfig {
    fn default() -> Self {
        Self {
            port: LAN_PORT,
            blob_dir: blob_dir(),
        }
    }
}

pub fn blob_dir() -> PathBuf {
    std::env::temp_dir().join("clipsync-lan-blobs")
}

pub fn local_addrs() -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(interfaces) = get_if_addrs() {
        for address in interfaces {
            match address {
                IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => found.push(ip.to_string()),
                IpAddr::V6(ip) if !ip.is_loopback() && !ip.is_unspecified() => {
                    found.push(format!("[{ip}]"))
                }
                _ => {}
            }
        }
    }
    found
}

fn get_if_addrs() -> Result<Vec<IpAddr>, String> {
    let script = if cfg!(windows) {
        "Get-NetIPAddress -AddressFamily IPv4 | Where-Object {$_.IPAddress -ne '127.0.0.1'} | Select-Object -ExpandProperty IPAddress"
    } else {
        "ifconfig 2>/dev/null | grep -o 'inet [0-9.]*' | awk '{print $2}' || ip -4 addr show 2>/dev/null | grep inet | awk '{print $2}' | cut -d/ -f1"
    };
    let output = if cfg!(windows) {
        std::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
            .output()
    } else {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .output()
    }
    .map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter_map(|line| line.trim().parse::<IpAddr>().ok())
        .collect())
}

#[derive(Clone)]
pub struct LanRuntime {
    inner: Arc<Inner>,
}

struct Inner {
    info: LanInfo,
    room_hash: String,
    blob_dir: PathBuf,
    _server: Arc<ServerTask>,
    _discovery: Option<Arc<ServerTask>>,
}

struct ServerTask(JoinHandle<()>);

impl Drop for ServerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl LanRuntime {
    pub async fn start(
        config: LanConfig,
        identity: LanIdentity,
        room_hash: String,
        clips: Sender<(String, String)>,
        peers: Sender<LanBeacon>,
    ) -> Result<Self, String> {
        let address = SocketAddr::from(([0, 0, 0, 0], config.port));
        let listener = TcpListener::bind(address)
            .await
            .map_err(|error| format!("局域网端口 {} 不可用：{error}", config.port))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        std::fs::create_dir_all(&config.blob_dir)
            .map_err(|error| format!("无法建立局域网文件目录：{error}"))?;
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).expect("os random unavailable");
        let info = LanInfo {
            port,
            addrs: local_addrs(),
            nonce: hex::encode(nonce),
        };
        let server = Arc::new(ServerTask(tokio::spawn(serve(
            listener,
            room_hash.clone(),
            config.blob_dir.clone(),
            clips,
        ))));
        let _discovery =
            spawn_discovery(info.clone(), identity, room_hash.clone(), peers)
                .await
                .ok();
        let inner = Arc::new(Inner {
            info: info.clone(),
            room_hash,
            blob_dir: config.blob_dir,
            _server: server,
            _discovery,
        });
        Ok(Self { inner })
    }

    pub fn info(&self) -> LanInfo {
        self.inner.info.clone()
    }

    pub fn room_hash(&self) -> &str {
        &self.inner.room_hash
    }

    pub fn blob_url(&self, name: &str) -> Option<String> {
        let host = self.inner.info.addrs.first()?;
        Some(format!(
            "http://{host}:{}/{ROOT}/{}/{BLOB_SEGMENT}/{name}",
            self.inner.info.port, self.inner.room_hash
        ))
    }

    pub fn store_blob(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if !valid_blob_name(name) {
            return Err("非法文件名".to_string());
        }
        std::fs::write(self.inner.blob_dir.join(name), bytes)
            .map_err(|error| format!("写入局域网文件失败：{error}"))
    }

    pub async fn fetch_blob(peer: &LanPeer, room_hash: &str, name: &str) -> Result<Vec<u8>, String> {
        if !valid_blob_name(name) {
            return Err("非法文件名".to_string());
        }
        let path = format!("/{ROOT}/{room_hash}/{BLOB_SEGMENT}/{name}");
        let mut last = "没有可用地址".to_string();
        for host in &peer.addrs {
            match send(host, peer.port, &path, "GET", &[]).await {
                Ok(bytes) => return Ok(bytes),
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    pub async fn publish(&self, peers: &[LanPeer], payload: &str) -> usize {
        let path = format!("/{ROOT}/{}/clip", self.inner.room_hash);
        let mut delivered = 0;
        for peer in peers {
            for host in &peer.addrs {
                if send(host, peer.port, &path, "POST", payload.as_bytes())
                    .await
                    .is_ok()
                {
                    delivered += 1;
                    break;
                }
            }
        }
        delivered
    }
}

async fn spawn_discovery(
    info: LanInfo,
    identity: LanIdentity,
    room_hash: String,
    peers: Sender<LanBeacon>,
) -> Result<Arc<ServerTask>, String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, LAN_DISCOVERY_PORT))
        .await
        .map_err(|error| format!("局域网发现端口不可用：{error}"))?;
    socket.set_broadcast(true).ok();
    let beacon = LanBeacon {
        id: identity.id.clone(),
        name: identity.name,
        os: identity.os,
        port: info.port,
        addrs: info.addrs,
        room_hash: room_hash.clone(),
        nonce: info.nonce,
    };
    let wire = serde_json::to_vec(&beacon).map_err(|error| error.to_string())?;
    let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), LAN_DISCOVERY_PORT);
    let task = tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        loop {
            tokio::select! {
                _ = tokio::time::sleep(BEACON_INTERVAL) => {
                    let _ = socket.send_to(&wire, target).await;
                }
                result = socket.recv_from(&mut buf) => {
                    let Ok((len, from)) = result else { break };
                    let Ok(found) = serde_json::from_slice::<LanBeacon>(&buf[..len]) else {
                        continue;
                    };
                    if found.room_hash != room_hash || found.id == identity.id {
                        continue;
                    }
                    let mut addrs = vec![from.ip().to_string()];
                    addrs.extend(found.addrs.iter().cloned());
                    addrs.dedup();
                    let _ = peers.send(LanBeacon { addrs, port: found.port, ..found }).await;
                }
            }
        }
    });
    Ok(Arc::new(ServerTask(task)))
}

async fn serve(
    listener: TcpListener,
    room_hash: String,
    blob_dir: PathBuf,
    clips: Sender<(String, String)>,
) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let hash = room_hash.clone();
        let dir = blob_dir.clone();
        let sink = clips.clone();
        tokio::spawn(async move {
            handle(stream, &hash, &dir, sink).await;
        });
    }
}

async fn handle(
    mut stream: TcpStream,
    room_hash: &str,
    blob_dir: &Path,
    clips: Sender<(String, String)>,
) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    let clip_path = format!("/{ROOT}/{room_hash}/clip");
    let blob_prefix = format!("/{ROOT}/{room_hash}/{BLOB_SEGMENT}/");
    let response = if request.method == "POST" && request.path == clip_path {
        let _ = clips.send((clip_topic(room_hash), String::from_utf8_lossy(&request.body).into_owned()));
        Response::ok(b"ok")
    } else if let Some(name) = request.path.strip_prefix(&blob_prefix) {
        if !valid_blob_name(name) {
            Response::status("404 Not Found")
        } else {
            let path = blob_dir.join(name);
            match request.method.as_str() {
                "PUT" => match std::fs::write(&path, &request.body) {
                    Ok(()) => Response::ok(b"ok"),
                    Err(_) => Response::status("500 Internal Server Error"),
                },
                "GET" => match std::fs::read(&path) {
                    Ok(bytes) => Response::ok(&bytes),
                    Err(_) => Response::status("404 Not Found"),
                },
                _ => Response::status("405 Method Not Allowed"),
            }
        }
    } else {
        Response::status("404 Not Found")
    };
    response.write(&mut stream).await;
}

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

struct Response {
    status: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn ok(body: &[u8]) -> Self {
        Self {
            status: "200 OK",
            body: body.to_vec(),
        }
    }

    fn status(status: &'static str) -> Self {
        Self {
            status,
            body: Vec::new(),
        }
    }

    async fn write(self, stream: &mut TcpStream) {
        let header = format!(
            "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.status,
            self.body.len()
        );
        let _ = stream.write_all(header.as_bytes()).await;
        let _ = stream.write_all(&self.body).await;
        let _ = stream.shutdown().await;
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let read = timeout(REQUEST_TIMEOUT, stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_HEAD {
            return None;
        }
        if let Some(index) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]);
    let mut lines = head.lines();
    let first = lines.next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let length: usize = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.eq_ignore_ascii_case("content-length")).then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    if length > MAX_BODY {
        return None;
    }
    while buffer.len() < head_end + length {
        let read = timeout(REQUEST_TIMEOUT, stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Some(Request {
        method,
        path,
        body: buffer[head_end..(head_end + length).min(buffer.len())].to_vec(),
    })
}

async fn send(host: &str, port: u16, path: &str, method: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    let mut stream = timeout(REQUEST_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| "超时".to_string())?
        .map_err(|error| error.to_string())?;
    let header = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(body)
        .await
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    timeout(REQUEST_TIMEOUT, stream.read_to_end(&mut response))
        .await
        .map_err(|_| "超时".to_string())?
        .map_err(|error| error.to_string())?;
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "响应无头部".to_string())?;
    let head = String::from_utf8_lossy(&response[..split]);
    if !head.starts_with("HTTP/1.1 200") {
        return Err(head.lines().next().unwrap_or("HTTP").to_string());
    }
    Ok(response[split + 4..].to_vec())
}

fn valid_blob_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && !name.contains("..")
}

#[cfg(test)]
mod tests {
    use super::{LanConfig, LanIdentity, LanPeer, LanRuntime};
    use crate::protocol::clip_topic;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let mut raw = [0u8; 4];
        getrandom::fill(&mut raw).expect("os random unavailable");
        std::env::temp_dir().join(format!("cs-lan-{label}-{}", hex::encode(raw)))
    }

    #[tokio::test]
    async fn clip_and_blob_cross_the_lan() {
        let dir = temp_dir("clip");
        let (tx, rx) = async_channel::unbounded();
        let (peer_tx, _peer_rx) = async_channel::unbounded();
        let runtime = LanRuntime::start(
            LanConfig {
                port: 0,
                blob_dir: dir.clone(),
            },
            LanIdentity {
                id: "self".to_string(),
                name: "self".to_string(),
                os: "test".to_string(),
            },
            "testhash".to_string(),
            tx,
            peer_tx,
        )
        .await
        .expect("lan server");
        let peer = LanPeer {
            id: "peer".to_string(),
            name: "peer".to_string(),
            os: "test".to_string(),
            addrs: vec!["127.0.0.1".to_string()],
            port: runtime.info().port,
        };

        assert_eq!(runtime.publish(&[peer.clone()], "wire").await, 1);
        let (topic, payload) = rx.recv().await.expect("clip");
        assert_eq!(topic, clip_topic("testhash"));
        assert_eq!(payload, "wire");

        runtime
            .store_blob("cs-abc.bin", b"cipher bytes")
            .expect("store");
        let fetched = LanRuntime::fetch_blob(&peer, "testhash", "cs-abc.bin")
            .await
            .expect("fetch");
        assert_eq!(fetched, b"cipher bytes");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn blob_names_cannot_escape_the_served_directory() {
        assert!(LanRuntime::fetch_blob(
            &LanPeer {
                id: "peer".to_string(),
                name: "peer".to_string(),
                os: "test".to_string(),
                addrs: vec!["127.0.0.1".to_string()],
                port: 1,
            },
            "hash",
            "../secret",
        )
        .await
        .is_err());
    }
}
