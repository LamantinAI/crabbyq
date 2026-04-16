use crate::brokers::MqttBroker;
use crate::publish::base::{IntoPublishPayload, PublishRequest, Publisher, Request};

/// MQTT-specific publishing facade built on top of the core [`Publisher`].
#[derive(Clone)]
pub struct MqttPublisher {
    core: Publisher,
}

impl MqttPublisher {
    /// Creates an MQTT-specific publisher from a [`MqttBroker`].
    pub fn new(broker: MqttBroker) -> Self {
        Self {
            core: Publisher::new(broker),
        }
    }

    /// Returns the broker-agnostic core publisher used by this MQTT publisher.
    pub fn core(&self) -> Publisher {
        self.core.clone()
    }

    /// Starts building a plain publish request through the core publisher API.
    pub fn publish<P>(&self, subject: &str, payload: P) -> PublishRequest
    where
        P: IntoPublishPayload,
    {
        self.core.publish(subject, payload)
    }

    /// Starts building a plain request-reply call through the core publisher API.
    pub fn request<P>(&self, subject: &str, payload: P) -> Request
    where
        P: IntoPublishPayload,
    {
        self.core.request(subject, payload)
    }
}
