use crate::brokers::NatsBroker;
use crate::brokers::base::HeaderMap;
use crate::errors::CrabbyError;
use crate::publish::base::{
    IntoPublishPayload, PreparedPublishPayload, PublishRequest, Publisher, Request, merge_headers,
};
use std::future::Future;
use std::future::IntoFuture;
use std::pin::Pin;

type JsPublishFuture = Pin<Box<dyn Future<Output = Result<NatsPublishAck, CrabbyError>> + Send>>;

/// JetStream publish acknowledgment returned by [`NatsPublisher::js_publish`].
pub type NatsPublishAck = async_nats::jetstream::publish::PublishAck;

/// NATS-specific publishing facade built on top of the core [`Publisher`].
///
/// `Publisher` remains the broker-agnostic publishing capability injected into
/// handlers through the `Publish` extractor. `NatsPublisher` exists for
/// transport-specific features such as JetStream publishing while still
/// delegating plain `publish(...)` and `request(...)` to the core API.
#[derive(Clone)]
pub struct NatsPublisher {
    core: Publisher,
    jetstream: async_nats::jetstream::Context,
}

impl NatsPublisher {
    /// Creates a NATS-specific publisher from a [`NatsBroker`].
    pub fn new(broker: NatsBroker) -> Self {
        let core = Publisher::new(broker.clone());
        let jetstream = async_nats::jetstream::new(broker.client());

        Self { core, jetstream }
    }

    /// Returns the broker-agnostic core publisher used by this NATS publisher.
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

    /// Starts building a JetStream publish request.
    ///
    /// Unlike [`NatsPublisher::publish`], this path goes through NATS
    /// JetStream and waits for a JetStream publish acknowledgment.
    pub fn js_publish<P>(&self, subject: &str, payload: P) -> NatsJsPublishRequest
    where
        P: IntoPublishPayload,
    {
        NatsJsPublishRequest {
            jetstream: self.jetstream.clone(),
            subject: subject.to_string(),
            prepared: payload.into_publish_payload(),
            extra_headers: None,
        }
    }
}

/// Builder for a JetStream publish operation created by [`NatsPublisher::js_publish`].
pub struct NatsJsPublishRequest {
    jetstream: async_nats::jetstream::Context,
    subject: String,
    prepared: Result<PreparedPublishPayload, CrabbyError>,
    extra_headers: Option<HeaderMap>,
}

impl NatsJsPublishRequest {
    /// Adds or overrides message headers before sending the JetStream publish request.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.extra_headers = Some(headers);
        self
    }
}

impl IntoFuture for NatsJsPublishRequest {
    type Output = Result<NatsPublishAck, CrabbyError>;
    type IntoFuture = JsPublishFuture;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let prepared = self.prepared?;
            let headers = merge_headers(prepared.headers, self.extra_headers);

            let ack = if let Some(headers) = headers {
                let mut nats_headers = async_nats::HeaderMap::new();
                for (key, value) in headers {
                    nats_headers.insert(key.as_str(), value.as_str());
                }

                self.jetstream
                    .publish_with_headers(self.subject, nats_headers, prepared.payload.into())
                    .await?
                    .await?
            } else {
                self.jetstream
                    .publish(self.subject, prepared.payload.into())
                    .await?
                    .await?
            };

            Ok(ack)
        })
    }
}
