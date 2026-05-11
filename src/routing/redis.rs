use crate::brokers::RedisBroker;
use crate::brokers::base::{Acknowledger, AckFuture, BrokerError, BrokerMessage, HeaderMap};
use crate::errors::CrabbyError;
use crate::event::Event;
use crate::extract::RuntimeState;
use crate::handler::IntoHandler;
use crate::response::HandlerOutcome;
use crate::routing::base::Router;
use crate::service::{CrabbyService, MessageStreamFactory, ServiceMessageStream};
use futures_util::stream::{self, BoxStream, StreamExt};
use redis::streams::{
    StreamAutoClaimOptions, StreamAutoClaimReply, StreamPendingCountReply, StreamReadOptions,
    StreamReadReply,
};
use redis::{AsyncCommands, Value};
#[cfg(feature = "json")]
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tower::Layer;
use tower::Service;
use tower::util::BoxService;

type StreamInitFuture =
    Pin<Box<dyn Future<Output = Result<ServiceMessageStream, BrokerError>> + Send>>;

/// Configuration for an ephemeral Redis Stream consumer registered with
/// [`RedisRouter::x_route_with`].
#[derive(Clone, Debug)]
pub struct RedisStreamRouteConfig {
    /// Stream key to read from (this is also used as the route's broker subject).
    pub stream: String,
    /// `XREAD` start id. Defaults to `$` (only entries newer than the start).
    pub start_id: String,
    /// `BLOCK <ms>` option. `None` disables blocking.
    pub block: Option<Duration>,
    /// `COUNT <n>` option.
    pub count: Option<usize>,
}

impl RedisStreamRouteConfig {
    /// Creates a new ephemeral Redis Stream route config with sensible defaults.
    pub fn new(stream: impl Into<String>) -> Self {
        Self {
            stream: stream.into(),
            start_id: "$".to_string(),
            block: Some(Duration::from_secs(5)),
            count: Some(100),
        }
    }

    /// Overrides the start id (e.g. `0` to read all history, `$` for new-only).
    pub fn start_id(mut self, id: impl Into<String>) -> Self {
        self.start_id = id.into();
        self
    }

    /// Sets the `BLOCK` interval.
    pub fn block(mut self, dur: Duration) -> Self {
        self.block = Some(dur);
        self
    }

    /// Disables `BLOCK`, making `XREAD` return immediately.
    pub fn no_block(mut self) -> Self {
        self.block = None;
        self
    }

    /// Sets the `COUNT` upper bound per `XREAD`.
    pub fn count(mut self, n: usize) -> Self {
        self.count = Some(n);
        self
    }
}

/// Periodic `XAUTOCLAIM` configuration for consumer-group routes.
///
/// When attached to a [`RedisGroupRouteConfig`], the consumer additionally
/// runs an `XAUTOCLAIM` loop alongside its main `XREADGROUP` read loop, so
/// pending entries idle for longer than [`AutoClaimConfig::min_idle`] are
/// reclaimed by this consumer and re-delivered to the handler.
///
/// Re-delivered entries carry the `redis-redelivered: 1` header so
/// non-idempotent handlers can branch on it.
#[derive(Clone, Debug)]
pub struct AutoClaimConfig {
    /// Minimum time a pending entry must have been idle before this consumer
    /// claims it.
    pub min_idle: Duration,
    /// How often the auto-claim loop runs.
    pub interval: Duration,
    /// `COUNT <n>` upper bound per `XAUTOCLAIM` call.
    pub count: usize,
}

impl AutoClaimConfig {
    /// Builds an auto-claim config using `min_idle / 2` as scan interval and
    /// `100` as count per scan.
    pub fn new(min_idle: Duration) -> Self {
        let interval = if min_idle.is_zero() {
            Duration::from_secs(30)
        } else {
            min_idle / 2
        };
        Self {
            min_idle,
            interval,
            count: 100,
        }
    }

    /// Overrides the scan interval.
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Overrides the `COUNT` argument used per scan.
    pub fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }
}

