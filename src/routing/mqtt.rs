use crate::brokers::MqttBroker;
use crate::brokers::base::HeaderMap;
use crate::brokers::mqtt::MqttRouteOptions;
use crate::errors::CrabbyError;
use crate::event::Event;
use crate::extract::RuntimeState;
use crate::handler::IntoHandler;
use crate::response::HandlerOutcome;
use crate::routing::base::Router;
use crate::service::CrabbyService;
use rumqttc::QoS;
use std::collections::HashMap;
use tower::Layer;
use tower::Service;
use tower::util::BoxService;

/// MQTT-specific router facade built on top of the core [`Router`].
///
/// Use [`Router`] for portable broker-agnostic routing, and switch to
/// [`MqttRouter`] when you want per-route QoS, shared subscriptions, or
/// retained-message routes.
///
/// Note: MQTT v5 user properties are not yet supported. The underlying
/// `MqttBroker` is currently MQTT v3.1.1 only.
pub struct MqttRouter<S = ()> {
    inner: Router<S>,
    default_qos: QoS,
    subscription_options: HashMap<String, MqttRouteOptions>,
}

impl MqttRouter<()> {
    /// Creates a new stateless MQTT router with a default QoS of `AtLeastOnce`.
    pub fn new() -> Self {
        Self {
            inner: Router::new(),
            default_qos: QoS::AtLeastOnce,
            subscription_options: HashMap::new(),
        }
    }

    /// Replaces the default `()` state with user-provided shared state.
    pub fn set_state<S: Clone + Send + Sync + 'static>(self, state: S) -> MqttRouter<S> {
        MqttRouter {
            inner: self.inner.set_state(state),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }
}

impl Default for MqttRouter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Clone + Send + Sync + 'static> MqttRouter<S> {
    /// Sets the default QoS used for routes that do not override it.
    pub fn qos(mut self, qos: QoS) -> Self {
        self.default_qos = qos;
        self
    }

    /// Registers a handler with the router's default QoS.
    pub fn route<H, T>(self, subject: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        Self {
            inner: self.inner.route(subject, handler),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Registers the same handler for multiple subjects with the default QoS.
    pub fn routes<I, H, T>(self, subjects: I, handler: H) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
        H: IntoHandler<RuntimeState<S>, T> + Clone + Send + 'static,
        T: 'static,
    {
        Self {
            inner: self.inner.routes(subjects, handler),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Registers a raw `tower::Service` for a single subject.
    pub fn route_service<T>(self, subject: &str, service: T) -> Self
    where
        T: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send + 'static,
        T::Future: Send + 'static,
    {
        Self {
            inner: self.inner.route_service(subject, service),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
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
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Configures an error topic for all routes in this router.
    pub fn on_error(self, topic: &str) -> Self {
        Self {
            inner: self.inner.on_error(topic),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Configures static headers for router-level error events.
    pub fn error_headers(self, headers: HeaderMap) -> Self {
        Self {
            inner: self.inner.error_headers(headers),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Includes all routes from another MQTT router into the current one.
    ///
    /// The included router's subscription options (per-route QoS overrides
    /// and shared subscription rewrites) are merged into the current router.
    pub fn include<OtherState: Clone + Send + Sync + 'static>(
        mut self,
        other: MqttRouter<OtherState>,
    ) -> Self {
        for (subject, opts) in other.subscription_options {
            self.subscription_options.insert(subject, opts);
        }
        Self {
            inner: self.inner.include(other.inner),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Includes all routes from a broker-agnostic core router.
    pub fn include_router<OtherState: Clone + Send + Sync + 'static>(
        self,
        other: Router<OtherState>,
    ) -> Self {
        Self {
            inner: self.inner.include(other),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Registers a route with a specific QoS, overriding the router default.
    pub fn qos_route<H, T>(mut self, subject: &str, qos: QoS, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        self.subscription_options.insert(
            subject.to_string(),
            MqttRouteOptions {
                qos,
                subscribe_as: subject.to_string(),
            },
        );
        Self {
            inner: self.inner.route(subject, handler),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Registers a shared-subscription route on `$share/<group>/<subject>`.
    ///
    /// The handler is dispatched against the underlying subject, so multiple
    /// service instances joining the same group share the message load on
    /// brokers that support shared subscriptions (MQTT v5 and most modern
    /// v3.1.1 brokers as an extension).
    pub fn shared_route<H, T>(mut self, group: &str, subject: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        self.subscription_options.insert(
            subject.to_string(),
            MqttRouteOptions {
                qos: self.default_qos,
                subscribe_as: format!("$share/{group}/{subject}"),
            },
        );
        Self {
            inner: self.inner.route(subject, handler),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Registers a shared-subscription route with an explicit QoS.
    pub fn shared_qos_route<H, T>(
        mut self,
        group: &str,
        subject: &str,
        qos: QoS,
        handler: H,
    ) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        self.subscription_options.insert(
            subject.to_string(),
            MqttRouteOptions {
                qos,
                subscribe_as: format!("$share/{group}/{subject}"),
            },
        );
        Self {
            inner: self.inner.route(subject, handler),
            default_qos: self.default_qos,
            subscription_options: self.subscription_options,
        }
    }

    /// Registers a route that handles retained messages on subscribe.
    ///
    /// In MQTT v3.1.1 retained messages are always delivered to a fresh
    /// subscription, so this method is currently an alias for [`route`] used
    /// to mark a handler as retained-aware. The marker will start to matter
    /// once MQTT v5 retain-handling options land.
    ///
    /// [`route`]: Self::route
    pub fn retained_route<H, T>(self, subject: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        self.route(subject, handler)
    }

    /// Consumes the router and binds it to an MQTT-backed service.
    pub fn into_service(self, broker: MqttBroker) -> CrabbyService<MqttBroker> {
        let broker = broker
            .with_default_qos(self.default_qos)
            .with_subscription_options(self.subscription_options);
        self.inner.into_service(broker)
    }

    /// Consumes the MQTT router and returns the underlying core router.
    pub fn into_router(self) -> Router<S> {
        self.inner
    }

    /// Wraps an existing core router in an MQTT-specific facade.
    pub fn from_router(router: Router<S>) -> Self {
        Self {
            inner: router,
            default_qos: QoS::AtLeastOnce,
            subscription_options: HashMap::new(),
        }
    }
}
