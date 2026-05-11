use crate::brokers::RedisBroker;
use crate::brokers::base::{BrokerError, HeaderMap};
use crate::errors::CrabbyError;
use crate::publish::base::{
    IntoPublishPayload, PreparedPublishPayload, PublishRequest, Publisher, Request, merge_headers,
};
use redis::streams::StreamMaxlen;
use std::future::Future;
use std::future::IntoFuture;
use std::pin::Pin;

type XAddFuture = Pin<Box<dyn Future<Output = Result<String, CrabbyError>> + Send>>;

/// Redis-specific publishing facade built on top of the core [`Publisher`].
///
/// `Publisher` remains the broker-agnostic publishing capability injected
/// into handlers through the `Publish` extractor. `RedisPublisher` exists for
/// transport-specific features such as `XADD` to Redis Streams while still
/// delegating plain `publish(...)` and `request(...)` to the core API.
#[derive(Clone)]
pub struct RedisPublisher {
    core: Publisher,
    client: redis::Client,
}

impl RedisPublisher {
    /// Creates a Redis-specific publisher from a [`RedisBroker`].
    pub fn new(broker: RedisBroker) -> Self {
        let client = broker.client();
        Self {
            core: Publisher::new(broker),
            client,
        }
    }

    /// Returns the broker-agnostic core publisher used by this Redis publisher.
    pub fn core(&self) -> Publisher {
        self.core.clone()
    }

    /// Starts building a plain `PUBLISH` request through the core publisher API.
    pub fn publish<P>(&self, subject: &str, payload: P) -> PublishRequest
    where
        P: IntoPublishPayload,
    {
        self.core.publish(subject, payload)
    }

    /// Starts building a plain request-reply call through the core publisher API.
    ///
    /// Note: Redis pub/sub does not support request-reply, so awaiting the
    /// resulting future returns an error. This method is provided for parity
    /// with the broker-agnostic [`Publisher`] surface.
    pub fn request<P>(&self, subject: &str, payload: P) -> Request
    where
        P: IntoPublishPayload,
    {
        self.core.request(subject, payload)
    }

    /// Starts building an `XADD` request that writes an entry to a Redis Stream.
    ///
    /// The payload is stored in a stream field named `payload`. Headers are
    /// stored as additional fields, one per header pair, so that they
    /// round-trip through `RedisRouter::x_route` / `x_group_route`.
    pub fn xadd<P>(&self, stream: &str, payload: P) -> RedisXAddRequest
    where
        P: IntoPublishPayload,
    {
        RedisXAddRequest {
            client: self.client.clone(),
            stream: stream.to_string(),
            prepared: payload.into_publish_payload(),
            extra_headers: None,
            extra_fields: Vec::new(),
            id: None,
            maxlen: None,
        }
    }
}

/// Builder for an `XADD` operation created by [`RedisPublisher::xadd`].
pub struct RedisXAddRequest {
    client: redis::Client,
    stream: String,
    prepared: Result<PreparedPublishPayload, CrabbyError>,
    extra_headers: Option<HeaderMap>,
    extra_fields: Vec<(String, Vec<u8>)>,
    id: Option<String>,
    maxlen: Option<StreamMaxlen>,
}

impl RedisXAddRequest {
    /// Adds or overrides message headers before sending the `XADD`.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.extra_headers = Some(headers);
        self
    }

    /// Adds a raw field/value pair to the stream entry.
    ///
    /// Use this for fields that should be stored separately from `payload` or
    /// from the framework's `headers` mapping.
    pub fn field(mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.extra_fields.push((name.into(), value.into()));
        self
    }

    /// Sets the entry id (defaults to `*`).
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets `MAXLEN = <n>` trimming.
    pub fn max_len(mut self, n: usize) -> Self {
        self.maxlen = Some(StreamMaxlen::Equals(n));
        self
    }

    /// Sets `MAXLEN ~ <n>` (approximate) trimming.
    pub fn max_len_approx(mut self, n: usize) -> Self {
        self.maxlen = Some(StreamMaxlen::Approx(n));
        self
    }
}

impl IntoFuture for RedisXAddRequest {
    type Output = Result<String, CrabbyError>;
    type IntoFuture = XAddFuture;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let prepared = self.prepared?;
            let headers = merge_headers(prepared.headers, self.extra_headers);
            let mut connection = self
                .client
                .get_multiplexed_async_connection()
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;

            let mut cmd = redis::cmd("XADD");
            cmd.arg(&self.stream);

            if let Some(maxlen) = self.maxlen {
                match maxlen {
                    StreamMaxlen::Equals(n) => {
                        cmd.arg("MAXLEN").arg("=").arg(n);
                    }
                    StreamMaxlen::Approx(n) => {
                        cmd.arg("MAXLEN").arg("~").arg(n);
                    }
                }
            }

            cmd.arg(self.id.as_deref().unwrap_or("*"));
            cmd.arg("payload").arg(prepared.payload.as_slice());

            if let Some(headers) = headers {
                for (key, value) in headers {
                    cmd.arg(key).arg(value);
                }
            }

            for (key, value) in self.extra_fields {
                cmd.arg(key).arg(value);
            }

            let id: String = cmd
                .query_async(&mut connection)
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;
            Ok(id)
        })
    }
}
