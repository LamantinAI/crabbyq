use crate::errors::CrabbyError;
use crate::event::Event;
use crate::extract::{FromEvent, FromEventParts, FromRef, RuntimeState};
use crate::response::{HandlerOutcome, IntoHandlerResult};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::Service;
use tower::util::BoxService;

pub trait IntoHandler<S, T>: Sized {
    fn into_handler(self, state: S) -> BoxService<Event, HandlerOutcome, CrabbyError>;
}

#[derive(Clone)]
pub struct StatefulHandler<F, S> {
    f: F,
    state: S,
}

impl<F, S> StatefulHandler<F, S> {
    pub fn new(f: F, state: S) -> Self {
        Self { f, state }
    }
}

impl<F, S, Fut, Res> Service<Event> for StatefulHandler<F, S>
where
    F: Fn(Event, S) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoHandlerResult + Send + 'static,
    S: Clone + Send + 'static,
{
    type Response = HandlerOutcome;
    type Error = CrabbyError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, event: Event) -> Self::Future {
        let f = self.f.clone();
        let state = self.state.clone();

        Box::pin(async move { f(event, state).await.into_handler_result() })
    }
}

#[derive(Clone)]
pub struct StatelessHandler<F> {
    f: F,
}

impl<F> StatelessHandler<F> {
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

impl<F, Fut, Res> Service<Event> for StatelessHandler<F>
where
    F: Fn(Event) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoHandlerResult + Send + 'static,
{
    type Response = HandlerOutcome;
    type Error = CrabbyError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, event: Event) -> Self::Future {
        let f = self.f.clone();

        Box::pin(async move { f(event).await.into_handler_result() })
    }
}

#[derive(Clone)]
pub struct FromEventHandler<F, S, A> {
    f: F,
    state: S,
    _marker: PhantomData<fn() -> A>,
}

impl<F, S, A> FromEventHandler<F, S, A> {
    pub fn new(f: F, state: S) -> Self {
        Self {
            f,
            state,
            _marker: PhantomData,
        }
    }
}

impl<F, Fut, S, A, Res> Service<Event> for FromEventHandler<F, S, A>
where
    F: Fn(A) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoHandlerResult + Send + 'static,
    A: FromEvent<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    type Response = HandlerOutcome;
    type Error = CrabbyError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, event: Event) -> Self::Future {
        let f = self.f.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let arg = A::from_event(event, &state).await?;
            f(arg).await.into_handler_result()
        })
    }
}

#[derive(Clone)]
pub struct FromPartsHandler<F, S, A> {
    f: F,
    state: S,
    _marker: PhantomData<fn() -> A>,
}

impl<F, S, A> FromPartsHandler<F, S, A> {
    pub fn new(f: F, state: S) -> Self {
        Self {
            f,
            state,
            _marker: PhantomData,
        }
    }
}

impl<F, Fut, S, A, Res> Service<Event> for FromPartsHandler<F, S, A>
where
    F: Fn(A) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoHandlerResult + Send + 'static,
    A: FromEventParts<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    type Response = HandlerOutcome;
    type Error = CrabbyError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, event: Event) -> Self::Future {
        let f = self.f.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let (mut parts, _payload) = event.into_parts();
            let arg = A::from_event_parts(&mut parts, &state).await?;
            f(arg).await.into_handler_result()
        })
    }
}

#[derive(Clone)]
pub struct PartsBodyHandler<F, S, A, B> {
    f: F,
    state: S,
    _marker: PhantomData<fn() -> (A, B)>,
}

impl<F, S, A, B> PartsBodyHandler<F, S, A, B> {
    pub fn new(f: F, state: S) -> Self {
        Self {
            f,
            state,
            _marker: PhantomData,
        }
    }
}

impl<F, Fut, S, A, B, Res> Service<Event> for PartsBodyHandler<F, S, A, B>
where
    F: Fn(A, B) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoHandlerResult + Send + 'static,
    A: FromEventParts<S> + Send + 'static,
    B: FromEvent<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    type Response = HandlerOutcome;
    type Error = CrabbyError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, event: Event) -> Self::Future {
        let f = self.f.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let (mut parts, payload) = event.into_parts();
            let first = A::from_event_parts(&mut parts, &state).await?;
            let second = B::from_event(Event { parts, payload }, &state).await?;
            f(first, second).await.into_handler_result()
        })
    }
}

#[derive(Clone)]
pub struct TwoPartsHandler<F, S, A, B> {
    f: F,
    state: S,
    _marker: PhantomData<fn() -> (A, B)>,
}

impl<F, S, A, B> TwoPartsHandler<F, S, A, B> {
    pub fn new(f: F, state: S) -> Self {
        Self {
            f,
            state,
            _marker: PhantomData,
        }
    }
}

impl<F, Fut, S, A, B, Res> Service<Event> for TwoPartsHandler<F, S, A, B>
where
    F: Fn(A, B) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoHandlerResult + Send + 'static,
    A: FromEventParts<S> + Send + 'static,
    B: FromEventParts<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    type Response = HandlerOutcome;
    type Error = CrabbyError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, event: Event) -> Self::Future {
        let f = self.f.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let (mut parts, _payload) = event.into_parts();
            let first = A::from_event_parts(&mut parts, &state).await?;
            let second = B::from_event_parts(&mut parts, &state).await?;
            f(first, second).await.into_handler_result()
        })
    }
}

