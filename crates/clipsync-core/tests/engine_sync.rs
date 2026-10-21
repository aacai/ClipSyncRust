use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_channel::{unbounded, Receiver, Sender};

use clipsync_core::engine::{self, Config, Event, FileUpload, LinkFactory, Status};
use clipsync_core::files::MAX_FILE_BYTES;
use clipsync_core::link::{LinkEvent, MqttLink};
use clipsync_core::protocol::{
    clip_topic, file_mid, presence_topic, room_filter, room_hash, sync_topic, PROTOCOL_VERSION,
};
use clipsync_core::room::RoomCrypto;
use clipsync_core::{ClipEnvelope, ClipPayload, FilePayload, PresenceEnvelope};

const ROOM: &str = "TESTROOM01";
const ME: &str = "dev-me";

#[derive(Default)]
struct Broker {
    published: Mutex<Vec<(String, String)>>,
    filters: Mutex<Vec<String>>,
    inject: Mutex<Option<Sender<LinkEvent>>>,
    links: AtomicUsize,
    refuse_connect: AtomicBool,
    down: AtomicBool,
}

impl Broker {
    fn published(&self) -> Vec<(String, String)> {
        self.published.lock().unwrap().clone()
    }

    fn filters(&self) -> Vec<String> {
        self.filters.lock().unwrap().clone()
    }

    fn links(&self) -> usize {
        self.links.load(Ordering::SeqCst)
    }

    fn inject(&self, event: LinkEvent) {
        let sender = self
            .inject
            .lock()
            .unwrap()
            .clone()
            .expect("no link is open");
        sender.try_send(event).expect("link pump stopped");
    }
}

#[derive(Clone, Default)]
struct FakeLinks {
    broker: Arc<Broker>,
}

struct FakeLink {
    broker: Arc<Broker>,
    events: Receiver<LinkEvent>,
    inject: Sender<LinkEvent>,
    connected: Arc<AtomicBool>,
}

impl LinkFactory for FakeLinks {
    type Link = FakeLink;

