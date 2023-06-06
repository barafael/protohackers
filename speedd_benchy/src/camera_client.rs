use std::{net::SocketAddr, time::Duration};

use futures::SinkExt;
use rand::prelude::SliceRandom;
use rand::rngs::ThreadRng;
use rand::Rng;
use serde::{Deserialize, Serialize};
use speedd_codecs::{camera::Camera, client, plate::PlateRecord, server};
use tokio::{
    io::AsyncWriteExt,
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};
use tokio_util::codec::{FramedRead, FramedWrite};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Connect,
    Wait(Duration),
    RequestHeartbeat(Duration),
    Identify(Camera),
    ReportPlate(PlateRecord),
    Disconnect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraClient {
    pub camera: Camera,
    pub actions: Vec<Action>,
    pub reports: Vec<PlateRecord>,
}

impl From<Camera> for CameraClient {
    fn from(camera: Camera) -> Self {
        Self {
            camera,
            actions: Vec::default(),
            reports: Vec::default(),
        }
    }
}

impl CameraClient {
    pub fn with_random_start_delay_then_connect(mut self, rng: &mut ThreadRng) -> Self {
        let initial_wait = Duration::from_secs(rng.gen_range(0..15));
        self.actions.push(Action::Wait(initial_wait));
        self.actions.push(Action::Connect);
        self
    }

    pub fn with_random_delay_then_identify(mut self, rng: &mut ThreadRng) -> Self {
        let initial_wait = Duration::from_secs(rng.gen_range(0..15));
        self.actions.push(Action::Wait(initial_wait));
        self.actions.push(Action::Identify(self.camera.clone()));
        self
    }

    pub fn append_shuffled_reports(&mut self, rng: &mut ThreadRng) {
        self.reports.shuffle(rng);
        self.reports.drain(..).for_each(|r| {
            self.actions.push(Action::ReportPlate(r));
            let wait_duration = Duration::from_millis(rng.gen_range(0..1500));
            self.actions.push(Action::Wait(wait_duration));
        });
    }

    pub fn append_reports(&mut self, rng: &mut ThreadRng) {
        self.reports.drain(..).for_each(|r| {
            self.actions.push(Action::ReportPlate(r));
            let wait_duration = Duration::from_millis(rng.gen_range(0..1500));
            self.actions.push(Action::Wait(wait_duration));
        });
    }

    pub async fn run(&self, addr: SocketAddr) -> anyhow::Result<()> {
        let mut connection: Option<(
            FramedRead<OwnedReadHalf, server::decoder::MessageDecoder>,
            FramedWrite<OwnedWriteHalf, client::encoder::MessageEncoder>,
        )> = None;
        for action in &self.actions {
            match action {
                Action::Connect => {
                    if let Some(ref c) = connection {
                        log::error!("Already connected to {c:?}!");
                    } else {
                        log::info!("Connecting to {addr:?}");
                        let stream = TcpStream::connect(addr).await?;
                        let (reader, writer) = stream.into_split();
                        let reader = FramedRead::new(reader, server::decoder::MessageDecoder);
                        let writer = FramedWrite::new(writer, client::encoder::MessageEncoder);
                        connection = Some((reader, writer));
                    }
                }
                Action::Wait(duration) => tokio::time::sleep(*duration).await,
                Action::RequestHeartbeat(interval) => {
                    if let Some((ref mut _reader, ref mut writer)) = connection {
                        let message = client::Message::WantHeartbeat(*interval);
                        writer.send(message).await?;
                        // TODO: spawn heartbeat monitor
                    } else {
                        log::error!("Requesting heartbeat before establishing connection");
                    }
                }
                Action::Identify(cam) => {
                    if let Some((ref mut _reader, ref mut writer)) = connection {
                        let message = client::Message::IAmCamera(cam.clone());
                        writer.send(message).await?;
                    } else {
                        log::error!("Identifying before establishing connection");
                    }
                }
                Action::ReportPlate(record) => {
                    if let Some((ref mut _reader, ref mut writer)) = connection {
                        let message = client::Message::Plate(record.clone());
                        writer.send(message).await?;
                    } else {
                        log::error!("Sending PlateRecord before establishing connection");
                    }
                }
                Action::Disconnect => {
                    if let Some((reader, writer)) = connection {
                        let reader = reader.into_inner();
                        let writer = writer.into_inner();
                        let mut stream = reader.reunite(writer)?;
                        stream.shutdown().await?;
                        break;
                    }
                    log::error!("Disconnecting before establishing connection");
                }
            }
        }
        Ok(())
    }
}
