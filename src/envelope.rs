use std::marker::PhantomData;
use futures::{Async, Poll};
use futures::unsync::oneshot::Sender;
use futures::sync::oneshot::Sender as SyncSender;

use fut::ActorFuture;
use actor::{Actor, AsyncContext};
use handler::{Handler, ResponseType, IntoResponse, MessageResult};
use message::Response;
use context::Context;


/// Converter trait, packs message to suitable envelope
pub trait ToEnvelope<A: Actor>
{
    /// Pack message into suitable envelope
    #[doc(hidden)]
    #[deprecated(since="0.4.6", note="Use pack_msg")]
    fn pack<M>(msg: M, tx: Option<SyncSender<Result<M::Item, M::Error>>>, _: bool) -> Envelope<A>
        where A: Handler<M>,
              M: ResponseType + Send + 'static,
              M::Item: Send, M::Error: Send {
        Self::pack_msg(msg, tx)
    }

    fn pack_msg<M>(msg: M, tx: Option<SyncSender<Result<M::Item, M::Error>>>) -> Envelope<A>
        where A: Handler<M>,
              M: ResponseType + Send + 'static,
              M::Item: Send, M::Error: Send {
        #[allow(deprecated)]
        Self::pack(msg, tx, true)
    }
}

impl<A> ToEnvelope<A> for Context<A>
    where A: Actor<Context=Context<A>>
{
    fn pack<M>(msg: M, tx: Option<SyncSender<Result<M::Item, M::Error>>>,
               cancel_on_drop: bool) -> Envelope<A>
        where A: Handler<M>,
              M: ResponseType + 'static,
              M::Item: Send, M::Error: Send,
    {
        Envelope(Box::new(
            RemoteEnvelope{msg: Some(msg),
                           tx: tx,
                           act: PhantomData,
                           cancel_on_drop: cancel_on_drop}))
    }

    fn pack_msg<M>(msg: M, tx: Option<SyncSender<Result<M::Item, M::Error>>>) -> Envelope<A>
        where A: Handler<M>,
              M: ResponseType + 'static,
              M::Item: Send, M::Error: Send,
    {
        Envelope(Box::new(
            RemoteEnvelope{msg: Some(msg),
                           tx: tx,
                           act: PhantomData,
                           cancel_on_drop: true}))
    }
}

pub struct Envelope<A>(Box<EnvelopeProxy<Actor=A>>);

impl<A> Envelope<A> where A: Actor {

    pub(crate) fn new<T>(envelop: T) -> Self
        where T: EnvelopeProxy<Actor=A> + Sized + 'static
    {
        Envelope(Box::new(envelop))
    }

