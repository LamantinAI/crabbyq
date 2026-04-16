use crate::brokers::base::{Broker, BrokerError, BrokerMessage, HeaderMap};
use async_trait::async_trait;
use futures_util::stream::{self, BoxStream};
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, QoS};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct MqttBroker {
    client: AsyncClient,
    eventloop: Arc<Mutex<Option<EventLoop>>>,
}

impl MqttBroker {
    pub fn new(options: MqttOptions, inflight: usize) -> Self {
        let (client, eventloop) = AsyncClient::new(options, inflight);
        Self {
            client,
            eventloop: Arc::new(Mutex::new(Some(eventloop))),
        }
    }
}

#[async_trait]
impl Broker for MqttBroker {
    type MessageStream = BoxStream<'static, BrokerMessage>;

    async fn subscribe(&self, subjects: &[String]) -> Result<Self::MessageStream, BrokerError> {
        for subject in subjects {
            self.client.subscribe(subject, QoS::AtLeastOnce).await?;
        }

        let eventloop = self
            .eventloop
            .lock()
            .await
            .take()
            .ok_or_else(|| anyhow::anyhow!("MQTT event loop is already running"))?;

        let stream = stream::unfold(eventloop, |mut eventloop| async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(publish))) => {
                        return Some((
                            BrokerMessage {
                                subject: publish.topic,
                                payload: publish.payload.to_vec(),
                                headers: None,
                                reply_to: None,
                                acknowledger: None,
                            },
                            eventloop,
                        ));
                    }
                    Ok(_) => continue,
                    Err(error) => {
                        tracing::error!("MQTT stream error: {}", error);
                        return None;
                    }
                }
            }
        });

        Ok(Box::pin(stream))
    }

    async fn publish(
        &self,
        subject: &str,
        payload: &[u8],
        _headers: Option<&HeaderMap>,
    ) -> Result<(), BrokerError> {
        self.client
            .publish(subject, QoS::AtLeastOnce, false, payload.to_vec())
            .await?;
        Ok(())
    }

    async fn request(
        &self,
        _subject: &str,
        _payload: &[u8],
        _headers: Option<&HeaderMap>,
    ) -> Result<BrokerMessage, BrokerError> {
        Err(anyhow::anyhow!("MQTT does not support request-reply in the core broker API").into())
    }
}
