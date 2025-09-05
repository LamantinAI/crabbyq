use crate::brokers::Broker;
use crate::handler::Handler;
use crate::event::Event;
use futures_util::StreamExt;

pub struct CrabbyService<S, B> {
    routes: Vec<(String, Box<dyn Handler<S>>)>,
    state: S,
    broker: B,
}

impl<S, B> CrabbyService<S, B>
where
    S: Clone + Send + Sync + 'static,
    B: Broker,
{
    pub fn new(
        routes: Vec<(String, Box<dyn Handler<S>>)>,
        state: S,
        broker: B,
    ) -> Self {
        Self {
            routes,
            state,
            broker,
        }
    }

    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Collecting ass subject's for subsription
        let subjects: Vec<String> = self.routes.iter().map(|(s, _)| s.clone()).collect();
        
        if subjects.is_empty() {
            tracing::warn!("No routes registered, service will not subscribe to any subjects");
            tokio::signal::ctrl_c().await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            return Ok(());
        }

        tracing::info!("Subscribing to subjects: {:?}", subjects);
        let mut stream = self.broker.subscribe(&subjects).await?;

        tracing::info!("🦀 CrabbyQ service started!");

        while let Some(msg) = stream.next().await {
            let event = Event::new(
                msg.subject,
                msg.payload.into(),
                msg.headers,
            );

            let mut handled = false;
            for (pattern, handler) in &self.routes {
                if &event.subject == pattern {
                    if let Err(e) = handler.call(event.clone(), self.state.clone()).await {
                        tracing::error!("Handler error for subject '{}': {}", event.subject, e);
                    }
                    handled = true;
                    break;
                }
            }

            if !handled {
                tracing::warn!("No handler found for subject: {}", event.subject);
            }
        }

        tracing::info!("🦀 CrabbyQ service stopped");
        Ok(())
    }
}