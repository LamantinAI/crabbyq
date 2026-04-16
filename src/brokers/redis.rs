use crate::brokers::base::{Broker, BrokerError, BrokerMessage, HeaderMap};
use async_trait::async_trait;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;

#[derive(Clone)]
pub struct RedisBroker {
    client: redis::Client,
}

impl RedisBroker {
    pub fn new(client: redis::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Broker for RedisBroker {
    type MessageStream = BoxStream<'static, BrokerMessage>;

    async fn subscribe(&self, subjects: &[String]) -> Result<Self::MessageStream, BrokerError> {
        let mut pubsub = self.client.get_async_pubsub().await?;

        for subject in subjects {
            pubsub.subscribe(subject).await?;
        }

        let stream = pubsub.into_on_message().filter_map(|message| async move {
            Some(BrokerMessage {
                subject: message.get_channel_name().to_string(),
                payload: message.get_payload_bytes().to_vec(),
                headers: None,
                reply_to: None,
                acknowledger: None,
            })
        });

        Ok(Box::pin(stream))
    }

    async fn publish(
        &self,
        subject: &str,
        payload: &[u8],
        _headers: Option<&HeaderMap>,
    ) -> Result<(), BrokerError> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        redis::cmd("PUBLISH")
            .arg(subject)
            .arg(payload)
            .query_async::<()>(&mut connection)
            .await?;
        Ok(())
    }

    async fn request(
        &self,
        _subject: &str,
        _payload: &[u8],
        _headers: Option<&HeaderMap>,
    ) -> Result<BrokerMessage, BrokerError> {
        Err(anyhow::anyhow!("Redis pub/sub does not support request-reply").into())
    }
}
