use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_channel::Sender;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::{copy, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::protocol::{clip_topic, ROOT};

pub const LAN_PORT: u16 = 43871;
pub const LAN_DISCOVERY_PORT: u16 = 43879;
pub const BLOB_SEGMENT: &str = "blob";
pub const LAN_LINK_TTL_MS: i64 = 24 * 3_600_000;
pub const LAN_FS_PATH: &str = "/cs/v1/fs";

pub const LAN_FS_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ClipSync Files</title>
<style>
:root { color-scheme: dark; --bg:#111318; --card:#1b1f27; --line:#2b303b; --text:#e8eaf0; --muted:#9299a8; --accent:#7aa2ff; }
@media (prefers-color-scheme: light) { :root { color-scheme: light; --bg:#f6f7fa; --card:#fff; --line:#e2e5ec; --text:#20242c; --muted:#6b7280; --accent:#2f6fe4; } }
* { box-sizing:border-box; }
body { margin:0; background:var(--bg); color:var(--text); font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; }
main { max-width:860px; margin:0 auto; padding:28px 18px 48px; }
h1 { font-size:20px; margin:0 0 4px; }
p.hint { color:var(--muted); margin:0 0 20px; }
.bar { display:flex; gap:10px; margin-bottom:16px; }
input { flex:1; min-width:0; padding:10px 12px; border:1px solid var(--line); border-radius:10px; background:var(--card); color:var(--text); outline:none; }
input:focus { border-color:var(--accent); }
button { font:inherit; color:var(--text); background:none; border:0; cursor:pointer; padding:0; }
button:hover { color:var(--accent); }
.cards { display:grid; grid-template-columns:repeat(auto-fill,minmax(180px,1fr)); gap:12px; margin-bottom:18px; }
.card { display:block; width:100%; text-align:left; padding:16px; border:1px solid var(--line); border-radius:14px; background:var(--card); }
.card strong { display:block; font-size:15px; margin-bottom:4px; }
.card small { color:var(--muted); word-break:break-all; }
.path { color:var(--muted); margin:0 0 10px; word-break:break-all; }
.path button { color:var(--accent); }
.path span { padding:0 4px; }
.row { display:flex; align-items:center; gap:10px; padding:10px 12px; border-bottom:1px solid var(--line); color:inherit; text-decoration:none; }
.row:hover { background:var(--card); }
.row .name { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.row .meta { color:var(--muted); font-variant-numeric:tabular-nums; }
.list { border:1px solid var(--line); border-radius:14px; overflow:hidden; background:var(--card); }
.empty { padding:28px 12px; text-align:center; color:var(--muted); }
.error { padding:12px; border-radius:10px; color:#ffb4b4; background:rgba(255,80,80,.12); }
</style>
</head>
<body>
<main>
<h1>ClipSync Files</h1>
<p class="hint">Share files over this LAN. The link contains an access token — do not post it publicly.</p>
<div class="bar"><input id="search" type="search" placeholder="Search in this root" autocomplete="off" spellcheck="false"></div>
<div id="message"></div>
<div id="roots" class="cards"></div>
<div id="browser" hidden>
<p id="crumb" class="path"></p>
<div id="list" class="list"></div>
</div>
</main>
<script>
(function () {
  var token = new URLSearchParams(location.search).get('token') || '';
  var roots = [];
  var state = { root: -1, path: '' };
  var timer = null;
  function byId(id) { return document.getElementById(id); }
  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, function (c) {
      return {'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c];
    });
  }
  function api(name, extra) {
    var params = new URLSearchParams();
    params.set('token', token);
    Object.keys(extra || {}).forEach(function (key) { params.set(key, extra[key]); });
    return '/cs/v1/fs/' + name + '?' + params.toString();
  }
  function showError(error) {
    byId('message').innerHTML = '<div class="error">' + escapeHtml(error.message || error) + '</div>';
  }
  function json(path) {
    return fetch(path, {cache:'no-store'}).then(function (response) {
      if (!response.ok) throw new Error('HTTP ' + response.status);
      return response.json();
    });
  }
  var sizeUnits = ['B', 'KB', 'MB', 'GB'];
  function humanSize(size) {
    var value = size;
    var unit = 0;
    while (value >= 1024 && unit < sizeUnits.length - 1) { value /= 1024; unit++; }
    return (unit === 0 ? value : value.toFixed(1)) + ' ' + sizeUnits[unit];
  }
  function loadRoots() {
    byId('message').innerHTML = '';
    json(api('roots')).then(function (data) {
      roots = data;
      byId('roots').innerHTML = roots.map(function (root) {
        return '<button class="card" data-index="' + root.index + '"><strong>' + escapeHtml(root.name) +
          '</strong><small>' + escapeHtml(root.name) + '</small></button>';
      }).join('') || '<div class="empty">No shared folders yet.</div>';
      Array.prototype.forEach.call(byId('roots').querySelectorAll('button'), function (button) {
        button.onclick = function () { openRoot(Number(button.getAttribute('data-index'))); };
      });
    }).catch(showError);
  }
  function openRoot(index) {
    state.root = index;
    state.path = '';
    byId('roots').innerHTML = '';
    byId('browser').hidden = false;
    load();
  }
  function renderCrumb() {
    var parts = state.path ? state.path.split('/') : [];
    var html = '<button data-path="">' + escapeHtml(roots[state.root].name) + '</button>';
    var current = '';
    parts.forEach(function (part) {
      current = current ? current + '/' + part : part;
      html += '<span>/</span><button data-path="' + escapeHtml(current) + '">' + escapeHtml(part) + '</button>';
    });
    byId('crumb').innerHTML = html;
    Array.prototype.forEach.call(byId('crumb').querySelectorAll('button'), function (button) {
      button.onclick = function () { state.path = button.getAttribute('data-path'); load(); };
    });
  }
  function render(data) {
    renderCrumb();
    var html = '';
    data.dirs.forEach(function (dir) {
      html += '<div class="row dir" data-path="' + escapeHtml(dir.path) + '"><span class="name">📁 ' +
        escapeHtml(dir.name) + '</span></div>';
    });
    data.files.forEach(function (file) {
      html += '<a class="row file" href="' + escapeHtml(api('file', {root:state.root, path:file.path})) +
        '" download><span class="name">' + escapeHtml(file.name) + '</span><span class="meta">' +
        humanSize(file.size) + '</span></a>';
    });
    byId('list').innerHTML = html || '<div class="empty">Nothing here.</div>';
    Array.prototype.forEach.call(byId('list').querySelectorAll('.dir'), function (row) {
      row.onclick = function () { state.path = row.getAttribute('data-path'); load(); };
    });
  }
  function load() {
    byId('message').innerHTML = '';
    var extra = {root:state.root, path:state.path};
    var query = byId('search').value.trim();
    if (query) extra.q = query;
    json(api('list', extra)).then(render).catch(showError);
  }
  byId('search').addEventListener('input', function () {
    if (state.root < 0) return;
    clearTimeout(timer);
    timer = setTimeout(load, 250);
  });
  loadRoots();
})();
</script>
</body>
</html>"#;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const BLOB_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024 + 4096;
const BEACON_INTERVAL: Duration = Duration::from_millis(1500);
const BEACON_MAX_AGE: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedRoot {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanInfo {
    pub port: u16,
    pub addrs: Vec<String>,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FsRootEntry {
    name: String,
    index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FsDirEntry {
    name: String,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FsFileEntry {
    name: String,
    size: u64,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FsListEntry {
    dirs: Vec<FsDirEntry>,
    files: Vec<FsFileEntry>,
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
    pub nonce: String,
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
    pub fs_roots: Vec<SharedRoot>,
    pub fs_token: String,
}

impl Default for LanConfig {
    fn default() -> Self {
        Self {
            port: LAN_PORT,
            blob_dir: blob_dir(),
            fs_roots: Vec::new(),
            fs_token: String::new(),
        }
    }
}

pub fn blob_dir() -> PathBuf {
    std::env::temp_dir().join("clipsync-lan-blobs")
}

pub fn local_addrs() -> Vec<String> {
    let mut found: Vec<String> = get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|addr| match addr {
            IpAddr::V4(ip) if is_lan_addr(&ip) => Some(ip.to_string()),
            _ => None,
        })
        .collect();
    found.sort_by_key(|host| std::cmp::Reverse(address_rank(host)));
    if let Some(via) = route_addr() {
        if let Some(index) = found.iter().position(|host| host == &via) {
            let primary = found.remove(index);
            found.insert(0, primary);
        } else {
            found.insert(0, via);
        }
    }
    found
}

fn is_lan_addr(ip: &std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 172 && (16..=31).contains(&o[1]) && o[1] != 17)
}

fn address_rank(host: &str) -> u8 {
    let parts: Vec<u8> = host.split('.').filter_map(|part| part.parse().ok()).collect();
    match parts.as_slice() {
        [192, 168, ..] => 2,
        [10, ..] => 1,
        _ => 0,
    }
}

fn route_addr() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    match addr.ip() {
        IpAddr::V4(ip) if is_lan_addr(&ip) => Some(ip.to_string()),
        _ => None,
    }
}

fn get_if_addrs() -> Result<Vec<IpAddr>, String> {
    let mut addrs = if cfg!(windows) {
        get_if_addrs_windows().unwrap_or_default()
    } else {
        get_if_addrs_unix().unwrap_or_default()
    };
    if addrs.is_empty() {
        if let Some(via) = route_addr().and_then(|host| host.parse().ok()) {
            addrs.push(via);
        }
    }
    Ok(addrs)
}

fn get_if_addrs_unix() -> Result<Vec<IpAddr>, String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg("ifconfig 2>/dev/null | grep -o 'inet [0-9.]*' | awk '{print $2}' || ip -4 addr show 2>/dev/null | grep inet | awk '{print $2}' | cut -d/ -f1")
        .output()
        .map_err(|error| error.to_string())?;
    Ok(parse_addr_lines(&String::from_utf8_lossy(&output.stdout)))
}

fn get_if_addrs_windows() -> Result<Vec<IpAddr>, String> {
    let powershell = std::process::Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg("Get-NetIPAddress -AddressFamily IPv4 | Where-Object {$_.IPAddress -ne '127.0.0.1'} | Select-Object -ExpandProperty IPAddress")
        .output();
    let mut addrs = powershell
        .ok()
        .map(|output| parse_addr_lines(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default();
    if addrs.is_empty() {
        let output = std::process::Command::new("ipconfig")
            .output()
            .map_err(|error| error.to_string())?;
        addrs = parse_windows_ipconfig(&String::from_utf8_lossy(&output.stdout));
    }
    Ok(addrs)
}

fn parse_addr_lines(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| line.trim().parse::<IpAddr>().ok())
        .collect()
}

fn parse_windows_ipconfig(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("ipv4") || lower.contains("ip address")
        })
        .filter_map(|line| {
            line.rsplit_once(':')
                .or_else(|| line.rsplit_once('：'))
                .map(|(_, value)| value.trim())
                .and_then(|value| value.parse::<IpAddr>().ok())
        })
        .collect()
}

fn broadcast_targets() -> Vec<SocketAddr> {
    let mut targets = vec![SocketAddr::new(
        IpAddr::V4(Ipv4Addr::BROADCAST),
        LAN_DISCOVERY_PORT,
    )];
    for host in local_addrs() {
        let Ok(ip) = host.parse::<Ipv4Addr>() else {
            continue;
        };
        let o = ip.octets();
        let directed = Ipv4Addr::new(o[0], o[1], o[2], 255);
        targets.push(SocketAddr::new(IpAddr::V4(directed), LAN_DISCOVERY_PORT));
    }
    targets.sort();
    targets.dedup();
    targets
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
        let info = LanInfo {
            port,
            addrs: local_addrs(),
            nonce: config.fs_token.clone(),
        };
        let server = Arc::new(ServerTask(tokio::spawn(serve(
            listener,
            room_hash.clone(),
            config.blob_dir.clone(),
            config.fs_roots.clone(),
            config.fs_token.clone(),
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
        let mut info = self.inner.info.clone();
        let addrs = local_addrs();
        if !addrs.is_empty() {
            info.addrs = addrs;
        }
        info
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

pub fn push_blob(peers: &[LanPeer], room_hash: &str, name: &str, bytes: &[u8]) -> usize {
    if !valid_blob_name(name) || peers.is_empty() {
        return 0;
    }
    let path = format!("/{ROOT}/{room_hash}/{BLOB_SEGMENT}/{name}");
    let mut delivered = 0;
    for peer in peers {
        for host in &peer.addrs {
            if send_blocking(host, peer.port, &path, "PUT", bytes).is_ok() {
                delivered += 1;
                break;
            }
        }
    }
    delivered
}

pub fn blob_name_from_link(link: &str) -> Option<&str> {
    let marker = format!("/{BLOB_SEGMENT}/");
    let index = link.rfind(&marker)?;
    let name = link[index + marker.len()..].split('?').next()?.trim();
    valid_blob_name(name).then_some(name)
}

pub fn blob_name_for_sha(sha256: &str) -> String {
    format!("cs-{sha256}.bin")
}

pub fn read_local_blob(dir: &Path, link: &str, sha256: &str) -> Option<Vec<u8>> {
    if let Some(name) = blob_name_from_link(link) {
        if let Ok(bytes) = std::fs::read(dir.join(name)) {
            return Some(bytes);
        }
    }
    let by_sha = blob_name_for_sha(sha256);
    std::fs::read(dir.join(by_sha)).ok()
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
    let task = tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        let mut beacon = beacon;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(BEACON_INTERVAL) => {
                    let addrs = local_addrs();
                    if !addrs.is_empty() {
                        beacon.addrs = addrs;
                    }
                    let Ok(wire) = serde_json::to_vec(&beacon) else {
                        continue;
                    };
                    for target in broadcast_targets() {
                        let _ = socket.send_to(&wire, target).await;
                    }
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
    fs_roots: Vec<SharedRoot>,
    fs_token: String,
    clips: Sender<(String, String)>,
) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            break;
        };
        let hash = room_hash.clone();
        let dir = blob_dir.clone();
        let roots = fs_roots.clone();
        let token = fs_token.clone();
        let sink = clips.clone();
        tokio::spawn(async move {
            handle(stream, &hash, &dir, &roots, &token, sink).await;
        });
    }
}

async fn handle(
    mut stream: TcpStream,
    room_hash: &str,
    blob_dir: &Path,
    fs_roots: &[SharedRoot],
    fs_token: &str,
    clips: Sender<(String, String)>,
) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    let (path, query) = split_target(&request.path);
    let clip_path = format!("/{ROOT}/{room_hash}/clip");
    let blob_prefix = format!("/{ROOT}/{room_hash}/{BLOB_SEGMENT}/");
    let fs_prefix = format!("{LAN_FS_PATH}/");
    let response = if request.method == "POST" && path == clip_path {
        let _ = clips.send((clip_topic(room_hash), String::from_utf8_lossy(&request.body).into_owned()));
        Response::ok(b"ok")
    } else if path == LAN_FS_PATH || path.starts_with(&fs_prefix) {
        if request.method != "GET" || !fs_token_ok(&query, fs_token) {
            Response::status("403 Forbidden")
        } else if path == LAN_FS_PATH {
            Response::html(LAN_FS_HTML.as_bytes())
        } else if path == format!("{LAN_FS_PATH}/roots") {
            let entries: Vec<FsRootEntry> = fs_roots
                .iter()
                .enumerate()
                .map(|(index, root)| FsRootEntry {
                    name: root.name.clone(),
                    index,
                })
                .collect();
            Response::json(serde_json::to_vec(&entries).unwrap_or_default())
        } else if path == format!("{LAN_FS_PATH}/list") {
            let root = query
                .get("root")
                .and_then(|value| value.parse::<usize>().ok())
                .and_then(|index| fs_roots.get(index));
            let query_text = query.get("q").map(String::as_str).filter(|q| !q.trim().is_empty());
            match root.and_then(|root| {
                fs_list_json(root, query.get("path").map(String::as_str).unwrap_or_default(), query_text)
            }) {
                Some(body) => Response::json(body),
                None => Response::status("403 Forbidden"),
            }
        } else if path == format!("{LAN_FS_PATH}/file") {
            let root = query
                .get("root")
                .and_then(|value| value.parse::<usize>().ok())
                .and_then(|index| fs_roots.get(index));
            let resolved = root.and_then(|root| {
                fs_resolve(root, query.get("path").map(String::as_str).unwrap_or_default())
            });
            match resolved {
                Some(path) if path.is_file() => Response::file(&path),
                _ => Response::status("403 Forbidden"),
            }
        } else {
            Response::status("404 Not Found")
        }
    } else if let Some(name) = path.strip_prefix(&blob_prefix) {
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
    content_type: Option<&'static str>,
    content_length: u64,
    body: Vec<u8>,
    file: Option<PathBuf>,
}

impl Response {
    fn ok(body: &[u8]) -> Self {
        Self::body("200 OK", body.to_vec(), None)
    }

    fn html(body: &[u8]) -> Self {
        Self::body("200 OK", body.to_vec(), Some("text/html; charset=utf-8"))
    }

    fn json(body: Vec<u8>) -> Self {
        Self::body("200 OK", body, Some("application/json; charset=utf-8"))
    }

    fn file(path: &Path) -> Self {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => Self {
                status: "200 OK",
                content_type: Some("application/octet-stream"),
                content_length: meta.len(),
                body: Vec::new(),
                file: Some(path.to_path_buf()),
            },
            _ => Self::status("404 Not Found"),
        }
    }

    fn body(status: &'static str, body: Vec<u8>, content_type: Option<&'static str>) -> Self {
        Self {
            status,
            content_type,
            content_length: body.len() as u64,
            body,
            file: None,
        }
    }

    fn status(status: &'static str) -> Self {
        Self::body(status, Vec::new(), None)
    }

    async fn write(self, stream: &mut TcpStream) {
        let mut header = format!(
            "HTTP/1.1 {}\r\nContent-Length: {}\r\n",
            self.status, self.content_length
        );
        if let Some(kind) = self.content_type {
            header.push_str(&format!("Content-Type: {kind}\r\n"));
        }
        header.push_str("Connection: close\r\n\r\n");
        let _ = stream.write_all(header.as_bytes()).await;
        if let Some(path) = self.file {
            if let Ok(mut file) = File::open(path).await {
                let _ = copy(&mut file, stream).await;
            }
        } else {
            let _ = stream.write_all(&self.body).await;
        }
        let _ = stream.shutdown().await;
    }
}

fn split_target(raw: &str) -> (&str, HashMap<String, String>) {
    let Some((path, tail)) = raw.split_once('?') else {
        return (raw, HashMap::new());
    };
    let mut query = HashMap::new();
    for part in tail.split('&') {
        if let Some((key, value)) = part.split_once('=') {
            query.insert(percent_decode(key), percent_decode(value));
        }
    }
    (path, query)
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &raw[index + 1..index + 3];
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn fs_token_ok(query: &HashMap<String, String>, token: &str) -> bool {
    !token.is_empty() && query.get("token").is_some_and(|value| value == token)
}

fn fs_resolve(root: &SharedRoot, relative: &str) -> Option<PathBuf> {
    if relative.contains("..") {
        return None;
    }
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, Component::Prefix(_) | Component::RootDir | Component::ParentDir))
    {
        return None;
    }
    let base = std::fs::canonicalize(&root.path).ok()?;
    if !base.is_dir() {
        return None;
    }
    let target = std::fs::canonicalize(base.join(relative)).ok()?;
    (target == base || target.starts_with(&base)).then_some(target)
}

fn fs_relative(base: &Path, path: &Path) -> Option<String> {
    let raw = path.strip_prefix(base).ok()?.to_string_lossy();
    Some(raw.replace('\\', "/"))
}

fn fs_list_json(root: &SharedRoot, relative: &str, query: Option<&str>) -> Option<Vec<u8>> {
    let base = fs_resolve(root, relative)?;
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    match query {
        Some(query) => fs_walk(&base, &query.to_lowercase(), &mut dirs, &mut files),
        None => fs_entries(&base, &mut dirs, &mut files, None),
    }
    dirs.sort_by(|a, b| a.path.cmp(&b.path));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    serde_json::to_vec(&FsListEntry { dirs, files }).ok()
}

fn fs_entries(
    base: &Path,
    dirs: &mut Vec<FsDirEntry>,
    files: &mut Vec<FsFileEntry>,
    needle: Option<&str>,
) {
    let Ok(read) = std::fs::read_dir(base) else {
        return;
    };
    let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| {
        let kind = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        (!kind, entry.file_name().to_string_lossy().to_lowercase())
    });
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if needle.is_some_and(|value| !name.to_lowercase().contains(value)) {
            continue;
        }
        let Some(path) = fs_relative(base, &entry.path()) else {
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            dirs.push(FsDirEntry { name, path });
        } else if kind.is_file() {
            if let Ok(size) = entry.metadata().map(|meta| meta.len()) {
                files.push(FsFileEntry { name, size, path });
            }
        }
    }
}

fn fs_walk(
    base: &Path,
    needle: &str,
    dirs: &mut Vec<FsDirEntry>,
    files: &mut Vec<FsFileEntry>,
) {
    let Ok(read) = std::fs::read_dir(base) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            fs_walk(&entry.path(), needle, dirs, files);
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.to_lowercase().contains(needle) {
            continue;
        }
        let Some(path) = fs_relative(base, &entry.path()) else {
            continue;
        };
        if kind.is_dir() {
            dirs.push(FsDirEntry { name, path });
        } else if kind.is_file() {
            if let Ok(size) = entry.metadata().map(|meta| meta.len()) {
                files.push(FsFileEntry { name, size, path });
            }
        }
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
        let read = timeout(BLOB_TIMEOUT, stream.read(&mut chunk))
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

fn send_blocking(host: &str, port: u16, path: &str, method: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream as StdTcpStream;

    let addr = SocketAddr::new(
        host.parse()
            .map_err(|error| format!("地址无效：{error}"))?,
        port,
    );
    let mut stream = StdTcpStream::connect_timeout(&addr, BLOB_TIMEOUT)
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(BLOB_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(BLOB_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let header = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .map_err(|error| error.to_string())?;
    stream
        .write_all(body)
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
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
    #[ignore = "needs real UDP broadcast sockets; run locally only"]
    async fn clip_and_blob_cross_the_lan() {
        let dir = temp_dir("clip");
        let (tx, rx) = async_channel::unbounded();
        let (peer_tx, _peer_rx) = async_channel::unbounded();
        let runtime = LanRuntime::start(
            LanConfig {
                port: 0,
                blob_dir: dir.clone(),
                fs_roots: Vec::new(),
                fs_token: "test-token".to_string(),
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
            nonce: "test-token".to_string(),
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
                nonce: String::new(),
            },
            "hash",
            "../secret",
        )
        .await
        .is_err());
    }
}
