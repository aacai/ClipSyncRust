use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_channel::{unbounded, Receiver, Sender};
use mqtt5::{
    ConnectOptions, ConnectionEvent, MqttClient, PublishOptions, QoS, SubscribeOptions, WillMessage,
};

use crate::engine::{Config, LinkFactory};
use crate::link::{LinkEvent, MqttLink, KEEP_ALIVE_SECONDS};
use crate::protocol::{client_id, presence_topic};
use crate::secrets;

pub struct BrokerLink {
    client: MqttClient,
    options: ConnectOptions,
    out: Sender<LinkEvent>,
    events: Receiver<LinkEvent>,
    connected: Arc<AtomicBool>,
}

impl BrokerLink {
    pub fn new(config: &Config, hash: &str, will_payload: &str) -> Self {
        let mut options = ConnectOptions::new(client_id(hash, &config.device_id))
            .with_clean_start(true)
            .with_keep_alive(Duration::from_secs(u64::from(KEEP_ALIVE_SECONDS)))
            .with_credentials(secrets::USERNAME, secrets::PASSWORD)
            // 断线重连归引擎管：它那边的退避会一路加到 60s，库再自己试一轮就成两套心跳了。
            .with_automatic_reconnect(false);
        // 遗嘱由引擎事先加密好交进来：broker 只会原样转发，不会再帮我们算一次
        if !will_payload.is_empty() {
            options = options.with_will(
                WillMessage::new(
                    presence_topic(hash, &config.device_id),
                    will_payload.as_bytes().to_vec(),
                )
                .with_qos(QoS::AtLeastOnce),
            );
        }
        let (out, events) = unbounded();
        Self {
            client: MqttClient::with_options(options.clone()),
            options,
            out,
            events,
            connected: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl MqttLink for BrokerLink {
    async fn connect(&self) -> Result<(), String> {
        let out = self.out.clone();
        let connected = Arc::clone(&self.connected);
        self.client
            .on_connection_event(move |event| match event {
                ConnectionEvent::Connected { .. } => {
                    connected.store(true, Ordering::SeqCst);
                    let _ = out.try_send(LinkEvent::Connected);
                }
                ConnectionEvent::Disconnected { .. } => {
                    connected.store(false, Ordering::SeqCst);
                    let _ = out.try_send(LinkEvent::Disconnected);
                }
                _ => {}
            })
            .await
            .map_err(|error| error.to_string())?;
        self.client
            .connect_with_options(secrets::endpoint().as_str(), self.options.clone())
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn subscribe(&self, filter: &str) -> Result<(), String> {
        let out = self.out.clone();
        self.client
            .subscribe_with_options(
                filter.to_string(),
                SubscribeOptions {
                    qos: QoS::AtLeastOnce,
                    no_local: true,
                    ..Default::default()
                },
                move |message| {
                    let _ = out.try_send(LinkEvent::Message {
                        topic: message.topic,
                        payload: String::from_utf8_lossy(&message.payload).to_string(),
                    });
                },
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn publish(&self, topic: &str, payload: &str) -> Result<(), String> {
        self.client
            .publish_with_options(
                topic.to_string(),
                payload.to_string().into_bytes(),
                PublishOptions {
                    qos: QoS::AtLeastOnce,
                    ..Default::default()
                },
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn next(&self) -> Option<LinkEvent> {
        self.events.recv().await.ok()
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn close(&self) {
        self.connected.store(false, Ordering::SeqCst);
        let client = self.client.clone();
        tokio::spawn(async move {
            let _ = client.disconnect().await;
        });
    }
}

#[derive(Clone)]
pub struct BrokerLinks;

impl LinkFactory for BrokerLinks {
    type Link = BrokerLink;

    fn create(&self, config: &Config, hash: &str, will_payload: &str) -> BrokerLink {
        BrokerLink::new(config, hash, will_payload)
    }
}
