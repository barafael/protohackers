use anyhow::Context;
use futures::{stream::StreamExt, Sink, SinkExt, Stream};
use room::Room;
use std::net::SocketAddr;
use tokio::{net::TcpListener, sync::mpsc};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec, LinesCodecError};

mod room;

const WELCOME: &str = "Welcome to budgetchat! What shall I call you?";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8000").await?;
    let (room, messages) = mpsc::channel(256);
    tokio::spawn(Room::default().event_loop(messages));
    loop {
        let (stream, addr) = listener.accept().await?;
        let (reader, writer) = stream.into_split();
        let reader = FramedRead::new(reader, LinesCodec::default());
        let writer = FramedWrite::new(writer, LinesCodec::default());
        tokio::spawn(handle_connection(reader, writer, addr, room.clone()));
    }
}

/// Greet the user and ask for their name, then run their two halves concurrently:
/// the user talks to the room, and the room talks to the user.
async fn handle_connection<R, W>(
    mut reader: R,
    mut writer: W,
    id: SocketAddr,
    room: mpsc::Sender<room::Message>,
) -> anyhow::Result<()>
where
    R: Stream<Item = Result<String, LinesCodecError>> + Unpin,
    W: Sink<String> + Unpin,
    W::Error: Into<anyhow::Error>,
{
    writer.send(WELCOME.to_string()).await.map_err(Into::into)?;
    let name = reader
        .next()
        .await
        .context("Connection closed while awaiting name")?
        .context("Failed to receive name")?;
    anyhow::ensure!(!name.is_empty());
    anyhow::ensure!(name.chars().all(|c| c.is_ascii_alphanumeric()));

    let (outbox, inbox) = mpsc::channel(64);
    room.send(room::Message::Join { id, name, outbox }).await?;

    let (said, heard) = tokio::join!(say(reader, id, room), hear(inbox, writer));
    said.and(heard)
}

/// Pass the user's lines to the room until they hang up, then leave.
async fn say<R>(
    mut reader: R,
    id: SocketAddr,
    room: mpsc::Sender<room::Message>,
) -> anyhow::Result<()>
where
    R: Stream<Item = Result<String, LinesCodecError>> + Unpin,
{
    let result = loop {
        match reader.next().await {
            Some(Ok(text)) => {
                if let Err(error) = room.send(room::Message::Chat { id, text }).await {
                    break Err(error.into());
                }
            }
            Some(Err(error)) => break Err(error.into()),
            None => break Ok(()),
        }
    };
    room.send(room::Message::Leave { id }).await?;
    result
}

/// Pass the room's lines to the user until the room lets go of them.
async fn hear<W>(mut inbox: mpsc::Receiver<String>, mut writer: W) -> anyhow::Result<()>
where
    W: Sink<String> + Unpin,
    W::Error: Into<anyhow::Error>,
{
    while let Some(line) = inbox.recv().await {
        writer.send(line).await.map_err(Into::into)?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::stream;

    fn addr() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 1))
    }

    #[tokio::test]
    async fn rejects_invalid_name() {
        let reader = stream::iter([Ok("not/ok".to_string())]);
        let mut written = Vec::new();
        let (room, mut messages) = mpsc::channel(1);

        let result = handle_connection(reader, &mut written, addr(), room).await;

        assert!(result.is_err());
        assert_eq!(written, [WELCOME]);
        assert!(messages.recv().await.is_none());
    }

    #[tokio::test]
    async fn hang_up_before_naming() {
        let mut written = Vec::new();
        let (room, mut messages) = mpsc::channel(1);

        let result = handle_connection(stream::empty(), &mut written, addr(), room).await;

        assert!(result.is_err());
        assert_eq!(written, [WELCOME]);
        assert!(messages.recv().await.is_none());
    }

    #[tokio::test]
    async fn lonely_user_session() {
        let reader = stream::iter([Ok("alice".to_string()), Ok("anybody here?".to_string())]);
        let mut written = Vec::new();
        let (room, messages) = mpsc::channel(4);

        let (room, result) = tokio::join!(
            Room::default().event_loop(messages),
            handle_connection(reader, &mut written, addr(), room),
        );

        result.unwrap();
        assert_eq!(written, [WELCOME, "* The room contains: "]);
        assert_eq!(room, Room::default());
    }
}
