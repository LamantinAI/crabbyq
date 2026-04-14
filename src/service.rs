pub struct CrabbyService<B> {
    routes: Vec<ServiceRoute>,
    broker: B,
    error_topic: Option<String>,
    error_headers: Option<crate::brokers::base::HeaderMap>,
    shutdown_signal: Option<ShutdownSignal>,
    shutdown_hook: Option<Box<dyn ShutdownHook>>,
}

use crate::brokers::Broker;
use crate::brokers::base::HeaderMap;
use crate::errors::CrabbyError;
use crate::event::Event;
use crate::publish::{Publisher, json_payload};
use crate::response::{HandlerOutcome, error_outcome};
use futures_util::StreamExt;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use tower::Service;
use tower::ServiceExt;
use tower::util::BoxService;

type ShutdownSignal = Pin<Box<dyn Future<Output = ()> + Send>>;
type ShutdownHookFuture = Pin<Box<dyn Future<Output = Result<(), CrabbyError>> + Send>>;

pub(crate) struct ServiceRoute {
    pub(crate) subject: String,
    pub(crate) error_topic: Option<String>,
    pub(crate) error_headers: Option<HeaderMap>,
    pub(crate) service: BoxService<Event, HandlerOutcome, CrabbyError>,
}

#[derive(Serialize)]
struct ErrorEvent {
    subject: String,
    reply_to: Option<String>,
    headers: Option<HeaderMap>,
    payload: Vec<u8>,
    error: String,
}

trait ShutdownHook: Send {
    fn call(self: Box<Self>, publisher: Publisher) -> ShutdownHookFuture;
}

impl<F, Fut> ShutdownHook for F
where
    F: FnOnce(Publisher) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), CrabbyError>> + Send + 'static,
{
    fn call(self: Box<Self>, publisher: Publisher) -> ShutdownHookFuture {
        Box::pin((*self)(publisher))
    }
}

