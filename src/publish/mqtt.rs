use crate::brokers::MqttBroker;
use crate::brokers::base::{BrokerError, HeaderMap};
use crate::errors::CrabbyError;
use crate::publish::base::{
    IntoPublishPayload, PreparedPublishPayload, PublishRequest, Publisher, Request, merge_headers,
};
use rumqttc::{AsyncClient, QoS};
use std::future::Future;
use std::future::IntoFuture;
use std::pin::Pin;

type MqttPublishFuture = Pin<Box<dyn Future<Output = Result<(), CrabbyError>> + Send>>;

/// MQTT-specific publishing facade built on top of the core [`Publisher`].
///
/// `Publisher` remains the broker-agnostic publishing capability injected
/// into handlers through the `Publish` extractor. `MqttPublisher` exists for
/// transport-specific options such as per-message QoS and the retain flag,
/// while still delegating plain `publish(...)` to the core API.
#[derive(Clone)]
pub struct MqttPublisher {
    core: Publisher,
    client: AsyncClient,
    default_qos: QoS,
}

impl MqttPublisher {
    /// Creates an MQTT-specific publisher from a [`MqttBroker`].
    pub fn new(broker: MqttBroker) -> Self {
        let client = broker.client();
        Self {
            core: Publisher::new(broker),
            client,
            default_qos: QoS::AtLeastOnce,
        }
    }

    /// Sets the default QoS used by [`MqttPublisher::mqtt_publish`] when no
    /// per-message override is provided.
    pub fn with_default_qos(mut self, qos: QoS) -> Self {
        self.default_qos = qos;
        self
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
    ///
    /// Note: MQTT does not natively support request-reply, so awaiting the
    /// resulting future returns an error.
    pub fn request<P>(&self, subject: &str, payload: P) -> Request
    where
        P: IntoPublishPayload,
    {
        self.core.request(subject, payload)
    }

    /// Starts building an MQTT publish request with per-message QoS and
    /// retain options.
    pub fn mqtt_publish<P>(&self, subject: &str, payload: P) -> MqttPublishRequest
    where
        P: IntoPublishPayload,
    {
        MqttPublishRequest {
            client: self.client.clone(),
            subject: subject.to_string(),
            prepared: payload.into_publish_payload(),
            extra_headers: None,
            qos: self.default_qos,
            retain: false,
        }
    }
}

/// Builder for an MQTT publish operation created by [`MqttPublisher::mqtt_publish`].
pub struct MqttPublishRequest {
    client: AsyncClient,
    subject: String,
    prepared: Result<PreparedPublishPayload, CrabbyError>,
    extra_headers: Option<HeaderMap>,
    qos: QoS,
    retain: bool,
}

impl MqttPublishRequest {
    /// Adds or overrides message headers before sending the publish.
    ///
    /// Note: MQTT v3.1.1 does not transmit headers, so this is currently a
    /// no-op on the wire. The hook is kept so headers attached by payload
    /// wrappers (e.g. `Json(...)` adds a `content-type` header) still travel
    /// with the request object for instrumentation purposes.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.extra_headers = Some(headers);
        self
    }

    /// Sets the Quality of Service flag for this publish.
    pub fn qos(mut self, qos: QoS) -> Self {
        self.qos = qos;
        self
    }

    /// Marks the message as retained.
    pub fn retain(mut self, retain: bool) -> Self {
        self.retain = retain;
        self
    }
}

impl IntoFuture for MqttPublishRequest {
    type Output = Result<(), CrabbyError>;
    type IntoFuture = MqttPublishFuture;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let prepared = self.prepared?;
            // Header merge is preserved for parity with the core publisher
            // builder, even though MQTT v3.1.1 does not carry headers on the
            // wire.
            let _headers = merge_headers(prepared.headers, self.extra_headers);
            self.client
                .publish(self.subject, self.qos, self.retain, prepared.payload)
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;
            Ok(())
        })
    }
}
