use crate::brokers::base::{Broker, BrokerError, BrokerMessage, HeaderMap};
use async_trait::async_trait;
use futures_util::stream::{self, BoxStream};
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, QoS};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Per-subject MQTT subscription settings injected by [`MqttRouter`].
///
/// [`MqttRouter`]: crate::routing::MqttRouter
#[derive(Clone, Debug)]
pub struct MqttRouteOptions {
    /// Quality of Service requested for this subscription.
    pub qos: QoS,
    /// Topic filter to send in the `SUBSCRIBE` packet.
    ///
    /// For plain and QoS-only routes this is just the route subject. For
    /// shared subscriptions it is `$share/<group>/<subject>` while the route
    /// subject remains the underlying topic so handler dispatch keeps working
    /// against the actual incoming message topic.
    pub subscribe_as: String,
}

#[derive(Clone)]
pub struct MqttBroker {
    client: AsyncClient,
    eventloop: Arc<Mutex<Option<EventLoop>>>,
    subscription_options: Arc<HashMap<String, MqttRouteOptions>>,
    default_qos: QoS,
}

impl MqttBroker {
    pub fn new(options: MqttOptions, inflight: usize) -> Self {
        let (client, eventloop) = AsyncClient::new(options, inflight);
        Self {
            client,
            eventloop: Arc::new(Mutex::new(Some(eventloop))),
            subscription_options: Arc::new(HashMap::new()),
            default_qos: QoS::AtLeastOnce,
        }
    }

    /// Returns a clone of the underlying `rumqttc` async client.
    pub fn client(&self) -> AsyncClient {
        self.client.clone()
    }

    /// Replaces the per-subject subscription options table with a new one.
    ///
    /// [`MqttRouter`][crate::routing::MqttRouter] uses this during
    /// `into_service(...)` to push per-route QoS overrides and shared
    /// subscription rewrites into the broker.
    pub fn with_subscription_options(
        mut self,
        options: HashMap<String, MqttRouteOptions>,
    ) -> Self {
        self.subscription_options = Arc::new(options);
        self
    }

    /// Overrides the default QoS applied to subscriptions without an entry in
    /// the subscription options table.
    pub fn with_default_qos(mut self, qos: QoS) -> Self {
        self.default_qos = qos;
        self
    }
}

#[async_trait]
impl Broker for MqttBroker {
    type MessageStream = BoxStream<'static, BrokerMessage>;

    async fn subscribe(&self, subjects: &[String]) -> Result<Self::MessageStream, BrokerError> {
        for subject in subjects {
            let (subscribe_as, qos) = match self.subscription_options.get(subject) {
                Some(opts) => (opts.subscribe_as.clone(), opts.qos),
                None => (subject.clone(), self.default_qos),
            };
            self.client.subscribe(subscribe_as, qos).await?;
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
            .publish(subject, self.default_qos, false, payload.to_vec())
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
