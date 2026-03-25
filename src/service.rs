pub struct CrabbyService<B> {
    routes: Vec<ServiceRoute>,
    broker: B,
    error_topic: Option<String>,
    error_headers: Option<crate::brokers::base::HeaderMap>,
}

use crate::brokers::Broker;
use crate::brokers::base::HeaderMap;
use crate::errors::CrabbyError;
use crate::event::Event;
use crate::publish::json_payload;
use crate::response::{HandlerOutcome, error_outcome};
use futures_util::StreamExt;
use serde::Serialize;
use tower::Service;
use tower::ServiceExt;
use tower::util::BoxService;

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

impl<B> CrabbyService<B>
where
    B: Broker,
{
    pub(crate) fn new(routes: Vec<ServiceRoute>, broker: B) -> Self {
        Self {
            routes,
            broker,
            error_topic: None,
            error_headers: None,
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

    /// Starts the broker subscription loop and dispatches incoming messages.
    ///
    /// Error behavior is:
    /// - handler failures are always logged;
    /// - route-level `on_error(...)` is used first when present;
    /// - otherwise the service-level fallback `on_error(...)`/`dlq(...)` is used;
    /// - if no error topic is configured, the failure is only logged.
    pub async fn serve(mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Collecting ass subject's for subsription
        let subjects: Vec<String> = self.routes.iter().map(|route| route.subject.clone()).collect();
        
        if subjects.is_empty() {
            tracing::warn!("No routes registered, service will not subscribe to any subjects");
            tokio::signal::ctrl_c().await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            return Ok(());
        }

        tracing::info!("Subscribing to subjects: {:?}", subjects);
        let mut stream = self.broker.subscribe(&subjects).await?;

        tracing::info!("CrabbyQ service started!");

        while let Some(msg) = stream.next().await {
            self.handle_message(msg).await;
        }

        tracing::info!("CrabbyQ service stopped");
        Ok(())
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
                let reply_to = result.reply_to.clone();
                let headers = result.headers.clone();
                let payload = result.payload.clone();

                self.publish_reply(&subject, reply_to.as_deref(), result.outcome.response)
                    .await;
                self.publish_error(
                    &subject,
                    error_message,
                    route_error_topic,
                    route_error_headers,
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
        &self,
        subject: &str,
        error_message: Option<String>,
        route_error_topic: Option<String>,
        route_error_headers: Option<HeaderMap>,
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
            .or(self.error_topic.as_deref());

        let Some(error_topic) = error_topic else {
            return;
        };

        let mut error_headers = self.error_headers.clone().unwrap_or_default();
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

                if let Err(e) = self
                    .broker
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

    async fn publish_reply(&self, subject: &str, reply_to: Option<&str>, response: Option<crate::publish::PreparedPublishPayload>) {
        let (Some(reply_to), Some(response)) = (reply_to, response) else {
            return;
        };

        if let Err(e) = self
            .broker
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
