use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::envelope::{ClipEnvelope, ClipPayload, PresenceEnvelope};
use crate::link::{self, LinkEvent, LinkHandle, MqttLink};
use crate::protocol::{
    clip_topic, now_millis, parse_topic, presence_topic, room_filter, room_hash, safety_prefix,
    sync_topic, text_mid, utf16_len, Channel, PROTOCOL_VERSION,
};
use crate::room::RoomCrypto;

pub const PRESENCE_INTERVAL: Duration = Duration::from_millis(120_000);
pub const PRESENCE_TTL: Duration = Duration::from_millis(360_000);
pub const RECOVERY_BASE: Duration = Duration::from_millis(5_000);
pub const RECOVERY_MAX: Duration = Duration::from_millis(60_000);
pub const KIND_TEXT: &str = "text";

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
    pub safety: String,
    pub os: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub mid: String,
    pub text: String,
    pub sender_id: String,
    pub sender_name: String,
    pub ts: i64,
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Status(Status),
    Clip(Clip),
    Peers(Vec<Peer>),
    KeyMismatch(Vec<String>),
}

#[derive(Debug)]
pub enum Command {
    Join(Config),
    Leave,
    PublishText(String),
    Rename(String),
    RetryNow,
}

pub trait LinkFactory: Send + Sync + 'static {
    type Link: MqttLink + Send + Sync + 'static;
    fn create(&self, config: &Config, hash: &str, safety_prefix: &str) -> Self::Link;
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
    safety_prefix: String,
    status: Status,
    link: Option<LinkHandle>,
    inbound: Option<async_channel::Receiver<LinkEvent>>,
    peers: HashMap<String, Peer>,
    seen: HashMap<String, Instant>,
    presence_ts: HashMap<String, i64>,
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
            safety_prefix: String::new(),
            status: Status::Idle,
            link: None,
            inbound: None,
            peers: HashMap::new(),
            seen: HashMap::new(),
            presence_ts: HashMap::new(),
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
        self.safety_prefix = safety_prefix(&self.safety);
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
        let created = self.links.create(config, &self.hash, &self.safety_prefix);
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
            Channel::Presence => self.on_presence(&parsed, payload).await,
            Channel::Sync => self.on_roll_call(payload).await,
        }
    }

    async fn on_clip(&mut self, topic: &str, raw: &str) {
        let (Some(crypto), Some(config)) = (self.crypto.as_ref(), self.desired.as_ref()) else {
            return;
        };
        let Ok(envelope) = serde_json::from_str::<ClipEnvelope>(raw) else {
            return;
        };
        if envelope.v != PROTOCOL_VERSION || envelope.kind != KIND_TEXT {
            return;
        }
        if envelope.dev == config.device_id {
            return;
        }
        if let Some(peer) = self.peers.get(&envelope.dev) {
            self.seen.insert(peer.id.clone(), Instant::now());
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
        }));
    }

    async fn on_presence(&mut self, parsed: &crate::RoomTopic, raw: &str) {
        let Some(config) = self.desired.clone() else {
            return;
        };
        if parsed.device_id.as_deref() == Some(config.device_id.as_str()) {
            return;
        }
        let Ok(envelope) = serde_json::from_str::<PresenceEnvelope>(raw) else {
            return;
        };
        if envelope.dev == config.device_id {
            return;
        }
        if let Some(last) = self.presence_ts.get(&envelope.dev) {
            if envelope.ts < *last {
                return;
            }
        }
        self.presence_ts.insert(envelope.dev.clone(), envelope.ts);
        let fresh = !self.peers.contains_key(&envelope.dev);
        if envelope.online {
            self.seen.insert(envelope.dev.clone(), Instant::now());
            self.peers.insert(
                envelope.dev.clone(),
                Peer {
                    id: envelope.dev.clone(),
                    name: envelope.name,
                    safety: envelope.sp,
                    os: envelope.os,
                },
            );
        } else {
            self.peers.remove(&envelope.dev);
            self.seen.remove(&envelope.dev);
            self.presence_ts.remove(&envelope.dev);
        }
        self.prune_peers();
        self.publish_peers();
        if fresh && envelope.online {
            self.publish_presence(true).await;
        }
    }

    async fn on_roll_call(&mut self, raw: &str) {
        let Some(config) = self.desired.as_ref() else {
            return;
        };
        let Ok(envelope) = serde_json::from_str::<PresenceEnvelope>(raw) else {
            return;
        };
        if envelope.dev == config.device_id {
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
            mid: text_mid(&payload, &config.device_id),
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
        };
        // 先发历史：那条复制不能因为发不出去就当没发生过。
        let _ = self.events.send(Event::Clip(clip));
        if let Some(handle) = self.link.clone() {
            let _ = handle.publish(&topic, &wire).await;
        }
    }

    async fn publish_presence(&mut self, online: bool) {
        let Some(config) = self.desired.clone() else {
            return;
        };
        let payload = serde_json::to_string(&PresenceEnvelope {
            dev: config.device_id.clone(),
            name: config.device_name.clone(),
            online,
            ts: now_millis(),
            sp: self.safety_prefix.clone(),
            os: config.device_os.clone(),
            ..Default::default()
        })
        .unwrap();
        if let Some(handle) = self.link.clone() {
            let _ = handle
                .publish(&presence_topic(&self.hash, &config.device_id), &payload)
                .await;
        }
    }

    async fn roll_call(&mut self) {
        let Some(config) = self.desired.clone() else {
            return;
        };
        let payload = serde_json::to_string(&PresenceEnvelope {
            dev: config.device_id.clone(),
            name: config.device_name.clone(),
            online: true,
            ts: now_millis(),
            sp: self.safety_prefix.clone(),
            os: config.device_os.clone(),
            answer: true,
            ..Default::default()
        })
        .unwrap();
        if let Some(handle) = self.link.clone() {
            let _ = handle.publish(&sync_topic(&self.hash), &payload).await;
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
        for id in gone {
            self.peers.remove(&id);
            self.seen.remove(&id);
            self.presence_ts.remove(&id);
        }
    }

    fn publish_peers(&mut self) {
        let mut peers: Vec<Peer> = self
            .peers
            .values()
            .filter(|peer| peer.safety == self.safety_prefix)
            .cloned()
            .collect();
        peers.sort_by(|a, b| a.name.cmp(&b.name));
        let mut mismatch: Vec<String> = self
            .peers
            .values()
            .filter(|peer| !peer.safety.is_empty() && peer.safety != self.safety_prefix)
            .map(|peer| peer.name.clone())
            .collect();
        mismatch.sort();
        let _ = self.events.send(Event::Peers(peers));
        let _ = self.events.send(Event::KeyMismatch(mismatch));
    }

    fn clear_peers(&mut self) {
        self.peers.clear();
        self.seen.clear();
        self.presence_ts.clear();
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
