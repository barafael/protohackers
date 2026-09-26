use bytes::BytesMut;
use futures_util::{Sink, SinkExt, StreamExt};
use lrcp_codec::Frame;
use std::time::Duration;
use tokio::{
    io::AsyncRead,
    sync::mpsc,
    time::{sleep, Instant},
};
use tokio_util::codec::{Decoder, FramedRead};

/// How long to wait for an acknowledgement before sending data again.
const RETRANSMISSION_TIMEOUT: Duration = Duration::from_secs(3);

/// How long to wait for the peer before giving up on the session.
const SESSION_EXPIRY_TIMEOUT: Duration = Duration::from_secs(60);

/// How much output may await acknowledgement before the writer stops taking more from the application.
const WINDOW: usize = 10_000;

/// The sending half of a session: sends the application's output to the peer, until acknowledged.
///
/// It *is* its data: the session id, how much of the output the peer acknowledged, and the output
/// the peer has yet to acknowledge. All of that is in flight: the writer sends output as soon as the
/// application produces it, without waiting for earlier output to be acknowledged.
#[derive(Debug, PartialEq, Eq)]
pub struct Writer {
    id: u32,
    /// The largest length the peer acknowledged.
    length: u32,
    /// The output after the first `length` bytes.
    unacknowledged: String,
}

/// What an acknowledgement means to the [`Writer`].
#[derive(Debug, PartialEq, Eq)]
enum Ack {
    /// Nothing new: a duplicate, or delayed.
    Stale,
    /// Some of the output: the peer is missing the rest.
    Partial,
    /// All of the output.
    Complete,
    /// More than was ever sent: the peer is misbehaving.
    Invalid,
}

impl Writer {
    pub fn with_id(id: u32) -> Self {
        Self {
            id,
            length: 0,
            unacknowledged: String::new(),
        }
    }

    /// Runs until the application closes, the session times out, the peer closes, or the inbox closes.
    /// On the first two, the writer closes the session: it tells the peer and the session's reader.
    pub async fn event_loop<A, S>(
        mut self,
        mut application: A,
        mut peer: S,
        mut inbox: mpsc::Receiver<Frame>,
        reader: mpsc::Sender<Frame>,
    ) -> Self
    where
        A: AsyncRead + Unpin,
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        if let Err(error) = self
            .run(&mut application, &mut peer, &mut inbox, &reader)
            .await
        {
            tracing::warn!(?error, "Writer for session {} failed", self.id);
            // Without a writer, the session is over.
            tell(&reader, self.id).await;
        }
        tracing::info!("Exiting writer for session {}", self.id);
        self
    }

    async fn run<A, S>(
        &mut self,
        application: &mut A,
        peer: &mut S,
        inbox: &mut mpsc::Receiver<Frame>,
        reader: &mpsc::Sender<Frame>,
    ) -> anyhow::Result<()>
    where
        A: AsyncRead + Unpin,
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        let retransmission = sleep(RETRANSMISSION_TIMEOUT);
        let expiry = sleep(SESSION_EXPIRY_TIMEOUT);
        tokio::pin!(retransmission, expiry);

        let mut application = FramedRead::new(application, Text);
        // Whether the application may produce more output.
        let mut open = true;
        loop {
            tokio::select! {
                // When the peer closes, the application closes as a consequence: that is no reason to close.
                biased;

                frame = inbox.recv() => {
                    let Some(frame) = frame else {
                        return Ok(());
                    };
                    tracing::debug!(?frame);
                    expiry.as_mut().reset(Instant::now() + SESSION_EXPIRY_TIMEOUT);
                    match frame {
                        Frame::Ack { length, .. } => match self.on_ack(length) {
                            Ack::Stale => {}
                            Ack::Partial => {
                                self.send_unacknowledged(peer).await?;
                                retransmission.as_mut().reset(Instant::now() + RETRANSMISSION_TIMEOUT);
                            }
                            Ack::Complete if !open => {
                                tracing::info!("Application closed, ending session {}", self.id);
                                return self.close(peer, reader).await;
                            }
                            Ack::Complete => {}
                            Ack::Invalid => {
                                tracing::warn!("Peer acknowledged {length} bytes, ending session {}", self.id);
                                return self.close(peer, reader).await;
                            }
                        },
                        Frame::Close(_) => return Ok(()),
                        Frame::Connect(_) | Frame::Data { .. } => {}
                    }
                }
                output = application.next(), if open && self.unacknowledged.len() < WINDOW => {
                    if let Some(output) = output {
                        if self.unacknowledged.is_empty() {
                            // Now waiting for the peer.
                            retransmission.as_mut().reset(Instant::now() + RETRANSMISSION_TIMEOUT);
                            expiry.as_mut().reset(Instant::now() + SESSION_EXPIRY_TIMEOUT);
                        }
                        self.send(peer, &output?).await?;
                    } else {
                        // Close once the peer has everything.
                        open = false;
                        if self.unacknowledged.is_empty() {
                            tracing::info!("Application closed, ending session {}", self.id);
                            return self.close(peer, reader).await;
                        }
                    }
                }
                () = &mut retransmission, if !self.unacknowledged.is_empty() => {
                    tracing::info!("Retransmitting for session {}", self.id);
                    self.send_unacknowledged(peer).await?;
                    retransmission.as_mut().reset(Instant::now() + RETRANSMISSION_TIMEOUT);
                }
                // Whether the peer owes an acknowledgement or not, silence means it is gone.
                () = &mut expiry => {
                    tracing::info!("No traffic, ending session {}", self.id);
                    return self.close(peer, reader).await;
                }
            }
        }
    }

    /// Take note that the peer acknowledged the first `length` bytes of the output.
    fn on_ack(&mut self, length: u32) -> Ack {
        if length <= self.length {
            return Ack::Stale;
        }
        if length > self.length + self.unacknowledged.len() as u32 {
            return Ack::Invalid;
        }
        // Lengths count bytes: never cut a character in half.
        let acknowledged = self
            .unacknowledged
            .floor_char_boundary((length - self.length) as usize);
        self.unacknowledged.drain(..acknowledged);
        self.length += acknowledged as u32;
        if self.unacknowledged.is_empty() {
            Ack::Complete
        } else {
            Ack::Partial
        }
    }

    /// Send more output.
    async fn send<S>(&mut self, peer: &mut S, output: &str) -> anyhow::Result<()>
    where
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        let position = self.length + self.unacknowledged.len() as u32;
        self.unacknowledged.push_str(output);
        send_all(peer, Frame::data(self.id, position, output)).await
    }

    /// Send all the output the peer has yet to acknowledge, again.
    async fn send_unacknowledged<S>(&self, peer: &mut S) -> anyhow::Result<()>
    where
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        send_all(
            peer,
            Frame::data(self.id, self.length, &self.unacknowledged),
        )
        .await
    }

    /// End the session, telling the session's reader and the peer.
    async fn close<S>(&self, peer: &mut S, reader: &mpsc::Sender<Frame>) -> anyhow::Result<()>
    where
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        tell(reader, self.id).await;
        peer.send(Frame::Close(self.id)).await.map_err(Into::into)
    }
}

