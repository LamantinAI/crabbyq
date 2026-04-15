use async_trait::async_trait;
use futures_util::Stream;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

pub type HeaderMap = HashMap<String, String>;
pub type BrokerError = Box<dyn std::error::Error + Send + Sync>;
pub type AckFuture = Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>>;

pub trait Acknowledger: Send {
    fn ack(self: Box<Self>) -> AckFuture;
}

// internal message structure
pub struct BrokerMessage {
    pub subject: String,            // also known as a stream topic or router_key
    pub payload: Vec<u8>,           // message bytes
    pub headers: Option<HeaderMap>, // supported by NATS, Kafka and etc
    pub reply_to: Option<String>,
    pub acknowledger: Option<Box<dyn Acknowledger>>,
}

// Common trait for all brokers
#[async_trait]
pub trait Broker: Send + Sync + 'static {
    type MessageStream: Stream<Item = BrokerMessage> + Send + Unpin;

    async fn subscribe(&self, subjects: &[String]) -> Result<Self::MessageStream, BrokerError>;

    async fn publish(
        &self,
        subject: &str,
        payload: &[u8],
        headers: Option<&HeaderMap>,
    ) -> Result<(), BrokerError>;

    async fn request(
        &self,
        subject: &str,
        payload: &[u8],
        headers: Option<&HeaderMap>,
    ) -> Result<BrokerMessage, BrokerError>;
}
