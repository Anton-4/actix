//! `ClientSession` is an actor, it manages peer tcp connection and
//! proxies commands from peer to `ChatServer`.
use std::time::{Instant, Duration};
use tokio_core::net::TcpStream;
use actix::prelude::*;

use server::{self, ChatServer};
use codec::{ChatRequest, ChatResponse, ChatCodec};


/// Chat server sends this messages to session
pub struct Message(pub String);

impl ResponseType for Message {
    type Item = ();
    type Error = ();
}

/// `ChatSession` actor is responsible for tcp peer communications.
pub struct ChatSession {
    /// unique session id
    id: usize,
    /// this is address of chat server
    addr: Address<ChatServer>,
    /// Client must send ping at least once per 10 seconds, otherwise we drop connection.
    hb: Instant,
    /// joined room
    room: String,
    /// Framed wrapper
    framed: FramedWriter<TcpStream, ChatCodec>,
}

impl Actor for ChatSession {
    type Context = actix::Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // we'll start heartbeat process on session start.
        self.hb(ctx);

        // register self in chat server. `AsyncContext::wait` register
        // future within context, but context waits until this future resolves
        // before processing any other events.
        self.addr.call(self, server::Connect{addr: ctx.address()}).then(|res, act, ctx| {
            match res {
                Ok(Ok(res)) => act.id = res,
                // something is wrong with chat server
                _ => ctx.stop(),
            }
            actix::fut::ok(())
        }).wait(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> bool {
        // notify chat server
        self.addr.send(server::Disconnect{id: self.id});
        true
    }
}

/// To use `Framed` with an actor, we have to implement `StreamHandler` trait
impl StreamHandler<ChatRequest, FramedError<ChatCodec>> for ChatSession {

    /// This is main event loop for client requests
    fn handle(&mut self, msg: ChatRequest, ctx: &mut Self::Context) {
        match msg {
            ChatRequest::List => {
                // Send ListRooms message to chat server and wait for response
                println!("List rooms");
                self.addr.call(self, server::ListRooms).then(|res, act, _| {
                    match res {
                        Ok(Ok(rooms)) =>
                            act.framed.send(ChatResponse::Rooms(rooms)),
                        _ => println!("Something is wrong"),
                    }
                    actix::fut::ok(())
                }).wait(ctx)
                // .wait(ctx) pauses all events in context,
                // so actor wont receive any new messages until it get list of rooms back
            },
            ChatRequest::Join(name) => {
                println!("Join to room: {}", name);
                self.room = name.clone();
                self.addr.send(server::Join{id: self.id, name: name.clone()});
                self.framed.send(ChatResponse::Joined(name));
            },
            ChatRequest::Message(message) => {
                // send message to chat server
                println!("Peer message: {}", message);
                self.addr.send(
                    server::Message{id: self.id,
                                    msg: message, room:
                                    self.room.clone()})
            }
            // we update heartbeat time on ping from peer
            ChatRequest::Ping =>
                self.hb = Instant::now(),
        }
    }
}

/// Handler for Message, chat server sends this message, we just send string to peer
impl Handler<Message> for ChatSession {
    type Result = ();

    fn handle(&mut self, msg: Message, _: &mut Self::Context) {
        // send message to peer
        self.framed.send(ChatResponse::Message(msg.0));
    }
}

/// Helper methods
impl ChatSession {

    pub fn new(addr: Address<ChatServer>,
               framed: FramedWriter<TcpStream, ChatCodec>) -> ChatSession {
        ChatSession {id: 0,
                     addr: addr,
                     hb: Instant::now(),
                     room: "Main".to_owned(),
                     framed: framed}
    }
    
    /// helper method that sends ping to client every second.
    ///
    /// also this method check heartbeats from client
    fn hb(&self, ctx: &mut actix::Context<Self>) {
        ctx.run_later(Duration::new(1, 0), |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > Duration::new(10, 0) {
                // heartbeat timed out
                println!("Client heartbeat failed, disconnecting!");

                // notify chat server
                act.addr.send(server::Disconnect{id: act.id});

                // stop actor
                ctx.stop();
            }

            act.framed.send(ChatResponse::Ping);
            act.hb(ctx);
        });
    }
}
