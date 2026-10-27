use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::envelope::{ClipEnvelope, ClipPayload, FilePayload, PresenceEnvelope, PresencePayload};
use crate::files::{sanitize_file_name, MAX_FILE_BYTES};
use crate::link::{self, LinkEvent, LinkHandle, MqttLink};
use crate::protocol::{
    clip_topic, now_millis, parse_topic, presence_topic, random_mid, room_filter, room_hash,
    sync_topic, utf16_len, Channel, KIND_FILE, PROTOCOL_VERSION,
};
use crate::room::RoomCrypto;

pub const PRESENCE_INTERVAL: Duration = Duration::from_millis(120_000);
pub const PRESENCE_TTL: Duration = Duration::from_millis(360_000);
pub const RECOVERY_BASE: Duration = Duration::from_millis(5_000);
pub const RECOVERY_MAX: Duration = Duration::from_millis(60_000);
pub const KIND_TEXT: &str = "text";
// 密钥不一致的名单是陌生人也能往我们频道灌帧喂出来的，给它一个上限
const MAX_UNKNOWN_KEY: usize = 8;
const MISMATCH_ID_CHARS: usize = 8;

#[derive(Debug, Clone)]
pub struct Config {
    pub room_id: String,
    pub password: String,
    pub room_key: Option<Vec<u8>>,
    pub device_id: String,
    pub device_name: String,
    pub device_os: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Connecting,
    Online {
        safety: String,
    },
    Offline {
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub os: String,
}

/// 解不开的那批：署名先从 clip 信封的明文里补，补不到只剩设备号前缀。
struct Mismatch {
    seen: Instant,
    label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAttachment {
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
    pub link: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub mid: String,
    pub text: String,
    pub sender_id: String,
    pub sender_name: String,
    pub ts: i64,
    pub mine: bool,
    pub file: Option<FileAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileUpload {
    pub mid: String,
    pub ts: i64,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
    pub link: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Status(Status),
    Clip(Clip),
    Peers(Vec<Peer>),
    KeyMismatch(Vec<String>),
    FileTooLarge(String),
}

#[derive(Debug)]
pub enum Command {
    Join(Config),
    Leave,
    PublishText(String),
    PublishFile { file: FileUpload, announce: bool },
    Rename(String),
    RetryNow,
}

pub trait LinkFactory: Send + Sync + 'static {
    type Link: MqttLink + Send + Sync + 'static;
    fn create(&self, config: &Config, hash: &str, will_payload: &str) -> Self::Link;
}

pub struct EngineHandle {
    commands: mpsc::UnboundedSender<Command>,
    pub events: mpsc::UnboundedReceiver<Event>,
}

impl EngineHandle {
    pub fn join(&self, config: Config) {
        let _ = self.commands.send(Command::Join(config));
    }

    pub fn publish_text(&self, text: &str) {
        let _ = self.commands.send(Command::PublishText(text.to_string()));
    }

    pub fn publish_file(&self, file: FileUpload, announce: bool) {
        let _ = self.commands.send(Command::PublishFile { file, announce });
    }

    pub fn rename(&self, name: &str) {
        let _ = self.commands.send(Command::Rename(name.to_string()));
    }

    pub fn retry_now(&self) {
        let _ = self.commands.send(Command::RetryNow);
    }

    pub fn leave(&self) {
        let _ = self.commands.send(Command::Leave);
    }
}

pub fn spawn<F: LinkFactory>(links: F) -> EngineHandle {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (events, out) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        Engine::new(links, events).run(command_rx).await;
    });
    EngineHandle {
        commands,
        events: out,
    }
}

struct Engine<F: LinkFactory> {
    links: F,
    events: mpsc::UnboundedSender<Event>,
    desired: Option<Config>,
    crypto: Option<RoomCrypto>,
    hash: String,
    safety: String,
    status: Status,
    link: Option<LinkHandle>,
    inbound: Option<async_channel::Receiver<LinkEvent>>,
    peers: HashMap<String, Peer>,
    seen: HashMap<String, Instant>,
    presence_ts: HashMap<String, i64>,
    unknown_key: HashMap<String, Mismatch>,
    backoff: Duration,
}

