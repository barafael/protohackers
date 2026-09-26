use crate::message::Message;
use db::Store;
use futures::{sink, stream, Sink, Stream};
use std::str::FromStr;
use std::{io, net::SocketAddr, sync::Arc};
use tokio::net::UdpSocket;

mod db;
mod message;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:8000".parse::<SocketAddr>().unwrap()).await?;
    let socket = Arc::new(socket);

    let requests = requests(socket.clone());
    let responses = responses(socket);
    tokio::pin!(requests, responses);

    Store::new().event_loop(requests, responses).await;
    Ok(())
}

/// The datagrams arriving at `socket`, as messages.
///
/// Not a `UdpFramed`: a decoder cannot tell an empty datagram (a query for the empty key) from no datagram.
fn requests(socket: Arc<UdpSocket>) -> impl Stream<Item = (Message, SocketAddr)> {
    stream::unfold(socket, |socket| async move {
        let mut buf = [0; 1024];
        loop {
            let (len, addr) = match socket.recv_from(&mut buf).await {
                Ok(received) => received,
                Err(e) => {
                    println!("Failed to receive: {e:#?}");
                    return None;
                }
            };
            let message = std::str::from_utf8(&buf[..len])
                .map_err(anyhow::Error::from)
                .and_then(Message::from_str);
            match message {
                Ok(message) => return Some(((message, addr), socket)),
                Err(e) => println!("Failed to parse message: {e:#?}"),
            }
        }
    })
}

/// Sends each response as a datagram from `socket`.
fn responses(socket: Arc<UdpSocket>) -> impl Sink<(String, SocketAddr), Error = io::Error> {
    sink::unfold(
        socket,
        |socket, (response, addr): (String, SocketAddr)| async move {
            let len = socket.send_to(response.as_bytes(), addr).await?;
            println!("{len:?} bytes sent");
            Ok(socket)
        },
    )
}
