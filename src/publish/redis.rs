use crate::brokers::RedisBroker;
use crate::publish::base::{IntoPublishPayload, PublishRequest, Publisher, Request};

/// Redis-specific publishing facade built on top of the core [`Publisher`].
#[derive(Clone)]
pub struct RedisPublisher {
    core: Publisher,
}

impl RedisPublisher {
    /// Creates a Redis-specific publisher from a [`RedisBroker`].
    pub fn new(broker: RedisBroker) -> Self {
        Self {
            core: Publisher::new(broker),
        }
    }

    /// Returns the broker-agnostic core publisher used by this Redis publisher.
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
