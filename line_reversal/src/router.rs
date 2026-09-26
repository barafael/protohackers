use crate::{reader::Reader, reverse::reverse_lines, writer::Writer};
use futures_util::{future, SinkExt, Stream, StreamExt};
use lrcp_codec::Frame;
use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
};
use tokio::{
    io::{duplex, split},
    select,
    sync::mpsc::{self, error::TrySendError},
    task::JoinSet,
};
use tokio_util::sync::{PollSendError, PollSender};

/// Maps a session ID to its socket address.
pub type Sessions = HashMap<u32, SocketAddr>;

/// The inboxes of one session's reader and writer.
type Inboxes = [mpsc::Sender<Frame>; 2];

/// Routes incoming frames to their sessions, and answers frames for unknown sessions.
///
/// It *is* its data: which session lives at which address.
/// The session inboxes and tasks are runtime resources, owned by the event loop.
///
/// Frames flow one way only: socket → router → session → outbound → socket.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Router {
    sessions: Sessions,
}

impl Router {
    pub async fn event_loop<S>(
        mut self,
        mut frames: S,
        outbound: mpsc::Sender<(Frame, SocketAddr)>,
    ) -> Self
    where
        S: Stream<Item = anyhow::Result<(Frame, SocketAddr)>> + Unpin,
    {
        let mut inboxes = HashMap::<u32, Inboxes>::new();
        let mut tasks = JoinSet::new();
        loop {
            select! {
                frame = frames.next() => {
                    let Some(frame) = frame else {
                        break;
                    };
                    let result = match frame {
                        Ok((frame, addr)) => {
                            self.on_frame(frame, addr, &outbound, &mut inboxes, &mut tasks).await
                        }
                        Err(e) => {
                            tracing::info!("{e:?}");
                            Ok(())
                        }
                    };
                    if let Err(error) = result {
                        tracing::error!(?error, "Router failed");
                        break;
                    }
                }
                Some(result) = tasks.join_next() => finished(result),
            }
        }

        // Natural shutdown: sessions end when their inboxes close.
        drop(inboxes);
        while let Some(result) = tasks.join_next().await {
            finished(result);
        }
        self
    }

    async fn on_frame(
        &mut self,
        frame: Frame,
        addr: SocketAddr,
        outbound: &mpsc::Sender<(Frame, SocketAddr)>,
        inboxes: &mut HashMap<u32, Inboxes>,
        tasks: &mut JoinSet<Session>,
    ) -> anyhow::Result<()> {
        match frame {
            Frame::Connect(session) => {
                // Does the session already exist?
                match self.sessions.entry(session) {
                    Entry::Occupied(_) => forward(inboxes, Frame::Connect(session)),
                    Entry::Vacant(entry) => {
                        tracing::info!("Opening new session with id {session}");
                        entry.insert(addr);
                        let ack = Frame::Ack { session, length: 0 };
                        outbound.send((ack, addr)).await?;
                        inboxes.insert(session, spawn_session(session, addr, outbound, tasks));
                    }
                }
            }
            Frame::Close(session) => {
                if self.sessions.remove(&session).is_some() {
                    tracing::info!("Removing session with id {session}");
                } else {
                    tracing::info!("Session with id {session} does not exist, ignoring close");
                }
                forward(inboxes, Frame::Close(session));
                inboxes.remove(&session);
                outbound.send((Frame::Close(session), addr)).await?;
            }
            Frame::Data {
                session,
                position,
                data,
            } => {
                if self.sessions.contains_key(&session) {
                    forward(
                        inboxes,
                        Frame::Data {
                            session,
                            position,
                            data,
                        },
                    );
                } else {
                    tracing::info!(
                        "Ignoring data for session with id {session} which does not exist"
                    );
                    outbound.send((Frame::Close(session), addr)).await?;
                }
            }
            Frame::Ack { session, length } => {
                if self.sessions.contains_key(&session) {
                    forward(inboxes, Frame::Ack { session, length });
                } else {
                    tracing::info!("Ignoring stray ack for session {session} with length {length} (no such session)");
                    outbound.send((Frame::Close(session), addr)).await?;
                }
            }
        }
        Ok(())
    }
}

/// Hand a frame to its session's reader and writer, without waiting:
/// a session which does not keep up loses the frame, as if the datagram had been lost.
///
/// Both halves get the frame, because the writer restarts its session timeout on any frame from the peer.
/// Each owns its copy.
fn forward(inboxes: &HashMap<u32, Inboxes>, frame: Frame) {
    let session = frame.session_id();
    let Some([reader, writer]) = inboxes.get(&session) else {
        return;
    };
    deliver(reader, frame.clone());
    deliver(writer, frame);
}

fn deliver(inbox: &mpsc::Sender<Frame>, frame: Frame) {
    match inbox.try_send(frame) {
        Ok(()) => {}
        Err(TrySendError::Full(frame)) => {
            tracing::info!("Session is busy, dropping {frame:?}");
        }
        Err(TrySendError::Closed(frame)) => {
            tracing::info!("Session has ended, dropping {frame:?}");
        }
    }
}

