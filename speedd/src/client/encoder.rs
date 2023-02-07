use super::Message;
use crate::camera::{Camera, PlateRecord};
use bytes::BufMut;
use tokio_util::codec::Encoder;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageEncoder;

impl Encoder<Message> for MessageEncoder {
    type Error = anyhow::Error;

    fn encode(&mut self, item: Message, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        match item {
            Message::Plate(PlateRecord { plate, timestamp }) => {
                dst.put_u8(0x20);
                dst.put_u8(plate.as_bytes().len() as u8);
                dst.put_slice(plate.as_bytes());
                dst.put_u32(timestamp);
                Ok(())
            }
            Message::WantHeartbeat(dur) => {
                dst.put_u8(0x40);
                let deciseconds = dur.as_millis() as u32 / 100;
                dst.put_u32(deciseconds);
                Ok(())
            }
            Message::IAmCamera(Camera { road, mile, limit }) => {
                dst.put_u8(0x80);
                dst.put_u16(road);
                dst.put_u16(mile);
                dst.put_u16(limit);
                Ok(())
            }
            Message::IAmDispatcher(roads) => {
                dst.put_u8(0x81);
                dst.put_u8(roads.len() as u8);
                for road in roads {
                    dst.put_u16(road);
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn encodes_example() {
        let msg = Message::IAmDispatcher(vec![66]);
        let mut encoder = MessageEncoder::default();

        let mut buffer = BytesMut::with_capacity(2);
        encoder.encode(msg, &mut buffer).unwrap();

        let expected = [0x81, 0x01, 0x00, 0x42];
        assert_eq!(buffer, expected[..]);
    }

    // TODO proptest
}
