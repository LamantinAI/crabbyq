use std::pin::Pin;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};

use crate::brokers::base::{Broker, BrokerMessage};

pub struct NatsBroker {
    client: async_nats::Client,
}

impl NatsBroker {
    pub fn new(client: async_nats::Client) -> Self {
        Self { client: client }
    }
}

#[async_trait]
impl Broker for NatsBroker {
    type MessageStream = Pin<Box<dyn Stream<Item = BrokerMessage> + Send>>;

    async fn subscribe(
        &self,
        subjects: &[String],
    ) -> Result<Self::MessageStream, Box<dyn std::error::Error + Send + Sync>> {
        let mut streams = Vec::new();

        for subject in subjects {
            let subscriber = self
                .client
                .subscribe(subject.clone())
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

            let stream = subscriber.map(|msg| BrokerMessage {
                subject: msg.subject.to_string(),
                payload: msg.payload.to_vec(),
                headers: None,
            });

            streams.push(stream);
        }

        let merged = futures_util::stream::select_all(streams);
        Ok(Box::pin(merged))
    }

    async fn publish(
        &self,
        subject: &str,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.client
            .publish(subject.to_string(), payload.to_vec().into())
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        Ok(())
    }
}