#[derive(Clone)]
pub struct TwoPartsBodyHandler<F, S, A, B, C> {
    f: F,
    state: S,
    _marker: PhantomData<fn() -> (A, B, C)>,
}

impl<F, S, A, B, C> TwoPartsBodyHandler<F, S, A, B, C> {
    pub fn new(f: F, state: S) -> Self {
        Self {
            f,
            state,
            _marker: PhantomData,
        }
    }
}

impl<F, Fut, S, A, B, C, Res> Service<Event> for TwoPartsBodyHandler<F, S, A, B, C>
where
    F: Fn(A, B, C) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoHandlerResult + Send + 'static,
    A: FromEventParts<S> + Send + 'static,
    B: FromEventParts<S> + Send + 'static,
    C: FromEvent<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    type Response = HandlerOutcome;
    type Error = CrabbyError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, event: Event) -> Self::Future {
        let f = self.f.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let (mut parts, payload) = event.into_parts();
            let first = A::from_event_parts(&mut parts, &state).await?;
            let second = B::from_event_parts(&mut parts, &state).await?;
            let third = C::from_event(Event { parts, payload }, &state).await?;
            f(first, second, third).await.into_handler_result()
        })
    }
}

impl<F, Fut, S> IntoHandler<S, (Event,)> for F
where
    F: Fn(Event) -> Fut + Clone + Send + 'static,
    Fut: Future + Send + 'static,
    Fut::Output: IntoHandlerResult + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_handler(self, _state: S) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(StatelessHandler::new(self))
    }
}

impl<F, Fut, S, T> IntoHandler<RuntimeState<S>, (Event, T)> for F
where
    F: Fn(Event, T) -> Fut + Clone + Send + 'static,
    Fut: Future + Send + 'static,
    Fut::Output: IntoHandlerResult + Send + 'static,
    S: Clone + Send + Sync + 'static,
    T: FromRef<S> + Send + 'static,
{
    fn into_handler(
        self,
        state: RuntimeState<S>,
    ) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(StatefulHandler::new(
            move |event, state: RuntimeState<S>| {
                let extracted = T::from_ref(&state.app_state);
                self(event, extracted)
            },
            state,
        ))
    }
}

impl<F, Fut, S, A> IntoHandler<S, (ExtractorArg<A>,)> for F
where
    F: Fn(A) -> Fut + Clone + Send + 'static,
    Fut: Future + Send + 'static,
    Fut::Output: IntoHandlerResult + Send + 'static,
    A: FromEventParts<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_handler(self, state: S) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(FromPartsHandler::new(self, state))
    }
}

impl<F, Fut, S, A> IntoHandler<S, (BodyOnlyArg<A>,)> for F
where
    F: Fn(A) -> Fut + Clone + Send + 'static,
    Fut: Future + Send + 'static,
    Fut::Output: IntoHandlerResult + Send + 'static,
    A: FromEvent<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_handler(self, state: S) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(FromEventHandler::new(self, state))
    }
}

impl<F, Fut, S, A, B> IntoHandler<S, (PartsArg<A>, BodyArg<B>)> for F
where
    F: Fn(A, B) -> Fut + Clone + Send + 'static,
    Fut: Future + Send + 'static,
    Fut::Output: IntoHandlerResult + Send + 'static,
    A: FromEventParts<S> + Send + 'static,
    B: FromEvent<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_handler(self, state: S) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(PartsBodyHandler::new(self, state))
    }
}

impl<F, Fut, S, A, B> IntoHandler<S, (TwoPartsArg<A, B>,)> for F
where
    F: Fn(A, B) -> Fut + Clone + Send + 'static,
    Fut: Future + Send + 'static,
    Fut::Output: IntoHandlerResult + Send + 'static,
    A: FromEventParts<S> + Send + 'static,
    B: FromEventParts<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_handler(self, state: S) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(TwoPartsHandler::new(self, state))
    }
}

impl<F, Fut, S, A, B, C> IntoHandler<S, (TwoPartsBodyArg<A, B, C>,)> for F
where
    F: Fn(A, B, C) -> Fut + Clone + Send + 'static,
    Fut: Future + Send + 'static,
    Fut::Output: IntoHandlerResult + Send + 'static,
    A: FromEventParts<S> + Send + 'static,
    B: FromEventParts<S> + Send + 'static,
    C: FromEvent<S> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_handler(self, state: S) -> BoxService<Event, HandlerOutcome, CrabbyError> {
        BoxService::new(TwoPartsBodyHandler::new(self, state))
    }
}

pub struct ExtractorArg<T>(pub PhantomData<fn() -> T>);

pub struct BodyOnlyArg<T>(pub PhantomData<fn() -> T>);

pub struct PartsArg<T>(pub PhantomData<fn() -> T>);

pub struct BodyArg<T>(pub PhantomData<fn() -> T>);

pub struct TwoPartsArg<A, B>(pub PhantomData<fn() -> (A, B)>);

pub struct TwoPartsBodyArg<A, B, C>(pub PhantomData<fn() -> (A, B, C)>);
