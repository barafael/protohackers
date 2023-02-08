use crate::{heartbeat, server};
use futures::{Sink, SinkExt, Stream, StreamExt};
use speedd_codecs::{camera::Camera, client, plate::PlateRecord};
use tokio::sync::mpsc;

pub struct CameraClient {
    cam: Camera,
}

impl CameraClient {
    pub fn new(cam: Camera) -> Self {
        Self { cam }
    }

    pub async fn run<R, W>(
        self,
        mut reader: R,
        mut writer: W,
        plate_tx: mpsc::Sender<(PlateRecord, Camera)>,
        mut heartbeat_sender: Option<mpsc::Sender<()>>,
        mut heartbeat_receiver: mpsc::Receiver<()>,
    ) -> anyhow::Result<()>
    where
        R: Stream<Item = Result<client::Message, anyhow::Error>> + Send + Unpin,
        W: Sink<server::Message, Error = anyhow::Error> + Send + Unpin,
    {
        println!("Starting Camera Client loop");
        loop {
            tokio::select! {
                Some(Ok(msg)) = reader.next() => {
                    println!("Received camera message {msg:?}");
                    self.handle_client_message(msg, &mut writer, &plate_tx, &mut heartbeat_sender).await?;
                }
                Some(()) = heartbeat_receiver.recv() => {
                    writer.send(server::Message::Heartbeat).await?;
                }
            }
        }
    }

    async fn handle_client_message<W>(
        &self,
        msg: client::Message,
        writer: &mut W,
        plate_tx: &mpsc::Sender<(PlateRecord, Camera)>,
        heartbeat_sender: &mut Option<mpsc::Sender<()>>,
    ) -> anyhow::Result<()>
    where
        W: Sink<server::Message, Error = anyhow::Error> + Send + Unpin,
    {
        match msg {
            client::Message::Plate(record) => {
                plate_tx.send((record, self.cam.clone())).await?;
            }
            client::Message::WantHeartbeat(dur) => {
                if let Some(sender) = heartbeat_sender.take() {
                    println!("Spawning a new heartbeat");
                    tokio::spawn(heartbeat::heartbeat(dur, sender));
                } else {
                    println!("Ignoring repeated heartbeat request");
                    writer
                        .send(server::Message::Error(
                            "You already specified a heartbeat".to_string(),
                        ))
                        .await?;
                }
            }
            client::Message::IAmCamera { .. } => {
                writer
                    .send(server::Message::Error(
                        "Yes, you are (a camera)".to_string(),
                    ))
                    .await?;
            }
            client::Message::IAmDispatcher(_roads) => {
                writer
                    .send(server::Message::Error(
                        "No you're not (a dispatcher)".to_string(),
                    ))
                    .await?;
            }
        }
        Ok(())
    }
}
