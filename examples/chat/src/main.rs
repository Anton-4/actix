#![cfg_attr(feature="cargo-clippy", allow(let_unit_value))]
extern crate rand;
extern crate bytes;
extern crate byteorder;
extern crate futures;
extern crate tokio_io;
extern crate tokio_core;
extern crate serde;
extern crate serde_json;
#[macro_use] extern crate serde_derive;

#[macro_use] extern crate actix;

use std::net;
use std::str::FromStr;
use futures::Stream;
use tokio_io::AsyncRead;
use tokio_core::net::{TcpListener, TcpStream};
use actix::prelude::*;

mod codec;
mod server;
mod session;

use codec::ChatCodec;
use server::ChatServer;
use session::ChatSession;


/// Define tcp server that will accept incoming tcp connection and create
/// chat actors.
struct Server {
    chat: Address<ChatServer>,
}

/// Make actor from `Server`
impl Actor for Server {
    /// Every actor has to provide execution `Context` in which it can run.
    type Context = Context<Self>;
}

#[derive(Message)]
struct TcpConnect(pub TcpStream, pub net::SocketAddr);

/// Handle stream of TcpStream's
impl Handler<TcpConnect> for Server {
    /// this is response for message, which is defined by `ResponseType` trait
    /// in this case we just return unit.
    type Result = ();

    fn handle(&mut self, msg: TcpConnect, _: &mut Context<Self>) {
        // For each incoming connection we create `ChatSession` actor
        // with out chat server address.
        let server = self.chat.clone();
        let _: () = ChatSession::create(
            move |ctx| {
                let (reader, writer) = FramedReader::wrap(msg.0.framed(ChatCodec));
                ChatSession::add_stream(reader, ctx);
                ChatSession::new(server, writer)
            });
    }
}


fn main() {
    let sys = actix::System::new("chat-server");

    // Start chat server actor
    let server: Address<_> = ChatServer::default().start();

    // Create server listener
    let addr = net::SocketAddr::from_str("127.0.0.1:12345").unwrap();
    let listener = TcpListener::bind(&addr, Arbiter::handle()).unwrap();

    // Our chat server `Server` is an actor, first we need to start it
    // and then add stream on incoming tcp connections to it.
    // TcpListener::incoming() returns stream of the (TcpStream, net::SocketAddr) items
    // So to be able to handle this events `Server` actor has to implement
    // stream handler `StreamHandler<(TcpStream, net::SocketAddr), io::Error>`
    let _: () = Server::create(|ctx| {
        ctx.add_message_stream(listener.incoming()
                               .map_err(|_| ()).map(|(st, addr)| TcpConnect(st, addr)));
        Server{chat: server}
    });

    println!("Running chat server on 127.0.0.1:12345");
    sys.run();
}
