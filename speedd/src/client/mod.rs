use crate::{
    camera::{Camera, PlateRecord},
    heartbeat, server,
};
use std::time::Duration;
use tokio::sync::mpsc;

pub mod decoder;
pub mod encoder;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Message {
    Plate(PlateRecord),
    WantHeartbeat(Duration),
    IAmCamera(Camera),
    IAmDispatcher(Vec<u16>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
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
                Action::Reply(server::Message::Heartbeat)
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
