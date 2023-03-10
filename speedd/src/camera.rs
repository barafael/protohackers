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
        tracing::info!("Starting Camera Client loop");
        loop {
            tokio::select! {
                Some(msg) = reader.next() => {
                    match msg {
                        Ok(msg) => {
                            tracing::trace!("Received camera message {msg:?}");
                            self.handle_client_message(msg, &mut writer, &plate_tx, &mut heartbeat_sender).await?;
                        }
                        Err(e) => writer.send(server::Message::Error(format!("Nahh... you're just a camera. {e:?}"))).await?,
                    }
                }
                Some(()) = heartbeat_receiver.recv() => {
                    writer.send(server::Message::Heartbeat).await?;
                }
                else => break,
            }
        }
        tracing::info!("Leaving Camera Client loop");
        Ok(())
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
                if let Some(heartbeat_sender) = heartbeat_sender.take() {
                    if dur.is_zero() {
                        tracing::warn!("Ignoring zero-duration heartbeat");
                    } else {
                        tracing::info!("Spawning a new heartbeat");
                        tokio::spawn(heartbeat::run(dur, heartbeat_sender));
                    }
                } else {
                    tracing::info!("Ignoring repeated heartbeat request");
                    writer
                        .send(server::Message::Error(
                            "You already specified a heartbeat".to_string(),
                        ))
                        .await?;
                }
            }
            client::Message::IAmCamera { .. } => {
                tracing::warn!("Ignoring repeated IAmCamera");
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
