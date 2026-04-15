use crate::brokers::NatsBroker;
use crate::brokers::base::{Acknowledger, BrokerError, BrokerMessage, HeaderMap};
use crate::errors::CrabbyError;
use crate::event::Event;
use crate::extract::RuntimeState;
use crate::handler::IntoHandler;
use crate::response::HandlerOutcome;
use crate::routing::base::Router;
use crate::service::{CrabbyService, MessageStreamFactory, ServiceMessageStream};
use futures_util::{StreamExt, future};
use std::future::Future;
use std::pin::Pin;
use tower::Layer;
use tower::Service;
use tower::util::BoxService;

type StreamInitFuture =
    Pin<Box<dyn Future<Output = Result<ServiceMessageStream, BrokerError>> + Send>>;

#[derive(Clone)]
struct JetStreamBinding {
    stream: async_nats::jetstream::stream::Stream,
}

struct NatsJsAcknowledger {
    message: async_nats::jetstream::Message,
}

impl Acknowledger for NatsJsAcknowledger {
    fn ack(self: Box<Self>) -> crate::brokers::base::AckFuture {
        Box::pin(async move { self.message.ack().await })
    }
}

struct NatsJsRouteStreamFactory {
    stream: async_nats::jetstream::stream::Stream,
    subject: String,
    durable: Option<String>,
}

impl MessageStreamFactory for NatsJsRouteStreamFactory {
    fn init(self: Box<Self>) -> StreamInitFuture {
        Box::pin(async move {
            let config = async_nats::jetstream::consumer::pull::Config {
                durable_name: self.durable.clone(),
                filter_subject: self.subject.clone(),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            };

            let consumer = if let Some(durable) = self.durable {
                self.stream
                    .get_or_create_consumer(&durable, config)
                    .await
                    .map_err(|error| Box::new(error) as BrokerError)?
            } else {
                self.stream
                    .create_consumer(config)
                    .await
                    .map_err(|error| Box::new(error) as BrokerError)?
            };

            let stream = consumer
                .messages()
                .await
                .map_err(|error| Box::new(error) as BrokerError)?
                .filter_map(|message| {
                    future::ready(match message {
                        Ok(message) => Some(BrokerMessage {
                            subject: message.message.subject.to_string(),
                            payload: message.message.payload.to_vec(),
                            headers: message.message.headers.as_ref().map(|headers| {
                                headers
                                    .iter()
                                    .filter_map(|(key, values)| {
                                        values
                                            .first()
                                            .map(|value| (key.to_string(), value.to_string()))
                                    })
                                    .collect()
                            }),
                            reply_to: message
                                .message
                                .reply
                                .as_ref()
                                .map(|reply| reply.to_string()),
                            acknowledger: Some(Box::new(NatsJsAcknowledger { message })),
                        }),
                        Err(error) => {
                            tracing::error!("JetStream stream error: {}", error);
                            None
                        }
                    })
                });

            Ok(Box::pin(stream) as ServiceMessageStream)
        })
    }
}

/// NATS-specific router facade built on top of the core [`Router`].
///
/// This type exists to host NATS-specific routing features such as JetStream
/// bindings without leaking them into the broker-agnostic core router.
///
/// Use [`Router`] for portable broker-agnostic routing, and switch to
/// [`NatsRouter`] when you want to mix plain NATS subscriptions with
/// NATS-specific features such as JetStream-backed routes.
pub struct NatsRouter<S = ()> {
    inner: Router<S>,
    jetstream: Option<JetStreamBinding>,
}

impl NatsRouter<()> {
    /// Creates a new stateless NATS router.
    pub fn new() -> Self {
        Self {
            inner: Router::new(),
            jetstream: None,
        }
    }

    /// Replaces the default `()` state with user-provided shared state.
    pub fn set_state<S: Clone + Send + Sync + 'static>(self, state: S) -> NatsRouter<S> {
        NatsRouter {
            inner: self.inner.set_state(state),
            jetstream: self.jetstream,
        }
    }
}

