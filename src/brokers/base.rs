use async_trait::async_trait;
use futures_util::Stream;
use std::collections::HashMap;

pub type HeaderMap = HashMap<String, String>;

// internal message structure
pub struct BrokerMessage {
    pub subject: String, // also known as a stream topic or router_key 
    pub payload: Vec<u8>, // message bytes
    pub headers: Option<HeaderMap>, // supported by NATS, Kafka and etc
    pub reply_to: Option<String>,
}

// Common trait for all brokers
#[async_trait]
pub trait Broker: Send + Sync + 'static {
    type MessageStream: Stream<Item = BrokerMessage> + Send + Unpin;

    async fn subscribe(
        &self, 
        subjects: &[String]
    ) -> Result<Self::MessageStream, Box<dyn std::error::Error + Send + Sync>>;

    async fn publish(
        &self, 
        subject: &str, 
        payload: &[u8],
        headers: Option<&HeaderMap>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
