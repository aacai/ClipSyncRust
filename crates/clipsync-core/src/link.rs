use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_channel::unbounded;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    Connected,
    Disconnected,
    Message { topic: String, payload: String },
}

/// `next` is polled from a `select!` alongside the request pump, so the link is `&self`
/// and implementations must not lose a message when a poll is cancelled.
pub trait MqttLink {
    fn connect(&self) -> impl Future<Output = Result<(), String>> + Send;
    fn subscribe(&self, filter: &str) -> impl Future<Output = Result<(), String>> + Send;
    fn publish(
        &self,
        topic: &str,
        payload: &str,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn next(&self) -> impl Future<Output = Option<LinkEvent>> + Send;
    fn is_connected(&self) -> bool;
    fn close(&self);
}

enum Request {
    Subscribe {
        filter: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Publish {
        topic: String,
        payload: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Close(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct LinkHandle {
    requests: mpsc::UnboundedSender<Request>,
    connected: Arc<AtomicBool>,
}

impl LinkHandle {
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    pub async fn subscribe(&self, filter: &str) -> Result<(), String> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send(Request::Subscribe {
                filter: filter.to_string(),
                reply,
            })
            .map_err(|_| "link closed".to_string())?;
        answer.await.map_err(|_| "link closed".to_string())?
    }

    pub async fn publish(&self, topic: &str, payload: &str) -> Result<(), String> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send(Request::Publish {
                topic: topic.to_string(),
                payload: payload.to_string(),
                reply,
            })
            .map_err(|_| "link closed".to_string())?;
        answer.await.map_err(|_| "link closed".to_string())?
    }

    pub async fn shutdown(&self) {
        let (reply, answer) = oneshot::channel();
        if self.requests.send(Request::Close(reply)).is_err() {
            return;
        }
        let _ = answer.await;
    }
}

/// Owns the link on a task so the engine can select over commands and inbound
/// events without borrowing itself across await points.
pub fn spawn<L: MqttLink + Send + Sync + 'static>(
    link: L,
) -> (
    LinkHandle,
    async_channel::Receiver<LinkEvent>,
    oneshot::Receiver<Result<(), String>>,
) {
    let (request_tx, mut requests) = mpsc::unbounded_channel();
    let (event_tx, events) = unbounded::<LinkEvent>();
    let connected = Arc::new(AtomicBool::new(false));
    let (boot_tx, boot_rx) = oneshot::channel();
    let flag = Arc::clone(&connected);

    tokio::spawn(async move {
        let boot = link.connect().await;
        flag.store(boot.is_ok(), Ordering::SeqCst);
        if boot.is_ok() {
            let _ = event_tx.try_send(LinkEvent::Connected);
        }
        let _ = boot_tx.send(boot);

        loop {
            tokio::select! {
                request = requests.recv() => match request {
                    Some(Request::Subscribe { filter, reply }) => {
                        let result = link.subscribe(&filter).await;
                        let _ = reply.send(result);
                    }
                    Some(Request::Publish { topic, payload, reply }) => {
                        let result = link.publish(&topic, &payload).await;
                        let _ = reply.send(result);
                    }
                    Some(Request::Close(reply)) => {
                        let _ = reply.send(());
                        break;
                    }
                    None => break,
                },
                inbound = link.next() => match inbound {
                    Some(LinkEvent::Connected) => {
                        flag.store(true, Ordering::SeqCst);
                        if event_tx.try_send(LinkEvent::Connected).is_err() { break }
                    }
                    Some(LinkEvent::Disconnected) => {
                        flag.store(false, Ordering::SeqCst);
                        if event_tx.try_send(LinkEvent::Disconnected).is_err() { break }
                    }
                    Some(message) => {
                        if event_tx.try_send(message).is_err() { break }
                    }
                    None => break,
                },
            }
        }
        flag.store(false, Ordering::SeqCst);
        link.close();
    });

    (
        LinkHandle {
            requests: request_tx,
            connected,
        },
        events,
        boot_rx,
    )
}

pub const KEEP_ALIVE_SECONDS: u16 = 60;