/// Configuration for a consumer-group route registered with
/// [`RedisRouter::x_group_route_with`].
#[derive(Clone, Debug)]
pub struct RedisGroupRouteConfig {
    /// Stream key to read from (also used as the route's broker subject).
    pub stream: String,
    /// Consumer group name. Created with `MKSTREAM` if [`mkstream`](Self::mkstream)
    /// is set.
    pub group: String,
    /// Consumer name inside the group.
    pub consumer: String,
    /// `XREADGROUP` start id. Defaults to `>` (new messages for this consumer).
    pub start_id: String,
    /// `BLOCK <ms>` option.
    pub block: Option<Duration>,
    /// `COUNT <n>` option.
    pub count: Option<usize>,
    /// Whether to call `XGROUP CREATE ... MKSTREAM` on startup if the stream
    /// or group do not exist yet. Defaults to `true`.
    pub mkstream: bool,
    /// Optional `XAUTOCLAIM` reliability loop config.
    pub auto_claim: Option<AutoClaimConfig>,
    /// Optional cap on the total number of times an entry may be delivered to
    /// the handler. After the cap, the auto-claim loop `XACK`s the entry to
    /// remove it from the pending list and (if [`dead_letter_topic`] is set)
    /// publishes a [`DeadLetterEvent`] to the dead-letter stream.
    ///
    /// Requires [`auto_claim`] to be configured. Has no effect on its own.
    ///
    /// [`auto_claim`]: Self::auto_claim
    /// [`dead_letter_topic`]: Self::dead_letter_topic
    pub max_deliveries: Option<usize>,
    /// Optional dead-letter stream key. When [`max_deliveries`] is exceeded
    /// and this is set, the entry is `XADD`-ed to this stream as a
    /// [`DeadLetterEvent`].
    ///
    /// [`max_deliveries`]: Self::max_deliveries
    pub dead_letter_topic: Option<String>,
}

impl RedisGroupRouteConfig {
    /// Creates a new consumer-group route config with sensible defaults.
    pub fn new(
        stream: impl Into<String>,
        group: impl Into<String>,
        consumer: impl Into<String>,
    ) -> Self {
        Self {
            stream: stream.into(),
            group: group.into(),
            consumer: consumer.into(),
            start_id: ">".to_string(),
            block: Some(Duration::from_secs(5)),
            count: Some(100),
            mkstream: true,
            auto_claim: None,
            max_deliveries: None,
            dead_letter_topic: None,
        }
    }

    pub fn start_id(mut self, id: impl Into<String>) -> Self {
        self.start_id = id.into();
        self
    }

    pub fn block(mut self, dur: Duration) -> Self {
        self.block = Some(dur);
        self
    }

    pub fn no_block(mut self) -> Self {
        self.block = None;
        self
    }

    pub fn count(mut self, n: usize) -> Self {
        self.count = Some(n);
        self
    }

    pub fn mkstream(mut self, val: bool) -> Self {
        self.mkstream = val;
        self
    }

    /// Enables periodic `XAUTOCLAIM` re-delivery with default scan interval
    /// (`min_idle / 2`) and count (`100`).
    pub fn auto_claim(mut self, min_idle: Duration) -> Self {
        self.auto_claim = Some(AutoClaimConfig::new(min_idle));
        self
    }

    /// Enables periodic `XAUTOCLAIM` re-delivery with a fully custom config.
    pub fn auto_claim_with(mut self, config: AutoClaimConfig) -> Self {
        self.auto_claim = Some(config);
        self
    }

    /// Caps the total number of times an entry may be delivered to the
    /// handler. After the cap is exceeded, the auto-claim loop `XACK`s the
    /// entry and (if `dead_letter(...)` is configured) publishes a
    /// [`DeadLetterEvent`].
    ///
    /// Requires [`auto_claim`] to be configured. Has no effect otherwise.
    ///
    /// [`auto_claim`]: Self::auto_claim
    pub fn max_deliveries(mut self, n: usize) -> Self {
        self.max_deliveries = Some(n);
        self
    }

