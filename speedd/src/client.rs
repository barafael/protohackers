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
            println!("Ignoring {record:?} due to client not having specialized as camera");
            Action::Reply(server::Message::Error("You are no camera".to_string()))
        }
        Message::WantHeartbeat(dur) => {
            if let Some(heartbeat_sender) = heartbeat_sender.take() {
                println!("Spawning a new heartbeat");
                tokio::spawn(heartbeat::heartbeat(dur, heartbeat_sender));
                Action::None
            } else {
                println!("Ignoring repeated heartbeat request");
                Action::Reply(server::Message::Error(
                    "You already specified a heartbeat".to_string(),
                ))
            }
        }
        Message::IAmCamera(c) => Action::SpawnCamera(c),
        Message::IAmDispatcher(roads) => Action::SpawnDispatcher(roads),
    }
}