async fn send_all<S>(peer: &mut S, frames: Vec<Frame>) -> anyhow::Result<()>
where
    S: Sink<Frame> + Unpin,
    S::Error: Into<anyhow::Error>,
{
    for frame in frames {
        peer.send(frame).await.map_err(Into::into)?;
    }
    Ok(())
}

/// Decodes the application's output as text, holding back a character until it is complete.
struct Text;

impl Decoder for Text {
    type Item = String;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> anyhow::Result<Option<String>> {
        let complete = match std::str::from_utf8(src) {
            Ok(text) => text.len(),
            // Incomplete, rather than invalid.
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(error) => return Err(error.into()),
        };
        if complete == 0 {
            return Ok(None);
        }
        Ok(Some(String::from_utf8(src.split_to(complete).to_vec())?))
    }
}

/// Tell the session's reader to close.
async fn tell(reader: &mpsc::Sender<Frame>, id: u32) {
    if reader.send(Frame::Close(id)).await.is_err() {
        tracing::info!("Reader for session {id} is already gone");
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures_util::StreamExt;
    use lrcp_codec::Lrcp;
    use tokio::{io::AsyncWriteExt, time::timeout};
    use tokio_util::codec::Encoder;

    fn data(position: u32, data: &str) -> Frame {
        Frame::Data {
            session: 1,
            position,
            data: data.to_string(),
        }
    }

    fn ack(length: u32) -> Frame {
        Frame::Ack { session: 1, length }
    }

    #[tokio::test]
    async fn sends_until_acknowledged_then_closes_with_application() {
        let application: &[u8] = b"olleh\n";
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, mut reader_rx) = mpsc::channel(8);

        let peer = async move {
            let frame = peer.next().await;
            inbox.send(ack(6)).await.unwrap();
            // Keep the inbox open: if it closed, too, the writer might end for that reason instead.
            (frame, inbox, peer)
        };
        let (writer, (frame, _inbox, peer)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        assert_eq!(frame, Some(data(0, "olleh\n")));
        assert_eq!(peer.collect::<Vec<_>>().await, [Frame::Close(1)]);
        assert_eq!(reader_rx.recv().await.unwrap(), Frame::Close(1));
        assert_eq!(
            writer,
            Writer {
                length: 6,
                ..Writer::with_id(1)
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn counts_unescaped_bytes() {
        let application: &[u8] = b"a/b\\c\n";
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, _reader_rx) = mpsc::channel(8);

        let peer = async move {
            let frame = peer.next().await;
            inbox.send(ack(6)).await.unwrap();
            (frame, inbox)
        };
        let (writer, (frame, _inbox)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        // Escaping is up to the codec.
        assert_eq!(frame, Some(data(0, "a/b\\c\n")));
        assert_eq!(writer.length, 6);
    }

    #[tokio::test(start_paused = true)]
    async fn every_frame_fits_a_datagram() {
        let output = format!("{}\n", "/".repeat(1000));
        let application = output.as_bytes();
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, _reader_rx) = mpsc::channel(8);

        let peer = async move {
            let mut received = String::new();
            while received.len() < 1001 {
                let frame: Frame = peer.next().await.unwrap();
                let mut datagram = BytesMut::new();
                Lrcp.encode(frame.clone(), &mut datagram).unwrap();
                assert!(datagram.len() < 1000);
                if let Frame::Data { position, data, .. } = frame {
                    if position as usize == received.len() {
                        received.push_str(&data);
                    }
                }
                inbox.send(ack(received.len() as u32)).await.unwrap();
            }
            (received, inbox, peer)
        };
        let (writer, (received, _inbox, _peer)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        assert_eq!(received.as_bytes(), application);
        assert_eq!(writer.length, 1001);
    }

    #[tokio::test(start_paused = true)]
    async fn sends_output_without_waiting_for_acknowledgements() {
        let output = format!("{}\n", "x".repeat(2999));
        let application = output.as_bytes();
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, _reader_rx) = mpsc::channel(8);

        let peer = async move {
            // No acknowledgement yet, and no time for a retransmission.
            let received = timeout(Duration::from_secs(1), async {
                let mut received = String::new();
                while received.len() < 3000 {
                    if let Some(Frame::Data { position, data, .. }) = peer.next().await {
                        if position as usize == received.len() {
                            received.push_str(&data);
                        }
                    }
                }
                received
            })
            .await;
            inbox.send(ack(3000)).await.unwrap();
            (received, inbox)
        };
        let (writer, (received, _inbox)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        assert_eq!(received.unwrap(), output);
        assert_eq!(writer.length, 3000);
    }

    #[tokio::test(start_paused = true)]
    async fn retransmits_what_was_not_acknowledged() {
        let application = "x".repeat(1500);
        let application = application.as_bytes();
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, _reader_rx) = mpsc::channel(8);

        let peer = async move {
            let start = Instant::now();
            let mut first = None;
            let mut end = 0;
            while end < 1500 {
                let frame = peer.next().await.unwrap();
                let Frame::Data { position, data, .. } = frame else {
                    panic!("{frame:?} is no data frame");
                };
                end = position + data.len() as u32;
                first.get_or_insert(end);
            }
            // As if all but the first frame got lost.
            let first = first.unwrap();
            inbox.send(ack(first)).await.unwrap();
            let again = peer.next().await.unwrap();
            let elapsed = start.elapsed();
            // Old news is no reason to send anything.
            inbox.send(ack(first)).await.unwrap();
            inbox.send(ack(1)).await.unwrap();
            let quiet = timeout(Duration::from_secs(1), peer.next()).await.is_err();
            inbox.send(ack(1500)).await.unwrap();
            (first, again, elapsed, quiet, inbox, peer)
        };
        let (writer, (first, again, elapsed, quiet, _inbox, _peer)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        let rest = "x".repeat(1500 - first as usize);
        assert_eq!(again, data(first, &rest));
        assert_eq!(elapsed, Duration::ZERO);
        assert!(quiet);
        assert_eq!(writer.length, 1500);
    }

    #[tokio::test(start_paused = true)]
    async fn retransmits_until_acknowledged() {
        let application: &[u8] = b"olleh\n";
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, _reader_rx) = mpsc::channel(8);

        let peer = async move {
            let first = peer.next().await;
            let start = Instant::now();
            let second = peer.next().await;
            let elapsed = start.elapsed();
            inbox.send(ack(6)).await.unwrap();
            (first, second, elapsed)
        };
        let (writer, (first, second, elapsed)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        assert_eq!(first, Some(data(0, "olleh\n")));
        assert_eq!(second, first);
        assert_eq!(elapsed, Duration::from_secs(3));
        assert_eq!(
            writer,
            Writer {
                length: 6,
                ..Writer::with_id(1)
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_without_acknowledgement() {
        let application: &[u8] = b"olleh\n";
        let mut sent = Vec::new();
        // The inbox stays open, but no acknowledgement ever arrives.
        let (_inbox, rx) = mpsc::channel(8);
        let (reader, mut reader_rx) = mpsc::channel(8);
        let start = Instant::now();

        let writer = Writer::with_id(1)
            .event_loop(application, &mut sent, rx, reader)
            .await;

        assert_eq!(start.elapsed(), Duration::from_secs(60));
        assert_eq!(reader_rx.recv().await.unwrap(), Frame::Close(1));
        // The peer is told, too.
        assert_eq!(sent.pop(), Some(Frame::Close(1)));
        // Sent every 3 seconds; at 60 seconds, retransmission and timeout coincide.
        assert!((20..=21).contains(&sent.len()));
        assert!(sent.iter().all(|frame| *frame == data(0, "olleh\n")));
        assert_eq!(writer.unacknowledged, "olleh\n");
    }

    #[tokio::test(start_paused = true)]
    async fn closes_when_more_is_acknowledged_than_was_sent() {
        let application: &[u8] = b"olleh\n";
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, mut reader_rx) = mpsc::channel(8);
        let start = Instant::now();

        let peer = async move {
            let frame = peer.next().await;
            inbox.send(ack(7)).await.unwrap();
            (frame, inbox, peer)
        };
        let (writer, (frame, _inbox, peer)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        assert_eq!(frame, Some(data(0, "olleh\n")));
        assert_eq!(peer.collect::<Vec<_>>().await, [Frame::Close(1)]);
        assert_eq!(reader_rx.recv().await.unwrap(), Frame::Close(1));
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert_eq!(writer.length, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn expires_when_idle() {
        // The application stays open, but has nothing to say.
        let (application, _output) = tokio::io::duplex(64);
        let mut sent = Vec::new();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, mut reader_rx) = mpsc::channel(8);
        let start = Instant::now();

        let peer = async move {
            // Something from the peer, then silence.
            sleep(Duration::from_secs(30)).await;
            inbox.send(Frame::Connect(1)).await.unwrap();
            inbox
        };
        let session = Writer::with_id(1).event_loop(application, &mut sent, rx, reader);
        let (writer, _inbox) = tokio::join!(timeout(Duration::from_secs(120), session), peer);

        assert_eq!(writer.expect("Still waiting"), Writer::with_id(1));
        assert_eq!(start.elapsed(), Duration::from_secs(90));
        assert_eq!(sent, [Frame::Close(1)]);
        assert_eq!(reader_rx.recv().await.unwrap(), Frame::Close(1));
    }

    #[tokio::test(start_paused = true)]
    async fn keeps_characters_intact() {
        let (application, mut output) = tokio::io::duplex(64);
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, _reader_rx) = mpsc::channel(8);

        let peer = async move {
            // A character in two parts.
            output.write_all(&[0xc3]).await.unwrap();
            sleep(Duration::from_millis(1)).await;
            output.write_all(&[0xa9, b'\n']).await.unwrap();
            drop(output);
            let frame = peer.next().await;
            inbox.send(ack(3)).await.unwrap();
            (frame, inbox, peer)
        };
        let (writer, (frame, _inbox, peer)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        assert_eq!(frame, Some(data(0, "é\n")));
        assert_eq!(peer.collect::<Vec<_>>().await, [Frame::Close(1)]);
        assert_eq!(writer.length, 3);
    }

    #[test]
    fn acknowledgements() {
        let mut writer = Writer {
            id: 1,
            length: 3,
            unacknowledged: "lo\nwo".to_string(),
        };
        assert_eq!(writer.on_ack(3), Ack::Stale);
        assert_eq!(writer.on_ack(2), Ack::Stale);
        assert_eq!(writer.on_ack(9), Ack::Invalid);
        assert_eq!(writer.unacknowledged, "lo\nwo");
        assert_eq!(writer.on_ack(5), Ack::Partial);
        assert_eq!(writer.unacknowledged, "\nwo");
        assert_eq!(writer.on_ack(4), Ack::Stale);
        assert_eq!(writer.on_ack(8), Ack::Complete);
        assert_eq!(
            writer,
            Writer {
                length: 8,
                ..Writer::with_id(1)
            }
        );
    }

    #[test]
    fn acknowledgements_do_not_split_characters() {
        let mut writer = Writer {
            unacknowledged: "é".to_string(),
            ..Writer::with_id(1)
        };
        assert_eq!(writer.on_ack(1), Ack::Partial);
        assert_eq!(writer.unacknowledged, "é");
    }
}