    /// Sets the stream that receives [`DeadLetterEvent`] entries when
    /// [`max_deliveries`] is exceeded.
    ///
    /// [`max_deliveries`]: Self::max_deliveries
    pub fn dead_letter(mut self, topic: impl Into<String>) -> Self {
        self.dead_letter_topic = Some(topic.into());
        self
    }
}

/// Redis-specific router facade built on top of the core [`Router`].
///
/// Use [`Router`] for portable broker-agnostic pub/sub routing, and switch to
/// [`RedisRouter`] when you want to mix plain Redis pub/sub with
/// Redis-specific transports such as Streams (`XREAD`/`XREADGROUP`) and
/// pattern subscriptions (`PSUBSCRIBE`).
///
/// Stream entries are encoded with a `payload` field holding the message body
/// and the remaining stream fields mapped one-to-one into message headers.
/// Consumer-group routes acknowledge entries automatically through `XACK` when
/// the handler returns `Ok(())`; on error the entry stays in the pending list
/// so it can be retried or claimed with `XAUTOCLAIM`.
pub struct RedisRouter<S = ()> {
    inner: Router<S>,
    broker: Option<RedisBroker>,
}

impl RedisRouter<()> {
    /// Creates a new stateless Redis router.
    pub fn new() -> Self {
        Self {
            inner: Router::new(),
            broker: None,
        }
    }

    /// Replaces the default `()` state with user-provided shared state.
    pub fn set_state<S: Clone + Send + Sync + 'static>(self, state: S) -> RedisRouter<S> {
        RedisRouter {
            inner: self.inner.set_state(state),
            broker: self.broker,
        }
    }
}

