use crate::event::Event;
use async_trait::async_trait;
use std::future::Future;

#[async_trait]
pub trait Handler<S>: Send + Sync + 'static {
    async fn call(&self, event: Event, state: S) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

pub struct FnHandler<F>(pub F);

#[async_trait]
impl<F, S, Fut> Handler<S> for FnHandler<F>
where
    F: Fn(Event, S) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send,
    S: Clone + Send + Sync + 'static, 
{
    async fn call(&self, event: Event, state: S) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        (self.0)(event, state).await
    }
}