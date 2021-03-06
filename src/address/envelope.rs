use std::marker::PhantomData;
use futures::sync::oneshot::Sender;

use actor::{Actor, AsyncContext};
use context::Context;
use handler::{Handler, ResponseType, MessageResult, MessageResponse};

/// Converter trait, packs message to suitable envelope
pub trait ToEnvelope<A: Actor> {

    /// Pack message into suitable envelope
    fn pack<M>(msg: M, tx: Option<Sender<MessageResult<M>>>) -> Envelope<A>
        where A: Handler<M>,
              M: ResponseType + Send + 'static,
              M::Item: Send, M::Error: Send;
}

impl<A> ToEnvelope<A> for Context<A> where A: Actor<Context=Context<A>>
{
    fn pack<M>(msg: M, tx: Option<Sender<MessageResult<M>>>) -> Envelope<A>
        where A: Handler<M>,
              M: ResponseType + 'static, M::Item: Send, M::Error: Send,
    {
        Envelope(Box::new(
            RemoteEnvelope{msg: Some(msg),
                           tx: tx,
                           act: PhantomData}))
    }
}

pub struct Envelope<A>(Box<EnvelopeProxy<Actor=A>>);

impl<A> Envelope<A> where A: Actor {

    /// Create envelope object
    pub(crate) fn new<T>(envelop: T) -> Self
        where T: EnvelopeProxy<Actor=A> + Sized + 'static
    {
        Envelope(Box::new(envelop))
    }

    pub(crate) fn handle(&mut self, act: &mut A, ctx: &mut A::Context) {
        self.0.handle(act, ctx)
    }
}

// This is not safe! Local envelope could be send to different thread!
unsafe impl<T> Send for Envelope<T> {}

pub trait EnvelopeProxy {

    type Actor: Actor;

    /// handle message within new actor and context
    fn handle(&mut self, act: &mut Self::Actor, ctx: &mut <Self::Actor as Actor>::Context);
}

pub struct RemoteEnvelope<A, M> where M: ResponseType {
    act: PhantomData<A>,
    msg: Option<M>,
    tx: Option<Sender<MessageResult<M>>>,
}

impl<A, M> From<RemoteEnvelope<A, M>> for Envelope<A>
    where A: Actor + Handler<M>,
          A::Context: AsyncContext<A>,
          M: ResponseType + Send + 'static,
{
    fn from(env: RemoteEnvelope<A, M>) -> Self {
        Envelope::new(env)
    }
}

impl<A, M> RemoteEnvelope<A, M> where A: Actor, M: ResponseType {

    pub fn envelope(msg: M, tx: Option<Sender<MessageResult<M>>>) -> RemoteEnvelope<A, M>
        where A: Handler<M>,
              M: Send + 'static, M::Item: Send, M::Item: Send
    {
        RemoteEnvelope{msg: Some(msg),
                       tx: tx,
                       act: PhantomData}
    }
}

impl<A, M> EnvelopeProxy for RemoteEnvelope<A, M>
    where M: ResponseType + 'static,
          A: Actor + Handler<M>, A::Context: AsyncContext<A>
{
    type Actor = A;

    fn handle(&mut self, act: &mut Self::Actor, ctx: &mut <Self::Actor as Actor>::Context) {
        let tx = self.tx.take();
        if tx.is_some() && tx.as_ref().unwrap().is_canceled() {
            return
        }

        if let Some(msg) = self.msg.take() {
            let fut = <Self::Actor as Handler<M>>::handle(act, msg, ctx);
            fut.handle(ctx, tx)
        }
    }
}