impl Default for RedisRouter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Clone + Send + Sync + 'static> RedisRouter<S> {
    /// Registers a handler for a plain Redis pub/sub channel.
    pub fn route<H, T>(self, subject: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        Self {
            inner: self.inner.route(subject, handler),
            broker: self.broker,
        }
    }

    /// Registers the same handler for multiple pub/sub channels.
    pub fn routes<I, H, T>(self, subjects: I, handler: H) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
        H: IntoHandler<RuntimeState<S>, T> + Clone + Send + 'static,
        T: 'static,
    {
        Self {
            inner: self.inner.routes(subjects, handler),
            broker: self.broker,
        }
    }

    /// Registers a raw `tower::Service` for a pub/sub channel.
    pub fn route_service<T>(self, subject: &str, service: T) -> Self
    where
        T: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send + 'static,
        T::Future: Send + 'static,
    {
        Self {
            inner: self.inner.route_service(subject, service),
            broker: self.broker,
        }
    }

    /// Applies a `tower::Layer` to all routes in this router.
    pub fn layer<L>(self, layer: L) -> Self
    where
        L: Layer<BoxService<Event, HandlerOutcome, CrabbyError>> + Send + Sync + 'static,
        L::Service: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send + 'static,
        <L::Service as Service<Event>>::Future: Send + 'static,
    {
        Self {
            inner: self.inner.layer(layer),
            broker: self.broker,
        }
    }

    /// Configures an error topic for all routes in this router.
    pub fn on_error(self, topic: &str) -> Self {
        Self {
            inner: self.inner.on_error(topic),
            broker: self.broker,
        }
    }

    /// Configures static headers for router-level error events.
    pub fn error_headers(self, headers: HeaderMap) -> Self {
        Self {
            inner: self.inner.error_headers(headers),
            broker: self.broker,
        }
    }

    /// Includes all routes from another Redis router into the current one.
    pub fn include<OtherState: Clone + Send + Sync + 'static>(
        self,
        other: RedisRouter<OtherState>,
    ) -> Self {
        Self {
            inner: self.inner.include(other.inner),
            broker: self.broker,
        }
    }

    /// Includes all routes from a broker-agnostic core router.
    pub fn include_router<OtherState: Clone + Send + Sync + 'static>(
        self,
        other: Router<OtherState>,
    ) -> Self {
        Self {
            inner: self.inner.include(other),
            broker: self.broker,
        }
    }

    /// Registers an ephemeral `XREAD`-backed route with default options.
    ///
    /// The stream key is also used as the broker subject for dispatch, so the
    /// handler matches messages from this stream only. Defaults: `BLOCK 5000`,
    /// `COUNT 100`, start id `$`.
    pub fn x_route<H, T>(self, stream: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        self.x_route_with(RedisStreamRouteConfig::new(stream), handler)
    }

    /// Registers an ephemeral `XREAD`-backed route with custom options.
    pub fn x_route_with<H, T>(self, config: RedisStreamRouteConfig, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        let broker = self.broker.clone().expect(
            "Redis broker is not bound. Call RedisRouter::with_broker(...) before x_route(...)",
        );
        let stream = config.stream.clone();
        let factory = RedisXReadStreamFactory {
            client: broker.client(),
            config,
        };
        Self {
            inner: self.inner.route_with_stream_factory(&stream, handler, factory),
            broker: self.broker,
        }
    }

    /// Registers a consumer-group `XREADGROUP` route with default options.
    ///
    /// Entries are acknowledged (`XACK`) automatically when the handler
    /// returns `Ok`. On error they remain in the pending list. Defaults:
    /// `BLOCK 5000`, `COUNT 100`, `MKSTREAM` enabled, start id `>`.
    pub fn x_group_route<H, T>(
        self,
        stream: &str,
        group: &str,
        consumer: &str,
        handler: H,
    ) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        self.x_group_route_with(
            RedisGroupRouteConfig::new(stream, group, consumer),
            handler,
        )
    }

    /// Registers a consumer-group `XREADGROUP` route with custom options.
    pub fn x_group_route_with<H, T>(self, config: RedisGroupRouteConfig, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        let broker = self.broker.clone().expect(
            "Redis broker is not bound. Call RedisRouter::with_broker(...) before x_group_route(...)",
        );
        let stream = config.stream.clone();
        let factory = RedisXReadGroupStreamFactory {
            client: broker.client(),
            config,
        };
        Self {
            inner: self.inner.route_with_stream_factory(&stream, handler, factory),
            broker: self.broker,
        }
    }

    /// Registers a `PSUBSCRIBE` route matching the given Redis channel pattern.
    ///
    /// Note: route dispatch is exact-match on the registered pattern string,
    /// so the handler receives every message whose channel matches the
    /// pattern, with the actual channel name stored in the `redis-channel`
    /// header.
    pub fn psub_route<H, T>(self, pattern: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        let broker = self.broker.clone().expect(
            "Redis broker is not bound. Call RedisRouter::with_broker(...) before psub_route(...)",
        );
        let factory = RedisPsubStreamFactory {
            client: broker.client(),
            pattern: pattern.to_string(),
        };
        Self {
            inner: self
                .inner
                .route_with_stream_factory(pattern, handler, factory),
            broker: self.broker,
        }
    }

    /// Binds this router to a [`RedisBroker`] so Redis-specific routes can
    /// open their own connections during service startup.
    ///
    /// This must be called before any `x_route`, `x_group_route`, or
    /// `psub_route` registration.
    pub fn with_broker(mut self, broker: RedisBroker) -> Self {
        self.broker = Some(broker);
        self
    }

    /// Consumes the router and binds it to the Redis-backed service.
    pub fn into_service(self, broker: RedisBroker) -> CrabbyService<RedisBroker> {
        self.inner.into_service(broker)
    }

    /// Consumes the Redis router and returns the underlying core router.
    pub fn into_router(self) -> Router<S> {
        self.inner
    }

    /// Wraps an existing core router in a Redis-specific facade.
    pub fn from_router(router: Router<S>) -> Self {
        Self {
            inner: router,
            broker: None,
        }
    }
}

struct RedisXReadStreamFactory {
    client: redis::Client,
    config: RedisStreamRouteConfig,
}

