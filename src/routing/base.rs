use crate::brokers::Broker;
use crate::brokers::base::HeaderMap;
use crate::errors::CrabbyError;
use crate::event::Event;
use crate::extract::RuntimeState;
use crate::handler::IntoHandler;
use crate::publish::Publisher;
use crate::response::HandlerOutcome;
use crate::service::{CrabbyService, MessageStreamFactory};
use std::sync::Arc;
use tower::Layer;
use tower::Service;
use tower::util::BoxService;

/// A builder for message routes and shared application state.
///
/// `Router::new()` starts with `()` state by default, so stateless handlers work
/// out of the box. Call [`Router::set_state`] to switch to typed shared state.
pub struct Router<S = ()> {
    routes: Vec<RouteDefinition>,
    state: S,
    error_topic: Option<String>,
    error_headers: Option<HeaderMap>,
    layers: Vec<Arc<dyn RouteLayer>>,
}

trait RouteBuilder: Send {
    fn build(
        self: Box<Self>,
        publisher: Publisher,
    ) -> BoxService<Event, HandlerOutcome, CrabbyError>;
}

trait RouteLayer: Send + Sync {
    fn layer(
        &self,
        service: BoxService<Event, HandlerOutcome, CrabbyError>,
    ) -> BoxService<Event, HandlerOutcome, CrabbyError>;
}

struct TowerRouteLayer<L> {
    layer: L,
}

impl<L> TowerRouteLayer<L> {
    fn new(layer: L) -> Self {
        Self { layer }
    }
}

impl<L> RouteLayer for TowerRouteLayer<L>
where
    L: Layer<BoxService<Event, HandlerOutcome, CrabbyError>> + Send + Sync + 'static,
    L::Service: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send + 'static,
    <L::Service as Service<Event>>::Future: Send + 'static,
{
    fn layer(
        &self,
        service: BoxService<Event, HandlerOutcome, CrabbyError>,
    ) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(self.layer.layer(service))
    }
}

struct RouteDefinition {
    subject: String,
    error_topic: Option<String>,
    error_headers: Option<HeaderMap>,
    layers: Vec<Arc<dyn RouteLayer>>,
    stream_factory: Option<Box<dyn MessageStreamFactory>>,
    builder: Box<dyn RouteBuilder>,
}

struct HandlerRoute<H, S> {
    handler: H,
    state: S,
    _marker: std::marker::PhantomData<fn()>,
}

impl<H, S> HandlerRoute<H, S> {
    fn new(handler: H, state: S) -> Self {
        Self {
            handler,
            state,
            _marker: std::marker::PhantomData,
        }
    }
}

struct TypedHandlerRoute<H, S, T> {
    inner: HandlerRoute<H, S>,
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<H, S, T> TypedHandlerRoute<H, S, T> {
    fn new(handler: H, state: S) -> Self {
        Self {
            inner: HandlerRoute::new(handler, state),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<H, S, T> RouteBuilder for TypedHandlerRoute<H, S, T>
where
    H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn build(
        self: Box<Self>,
        publisher: Publisher,
    ) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        let Self { inner, .. } = *self;
        let HandlerRoute { handler, state, .. } = inner;
        handler.into_handler(RuntimeState::new(state, publisher))
    }
}

struct ServiceRoute<T> {
    service: T,
}

impl<T> ServiceRoute<T> {
    fn new(service: T) -> Self {
        Self { service }
    }
}

impl<T> RouteBuilder for ServiceRoute<T>
where
    T: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send + 'static,
    T::Future: Send + 'static,
{
    fn build(
        self: Box<Self>,
        _publisher: Publisher,
    ) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(self.service)
    }
}

impl Router<()> {
    /// Creates a new stateless router.
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            state: (),
            error_topic: None,
            error_headers: None,
            layers: Vec::new(),
        }
    }

    /// Replaces the default `()` state with user-provided shared state.
    pub fn set_state<S: Clone + Send + Sync + 'static>(self, state: S) -> Router<S> {
        Router {
            routes: self.routes,
            state,
            error_topic: self.error_topic,
            error_headers: self.error_headers,
            layers: self.layers,
        }
    }
}