    fn create(&self, _config: &Config, _hash: &str, _safety_prefix: &str) -> FakeLink {
        let (inject, events) = unbounded();
        *self.broker.inject.lock().unwrap() = Some(inject.clone());
        self.broker.links.fetch_add(1, Ordering::SeqCst);
        FakeLink {
            broker: Arc::clone(&self.broker),
            events,
            inject,
            connected: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl MqttLink for FakeLink {
    async fn connect(&self) -> Result<(), String> {
        if self.broker.refuse_connect.swap(false, Ordering::SeqCst) {
            return Err("refused".to_string());
        }
        self.connected.store(true, Ordering::SeqCst);
        let _ = self.inject.try_send(LinkEvent::Connected);
        Ok(())
    }

    async fn subscribe(&self, filter: &str) -> Result<(), String> {
        self.broker.filters.lock().unwrap().push(filter.to_string());
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: &str) -> Result<(), String> {
        if self.broker.down.load(Ordering::SeqCst) || !self.connected.load(Ordering::SeqCst) {
            return Err("not connected".to_string());
        }
        self.broker
            .published
            .lock()
            .unwrap()
            .push((topic.to_string(), payload.to_string()));
        Ok(())
    }

    async fn next(&self) -> Option<LinkEvent> {
        self.events.recv().await.ok()
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn close(&self) {
        self.connected.store(false, Ordering::SeqCst);
    }
}

struct Harness {
    engine: engine::EngineHandle,
    broker: Arc<Broker>,
    hash: String,
    safety: String,
    prefix: String,
}

fn config() -> Config {
    Config {
        room_id: ROOM.to_string(),
        password: String::new(),
        room_key: None,
        device_id: ME.to_string(),
        device_name: "Me".to_string(),
        device_os: "rust".to_string(),
    }
}

fn crypto() -> RoomCrypto {
    RoomCrypto::open(ROOM, "").expect("room key")
}

impl Harness {
    async fn start() -> Self {
        let broker = Arc::new(Broker::default());
        let engine = engine::spawn(FakeLinks {
            broker: Arc::clone(&broker),
        });
        let safety = crypto().safety_number();
        let prefix = safety.split(' ').next().unwrap_or_default().to_string();
        let hash = room_hash(ROOM);
        let harness = Self {
            engine,
            broker,
            hash,
            safety,
            prefix,
        };
        harness.engine.join(config());
        harness
    }

    async fn next(&mut self) -> Event {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.engine.events.recv())
            .await
            .expect("no event")
            .expect("engine stopped")
    }

    async fn online(&mut self) {
        loop {
            match self.next().await {
                Event::Status(Status::Online { safety }) => {
                    assert_eq!(safety, self.safety);
                    return;
                }
                Event::Status(Status::Connecting) => {}
                Event::Status(other) => panic!("unexpected status {other:?}"),
                _ => {}
            }
        }
    }

    async fn status(&mut self) -> Status {
        loop {
            if let Event::Status(status) = self.next().await {
                return status;
            }
        }
    }

    fn presence(&self, dev: &str, name: &str, online: bool) -> (String, String) {
        let payload = serde_json::to_string(&PresenceEnvelope {
            v: PROTOCOL_VERSION,
            dev: dev.to_string(),
            name: name.to_string(),
            online,
            ts: clipsync_core::protocol::now_millis(),
            sp: self.prefix.clone(),
            os: "kotlin".to_string(),
            answer: false,
        })
        .unwrap();
        (presence_topic(&self.hash, dev), payload)
    }

    fn clip(&self, dev: &str, name: &str, text: &str, with: &RoomCrypto) -> (String, String) {
        let topic = clip_topic(&self.hash);
        let payload = serde_json::to_string(&ClipPayload {
            text: text.to_string(),
            len: clipsync_core::protocol::utf16_len(text) as i32,
        })
        .unwrap();
        let ct = with
            .encrypt(payload.as_bytes(), topic.as_bytes())
            .expect("encrypt");
        let envelope = ClipEnvelope {
            v: PROTOCOL_VERSION,
            mid: "0123456789abcdef".to_string(),
            ts: clipsync_core::protocol::now_millis(),
            dev: dev.to_string(),
            name: name.to_string(),
            kind: "text".to_string(),
            ct,
        };
        (topic, serde_json::to_string(&envelope).unwrap())
    }

    fn file_clip(
        &self,
        dev: &str,
        name: &str,
        payload: &FilePayload,
        with: &RoomCrypto,
    ) -> (String, String) {
        let topic = clip_topic(&self.hash);
        let json = serde_json::to_string(payload).unwrap();
        let ct = with
            .encrypt(json.as_bytes(), topic.as_bytes())
            .expect("encrypt");
        let envelope = ClipEnvelope {
            v: PROTOCOL_VERSION,
            mid: "fedcba9876543210".to_string(),
            ts: clipsync_core::protocol::now_millis(),
            dev: dev.to_string(),
            name: name.to_string(),
            kind: "file".to_string(),
            ct,
        };
        (topic, serde_json::to_string(&envelope).unwrap())
    }
}

#[tokio::test(start_paused = true)]
async fn join_subscribes_then_announces_and_calls_roll() {
    let mut h = Harness::start().await;
    h.online().await;

    assert_eq!(h.broker.filters(), vec![room_filter(&h.hash)]);
    assert_eq!(h.broker.links(), 1);

    let published = h.broker.published();
    assert_eq!(published.len(), 2);
    assert_eq!(published[0].0, presence_topic(&h.hash, ME));
    let first: PresenceEnvelope = serde_json::from_str(&published[0].1).unwrap();
    assert!(first.online);
    assert_eq!(first.sp, h.prefix);
    assert_eq!(published[1].0, sync_topic(&h.hash));
    let roll: PresenceEnvelope = serde_json::from_str(&published[1].1).unwrap();
    assert!(roll.answer);
}

#[tokio::test(start_paused = true)]
async fn own_clip_echo_is_dropped() {
    let mut h = Harness::start().await;
    h.online().await;
    let (topic, wire) = h.clip(ME, "Me", "echo", &crypto());
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });

    let (other_topic, other_wire) = h.clip("peer", "Peer", "real", &crypto());
    h.broker.inject(LinkEvent::Message {
        topic: other_topic,
        payload: other_wire,
    });
    match h.next().await {
        Event::Clip(clip) => assert_eq!(clip.text, "real"),
        other => panic!("expected the peer clip, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn clip_from_another_room_key_is_dropped() {
    let mut h = Harness::start().await;
    h.online().await;
    let stranger = RoomCrypto::open("OTHERROOM", "").unwrap();
    let (topic, wire) = h.clip("peer", "Peer", "secret", &stranger);
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });

    let (topic, wire) = h.clip("peer2", "Peer2", "known", &crypto());
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    match h.next().await {
        Event::Clip(clip) => assert_eq!(clip.text, "known"),
        other => panic!("expected only the readable clip, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn presence_builds_the_roster_and_flags_a_key_mismatch() {
    let mut h = Harness::start().await;
    h.online().await;

    let (topic, wire) = h.presence("p1", "Alice", true);
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    match h.next().await {
        Event::Peers(peers) => {
            assert_eq!(peers.len(), 1);
            assert_eq!(peers[0].name, "Alice");
        }
        other => panic!("expected peers, got {other:?}"),
    }
    match h.next().await {
        Event::KeyMismatch(names) => assert!(names.is_empty()),
        other => panic!("expected no mismatch, got {other:?}"),
    }

    let stranger = RoomCrypto::open("OTHERROOM", "").unwrap();
    let wire = serde_json::to_string(&PresenceEnvelope {
        v: PROTOCOL_VERSION,
        dev: "p2".to_string(),
        name: "Bob".to_string(),
        online: true,
        ts: clipsync_core::protocol::now_millis(),
        sp: stranger.safety_prefix(),
        os: "kotlin".to_string(),
        answer: false,
    })
    .unwrap();
    h.broker.inject(LinkEvent::Message {
        topic: presence_topic(&h.hash, "p2"),
        payload: wire,
    });

    match h.next().await {
        Event::Peers(peers) => assert!(peers.iter().all(|peer| peer.name != "Bob")),
        other => panic!("expected peers, got {other:?}"),
    }
    match h.next().await {
        Event::KeyMismatch(names) => assert_eq!(names, vec!["Bob".to_string()]),
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn goodbye_presence_removes_the_peer() {
    let mut h = Harness::start().await;
    h.online().await;
    let (topic, wire) = h.presence("p1", "Alice", true);
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    wait_peers(&mut h, 1).await;

    let (topic, wire) = h.presence("p1", "Alice", false);
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    wait_peers(&mut h, 0).await;
}

#[tokio::test(start_paused = true)]
async fn peers_expire_after_three_heartbeats() {
    let mut h = Harness::start().await;
    h.online().await;
    let (topic, wire) = h.presence("p1", "Alice", true);
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    wait_peers(&mut h, 1).await;

    tokio::time::advance(clipsync_core::engine::PRESENCE_TTL + std::time::Duration::from_millis(1))
        .await;
    wait_peers(&mut h, 0).await;
}

#[tokio::test(start_paused = true)]
async fn foreign_traffic_never_reaches_the_roster() {
    let mut h = Harness::start().await;
    h.online().await;

    h.broker.inject(LinkEvent::Message {
        topic: format!("cs/v1/{}/clip/extra", h.hash),
        payload: "{\"v\":1}".to_string(),
    });
    h.broker.inject(LinkEvent::Message {
        topic: presence_topic(&h.hash, "impostor"),
        payload: "{\"v\":1,\"dev\":\"ghost\",\"name\":\"Ghost\",\"online\":true,\"ts\":1,\"sp\":\"000\",\"os\":\"x\"}".to_string(),
    });
    h.broker.inject(LinkEvent::Message {
        topic: presence_topic(&h.hash, "p1"),
        payload: "not json".to_string(),
    });

    let (topic, wire) = h.presence("p1", "Alice", true);
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    wait_peers(&mut h, 1).await;
}

#[tokio::test(start_paused = true)]
async fn disconnect_hands_over_to_the_recovery_loop() {
    let mut h = Harness::start().await;
    h.online().await;

    h.broker.inject(LinkEvent::Disconnected);
    assert_eq!(h.status().await, Status::Offline { reason: None });

    tokio::time::advance(clipsync_core::engine::RECOVERY_BASE).await;
    h.online().await;
    assert_eq!(h.broker.links(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_failed_recovery_doubles_the_backoff() {
    let mut h = Harness::start().await;
    h.online().await;

    h.broker.refuse_connect.store(true, Ordering::SeqCst);
    h.broker.inject(LinkEvent::Disconnected);
    assert_eq!(h.status().await, Status::Offline { reason: None });

    tokio::time::advance(clipsync_core::engine::RECOVERY_BASE).await;
    assert_eq!(
        h.status().await,
        Status::Offline {
            reason: Some("refused".to_string())
        }
    );
    assert_eq!(h.broker.links(), 2);

    tokio::time::advance(clipsync_core::engine::RECOVERY_BASE).await;
    assert_eq!(h.broker.links(), 2);
    tokio::time::advance(std::time::Duration::from_millis(1)).await;
    h.online().await;
    assert_eq!(h.broker.links(), 3);
}

#[tokio::test(start_paused = true)]
async fn leave_announces_offline_before_closing() {
    let mut h = Harness::start().await;
    h.online().await;
    let before = h.broker.published().len();

    h.engine.leave();
    assert_eq!(h.status().await, Status::Idle);

    let published = h.broker.published();
    assert_eq!(published.len(), before + 1);
    let last: PresenceEnvelope = serde_json::from_str(&published[before].1).unwrap();
    assert!(!last.online);
}

#[tokio::test(start_paused = true)]
async fn a_copy_is_recorded_even_when_the_wire_is_down() {
    let mut h = Harness::start().await;
    h.online().await;
    h.broker.down.store(true, Ordering::SeqCst);

    h.engine.publish_text("clipboard");
    match h.next().await {
        Event::Clip(clip) => {
            assert!(clip.mine);
            assert_eq!(clip.text, "clipboard");
        }
        other => panic!("expected the local clip, got {other:?}"),
    }
    assert_eq!(
        h.broker
            .published()
            .iter()
            .filter(|(topic, _)| topic == &clip_topic(&h.hash))
            .count(),
        0
    );
}

#[tokio::test(start_paused = true)]
async fn publish_file_sends_encrypted_metadata_and_announces_the_record() {
    let mut h = Harness::start().await;
    h.online().await;

    let bytes = b"hello";
    let sha = clipsync_core::crypto::sha256_hex(bytes);
    let upload = FileUpload {
        mid: file_mid("report.pdf", bytes.len(), ME),
        ts: 1_700_000_000_000,
        name: "report.pdf".into(),
        mime: "application/pdf".into(),
        size: bytes.len() as i64,
        sha256: sha.clone(),
        link: "https://litterbox.catbox.moe/cs-abc.bin".into(),
        expires_at: 1_700_259_200_000,
    };
    h.engine.publish_file(upload.clone(), true);

    match h.next().await {
        Event::Clip(clip) => {
            assert!(clip.mine);
            assert_eq!(clip.text, "report.pdf");
            let file = clip.file.expect("announced clip carries its file");
            assert_eq!(file.sha256, sha);
            assert_eq!(file.link, upload.link);
        }
        other => panic!("expected the local file clip, got {other:?}"),
    }

    let published = h.broker.published();
    let (topic, wire) = published
        .iter()
        .rev()
        .find(|(topic, _)| topic == &clip_topic(&h.hash))
        .expect("file envelope was published");
    let envelope: ClipEnvelope = serde_json::from_str(wire).unwrap();
    assert_eq!(envelope.kind, "file");
    assert_eq!(envelope.mid, upload.mid);
    assert_eq!(envelope.ts, upload.ts);
    let plain = crypto().decrypt(&envelope.ct, topic.as_bytes()).unwrap();
    let payload: FilePayload = serde_json::from_str(&String::from_utf8(plain).unwrap()).unwrap();
    assert_eq!(payload.name, "report.pdf");
    assert_eq!(payload.size, bytes.len() as i64);
    assert_eq!(payload.expires_at, upload.expires_at);
}

#[tokio::test(start_paused = true)]
async fn resend_publishes_without_adding_a_second_record() {
    let mut h = Harness::start().await;
    h.online().await;
    let upload = FileUpload {
        mid: "fedcba9876543210".into(),
        ts: 42,
        name: "queued.png".into(),
        mime: "image/png".into(),
        size: 3,
        sha256: "ab".into(),
        link: "https://litterbox.catbox.moe/cs-x.bin".into(),
        expires_at: 99,
    };
    h.engine.publish_file(upload, false);
    for _ in 0..100 {
        if h.broker
            .published()
            .iter()
            .any(|(topic, _)| topic == &clip_topic(&h.hash))
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    let (topic, wire) = h
        .broker
        .published()
        .into_iter()
        .find(|(topic, _)| topic == &clip_topic(&h.hash))
        .expect("resent envelope was published");
    let envelope: ClipEnvelope = serde_json::from_str(&wire).unwrap();
    assert_eq!(envelope.kind, "file");
    assert_eq!(envelope.ts, 42);
    let _ = topic;

    h.broker.inject(LinkEvent::Message {
        topic: clip_topic(&h.hash),
        payload: "not json".to_string(),
    });
    let (topic, wire) = h.clip("peer", "Peer", "after", &crypto());
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    match h.next().await {
        Event::Clip(clip) => assert_eq!(clip.text, "after"),
        other => panic!("resend should not emit a clip, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn incoming_file_metadata_becomes_a_file_clip() {
    let mut h = Harness::start().await;
    h.online().await;
    let payload = FilePayload {
        name: "../../etc/photo.png".into(),
        mime: "image/png".into(),
        size: 8,
        sha256: "abc".into(),
        link: "https://litterbox.catbox.moe/cs-y.bin".into(),
        expires_at: 123,
    };
    let (topic, wire) = h.file_clip("peer", "Pixel", &payload, &crypto());
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    match h.next().await {
        Event::Clip(clip) => {
            assert!(!clip.mine);
            assert_eq!(clip.text, "photo.png");
            assert_eq!(clip.mid, "fedcba9876543210");
            let file = clip.file.expect("file metadata is attached");
            assert_eq!(file.name, "../../etc/photo.png");
            assert_eq!(file.link, payload.link);
        }
        other => panic!("expected the incoming file, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn oversized_incoming_file_is_reported_and_dropped() {
    let mut h = Harness::start().await;
    h.online().await;
    let payload = FilePayload {
        name: "huge.iso".into(),
        mime: "application/octet-stream".into(),
        size: MAX_FILE_BYTES + 1,
        sha256: "abc".into(),
        link: "https://litterbox.catbox.moe/cs-z.bin".into(),
        expires_at: 123,
    };
    let (topic, wire) = h.file_clip("peer", "Pixel", &payload, &crypto());
    h.broker.inject(LinkEvent::Message {
        topic,
        payload: wire,
    });
    match h.next().await {
        Event::FileTooLarge(name) => assert_eq!(name, "huge.iso"),
        other => panic!("expected a too-large report, got {other:?}"),
    }
}

async fn wait_peers(h: &mut Harness, want: usize) {
    loop {
        if let Event::Peers(peers) = h.next().await {
            if peers.len() == want {
                return;
            }
        }
    }
}