    pub(crate) fn local<M>(msg: M,
                           tx: Option<Sender<Result<M::Item, M::Error>>>,
                           cancel_on_drop: bool) -> Self
        where M: ResponseType + 'static,
              A: Actor + Handler<M>, A::Context: AsyncContext<A>
    {
        Envelope(Box::new(
            LocalEnvelope{msg: Some(msg),
                          tx: tx,
                          act: PhantomData,
                          cancel_on_drop: cancel_on_drop}))
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

struct LocalEnvelope<A, M> where M: ResponseType {
    msg: Option<M>,
    act: PhantomData<A>,
    tx: Option<Sender<Result<M::Item, M::Error>>>,
    cancel_on_drop: bool,
}

impl<A, M> EnvelopeProxy for LocalEnvelope<A, M>
    where M: ResponseType + 'static,
          A: Actor + Handler<M>, A::Context: AsyncContext<A>
{
    type Actor = A;

    fn handle(&mut self, act: &mut Self::Actor, ctx: &mut <Self::Actor as Actor>::Context)
    {
        let tx = self.tx.take();
        if tx.is_some() && self.cancel_on_drop && tx.as_ref().unwrap().is_canceled() {
            return
        }

        if let Some(msg) = self.msg.take() {
            let fut = <Self::Actor as Handler<M>>::handle(act, msg, ctx);
            let tx = if let Some(tx) = tx {
                Some(EnvelopFutureItem::Local(tx))
            } else {
                None
            };
            let f: EnvelopFuture<Self::Actor, _> = EnvelopFuture {
                msg: PhantomData, fut: fut.into_response(), tx: tx};
            ctx.spawn(f);
        }
    }
}

pub struct RemoteEnvelope<A, M> where M: ResponseType {
    act: PhantomData<A>,
    msg: Option<M>,
    tx: Option<SyncSender<Result<M::Item, M::Error>>>,
    cancel_on_drop: bool,
}

impl<A, M> RemoteEnvelope<A, M> where A: Actor, M: ResponseType {

    pub fn new(msg: M,
               tx: Option<SyncSender<Result<M::Item, M::Error>>>,
               cancel_on_drop: bool) -> RemoteEnvelope<A, M>
        where A: Handler<M>,
              M: Send + 'static, M::Item: Send, M::Item: Send
    {
        RemoteEnvelope{msg: Some(msg),
                       tx: tx,
                       act: PhantomData,
                       cancel_on_drop: cancel_on_drop}
    }

    pub fn envelope(msg: M, tx: Option<SyncSender<MessageResult<M>>>) -> RemoteEnvelope<A, M>
        where A: Handler<M>,
              M: Send + 'static, M::Item: Send, M::Item: Send
    {
        RemoteEnvelope{msg: Some(msg),
                       tx: tx,
                       act: PhantomData,
                       cancel_on_drop: true}
    }
}

impl<A, M> EnvelopeProxy for RemoteEnvelope<A, M>
    where M: ResponseType + 'static,
          A: Actor + Handler<M>, A::Context: AsyncContext<A>
{
    type Actor = A;

    fn handle(&mut self, act: &mut Self::Actor, ctx: &mut <Self::Actor as Actor>::Context) {
        let tx = self.tx.take();
        if tx.is_some() && self.cancel_on_drop && tx.as_ref().unwrap().is_canceled() {
            return
        }

        if let Some(msg) = self.msg.take() {
            let fut = <Self::Actor as Handler<M>>::handle(act, msg, ctx);
            let tx = if let Some(tx) = tx {
                Some(EnvelopFutureItem::Remote(tx))
            } else {
                None
            };
            let f: EnvelopFuture<Self::Actor, _> = EnvelopFuture {
                msg: PhantomData, fut: fut.into_response(), tx: tx};
            ctx.spawn(f);
        }
    }
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

enum EnvelopFutureItem<M> where M: ResponseType {
    Local(Sender<Result<M::Item, M::Error>>),
    Remote(SyncSender<Result<M::Item, M::Error>>),
}

pub(crate) struct EnvelopFuture<A, M> where A: Actor, M: ResponseType {
    msg: PhantomData<M>,
    fut: Response<A, M>,
    tx: Option<EnvelopFutureItem<M>>,
}

impl<A, M> ActorFuture for EnvelopFuture<A, M>
    where A: Actor + Handler<M>,
          M: ResponseType,
{
    type Item = ();
    type Error = ();
    type Actor = A;

    fn poll(&mut self,
            act: &mut A,
            ctx: &mut <Self::Actor as Actor>::Context) -> Poll<Self::Item, Self::Error>
    {
        match self.fut.poll_response(act, ctx) {
            Ok(Async::Ready(val)) => {
                match self.tx.take() {
                    Some(EnvelopFutureItem::Local(tx)) => { let _ = tx.send(Ok(val)); },
                    Some(EnvelopFutureItem::Remote(tx)) => { let _ = tx.send(Ok(val)); },
                    _ => (),
                }
                Ok(Async::Ready(()))
            },
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => {
                match self.tx.take() {
                    Some(EnvelopFutureItem::Local(tx)) => { let _ = tx.send(Err(err)); },
                    Some(EnvelopFutureItem::Remote(tx)) => { let _ = tx.send(Err(err)); },
                    _ => (),
                }
                Err(())
            }
        }
    }
}
