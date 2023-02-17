use std::cmp::Ordering;

use tokio::io::DuplexStream;

#[derive(Debug)]
pub struct Session {
    pub(crate) id: u32,
    pub(crate) length: u32,
    pub(crate) acked_bytes: u32,
    last_ack: u32,
    channel: DuplexStream,
}

impl Session {
    pub fn with_id(id: u32) -> Self {
        let (keep, give) = tokio::io::duplex(1000);
        let rev = crate::reverse::Reverse::new().run(give);
        tokio::spawn(rev);
        Self {
            id,
            length: 0,
            acked_bytes: 0,
            last_ack: 0,
            channel: keep,
        }
    }

    pub fn handle_ack(&mut self, length: u32) {
        todo!();
        if length < self.last_ack {
            println!("Received duplicate ACK");
            return;
        }
        match length.cmp(&self.length) {
            Ordering::Greater => {
                println!("ACKed data which wasn't sent yet");
            }
            Ordering::Less => {
                println!("ACKed old data, starting retransmission");
                self.last_ack = length;
            }
            Ordering::Equal => {
                println!("ACKed");
            }
        }
    }
}
