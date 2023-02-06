use bytes::{Buf, BufMut};
use tokio_util::codec::Encoder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Error(String),
    Ticket(TicketRecord),
    Heartbeat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketRecord {
    plate: String,
    road: u16,
    mile1: u16,
    timestamp1: u32,
    mile2: u16,
    timestamp2: u32,
    speed: u16,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageEncoder;

impl Encoder<Message> for MessageEncoder {
    type Error = anyhow::Error;

    fn encode(&mut self, item: Message, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        match item {
            Message::Error(msg) => {
                let bytes = msg.as_bytes();
                dst.put_u8(0x10);
                dst.put_u8(bytes.len() as u8);
                dst.put_slice(&bytes);
            }
            Message::Ticket(TicketRecord {
                plate,
                road,
                mile1,
                timestamp1,
                mile2,
                timestamp2,
                speed,
            }) => {
                dst.put_u8(0x21);
                dst.put_u8(plate.as_bytes().len() as u8);
                dst.put_slice(plate.as_bytes());
                dst.put_u16(road);
                dst.put_u16(mile1);
                dst.put_u32(timestamp1);
                dst.put_u16(mile2);
                dst.put_u32(timestamp2);
                dst.put_u16(speed);
            }
            Message::Heartbeat => {
                dst.put_u8(0x41);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn example() {
        let ticket = Message::Ticket(TicketRecord {
            plate: "RE05BKG".to_string(),
            road: 368,
            mile1: 1234,
            timestamp1: 1000000,
            mile2: 1235,
            timestamp2: 1000060,
            speed: 6000,
        });
        let mut buffer = BytesMut::with_capacity(5);
        let mut encoder = MessageEncoder::default();
        encoder.encode(ticket, &mut buffer).unwrap();
        let expected = [
            0x21, 0x07, 0x52, 0x45, 0x30, 0x35, 0x42, 0x4b, 0x47, 0x01, 0x70, 0x04, 0xd2, 0x00,
            0x0f, 0x42, 0x40, 0x04, 0xd3, 0x00, 0x0f, 0x42, 0x7c, 0x17, 0x70,
        ];
        assert_eq!(&expected, &buffer.freeze()[..]);
    }
}