impl MessageStreamFactory for RedisXReadStreamFactory {
    fn init(self: Box<Self>) -> StreamInitFuture {
        Box::pin(async move {
            let connection = self
                .client
                .get_multiplexed_async_connection()
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;
            let state = XReadState {
                connection,
                stream: self.config.stream.clone(),
                last_id: self.config.start_id.clone(),
                block_ms: self.config.block.map(|d| d.as_millis() as usize),
                count: self.config.count,
                buffer: Vec::new(),
                group: None,
            };
            let stream: BoxStream<'static, BrokerMessage> =
                stream::unfold(state, xread_next).boxed();
            Ok(Box::pin(stream) as ServiceMessageStream)
        })
    }
}

struct RedisXReadGroupStreamFactory {
    client: redis::Client,
    config: RedisGroupRouteConfig,
}

impl MessageStreamFactory for RedisXReadGroupStreamFactory {
    fn init(self: Box<Self>) -> StreamInitFuture {
        Box::pin(async move {
            let mut connection = self
                .client
                .get_multiplexed_async_connection()
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;

            if self.config.mkstream {
                let create_result: redis::RedisResult<()> = redis::cmd("XGROUP")
                    .arg("CREATE")
                    .arg(&self.config.stream)
                    .arg(&self.config.group)
                    .arg("$")
                    .arg("MKSTREAM")
                    .query_async(&mut connection)
                    .await;

                if let Err(error) = create_result {
                    let already_exists = error
                        .to_string()
                        .contains("BUSYGROUP");
                    if !already_exists {
                        return Err(Box::new(error) as BrokerError);
                    }
                }
            }

            let read_state = XReadState {
                connection,
                stream: self.config.stream.clone(),
                last_id: self.config.start_id.clone(),
                block_ms: self.config.block.map(|d| d.as_millis() as usize),
                count: self.config.count,
                buffer: Vec::new(),
                group: Some(GroupRead {
                    name: self.config.group.clone(),
                    consumer: self.config.consumer.clone(),
                    client: self.client.clone(),
                }),
            };
            let read_stream: BoxStream<'static, BrokerMessage> =
                stream::unfold(read_state, xread_next).boxed();

            let merged: BoxStream<'static, BrokerMessage> = if let Some(auto_claim) =
                self.config.auto_claim.clone()
            {
                let claim_connection = self
                    .client
                    .get_multiplexed_async_connection()
                    .await
                    .map_err(|e| Box::new(e) as BrokerError)?;
                let claim_state = AutoClaimState {
                    connection: claim_connection,
                    stream: self.config.stream.clone(),
                    group: self.config.group.clone(),
                    consumer: self.config.consumer.clone(),
                    client: self.client.clone(),
                    cursor: "0-0".to_string(),
                    buffer: Vec::new(),
                    config: auto_claim,
                    max_deliveries: self.config.max_deliveries,
                    dead_letter_topic: self.config.dead_letter_topic.clone(),
                };
                let claim_stream: BoxStream<'static, BrokerMessage> =
                    stream::unfold(claim_state, xautoclaim_next).boxed();
                stream::select(read_stream, claim_stream).boxed()
            } else {
                read_stream
            };

            Ok(Box::pin(merged) as ServiceMessageStream)
        })
    }
}

struct GroupRead {
    name: String,
    consumer: String,
    client: redis::Client,
}

struct XReadState {
    connection: redis::aio::MultiplexedConnection,
    stream: String,
    last_id: String,
    block_ms: Option<usize>,
    count: Option<usize>,
    buffer: Vec<BrokerMessage>,
    group: Option<GroupRead>,
}

async fn xread_next(mut state: XReadState) -> Option<(BrokerMessage, XReadState)> {
    loop {
        if let Some(message) = state.buffer.pop() {
            return Some((message, state));
        }

        let mut options = StreamReadOptions::default();
        if let Some(ms) = state.block_ms {
            options = options.block(ms);
        }
        if let Some(n) = state.count {
            options = options.count(n);
        }
        if let Some(ref group) = state.group {
            options = options.group(&group.name, &group.consumer);
        }

        let reply: redis::RedisResult<Option<StreamReadReply>> = state
            .connection
            .xread_options(&[state.stream.as_str()], &[state.last_id.as_str()], &options)
            .await;

        match reply {
            Ok(Some(read)) => {
                for stream_key in read.keys {
                    for entry in stream_key.ids {
                        let entry_id = entry.id.clone();
                        if state.group.is_none() {
                            state.last_id = entry_id.clone();
                        }
                        let message = stream_entry_to_message(
                            &state.stream,
                            entry,
                            state.group.as_ref().map(|g| (g.name.clone(), g.client.clone())),
                            entry_id,
                            false,
                        );
                        state.buffer.push(message);
                    }
                }
                state.buffer.reverse();
                if state.buffer.is_empty() {
                    continue;
                }
            }
            Ok(None) => continue,
            Err(error) => {
                tracing::error!("Redis Streams read error on '{}': {}", state.stream, error);
                return None;
            }
        }
    }
}

