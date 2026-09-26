use futures_util::{Sink, SinkExt, StreamExt};
use lrcp_codec::Frame;
use lrcp_codec::Lrcp;
use router::Router;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio_util::udp::UdpFramed;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod reader;
mod reverse;
mod router;
mod writer;

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

    let socket = UdpSocket::bind("0.0.0.0:8000".parse::<SocketAddr>().unwrap()).await?;
    let framed = UdpFramed::new(socket, Lrcp);
    let (sink, stream) = framed.split();

    // Collect UDP messages to be sent from the router and all sessions.
    let (outbound, outbound_rx) = mpsc::channel::<(Frame, SocketAddr)>(64);

    // Failing to send ends the server; the router only ends with the socket.
    tokio::select! {
        _router = Router::default().event_loop(stream, outbound) => Ok(()),
        sent = send_all(outbound_rx, sink) => sent,
    }
}

/// Forward messages from the router and the sessions to the UDP socket.
async fn send_all<S>(
    mut outbound: mpsc::Receiver<(Frame, SocketAddr)>,
    mut sink: S,
) -> anyhow::Result<()>
where
    S: Sink<(Frame, SocketAddr), Error = anyhow::Error> + Unpin,
{
    while let Some(msg) = outbound.recv().await {
        sink.send(msg).await?;
    }
    Ok(())
}
