use crate::{camera::Camera, plate::PlateRecord};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub mod decoder;
pub mod encoder;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Message {
    Plate(PlateRecord),
    WantHeartbeat(Duration),
    IAmCamera(Camera),
    IAmDispatcher(Vec<u16>),
}