struct AutoClaimState {
    connection: redis::aio::MultiplexedConnection,
    stream: String,
    group: String,
    consumer: String,
    client: redis::Client,
    cursor: String,
    buffer: Vec<BrokerMessage>,
    config: AutoClaimConfig,
    max_deliveries: Option<usize>,
    dead_letter_topic: Option<String>,
}

async fn xautoclaim_next(
    mut state: AutoClaimState,
) -> Option<(BrokerMessage, AutoClaimState)> {
    loop {
        if let Some(message) = state.buffer.pop() {
            return Some((message, state));
        }

        // The sleep paces consecutive XAUTOCLAIM scans. We sleep before the
        // scan so a freshly started consumer does not immediately churn the
        // PEL on launch.
        tokio::time::sleep(state.config.interval).await;

        let options = StreamAutoClaimOptions::default().count(state.config.count);
        let min_idle_ms = state.config.min_idle.as_millis() as usize;

        let reply: redis::RedisResult<StreamAutoClaimReply> = state
            .connection
            .xautoclaim_options(
                state.stream.as_str(),
                state.group.as_str(),
                state.consumer.as_str(),
                min_idle_ms,
                state.cursor.as_str(),
                options,
            )
            .await;

        let claimed = match reply {
            Ok(reply) => {
                state.cursor = if reply.next_stream_id.is_empty() {
                    "0-0".to_string()
                } else {
                    reply.next_stream_id
                };
                reply.claimed
            }
            Err(error) => {
                tracing::error!(
                    "Redis XAUTOCLAIM error on '{}' group '{}': {}",
                    state.stream,
                    state.group,
                    error
                );
                // Keep the loop alive — broker hiccups should not tear down
                // the route. The next interval will retry.
                continue;
            }
        };

        if claimed.is_empty() {
            continue;
        }

        // Resolve per-entry delivery counts via XPENDING when we need to
        // make dead-letter decisions or surface the count to handlers.
        let delivery_counts = if state.max_deliveries.is_some() {
            fetch_delivery_counts(
                &mut state.connection,
                &state.stream,
                &state.group,
                &state.consumer,
                &claimed,
            )
            .await
        } else {
            HashMap::new()
        };

        for entry in claimed {
            let entry_id = entry.id.clone();
            let count = delivery_counts.get(&entry_id).copied();

            if let (Some(count), Some(max)) = (count, state.max_deliveries) {
                if count > max {
                    handle_dead_letter(
                        &state.client,
                        &state.stream,
                        &state.group,
                        &entry_id,
                        count,
                        max,
                        &entry,
                        state.dead_letter_topic.as_deref(),
                    )
                    .await;
                    continue;
                }
            }

            let mut message = stream_entry_to_message(
                &state.stream,
                entry,
                Some((state.group.clone(), state.client.clone())),
                entry_id,
                true,
            );
            if let Some(count) = count {
                let headers = message.headers.get_or_insert_with(HashMap::new);
                headers.insert("redis-delivery-count".to_string(), count.to_string());
            }
            state.buffer.push(message);
        }
        state.buffer.reverse();
    }
}

