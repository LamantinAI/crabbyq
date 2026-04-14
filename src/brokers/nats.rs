use std::pin::Pin;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};

use crate::brokers::base::{Broker, BrokerMessage, HeaderMap};

#[derive(Clone)]
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
    type MessageStream = Pin<Box<dyn Stream<Item = BrokerMessage> + Send + Unpin>>;

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
                headers: msg.headers.map(|headers| {
                    headers
                        .iter()
                        .filter_map(|(key, values)| {
                            values.first().map(|value| (key.to_string(), value.to_string()))
                        })
                        .collect()
                }),
                reply_to: msg.reply.map(|reply| reply.to_string()),
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
        headers: Option<&HeaderMap>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(headers) = headers {
            let mut nats_headers = async_nats::HeaderMap::new();
            for (key, value) in headers {
                nats_headers.insert(key.as_str(), value.as_str());
            }

            self.client
                .publish_with_headers(subject.to_string(), nats_headers, payload.to_vec().into())
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        } else {
            self.client
                .publish(subject.to_string(), payload.to_vec().into())
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        }
        Ok(())
    }

    async fn request(
        &self,
        subject: &str,
        payload: &[u8],
        headers: Option<&HeaderMap>,
    ) -> Result<BrokerMessage, Box<dyn std::error::Error + Send + Sync>> {
        let message = if let Some(headers) = headers {
            let mut nats_headers = async_nats::HeaderMap::new();
            for (key, value) in headers {
                nats_headers.insert(key.as_str(), value.as_str());
            }

            self.client
                .request_with_headers(subject.to_string(), nats_headers, payload.to_vec().into())
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?
        } else {
            self.client
                .request(subject.to_string(), payload.to_vec().into())
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?
        };

        Ok(BrokerMessage {
            subject: message.subject.to_string(),
            payload: message.payload.to_vec(),
            headers: message.headers.map(|headers| {
                headers
                    .iter()
                    .filter_map(|(key, values)| {
                        values.first().map(|value| (key.to_string(), value.to_string()))
                    })
                    .collect()
            }),
            reply_to: message.reply.map(|reply| reply.to_string()),
        })
    }
}
