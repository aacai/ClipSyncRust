use std::time::Duration;

use async_channel::unbounded;
use clipsync_core::engine::{self, Config, Event, Status};
use clipsync_core::mqtt::BrokerLinks;
use clipsync_core::protocol::{presence_topic, room_filter, room_hash};
use clipsync_core::secrets;
use mqtt5::{ConnectOptions, MqttClient, QoS, SubscribeOptions, WillMessage};
use tokio::sync::mpsc::UnboundedReceiver;

const ROOM: &str = "rust-interop-smoke";

fn options(client_id: String) -> ConnectOptions {
    ConnectOptions::new(client_id)
        .with_clean_start(true)
        .with_credentials(secrets::USERNAME, secrets::PASSWORD)
        .with_automatic_reconnect(false)
}

fn config(device: &str) -> Config {
    Config {
        room_id: ROOM.to_string(),
        password: "shared-key".to_string(),
        room_key: None,
        device_id: format!("rust-{device}"),
        device_name: format!("Rust {device}"),
        device_os: "rust".to_string(),
    }
}

async fn next_matching(rx: &mut UnboundedReceiver<Event>, want: impl Fn(&Event) -> bool) -> Event {
    let deadline = Duration::from_secs(30);
    loop {
        let event = tokio::time::timeout(deadline, rx.recv())
            .await
            .expect("timed out waiting for the broker")
            .expect("engine stopped");
        if want(&event) {
            return event;
        }
    }
}

async fn online(rx: &mut UnboundedReceiver<Event>) -> String {
    match next_matching(rx, |event| {
        matches!(event, Event::Status(Status::Online { .. }))
    })
    .await
    {
        Event::Status(Status::Online { safety }) => safety,
        other => panic!("expected online, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to the public broker"]
async fn two_engines_exchange_an_encrypted_clip() {
    let mut alice = engine::spawn(BrokerLinks);
    let mut bob = engine::spawn(BrokerLinks);
    alice.join(config("alice"));
    bob.join(config("bob"));

    let alice_safety = online(&mut alice.events).await;
    let bob_safety = online(&mut bob.events).await;
    assert_eq!(alice_safety, bob_safety, "same room, same key");

    next_matching(&mut bob.events, |event| {
        matches!(event, Event::Peers(peers) if peers.iter().any(|peer| peer.name == "Rust alice"))
    })
    .await;

    bob.publish_text("ping from bob");
    match next_matching(&mut alice.events, |event| matches!(event, Event::Clip(_))).await {
        Event::Clip(clip) => {
            assert_eq!(clip.text, "ping from bob");
            assert_eq!(clip.sender_name, "Rust bob");
            assert!(!clip.mine);
        }
        other => panic!("expected a clip, got {other:?}"),
    }

    alice.leave();
    bob.leave();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to the public broker"]
async fn the_broker_does_not_echo_our_own_publishes() {
    let hash = room_hash(ROOM);
    let client =
        MqttClient::with_options(options(format!("cs-probe-noecho-{}", std::process::id())));
    client
        .connect(secrets::endpoint().as_str())
        .await
        .expect("connect");
    let (tx, rx) = unbounded::<String>();
    client
        .subscribe_with_options(
            room_filter(&hash),
            SubscribeOptions {
                qos: QoS::AtLeastOnce,
                no_local: true,
                ..Default::default()
            },
            move |message| {
                let _ = tx.try_send(message.topic);
            },
        )
        .await
        .expect("subscribe");
    client
        .publish_qos(
            presence_topic(&hash, "probe"),
            b"echo-check".to_vec(),
            QoS::AtLeastOnce,
        )
        .await
        .expect("publish");

    assert!(
        tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .is_err(),
        "no_local was not honoured"
    );
    client.disconnect().await.expect("disconnect");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to the public broker"]
async fn the_broker_hands_over_the_presence_will() {
    let hash = room_hash(ROOM);
    let gone = presence_topic(&hash, "probe-will");
    let observer =
        MqttClient::with_options(options(format!("cs-probe-observer-{}", std::process::id())));
    observer
        .connect(secrets::endpoint().as_str())
        .await
        .expect("observer connect");
    let (tx, rx) = unbounded::<String>();
    observer
        .subscribe_with_options(
            room_filter(&hash),
            SubscribeOptions {
                qos: QoS::AtLeastOnce,
                ..Default::default()
            },
            move |message| {
                let _ = tx.try_send(String::from_utf8_lossy(&message.payload).to_string());
            },
        )
        .await
        .expect("observer subscribe");

    let victim = MqttClient::with_options(
        options(format!("cs-probe-will-{}", std::process::id())).with_will(
            WillMessage::new(gone, b"{\"online\":false}".to_vec()).with_qos(QoS::AtLeastOnce),
        ),
    );
    victim
        .connect(secrets::endpoint().as_str())
        .await
        .expect("victim connect");
    tokio::time::sleep(Duration::from_millis(500)).await;
    victim
        .disconnect_abnormally()
        .await
        .expect("abnormal disconnect");

    let payload = tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .expect("no will arrived")
        .expect("observer stopped");
    assert_eq!(payload, "{\"online\":false}");
    observer.disconnect().await.expect("observer disconnect");
}