async fn fetch_delivery_counts(
    connection: &mut redis::aio::MultiplexedConnection,
    stream: &str,
    group: &str,
    consumer: &str,
    claimed: &[redis::streams::StreamId],
) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    if claimed.is_empty() {
        return counts;
    }

    let ids: Vec<&str> = claimed.iter().map(|c| c.id.as_str()).collect();
    let start = ids.iter().min().copied().unwrap_or("-");
    let end = ids.iter().max().copied().unwrap_or("+");

    let reply: redis::RedisResult<StreamPendingCountReply> = connection
        .xpending_consumer_count(stream, group, start, end, claimed.len(), consumer)
        .await;

    match reply {
        Ok(reply) => {
            for entry in reply.ids {
                counts.insert(entry.id, entry.times_delivered);
            }
        }
        Err(error) => {
            tracing::warn!(
                "Redis XPENDING failed for '{stream}' group '{group}': {error}; \
                 dead-letter decisions will be skipped for this batch",
            );
        }
    }

    counts
}

#[cfg(feature = "json")]
#[derive(Serialize)]
struct DeadLetterEvent<'a> {
    subject: &'a str,
    stream: &'a str,
    group: &'a str,
    entry_id: &'a str,
    delivery_count: usize,
    max_deliveries: usize,
    headers: HashMap<String, String>,
    payload: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
async fn handle_dead_letter(
    client: &redis::Client,
    stream: &str,
    group: &str,
    entry_id: &str,
    delivery_count: usize,
    max_deliveries: usize,
    entry: &redis::streams::StreamId,
    dead_letter_topic: Option<&str>,
) {
    let ack_connection = client.get_multiplexed_async_connection().await;
    match ack_connection {
        Ok(mut connection) => {
            let ack_result: redis::RedisResult<i64> = redis::cmd("XACK")
                .arg(stream)
                .arg(group)
                .arg(entry_id)
                .query_async(&mut connection)
                .await;
            if let Err(error) = ack_result {
                tracing::error!(
                    "Dead-letter XACK failed for '{stream}' entry '{entry_id}': {error}",
                );
                return;
            }
        }
        Err(error) => {
            tracing::error!(
                "Dead-letter connect failed for '{stream}' entry '{entry_id}': {error}",
            );
            return;
        }
    }

    tracing::warn!(
        "Dead-lettered entry '{entry_id}' from stream '{stream}' group '{group}' \
         after {delivery_count} deliveries (max {max_deliveries})",
    );

    let Some(topic) = dead_letter_topic else {
        return;
    };

    let (payload_bytes, headers_map) = split_entry_payload(entry);

    let publish_result = publish_dead_letter(
        client,
        topic,
        stream,
        group,
        entry_id,
        delivery_count,
        max_deliveries,
        headers_map,
        payload_bytes,
    )
    .await;

    if let Err(error) = publish_result {
        tracing::error!(
            "Dead-letter publish to '{topic}' failed for entry '{entry_id}': {error}",
        );
    }
}

fn split_entry_payload(entry: &redis::streams::StreamId) -> (Vec<u8>, HashMap<String, String>) {
    let mut payload = Vec::new();
    let mut headers = HashMap::new();
    for (field, value) in entry.map.iter() {
        match field.as_str() {
            "payload" => payload = value_to_bytes(value),
            _ => {
                if let Some(text) = value_to_string(value) {
                    headers.insert(field.clone(), text);
                }
            }
        }
    }
    (payload, headers)
}

#[allow(clippy::too_many_arguments)]
async fn publish_dead_letter(
    client: &redis::Client,
    topic: &str,
    stream: &str,
    group: &str,
    entry_id: &str,
    delivery_count: usize,
    max_deliveries: usize,
    headers: HashMap<String, String>,
    payload: Vec<u8>,
) -> Result<(), BrokerError> {
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| Box::new(e) as BrokerError)?;

    let mut cmd = redis::cmd("XADD");
    cmd.arg(topic).arg("*");

    #[cfg(feature = "json")]
    {
        let event = DeadLetterEvent {
            subject: stream,
            stream,
            group,
            entry_id,
            delivery_count,
            max_deliveries,
            headers,
            payload,
        };
        let bytes =
            serde_json::to_vec(&event).map_err(|e| Box::new(e) as BrokerError)?;
        cmd.arg("payload").arg(bytes.as_slice());
        cmd.arg("content-type").arg("application/json");
    }
    #[cfg(not(feature = "json"))]
    {
        cmd.arg("payload").arg(payload.as_slice());
        cmd.arg("stream").arg(stream);
        cmd.arg("group").arg(group);
        cmd.arg("entry_id").arg(entry_id);
        cmd.arg("delivery_count").arg(delivery_count.to_string());
        cmd.arg("max_deliveries").arg(max_deliveries.to_string());
        for (k, v) in headers {
            cmd.arg(k).arg(v);
        }
    }

    let _: String = cmd
        .query_async(&mut connection)
        .await
        .map_err(|e| Box::new(e) as BrokerError)?;
    Ok(())
}