impl<B> CrabbyService<B>
where
    B: Broker + Clone,
{
    pub(crate) fn new(routes: Vec<ServiceRoute>, broker: B) -> Self {
        Self {
            routes,
            broker,
            error_topic: None,
            error_headers: None,
            shutdown_signal: None,
            shutdown_hook: None,
        }
    }

    /// Configures a fallback error topic for routes that do not define their
    /// own [`Router::on_error`][crate::Router::on_error] policy.
    ///
    /// Handler failures are always logged. This topic is only used when the
    /// matched route does not carry a router-level error topic.
    pub fn on_error(mut self, topic: &str) -> Self {
        self.error_topic = Some(topic.to_string());
        self
    }

    /// Alias for [`CrabbyService::on_error`] using DLQ terminology.
    pub fn dlq(self, topic: &str) -> Self {
        self.on_error(topic)
    }

    /// Configures fallback headers for published error events.
    ///
    /// These headers are merged with route-level error headers when an error is
    /// published. Route-level headers override service-level ones on duplicate
    /// keys.
    pub fn error_headers(mut self, headers: HeaderMap) -> Self {
        self.error_headers = Some(headers);
        self
    }

    /// Replaces the default Ctrl+C handling with a custom graceful shutdown signal.
    ///
    /// Once this signal resolves, the service stops accepting new messages,
    /// runs the optional shutdown hook while the broker is still available, and
    /// then returns from [`CrabbyService::serve`].
    pub fn with_graceful_shutdown<F>(mut self, signal: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.shutdown_signal = Some(Box::pin(signal));
        self
    }

    /// Registers a final async hook that runs during graceful shutdown.
    ///
    /// The hook receives a live [`Publisher`] so the service can emit one last
    /// message before the broker-backed runtime is dropped.
    pub fn on_shutdown<F, Fut>(mut self, hook: F) -> Self
    where
        F: FnOnce(Publisher) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), CrabbyError>> + Send + 'static,
    {
        self.shutdown_hook = Some(Box::new(hook));
        self
    }

    /// Starts the broker subscription loop and dispatches incoming messages.
    ///
    /// Error behavior is:
    /// - handler failures are always logged;
    /// - route-level `on_error(...)` is used first when present;
    /// - otherwise the service-level fallback `on_error(...)`/`dlq(...)` is used;
    /// - if no error topic is configured, the failure is only logged.
    pub async fn serve(mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let publisher = Publisher::new(self.broker.clone());

        // Collecting ass subject's for subsription
        let subjects: Vec<String> = self.routes.iter().map(|route| route.subject.clone()).collect();
        
        if subjects.is_empty() {
            tracing::warn!("No routes registered, service will not subscribe to any subjects");
            self.wait_for_shutdown().await?;
            self.run_shutdown_hook(publisher).await;
            return Ok(());
        }

        tracing::info!("Subscribing to subjects: {:?}", subjects);
        let mut stream = self.broker.subscribe(&subjects).await?;

        tracing::info!("CrabbyQ service started!");

        let mut shutdown_signal = self
            .shutdown_signal
            .take()
            .unwrap_or_else(default_shutdown_signal);

        loop {
            tokio::select! {
                _ = &mut shutdown_signal => {
                    tracing::info!("Shutdown signal received, stopping message intake");
                    break;
                }
                message = stream.next() => {
                    match message {
                        Some(msg) => self.handle_message(msg).await,
                        None => break,
                    }
                }
            }
        }

        self.run_shutdown_hook(publisher).await;
        tracing::info!("CrabbyQ service stopped");
        Ok(())
    }

    async fn wait_for_shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(signal) = self.shutdown_signal.take() {
            signal.await;
            return Ok(());
        }

        tokio::signal::ctrl_c().await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn run_shutdown_hook(&mut self, publisher: Publisher) {
        let Some(hook) = self.shutdown_hook.take() else {
            return;
        };

        if let Err(error) = hook.call(publisher).await {
            tracing::error!("Shutdown hook error: {}", error);
        }
    }

    async fn handle_message(&mut self, msg: crate::brokers::base::BrokerMessage) {
        let subject = msg.subject.clone();
        let event = Event::new(
            msg.subject,
            msg.payload.into(),
            msg.headers,
            msg.reply_to,
        );

        match self.dispatch_event(&subject, event).await {
            Some(result) => {
                let error_message = result.outcome.error_message.clone();
                let route_error_topic = result.route_error_topic.clone();
                let route_error_headers = result.route_error_headers.clone();
                let service_error_topic = self.error_topic.clone();
                let service_error_headers = self.error_headers.clone();
                let reply_to = result.reply_to.clone();
                let headers = result.headers.clone();
                let payload = result.payload.clone();

                Self::publish_reply(&self.broker, &subject, reply_to.as_deref(), result.outcome.response)
                    .await;
                Self::publish_error(
                    &self.broker,
                    &subject,
                    error_message,
                    route_error_topic,
                    route_error_headers,
                    service_error_topic,
                    service_error_headers,
                    reply_to,
                    headers,
                    payload,
                )
                .await;
            }
            None => tracing::warn!("No handler found for subject: {}", subject),
        }
    }

    async fn publish_error(
        broker: &B,
        subject: &str,
        error_message: Option<String>,
        route_error_topic: Option<String>,
        route_error_headers: Option<HeaderMap>,
        service_error_topic: Option<String>,
        service_error_headers: Option<HeaderMap>,
        reply_to: Option<String>,
        message_headers: Option<HeaderMap>,
        payload: Vec<u8>,
    ) {
        let Some(error_message) = error_message else {
            return;
        };

        // Logging is unconditional so operational visibility does not depend on
        // whether the application configured an error topic.
        tracing::error!("Handler error for subject '{}': {}", subject, error_message);

        let error_topic = route_error_topic
            .as_deref()
            .or(service_error_topic.as_deref());

        let Some(error_topic) = error_topic else {
            return;
        };

        let mut error_headers = service_error_headers.unwrap_or_default();
        if let Some(route_headers) = route_error_headers {
            error_headers.extend(route_headers);
        }

        match json_payload(ErrorEvent {
            subject: subject.to_string(),
            reply_to,
            headers: message_headers,
            payload,
            error: error_message.clone(),
        }) {
            Ok(mut prepared) => {
                prepared.headers = match (
                    prepared.headers,
                    (!error_headers.is_empty()).then_some(error_headers),
                ) {
                    (None, extra) => extra,
                    (Some(base), None) => Some(base),
                    (Some(mut base), Some(extra)) => {
                        base.extend(extra);
                        Some(base)
                    }
                };

                if let Err(e) = broker
                    .publish(error_topic, &prepared.payload, prepared.headers.as_ref())
                    .await
                {
                    tracing::error!(
                        "Error publish failure for subject '{}' to topic '{}': {}",
                        subject,
                        error_topic,
                        e
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    "Failed to serialize error event for subject '{}': {}",
                    subject,
                    e
                );
            }
        }
    }

    async fn dispatch_event(
        &mut self,
        subject: &str,
        event: Event,
    ) -> Option<DispatchResult> {
        let reply_to = event.reply_to().map(str::to_owned);
        let headers = event.headers().cloned();
        let payload = event.payload.clone().to_vec();

        for route in &mut self.routes {
            if subject != route.subject {
                continue;
            }

            let outcome = match route.service.ready().await {
                Ok(ready) => match ready.call(event).await {
                    Ok(outcome) => outcome,
                    Err(e) => match error_outcome(e) {
                        Ok(outcome) => outcome,
                        Err(error) => {
                            tracing::error!("Failed to convert service error for subject '{}': {}", subject, error);
                            HandlerOutcome {
                                response: None,
                                error_message: Some("internal handler error".to_string()),
                            }
                        }
                    },
                },
                Err(e) => match error_outcome(e) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        tracing::error!("Failed to convert readiness error for subject '{}': {}", subject, error);
                        HandlerOutcome {
                            response: None,
                            error_message: Some("service readiness error".to_string()),
                        }
                    }
                },
            };

            return Some(DispatchResult {
                reply_to,
                headers,
                payload,
                route_error_topic: route.error_topic.clone(),
                route_error_headers: route.error_headers.clone(),
                outcome,
            });
        }

        None
    }

    async fn publish_reply(
        broker: &B,
        subject: &str,
        reply_to: Option<&str>,
        response: Option<crate::publish::PreparedPublishPayload>,
    ) {
        let (Some(reply_to), Some(response)) = (reply_to, response) else {
            return;
        };

        if let Err(e) = broker
            .publish(&reply_to, &response.payload, response.headers.as_ref())
            .await
        {
            tracing::error!("Reply publish error for subject '{}': {}", subject, e);
        }
    }
}

struct DispatchResult {
    reply_to: Option<String>,
    headers: Option<HeaderMap>,
    payload: Vec<u8>,
    route_error_topic: Option<String>,
    route_error_headers: Option<HeaderMap>,
    outcome: HandlerOutcome,
}

fn default_shutdown_signal() -> ShutdownSignal {
    Box::pin(async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to listen for Ctrl+C: {}", error);
        }
    })
}