impl<F: LinkFactory> Engine<F> {
    fn new(links: F, events: mpsc::UnboundedSender<Event>) -> Self {
        Self {
            links,
            events,
            desired: None,
            crypto: None,
            hash: String::new(),
            safety: String::new(),
            status: Status::Idle,
            link: None,
            inbound: None,
            peers: HashMap::new(),
            seen: HashMap::new(),
            presence_ts: HashMap::new(),
            unknown_key: HashMap::new(),
            backoff: RECOVERY_BASE,
        }
    }

    async fn run(&mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        let mut heartbeat: Option<Instant> = None;
        let mut recovery: Option<Instant> = None;
        loop {
            let inbound = self.inbound.clone();
            let received = async move {
                match inbound {
                    Some(receiver) => receiver.recv().await.ok(),
                    None => std::future::pending::<Option<LinkEvent>>().await,
                }
            };
            let online = self.is_online();
            let want_room = self.desired.is_some();
            let heartbeat_at = heartbeat;
            let recovery_at = recovery;
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    self.on_command(command, &mut heartbeat, &mut recovery).await;
                }
                event = received => {
                    match event {
                        Some(event) => self.on_link_event(event, &mut heartbeat, &mut recovery).await,
                        None => self.inbound = None,
                    }
                }
                _ = wait_until(heartbeat_at), if self.link.is_some() => {
                    self.prune_peers();
                    self.publish_peers();
                    if self.link.as_ref().is_some_and(|handle| handle.is_connected()) {
                        self.publish_presence(true).await;
                    }
                    heartbeat = Some(Instant::now() + PRESENCE_INTERVAL);
                }
                _ = wait_until(recovery_at), if want_room && !online => {
                    self.recover(&mut heartbeat, &mut recovery).await;
                }
            }
        }
        self.close_link().await;
    }

    async fn on_command(
        &mut self,
        command: Command,
        heartbeat: &mut Option<Instant>,
        recovery: &mut Option<Instant>,
    ) {
        match command {
            Command::Join(config) => self.join(config, heartbeat, recovery).await,
            Command::Leave => {
                if self.is_online() {
                    self.publish_presence(false).await;
                }
                self.desired = None;
                self.close_link().await;
                self.set_status(Status::Idle, recovery);
            }
            Command::PublishText(text) => self.publish_text(&text).await,
            Command::PublishFile { file, announce } => self.publish_file(file, announce).await,
            Command::Rename(name) => {
                let name = name.trim().to_string();
                let changed = match self.desired.as_mut() {
                    Some(config) if !name.is_empty() && config.device_name != name => {
                        config.device_name = name;
                        true
                    }
                    _ => false,
                };
                if changed {
                    self.publish_presence(true).await;
                }
            }
            Command::RetryNow => {
                if self.desired.is_some() && !self.is_online() {
                    *recovery = Some(Instant::now());
                }
            }
        }
    }

    async fn join(
        &mut self,
        config: Config,
        heartbeat: &mut Option<Instant>,
        recovery: &mut Option<Instant>,
    ) {
        self.close_link().await;
        self.desired = Some(config);
        self.set_status(Status::Connecting, recovery);
        let config = self.desired.clone().unwrap();
        let hash = room_hash(&config.room_id);
        let crypto = match config.room_key.as_deref() {
            Some(key) => RoomCrypto::restore(key),
            None => RoomCrypto::open(&config.room_id, &config.password),
        };
        let crypto = match crypto {
            Ok(crypto) => crypto,
            Err(error) => {
                self.desired = None;
                self.set_status(
                    Status::Offline {
                        reason: Some(error.to_string()),
                    },
                    recovery,
                );
                return;
            }
        };
        self.safety = crypto.safety_number();
        self.crypto = Some(crypto);
        self.hash = hash;
        self.open_link(&config, heartbeat, recovery).await;
    }

    async fn open_link(
        &mut self,
        config: &Config,
        heartbeat: &mut Option<Instant>,
        recovery: &mut Option<Instant>,
    ) {
        let created = self
            .links
            .create(config, &self.hash, &self.will_wire(config));
        let (handle, events, boot) = link::spawn(created);
        self.link = Some(handle);
        self.inbound = Some(events);
        let connected = boot
            .await
            .unwrap_or_else(|_| Err("link task stopped".to_string()));
        let hash = self.hash.clone();
        let outcome = async {
            connected?;
            self.link
                .as_ref()
                .unwrap()
                .subscribe(&room_filter(&hash))
                .await?;
            Ok::<(), String>(())
        };
        if let Err(reason) = outcome.await {
            self.close_link().await;
            self.set_status(
                Status::Offline {
                    reason: Some(reason),
                },
                recovery,
            );
            self.arm_recovery(recovery);
            return;
        }
        self.publish_presence(true).await;
        self.roll_call().await;
        self.backoff = RECOVERY_BASE;
        self.set_status(
            Status::Online {
                safety: self.safety.clone(),
            },
            recovery,
        );
        *heartbeat = Some(Instant::now() + PRESENCE_INTERVAL);
    }

    async fn on_link_event(
        &mut self,
        event: LinkEvent,
        heartbeat: &mut Option<Instant>,
        recovery: &mut Option<Instant>,
    ) {
        match event {
            LinkEvent::Connected => {
                if !self.is_online() {
                    self.backoff = RECOVERY_BASE;
                    self.set_status(
                        Status::Online {
                            safety: self.safety.clone(),
                        },
                        recovery,
                    );
                    self.publish_presence(true).await;
                    self.roll_call().await;
                    *heartbeat = Some(Instant::now() + PRESENCE_INTERVAL);
                }
            }
            LinkEvent::Disconnected => {
                if self.desired.is_some() {
                    self.clear_peers();
                    self.set_status(Status::Offline { reason: None }, recovery);
                    self.arm_recovery(recovery);
                }
            }
            LinkEvent::Message { topic, payload } => self.on_message(&topic, &payload).await,
        }
    }

    async fn on_message(&mut self, topic: &str, payload: &str) {
        let Some(parsed) = parse_topic(topic) else {
            return;
        };
        match parsed.channel {
            Channel::Clip => self.on_clip(topic, payload).await,
            Channel::Presence => self.on_presence(topic, &parsed, payload).await,
            Channel::Sync => self.on_roll_call(topic, payload).await,
        }
    }

    async fn on_clip(&mut self, topic: &str, raw: &str) {
        let Ok(envelope) = serde_json::from_str::<ClipEnvelope>(raw) else {
            return;
        };
        if envelope.v != PROTOCOL_VERSION {
            return;
        }
        if self
            .desired
            .as_ref()
            .is_some_and(|config| config.device_id == envelope.dev)
        {
            return;
        }
        if let Some(peer) = self.peers.get(&envelope.dev) {
            self.seen.insert(peer.id.clone(), Instant::now());
        }
        let mut labeled = false;
        if !envelope.name.is_empty() {
            if let Some(mismatch) = self.unknown_key.get_mut(&envelope.dev) {
                mismatch.label = envelope.name.clone();
                labeled = true;
            }
        }
        if labeled {
            self.publish_peers();
        }
        let Some(crypto) = self.crypto.as_ref() else {
            return;
        };
        if envelope.kind == KIND_FILE {
            self.on_file_clip(&envelope, crypto, topic);
            return;
        }
        if envelope.kind != KIND_TEXT {
            return;
        }
        let Ok(plain) = crypto.decrypt(&envelope.ct, topic.as_bytes()) else {
            return;
        };
        let Ok(payload) = serde_json::from_str::<ClipPayload>(&String::from_utf8_lossy(&plain))
        else {
            return;
        };
        let _ = self.events.send(Event::Clip(Clip {
            mid: envelope.mid,
            text: payload.text,
            sender_id: envelope.dev,
            sender_name: envelope.name,
            ts: envelope.ts,
            mine: false,
            file: None,
        }));
    }

    fn on_file_clip(&self, envelope: &ClipEnvelope, crypto: &RoomCrypto, topic: &str) {
        let Ok(plain) = crypto.decrypt(&envelope.ct, topic.as_bytes()) else {
            return;
        };
        let Ok(payload) = serde_json::from_str::<FilePayload>(&String::from_utf8_lossy(&plain))
        else {
            return;
        };
        if payload.size > MAX_FILE_BYTES {
            let _ = self
                .events
                .send(Event::FileTooLarge(sanitize_file_name(&payload.name)));
            return;
        }
        let display = sanitize_file_name(&payload.name);
        let _ = self.events.send(Event::Clip(Clip {
            mid: envelope.mid.clone(),
            text: display,
            sender_id: envelope.dev.clone(),
            sender_name: envelope.name.clone(),
            ts: envelope.ts,
            mine: false,
            file: Some(FileAttachment {
                name: payload.name,
                mime: payload.mime,
                size: payload.size,
                sha256: payload.sha256,
                link: payload.link,
                expires_at: payload.expires_at,
            }),
        }));
    }

    async fn on_presence(&mut self, topic: &str, parsed: &crate::RoomTopic, raw: &str) {
        let (Some(config), Some(crypto)) = (self.desired.clone(), self.crypto.as_ref()) else {
            return;
        };
        let Some(device_id) = parsed.device_id.clone() else {
            return;
        };
        if device_id == config.device_id {
            return;
        }
        let Ok(envelope) = serde_json::from_str::<PresenceEnvelope>(raw) else {
            return;
        };
        if envelope.v != PROTOCOL_VERSION {
            return;
        }
        let Ok(plain) = crypto.decrypt(&envelope.ct, topic.as_bytes()) else {
            // 像一条正经 presence 却解不开：同一间房、不同钥匙，报出来让人去对安全码
            self.note_unknown_key(&device_id);
            return;
        };
        let Ok(payload) = serde_json::from_str::<PresencePayload>(&String::from_utf8_lossy(&plain))
        else {
            return;
        };
        if payload.dev == config.device_id {
            return;
        }
        if self.unknown_key.remove(&device_id).is_some() {
            self.publish_peers();
        }
        if let Some(last) = self.presence_ts.get(&payload.dev) {
            if payload.ts < *last {
                return;
            }
        }
        self.presence_ts.insert(payload.dev.clone(), payload.ts);
        let fresh = !self.peers.contains_key(&payload.dev);
        if payload.online {
            self.seen.insert(payload.dev.clone(), Instant::now());
            self.peers.insert(
                payload.dev.clone(),
                Peer {
                    id: payload.dev.clone(),
                    name: payload.name,
                    os: payload.os,
                },
            );
        } else {
            self.peers.remove(&payload.dev);
            self.seen.remove(&payload.dev);
            self.presence_ts.remove(&payload.dev);
            self.unknown_key.remove(&payload.dev);
        }
        self.prune_peers();
        self.publish_peers();
        if fresh && payload.online {
            self.publish_presence(true).await;
        }
    }

    async fn on_roll_call(&mut self, topic: &str, raw: &str) {
        let (Some(config), Some(crypto)) = (self.desired.as_ref(), self.crypto.as_ref()) else {
            return;
        };
        let Ok(envelope) = serde_json::from_str::<PresenceEnvelope>(raw) else {
            return;
        };
        if envelope.v != PROTOCOL_VERSION {
            return;
        }
        // sync 频道没有设备号，解不开就只当没听见；那台设备随后会在自己的 presence 频道报上来
        let Ok(plain) = crypto.decrypt(&envelope.ct, topic.as_bytes()) else {
            return;
        };
        let Ok(payload) = serde_json::from_str::<PresencePayload>(&String::from_utf8_lossy(&plain))
        else {
            return;
        };
        if payload.dev == config.device_id {
            return;
        }
        self.publish_presence(true).await;
    }

    async fn publish_text(&mut self, text: &str) {
        let (Some(config), Some(crypto)) = (self.desired.clone(), self.crypto.as_ref()) else {
            return;
        };
        let topic = clip_topic(&self.hash);
        let Ok(payload) = serde_json::to_string(&ClipPayload {
            text: text.to_string(),
            len: utf16_len(text) as i32,
        }) else {
            return;
        };
        let ct = match crypto.encrypt(payload.as_bytes(), topic.as_bytes()) {
            Ok(ct) => ct,
            Err(_) => return,
        };
        let envelope = ClipEnvelope {
            mid: random_mid(),
            ts: now_millis(),
            dev: config.device_id.clone(),
            name: config.device_name.clone(),
            kind: KIND_TEXT.to_string(),
            ct,
            ..Default::default()
        };
        let wire = serde_json::to_string(&envelope).unwrap();
        let clip = Clip {
            mid: envelope.mid.clone(),
            text: text.to_string(),
            sender_id: envelope.dev.clone(),
            sender_name: envelope.name.clone(),
            ts: envelope.ts,
            mine: true,
            file: None,
        };
        // 先发历史：那条复制不能因为发不出去就当没发生过。
        let _ = self.events.send(Event::Clip(clip));
        if let Some(handle) = self.link.clone() {
            let _ = handle.publish(&topic, &wire).await;
        }
    }

    async fn publish_file(&mut self, file: FileUpload, announce: bool) {
        let (Some(config), Some(crypto)) = (self.desired.clone(), self.crypto.as_ref()) else {
            return;
        };
        let topic = clip_topic(&self.hash);
        let payload = FilePayload {
            name: file.name.clone(),
            mime: file.mime.clone(),
            size: file.size,
            sha256: file.sha256.clone(),
            link: file.link.clone(),
            expires_at: file.expires_at,
        };
        let Ok(json) = serde_json::to_string(&payload) else {
            return;
        };
        let Ok(ct) = crypto.encrypt(json.as_bytes(), topic.as_bytes()) else {
            return;
        };
        let envelope = ClipEnvelope {
            mid: file.mid.clone(),
            ts: file.ts,
            dev: config.device_id.clone(),
            name: config.device_name.clone(),
            kind: KIND_FILE.to_string(),
            ct,
            ..Default::default()
        };
        let wire = serde_json::to_string(&envelope).unwrap();
        if announce {
            let _ = self.events.send(Event::Clip(Clip {
                mid: file.mid,
                text: file.name,
                sender_id: envelope.dev.clone(),
                sender_name: envelope.name.clone(),
                ts: envelope.ts,
                mine: true,
                file: Some(FileAttachment {
                    name: payload.name,
                    mime: payload.mime,
                    size: payload.size,
                    sha256: payload.sha256,
                    link: payload.link,
                    expires_at: payload.expires_at,
                }),
            }));
        }
        if let Some(handle) = self.link.clone() {
            let _ = handle.publish(&topic, &wire).await;
        }
    }

    /// presence 整条进密文，AAD 用它落进的那条 topic：点名帧在 sync 频道，正常帧在各自的 presence 频道。
    fn presence_wire(
        &self,
        topic: &str,
        config: &Config,
        online: bool,
        answer: bool,
    ) -> Option<String> {
        let crypto = self.crypto.as_ref()?;
        let json = serde_json::to_string(&PresencePayload {
            dev: config.device_id.clone(),
            name: config.device_name.clone(),
            os: config.device_os.clone(),
            online,
            ts: now_millis(),
            answer,
        })
        .ok()?;
        let ct = crypto.encrypt(json.as_bytes(), topic.as_bytes()).ok()?;
        serde_json::to_string(&PresenceEnvelope {
            ct,
            ..Default::default()
        })
        .ok()
    }

    /// 遗嘱在握手时就定死内容：断线后由 broker 原样发出去，所以这里得先加密好。
    fn will_wire(&self, config: &Config) -> String {
        let topic = presence_topic(&self.hash, &config.device_id);
        self.presence_wire(&topic, config, false, false)
            .unwrap_or_default()
    }

    async fn publish_presence(&mut self, online: bool) {
        let Some(config) = self.desired.clone() else {
            return;
        };
        let topic = presence_topic(&self.hash, &config.device_id);
        let Some(wire) = self.presence_wire(&topic, &config, online, false) else {
            return;
        };
        if let Some(handle) = self.link.clone() {
            let _ = handle.publish(&topic, &wire).await;
        }
    }

    async fn roll_call(&mut self) {
        let Some(config) = self.desired.clone() else {
            return;
        };
        let topic = sync_topic(&self.hash);
        let Some(wire) = self.presence_wire(&topic, &config, true, true) else {
            return;
        };
        if let Some(handle) = self.link.clone() {
            let _ = handle.publish(&topic, &wire).await;
        }
    }

    fn prune_peers(&mut self) {
        let cutoff = Instant::now() - PRESENCE_TTL;
        let gone: Vec<String> = self
            .seen
            .iter()
            .filter(|(_, seen)| **seen < cutoff)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &gone {
            self.peers.remove(id);
            self.seen.remove(id);
            self.presence_ts.remove(id);
        }
        let stale: Vec<String> = self
            .unknown_key
            .iter()
            .filter(|(_, m)| m.seen < cutoff)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &stale {
            self.unknown_key.remove(id);
        }
        if !gone.is_empty() || !stale.is_empty() {
            self.publish_peers();
        }
    }

    /// 往我们频道灌一条像模像样却解不开的 presence，代价是把名单灌到上限为止。
    fn note_unknown_key(&mut self, device_id: &str) {
        if self.unknown_key.contains_key(device_id) || self.unknown_key.len() >= MAX_UNKNOWN_KEY {
            return;
        }
        self.unknown_key.insert(
            device_id.to_string(),
            Mismatch {
                seen: Instant::now(),
                label: device_id.chars().take(MISMATCH_ID_CHARS).collect(),
            },
        );
        self.publish_peers();
    }

    fn publish_peers(&mut self) {
        let mut peers: Vec<Peer> = self.peers.values().cloned().collect();
        peers.sort_by(|a, b| a.name.cmp(&b.name));
        let mut mismatch: Vec<String> =
            self.unknown_key.values().map(|m| m.label.clone()).collect();
        mismatch.sort();
        mismatch.truncate(MAX_UNKNOWN_KEY);
        let _ = self.events.send(Event::Peers(peers));
        let _ = self.events.send(Event::KeyMismatch(mismatch));
    }

    fn clear_peers(&mut self) {
        self.peers.clear();
        self.seen.clear();
        self.presence_ts.clear();
        self.unknown_key.clear();
        self.publish_peers();
    }

    fn set_status(&mut self, status: Status, recovery: &mut Option<Instant>) {
        if status == self.status {
            return;
        }
        self.status = status.clone();
        if !self.is_online() {
            *recovery = None;
        }
        let _ = self.events.send(Event::Status(status));
    }

    fn is_online(&self) -> bool {
        matches!(self.status, Status::Online { .. })
    }

    fn arm_recovery(&mut self, recovery: &mut Option<Instant>) {
        if self.desired.is_some() {
            *recovery = Some(Instant::now() + self.backoff);
        }
    }

    async fn recover(&mut self, heartbeat: &mut Option<Instant>, recovery: &mut Option<Instant>) {
        let Some(config) = self.desired.clone() else {
            return;
        };
        if self
            .link
            .as_ref()
            .is_some_and(|handle| handle.is_connected())
        {
            return;
        }
        self.backoff = (self.backoff * 2).min(RECOVERY_MAX);
        self.close_link().await;
        self.open_link(&config, heartbeat, recovery).await;
    }

    async fn close_link(&mut self) {
        self.inbound = None;
        if let Some(handle) = self.link.take() {
            let _ = handle.shutdown().await;
        }
        self.clear_peers();
    }
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}