impl<S: Clone + Send + Sync + 'static> Router<S> {
    /// Registers a handler for a single route key.
    pub fn route<H, T>(self, subject: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        self.route_handler(subject, handler)
    }

    /// Registers the same handler for multiple route keys.
    pub fn routes<I, H, T>(mut self, subjects: I, handler: H) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
        H: IntoHandler<RuntimeState<S>, T> + Clone + Send + 'static,
        T: 'static,
    {
        for subject in subjects {
            self = self.route_handler(subject.as_ref(), handler.clone());
        }

        self
    }

    /// Registers a raw `tower::Service` for a single route key.
    pub fn route_service<T>(mut self, subject: &str, service: T) -> Self
    where
        T: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send + 'static,
        T::Future: Send + 'static,
    {
        self.assert_route_available(subject);
        self.routes.push(RouteDefinition {
            subject: subject.to_string(),
            error_topic: self.error_topic.clone(),
            error_headers: self.error_headers.clone(),
            layers: self.layers.clone(),
            stream_factory: None,
            builder: Box::new(ServiceRoute::new(service)),
        });
        self
    }

    /// Applies a `tower::Layer` to all routes in this router.
    ///
    /// The layer is attached to already registered routes and is also applied
    /// to routes added later. When routers are combined with [`Router::include`],
    /// each included route keeps the layers it was defined with.
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<BoxService<Event, HandlerOutcome, CrabbyError>> + Send + Sync + 'static,
        L::Service: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send + 'static,
        <L::Service as Service<Event>>::Future: Send + 'static,
    {
        let layer = Arc::new(TowerRouteLayer::new(layer)) as Arc<dyn RouteLayer>;
        self.layers.push(layer.clone());
        for route in &mut self.routes {
            route.layers.push(layer.clone());
        }
        self
    }

    /// Configures an error topic for all routes in this router.
    ///
    /// Handler failures are always logged. When an error topic is configured,
    /// the service also publishes a structured error event there.
    ///
    /// Router-level error topics override any fallback error topic configured
    /// on the final [`CrabbyService`][crate::service::CrabbyService].
    pub fn on_error(mut self, topic: &str) -> Self {
        let topic = topic.to_string();
        self.error_topic = Some(topic.clone());
        for route in &mut self.routes {
            route.error_topic = Some(topic.clone());
        }
        self
    }

    /// Configures static headers for router-level error events.
    ///
    /// These headers are attached when a handler error is published through the
    /// router-level [`Router::on_error`] policy. If the final service also has
    /// fallback error headers configured, both sets are merged and route-level
    /// headers win on duplicate keys.
    pub fn error_headers(mut self, headers: HeaderMap) -> Self {
        self.error_headers = Some(headers.clone());
        for route in &mut self.routes {
            route.error_headers = Some(headers.clone());
        }
        self
    }

    /// Includes all routes from another router into the current router.
    ///
    /// This is intentionally broker-agnostic: no prefix rewriting or path-like
    /// nesting is applied to route keys.
    pub fn include<OtherState>(mut self, other: Router<OtherState>) -> Self {
        for route in other.routes {
            self.assert_route_available(&route.subject);
            self.routes.push(route);
        }
        self
    }

    fn route_handler<H, T>(mut self, subject: &str, handler: H) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
    {
        self.assert_route_available(subject);
        self.routes.push(RouteDefinition {
            subject: subject.to_string(),
            error_topic: self.error_topic.clone(),
            error_headers: self.error_headers.clone(),
            layers: self.layers.clone(),
            stream_factory: None,
            builder: Box::new(TypedHandlerRoute::<H, S, T>::new(
                handler,
                self.state.clone(),
            )),
        });
        self
    }

    pub(crate) fn route_with_stream_factory<H, T, F>(
        mut self,
        subject: &str,
        handler: H,
        stream_factory: F,
    ) -> Self
    where
        H: IntoHandler<RuntimeState<S>, T> + Send + 'static,
        T: 'static,
        F: MessageStreamFactory + 'static,
    {
        self.assert_route_available(subject);
        self.routes.push(RouteDefinition {
            subject: subject.to_string(),
            error_topic: self.error_topic.clone(),
            error_headers: self.error_headers.clone(),
            layers: self.layers.clone(),
            stream_factory: Some(Box::new(stream_factory)),
            builder: Box::new(TypedHandlerRoute::<H, S, T>::new(
                handler,
                self.state.clone(),
            )),
        });
        self
    }

    fn assert_route_available(&self, subject: &str) {
        assert!(
            !self.routes.iter().any(|route| route.subject == subject),
            "duplicate route key '{subject}' is already registered"
        );
    }

    /// Consumes the router and binds it to a broker-backed service.
    pub fn into_service<B: Broker + Clone>(self, broker: B) -> CrabbyService<B> {
        let publisher = Publisher::new(broker.clone());
        let routes = self
            .routes
            .into_iter()
            .map(|route| {
                let mut service = route.builder.build(publisher.clone());
                for layer in route.layers {
                    service = layer.layer(service);
                }

                crate::service::ServiceRoute {
                    subject: route.subject,
                    error_topic: route.error_topic,
                    error_headers: route.error_headers,
                    stream_factory: route.stream_factory,
                    service,
                }
            })
            .collect();

        CrabbyService::new(routes, broker)
    }
}
