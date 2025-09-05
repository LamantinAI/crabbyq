use crate::brokers::Broker;

use crate::handler::Handler;
use crate::service::CrabbyService;
use crate::event::Event;

pub struct Router {}

pub struct RouterWithState<S> {
    routes: Vec<(String, Box<dyn Handler<S>>)>,
    state: S,
}

impl Router {
    pub fn new() -> Self {
        Router {}
    }

    pub fn set_state<S: Clone + Send + Sync + 'static>(self, state: S) -> RouterWithState<S> {
        RouterWithState {
            routes: Vec::new(),
            state,
        }
    }
}

impl<S: Clone + Send + Sync + 'static> RouterWithState<S> {
    pub fn route<F, Fut>(mut self, subject: &str, handler: F) -> Self 
    where
        F: Fn(Event, S) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send,
    {
        struct AsyncHandler<F>(F);
        
        #[async_trait::async_trait]
        impl<F, S, Fut> Handler<S> for AsyncHandler<F>
        where
            F: Fn(Event, S) -> Fut + Send + Sync + 'static,
            Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send,
            S: Clone + Send + Sync + 'static,
        {
            async fn call(&self, event: Event, state: S) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                (self.0)(event, state).await
            }
        }
        
        self.routes.push((subject.to_string(), Box::new(AsyncHandler(handler))));
        self
    }

    pub fn into_service<B:Broker>(self, broker: B) -> CrabbyService<S, B> {
        CrabbyService::new(self.routes, self.state, broker)
    }
}