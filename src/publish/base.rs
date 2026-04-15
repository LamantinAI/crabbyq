use crate::brokers::Broker;
use crate::brokers::base::{BrokerMessage, HeaderMap};
use crate::errors::CrabbyError;
use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::future::Future;
use std::future::IntoFuture;
use std::pin::Pin;
use std::sync::Arc;

type PublishFuture = Pin<Box<dyn Future<Output = Result<(), CrabbyError>> + Send>>;
type RequestFuture = Pin<Box<dyn Future<Output = Result<Reply, CrabbyError>> + Send>>;

#[doc(hidden)]
pub struct PreparedPublishPayload {
    pub(crate) payload: Vec<u8>,
    pub(crate) headers: Option<HeaderMap>,
}

trait PublishBackend: Send + Sync {
    fn publish(
        &self,
        subject: String,
        payload: Vec<u8>,
        headers: Option<HeaderMap>,
    ) -> PublishFuture;
    fn request(
        &self,
        subject: String,
        payload: Vec<u8>,
        headers: Option<HeaderMap>,
    ) -> RequestFuture;
}

impl<B> PublishBackend for B
where
    B: Broker + Clone,
{
    fn publish(
        &self,
        subject: String,
        payload: Vec<u8>,
        headers: Option<HeaderMap>,
    ) -> PublishFuture {
        let broker = self.clone();
        Box::pin(async move { broker.publish(&subject, &payload, headers.as_ref()).await })
    }

    fn request(
        &self,
        subject: String,
        payload: Vec<u8>,
        headers: Option<HeaderMap>,
    ) -> RequestFuture {
        let broker = self.clone();
        Box::pin(async move {
            let reply = broker.request(&subject, &payload, headers.as_ref()).await?;
            Ok(Reply::from_broker_message(reply))
        })
    }
}

/// A broker-backed publishing capability available inside handlers.
///
/// A `Publisher` is created automatically by `Router::into_service(...)` and
/// can be accessed through the `Publish` extractor. It is intentionally
/// transport-agnostic: handlers publish to route keys and the active broker
/// backend is responsible for delivering the message.
#[derive(Clone)]
pub struct Publisher {
    inner: Arc<dyn PublishBackend>,
}

impl Publisher {
    /// Creates a broker-backed publisher or request client.
    ///
    /// `Publisher` is injected automatically into handlers through the
    /// `Publish` extractor, but it can also be created manually when the
    /// application needs to publish messages or perform RPC-style requests
    /// outside of a handler.
    pub fn new<B>(broker: B) -> Self
    where
        B: Broker + Clone,
    {
        Self {
            inner: Arc::new(broker),
        }
    }

    /// Starts building a publish request for a route key.
    ///
    /// The payload can be wrapped in `Json(...)`, `Cbor(...)`, `Body(...)`, or
    /// passed as raw bytes/string types. Use `.headers(...)` on the returned
    /// request builder to attach additional headers before awaiting it.
    pub fn publish<P>(&self, subject: &str, payload: P) -> PublishRequest
    where
        P: IntoPublishPayload,
    {
        PublishRequest {
            publisher: self.clone(),
            subject: subject.to_string(),
            prepared: payload.into_publish_payload(),
            extra_headers: None,
        }
    }

    /// Starts building an RPC-style request for a route key.
    ///
    /// The payload uses the same wrappers as [`Publisher::publish`], and the
    /// returned reply can be decoded with [`Reply::into_json`] or
    /// [`Reply::into_cbor`].
    pub fn request<P>(&self, subject: &str, payload: P) -> Request
    where
        P: IntoPublishPayload,
    {
        Request {
            publisher: self.clone(),
            subject: subject.to_string(),
            prepared: payload.into_publish_payload(),
            extra_headers: None,
        }
    }
}

#[doc(hidden)]
pub trait IntoPublishPayload {
    fn into_publish_payload(self) -> Result<PreparedPublishPayload, CrabbyError>;
}

pub struct PublishRequest {
    publisher: Publisher,
    subject: String,
    prepared: Result<PreparedPublishPayload, CrabbyError>,
    extra_headers: Option<HeaderMap>,
}