impl<S: Clone + Send + Sync + 'static> NatsRouter<S> {
    /// Registers a handler for a single route key.
    pub fn route<H, T>(self, subject: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        Self {
            inner: self.inner.route(subject, handler),
            jetstream: self.jetstream,
        }
    }

    /// Registers the same handler for multiple route keys.
    pub fn routes<I, H, T>(self, subjects: I, handler: H) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
        H: IntoHandler<RuntimeState<S>, T> + Clone + Send + 'static,
        T: 'static,
    {
        Self {
            inner: self.inner.routes(subjects, handler),
            jetstream: self.jetstream,
        }
    }

    /// Registers a raw `tower::Service` for a single route key.
    pub fn route_service<T>(self, subject: &str, service: T) -> Self
    where
        T: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send + 'static,
        T::Future: Send + 'static,
    {
        Self {
            inner: self.inner.route_service(subject, service),
            jetstream: self.jetstream,
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
            jetstream: self.jetstream,
        }
    }

    /// Configures an error topic for all routes in this router.
    pub fn on_error(self, topic: &str) -> Self {
        Self {
            inner: self.inner.on_error(topic),
            jetstream: self.jetstream,
        }
    }

    /// Configures static headers for router-level error events.
    pub fn error_headers(self, headers: HeaderMap) -> Self {
        Self {
            inner: self.inner.error_headers(headers),
            jetstream: self.jetstream,
        }
    }

    /// Includes all routes from another NATS router into the current router.
    pub fn include<OtherState: Clone + Send + Sync + 'static>(
        self,
        other: NatsRouter<OtherState>,
    ) -> Self {
        Self {
            inner: self.inner.include(other.inner),
            jetstream: self.jetstream,
        }
    }

    /// Includes all routes from a broker-agnostic core router.
    pub fn include_router<OtherState: Clone + Send + Sync + 'static>(
        self,
        other: Router<OtherState>,
    ) -> Self {
        Self {
            inner: self.inner.include(other),
            jetstream: self.jetstream,
        }
    }

    /// Sets the default JetStream stream for subsequent JS routes.
    ///
    /// The stream itself is intentionally created with `async-nats`, so
    /// CrabbyQ can reuse the native JetStream management API instead of
    /// wrapping stream configuration on its own.
    ///
    /// After calling this method, you can register JetStream-backed handlers
    /// with [`NatsRouter::js_route`] and [`NatsRouter::js_durable_route`].
    pub fn jetstream(self, stream: async_nats::jetstream::stream::Stream) -> Self {
        Self {
            inner: self.inner,
            jetstream: Some(JetStreamBinding { stream }),
        }
    }

    /// Registers a JetStream-backed route using the current stream binding.
    ///
    /// This route is consumed through JetStream rather than the plain
    /// `subscribe(...)` loop used by [`NatsRouter::route`]. The underlying
    /// consumer is ephemeral unless you use
    /// [`NatsRouter::js_durable_route`] instead.
    ///
    /// Panics if [`NatsRouter::jetstream`] has not been configured first.
    pub fn js_route<H, T>(self, subject: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        let binding = self
            .jetstream
            .as_ref()
            .expect("JetStream stream is not configured. Call NatsRouter::jetstream(...) before js_route(...)")
            .clone();

        Self {
            inner: self.inner.route_with_stream_factory(
                subject,
                handler,
                NatsJsRouteStreamFactory {
                    stream: binding.stream,
                    subject: subject.to_string(),
                    durable: None,
                },
            ),
            jetstream: self.jetstream,
        }
    }

    /// Registers a JetStream-backed route with a durable consumer name.
    ///
    /// Use this when a specific route should always reuse the same JetStream
    /// durable consumer across restarts.
    ///
    /// Panics if [`NatsRouter::jetstream`] has not been configured first.
    pub fn js_durable_route<H, T>(self, subject: &str, durable: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        let binding = self
            .jetstream
            .as_ref()
            .expect("JetStream stream is not configured. Call NatsRouter::jetstream(...) before js_durable_route(...)")
            .clone();

        Self {
            inner: self.inner.route_with_stream_factory(
                subject,
                handler,
                NatsJsRouteStreamFactory {
                    stream: binding.stream,
                    subject: subject.to_string(),
                    durable: Some(durable.to_string()),
                },
            ),
            jetstream: self.jetstream,
        }
    }

    /// Consumes the router and binds it to a NATS-backed service.
    pub fn into_service(self, broker: NatsBroker) -> CrabbyService<NatsBroker> {
        self.inner.into_service(broker)
    }

    /// Consumes the NATS router and returns the underlying core router.
    pub fn into_router(self) -> Router<S> {
        self.inner
    }

    /// Wraps an existing core router in a NATS-specific facade.
    pub fn from_router(router: Router<S>) -> Self {
        Self {
            inner: router,
            jetstream: None,
        }
    }
}