/// Spawn a session: reader, application and writer.
fn spawn_session(
    id: u32,
    addr: SocketAddr,
    outbound: &mpsc::Sender<(Frame, SocketAddr)>,
    tasks: &mut JoinSet<Session>,
) -> Inboxes {
    let (reader_tx, reader_rx) = mpsc::channel(64);
    let (writer_tx, writer_rx) = mpsc::channel(64);

    // Frames from the session go to the session's peer.
    let to_peer = || {
        PollSender::new(outbound.clone())
            .with(move |frame: Frame| future::ready(Ok::<_, PollSendError<_>>((frame, addr))))
    };

    let (transport, application) = duplex(1000);
    let (read, write) = split(transport);
    let reader = Reader::with_id(id).event_loop(write, to_peer(), reader_rx);
    let writer = Writer::with_id(id).event_loop(read, to_peer(), writer_rx, reader_tx.clone());
    let application = reverse_lines(application);
    tasks.spawn(async move {
        let (reader, application, writer) = tokio::join!(reader, application, writer);
        Session {
            reader,
            application,
            writer,
        }
    });

    [reader_tx, writer_tx]
}

/// What a session leaves behind.
#[derive(Debug)]
struct Session {
    reader: Reader,
    application: anyhow::Result<()>,
    writer: Writer,
}

fn finished(result: Result<Session, tokio::task::JoinError>) {
    match result {
        Ok(Session {
            reader,
            application: Ok(()),
            writer,
        }) => tracing::info!(?reader, ?writer, "Session finished"),
        Ok(Session {
            reader,
            application: Err(error),
            writer,
        }) => tracing::warn!(?reader, ?writer, ?error, "Application failed"),
        Err(error) => tracing::error!(?error, "Session task failed"),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures_util::stream;

    fn from(port: u16, frame: Frame) -> anyhow::Result<(Frame, SocketAddr)> {
        Ok((frame, addr(port)))
    }

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    async fn drain(mut outbound: mpsc::Receiver<(Frame, SocketAddr)>) -> Vec<(Frame, SocketAddr)> {
        let mut frames = Vec::new();
        while let Some(frame) = outbound.recv().await {
            frames.push(frame);
        }
        frames
    }

    #[tokio::test]
    async fn connects_and_closes_strangers() {
        let frames = stream::iter([
            from(1, Frame::Connect(1)),
            from(
                2,
                Frame::Data {
                    session: 2,
                    position: 0,
                    data: "hello\n".to_string(),
                },
            ),
            from(
                2,
                Frame::Ack {
                    session: 3,
                    length: 0,
                },
            ),
            from(2, Frame::Close(4)),
            Err(anyhow::anyhow!("garbage")),
        ]);
        let (outbound, outbound_rx) = mpsc::channel(8);

        let router = Router::default().event_loop(frames, outbound).await;

        assert_eq!(
            drain(outbound_rx).await,
            [
                (
                    Frame::Ack {
                        session: 1,
                        length: 0
                    },
                    addr(1)
                ),
                (Frame::Close(2), addr(2)),
                (Frame::Close(3), addr(2)),
                (Frame::Close(4), addr(2)),
            ]
        );
        assert_eq!(router.sessions, Sessions::from([(1, addr(1))]));
    }

    #[tokio::test]
    async fn close_ends_session() {
        let frames = stream::iter([from(1, Frame::Connect(1)), from(1, Frame::Close(1))]);
        let (outbound, outbound_rx) = mpsc::channel(8);

        let router = Router::default().event_loop(frames, outbound).await;

        assert_eq!(
            drain(outbound_rx).await,
            [
                (
                    Frame::Ack {
                        session: 1,
                        length: 0
                    },
                    addr(1)
                ),
                (Frame::Close(1), addr(1)),
            ]
        );
        assert_eq!(router, Router::default());
    }

    /// The example session from the problem statement, without a socket.
    #[tokio::test]
    async fn reverses_a_line() {
        let (peer, frames) = futures::channel::mpsc::unbounded();
        let (outbound, mut outbound_rx) = mpsc::channel::<(Frame, SocketAddr)>(8);
        let send = |frame: Frame| peer.unbounded_send(from(1, frame)).unwrap();

        let conversation = async {
            send(Frame::Connect(12345));
            let connected = outbound_rx.recv().await.unwrap().0;

            send(Frame::Data {
                session: 12345,
                position: 0,
                data: "hello\n".to_string(),
            });
            // The acknowledgement and the answer may come in either order.
            let answers = [
                outbound_rx.recv().await.unwrap().0,
                outbound_rx.recv().await.unwrap().0,
            ];

            send(Frame::Ack {
                session: 12345,
                length: 6,
            });
            send(Frame::Close(12345));
            let closed = outbound_rx.recv().await.unwrap().0;
            peer.close_channel();
            (connected, answers, closed)
        };
        let (router, (connected, answers, closed)) =
            tokio::join!(Router::default().event_loop(frames, outbound), conversation,);

        assert_eq!(
            connected,
            Frame::Ack {
                session: 12345,
                length: 0
            }
        );
        assert!(answers.contains(&Frame::Ack {
            session: 12345,
            length: 6
        }));
        assert!(answers.contains(&Frame::Data {
            session: 12345,
            position: 0,
            data: "olleh\n".to_string()
        }));
        assert_eq!(closed, Frame::Close(12345));
        assert_eq!(router, Router::default());
    }
}