impl PublishRequest {
    /// Adds or overrides message headers before sending the publish request.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.extra_headers = Some(headers);
        self
    }
}

pub struct Request {
    publisher: Publisher,
    subject: String,
    prepared: Result<PreparedPublishPayload, CrabbyError>,
    extra_headers: Option<HeaderMap>,
}

impl Request {
    /// Adds or overrides message headers before sending the request.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.extra_headers = Some(headers);
        self
    }
}

impl IntoFuture for PublishRequest {
    type Output = Result<(), CrabbyError>;
    type IntoFuture = PublishFuture;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let prepared = self.prepared?;
            let headers = merge_headers(prepared.headers, self.extra_headers);

            self.publisher
                .inner
                .publish(self.subject, prepared.payload, headers)
                .await
        })
    }
}

impl IntoFuture for Request {
    type Output = Result<Reply, CrabbyError>;
    type IntoFuture = RequestFuture;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let prepared = self.prepared?;
            let headers = merge_headers(prepared.headers, self.extra_headers);

            self.publisher
                .inner
                .request(self.subject, prepared.payload, headers)
                .await
        })
    }
}

/// A broker reply returned by [`Publisher::request`].
pub struct Reply {
    subject: String,
    payload: Vec<u8>,
    headers: Option<HeaderMap>,
}

impl Reply {
    fn from_broker_message(message: BrokerMessage) -> Self {
        Self {
            subject: message.subject,
            payload: message.payload,
            headers: message.headers,
        }
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn headers(&self) -> Option<&HeaderMap> {
        self.headers.as_ref()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.payload
    }

    pub fn into_json<T>(self) -> Result<T, CrabbyError>
    where
        T: DeserializeOwned,
    {
        Ok(serde_json::from_slice(&self.payload)?)
    }

    pub fn into_cbor<T>(self) -> Result<T, CrabbyError>
    where
        T: DeserializeOwned,
    {
        Ok(ciborium::from_reader(self.payload.as_slice())?)
    }
}

pub fn merge_headers(base: Option<HeaderMap>, extra: Option<HeaderMap>) -> Option<HeaderMap> {
    match (base, extra) {
        (None, None) => None,
        (Some(headers), None) | (None, Some(headers)) => Some(headers),
        (Some(mut base), Some(extra)) => {
            base.extend(extra);
            Some(base)
        }
    }
}

impl IntoPublishPayload for Vec<u8> {
    fn into_publish_payload(self) -> Result<PreparedPublishPayload, CrabbyError> {
        Ok(PreparedPublishPayload {
            payload: self,
            headers: None,
        })
    }
}

impl IntoPublishPayload for Bytes {
    fn into_publish_payload(self) -> Result<PreparedPublishPayload, CrabbyError> {
        Ok(PreparedPublishPayload {
            payload: self.to_vec(),
            headers: None,
        })
    }
}

impl IntoPublishPayload for String {
    fn into_publish_payload(self) -> Result<PreparedPublishPayload, CrabbyError> {
        Ok(PreparedPublishPayload {
            payload: self.into_bytes(),
            headers: None,
        })
    }
}

impl IntoPublishPayload for &str {
    fn into_publish_payload(self) -> Result<PreparedPublishPayload, CrabbyError> {
        Ok(PreparedPublishPayload {
            payload: self.as_bytes().to_vec(),
            headers: None,
        })
    }
}

pub fn json_payload<T>(value: T) -> Result<PreparedPublishPayload, CrabbyError>
where
    T: Serialize,
{
    let mut headers = HeaderMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());

    Ok(PreparedPublishPayload {
        payload: serde_json::to_vec(&value)?,
        headers: Some(headers),
    })
}

pub fn cbor_payload<T>(value: T) -> Result<PreparedPublishPayload, CrabbyError>
where
    T: Serialize,
{
    let mut headers = HeaderMap::new();
    headers.insert("content-type".to_string(), "application/cbor".to_string());

    let mut payload = Vec::new();
    ciborium::into_writer(&value, &mut payload)?;

    Ok(PreparedPublishPayload {
        payload,
        headers: Some(headers),
    })
}
