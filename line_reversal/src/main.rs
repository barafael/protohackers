use crate::reverse::Reverse;
use crate::writer::Writer;
use futures_util::{SinkExt, StreamExt};
use lrcp_codec::Frame;
use lrcp_codec::Lrcp;
use reader::Reader;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::duplex;
use tokio::io::split;
use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio_util::udp::UdpFramed;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod reader;
mod reverse;
mod writer;

/// Maps a session ID to its socket address.
pub type Sessions = HashMap<u32, SocketAddr>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .compact()
        .with_ansi(false)
        .with_max_level(Level::DEBUG)
        .with_file(true)
        .with_line_number(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    tracing_log::LogTracer::init()?;

    let mut sessions = Sessions::default();

    let socket = UdpSocket::bind("0.0.0.0:8000".parse::<SocketAddr>().unwrap()).await?;
    let framed = UdpFramed::new(socket, Lrcp::default());
    let (mut sink, mut stream) = framed.split();

    // Broadcast received UDP messages to all client handlers.
    let (b_tx, _b_rx) = broadcast::channel::<Arc<Frame>>(64);

    // Collect UDP messages to be sent from all client handlers.
    let (m_tx, mut m_rx) = mpsc::channel::<Frame>(64);

    loop {
        tokio::select! {
            // Incoming UDP messages: lookup session and/or forward to `b_tx`.
            Some(msg) = stream.next() => {
                match msg {
                    Ok((Frame::Connect(session), addr)) => {
                        // Does the session already exist?
                        if sessions.contains_key(&session) {
                            b_tx.send(Arc::new(Frame::Connect(session)))?;
                        } else {
                            handle_connect(&mut sessions, session, addr, &m_tx, &b_tx).await?;
                        }
                    }
                    Ok((Frame::Close(session), addr)) => {
                        if sessions.remove(&session).is_some() {
                            tracing::info!("Removing session with id {session}");
                        } else {
                            tracing::info!("Session with id {session} does not exist, ignoring close");
                        }
                        b_tx.send(Arc::new(Frame::Close(session)))?;
                        sink.send((Frame::Close(session), addr)).await?;
                    }
                    Ok((Frame::Data { session, position, data }, addr)) => {
                        if sessions.contains_key(&session) {
                            b_tx.send(Arc::new(Frame::Data { session, position, data }))?;
                        } else {
                            tracing::info!(
                                "Ignoring data for session with id {session} which does not exist"
                            );
                            sink.send((Frame::Close(session), addr)).await?;
                        }
                    }
                    Ok( (Frame::Ack { session, length }, addr)) => {
                        if sessions.contains_key(&session) {
                            b_tx.send(Arc::new(Frame::Ack{ session, length }))?;
                        } else {
                            tracing::info!("Ignoring stray ack for session {session} with length {length} (no such session)");
                            sink.send((Frame::Close(session), addr)).await?;
                        }
                    }
                    Err(e) => {
                        tracing::info!("{e:?}");
                    }
                }
            }
            // Forward messages from client handlers to the UDP socket.
            Some(msg) = m_rx.recv() => {
                let session = msg.session_id();
                if let Some(addr) = sessions.get(&session) {
                    sink.send((msg, *addr)).await?;
                } else {
                    tracing::info!("Could not find session for id {session}");
                    tracing::info!("Dropping {msg:?}")
                }
            }
            else => break
        }
    }
    Ok(())
}

async fn handle_connect(
    sessions: &mut Sessions,
    id: u32,
    addr: SocketAddr,
    m_tx: &mpsc::Sender<Frame>,
    b_tx: &broadcast::Sender<Arc<Frame>>,
) -> anyhow::Result<()> {
    tracing::info!("Opening new session with id {id}");
    sessions.insert(id, addr);

    m_tx.send(Frame::Ack {
        session: id,
        length: 0,
    })
    .await?;

    let reader = Reader::with_id(id);
    let writer = Writer::with_id(id);
    let (transport, application) = duplex(1000);
    let (read, write) = split(transport);
    let rev = Reverse::new().run(application);
    let reader = reader.run(write, m_tx.clone(), b_tx.subscribe());
    let writer = writer.run(read, m_tx.clone(), b_tx.subscribe(), b_tx.clone());
    tokio::spawn(reader);
    tokio::spawn(rev);
    tokio::spawn(writer);
    Ok(())
}
