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

    // The router ends with the socket.
    tokio::select! {
        _router = Router::default().event_loop(stream, outbound) => {}
        () = send_all(outbound_rx, sink) => {}
    }
    Ok(())
}

/// Forward messages from the router and the sessions to the UDP socket.
///
/// A message which cannot be sent is lost, like a dropped datagram: LRCP copes with that,
/// and the other sessions go on.
async fn send_all<S>(mut outbound: mpsc::Receiver<(Frame, SocketAddr)>, mut sink: S)
where
    S: Sink<(Frame, SocketAddr), Error = anyhow::Error> + Unpin,
{
    while let Some((frame, addr)) = outbound.recv().await {
        if let Err(error) = sink.send((frame, addr)).await {
            tracing::warn!(?error, %addr, "Failed to send message");
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::BytesMut;
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio_util::codec::Encoder;

    /// Encodes like `UdpFramed`: a frame which cannot be encoded is an error, but the sink stays usable.
    #[derive(Default)]
    struct Datagrams(Vec<(Vec<u8>, SocketAddr)>);

    impl Sink<(Frame, SocketAddr)> for Datagrams {
        type Error = anyhow::Error;

        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<anyhow::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(
            self: Pin<&mut Self>,
            (frame, addr): (Frame, SocketAddr),
        ) -> anyhow::Result<()> {
            let mut datagram = BytesMut::new();
            Lrcp.encode(frame, &mut datagram)?;
            self.get_mut().0.push((datagram.to_vec(), addr));
            Ok(())
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<anyhow::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<anyhow::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn a_frame_which_cannot_be_sent_is_lost() {
        let addr = SocketAddr::from(([127, 0, 0, 1], 1));
        let too_long = Frame::Data {
            session: 1,
            position: 0,
            data: "x".repeat(1000),
        };
        let (outbound, outbound_rx) = mpsc::channel(2);
        outbound.send((too_long, addr)).await.unwrap();
        outbound.send((Frame::Close(1), addr)).await.unwrap();
        drop(outbound);

        let mut datagrams = Datagrams::default();
        send_all(outbound_rx, &mut datagrams).await;

        assert_eq!(datagrams.0, [(b"/close/1/".to_vec(), addr)]);
    }
}