fn stream_entry_to_message(
    stream: &str,
    entry: redis::streams::StreamId,
    group: Option<(String, redis::Client)>,
    entry_id: String,
    redelivered: bool,
) -> BrokerMessage {
    let mut payload: Vec<u8> = Vec::new();
    let mut reply_to: Option<String> = None;
    let mut headers: HashMap<String, String> = HashMap::new();

    for (field, value) in entry.map.into_iter() {
        match field.as_str() {
            "payload" => payload = value_to_bytes(&value),
            "reply_to" => reply_to = value_to_string(&value),
            _ => {
                if let Some(text) = value_to_string(&value) {
                    headers.insert(field, text);
                }
            }
        }
    }

    if redelivered {
        headers.insert("redis-redelivered".to_string(), "1".to_string());
    }

    let acknowledger: Option<Box<dyn Acknowledger>> = group.map(|(group_name, client)| {
        Box::new(RedisXAcknowledger {
            client,
            stream: stream.to_string(),
            group: group_name,
            entry_id,
        }) as Box<dyn Acknowledger>
    });

    BrokerMessage {
        subject: stream.to_string(),
        payload,
        headers: if headers.is_empty() { None } else { Some(headers) },
        reply_to,
        acknowledger,
    }
}

fn value_to_bytes(value: &Value) -> Vec<u8> {
    match value {
        Value::BulkString(bytes) => bytes.clone(),
        Value::SimpleString(s) => s.as_bytes().to_vec(),
        Value::Int(i) => i.to_string().into_bytes(),
        _ => Vec::new(),
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::BulkString(bytes) => String::from_utf8(bytes.clone()).ok(),
        Value::SimpleString(s) => Some(s.clone()),
        Value::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

struct RedisXAcknowledger {
    client: redis::Client,
    stream: String,
    group: String,
    entry_id: String,
}

impl Acknowledger for RedisXAcknowledger {
    fn ack(self: Box<Self>) -> AckFuture {
        Box::pin(async move {
            let mut connection = self
                .client
                .get_multiplexed_async_connection()
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;
            let _: i64 = redis::cmd("XACK")
                .arg(&self.stream)
                .arg(&self.group)
                .arg(&self.entry_id)
                .query_async(&mut connection)
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;
            Ok(())
        })
    }
}

struct RedisPsubStreamFactory {
    client: redis::Client,
    pattern: String,
}

impl MessageStreamFactory for RedisPsubStreamFactory {
    fn init(self: Box<Self>) -> StreamInitFuture {
        Box::pin(async move {
            let mut pubsub = self
                .client
                .get_async_pubsub()
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;
            pubsub
                .psubscribe(&self.pattern)
                .await
                .map_err(|e| Box::new(e) as BrokerError)?;
            let pattern = self.pattern.clone();
            let stream = pubsub.into_on_message().map(move |msg| {
                let actual_channel = msg.get_channel_name().to_string();
                let mut headers = HashMap::new();
                headers.insert("redis-channel".to_string(), actual_channel);
                BrokerMessage {
                    subject: pattern.clone(),
                    payload: msg.get_payload_bytes().to_vec(),
                    headers: Some(headers),
                    reply_to: None,
                    acknowledger: None,
                }
            });
            Ok(Box::pin(stream) as ServiceMessageStream)
        })
    }
}

