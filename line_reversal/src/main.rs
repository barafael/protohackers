use futures_util::{SinkExt, StreamExt};
use lrcp_codec::Frame;
use lrcp_codec::Lrcp;
use session::Session;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio_util::udp::UdpFramed;

mod reverse;
mod session;

#[derive(Debug, Default)]
pub struct Sessions(HashMap<(u32, SocketAddr), Session>);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut sessions = Sessions::default();

    let socket = UdpSocket::bind("0.0.0.0:8000".parse::<SocketAddr>().unwrap()).await?;
    let framed = UdpFramed::new(socket, Lrcp::default());
    let (mut sink, mut stream) = framed.split();
    while let Some(msg) = stream.next().await {
        dbg!(&msg);
        match msg {
            Ok((frame, addr)) => match frame {
                Frame::Connect { session } => {
                    let key = (session, addr);
                    // Does the session already exist?
                    if let Some(value) = sessions.0.get(&key) {
                        println!("Sending repeated ACK for existing session (id: {session})");
                        sink.send((
                            Frame::Ack {
                                session: value.id,
                                length: value.length,
                            },
                            addr,
                        ))
                        .await?;
                    } else {
                        println!("Opening new session with id {session}");
                        let ses = Session::with_id(session);
                        sessions.0.insert(key, ses);
                        // send ACK
                        sink.send((Frame::Ack { session, length: 0 }, addr)).await?;
                    }
                }
                Frame::Data {
                    session,
                    position,
                    data,
                } => {
                    let key = (session, addr);
                    if !sessions.0.contains_key(&key) {
                        sink.send((Frame::Close(session), addr)).await?;
                    } else {
                        let value = sessions.0.get_mut(&key).unwrap();
                    }
                    todo!()
                }
                Frame::Ack { session, length } => {
                    let key = (session, addr);
                    if !sessions.0.contains_key(&key) {
                        sink.send((Frame::Close(session), addr)).await?;
                    }
                }
                Frame::Close(s) => {
                    let key = (s, addr);
                    sink.send((Frame::Close(key.0), addr)).await?;
                    if let Some(s) = sessions.0.remove(&key) {
                        println!("Removing session {s:?}");
                    } else {
                        println!("Session with id {s} does not exist, not closing");
                    }
                }
            },
            Err(e) => {
                eprintln!("{e:?}");
            }
        }
    }
    Ok(())
}
