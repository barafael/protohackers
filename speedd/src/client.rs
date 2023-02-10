use crate::{heartbeat, server};
use speedd_codecs::{camera::Camera, client::Message};
use tokio::sync::mpsc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Reply(server::Message),
    SpawnCamera(Camera),
    SpawnDispatcher(Vec<u16>),
}

pub fn client_action(msg: Message, heartbeat_sender: &mut Option<mpsc::Sender<()>>) -> Action {
    match msg {
        Message::Plate(record) => {
            tracing::warn!("Ignoring {record:?} due to client not having specialized as camera");
            Action::Reply(server::Message::Error("You are no camera".to_string()))
        }
        Message::WantHeartbeat(dur) => {
            if let Some(heartbeat_sender) = heartbeat_sender.take() {
                if !dur.is_zero() {
                    tracing::info!("Spawning a new heartbeat");
                    tokio::spawn(heartbeat::heartbeat(dur, heartbeat_sender));
                } else {
                    tracing::warn!("Ignoring zero-duration heartbeat");
                }
                Action::None
            } else {
                tracing::warn!("Ignoring repeated heartbeat request");
                Action::Reply(server::Message::Error(
                    "You already specified a heartbeat".to_string(),
                ))
            }
        }
        Message::IAmCamera(c) => Action::SpawnCamera(c),
        Message::IAmDispatcher(roads) => Action::SpawnDispatcher(roads),
    }
}
