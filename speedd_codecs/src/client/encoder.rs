use super::Message;
use crate::{camera::Camera, plate::PlateRecord, str_len};
use bytes::BufMut;
use tokio_util::codec::Encoder;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageEncoder;

impl Encoder<Message> for MessageEncoder {
    type Error = anyhow::Error;

    /// Checks before writing anything, so that a failure leaves no partial frame behind.
    fn encode(&mut self, item: Message, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        match item {
            Message::Plate(PlateRecord { plate, timestamp }) => {
                let len = str_len(&plate)?;
                dst.put_u8(0x20);
                dst.put_u8(len);
                dst.put_slice(plate.as_bytes());
                dst.put_u32(timestamp);
                Ok(())
            }
            Message::WantHeartbeat(dur) => {
                let deciseconds = u32::try_from(dur.as_millis() / 100)?;
                dst.put_u8(0x40);
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
                let numroads = u8::try_from(roads.len())?;
                dst.put_u8(0x81);
                dst.put_u8(numroads);
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
    use std::time::Duration;

    #[test]
    fn encodes_example() {
        let msg = Message::IAmDispatcher(vec![66]);
        let mut encoder = MessageEncoder;

        let mut buffer = BytesMut::with_capacity(2);
        encoder.encode(msg, &mut buffer).unwrap();

        let expected = [0x81, 0x01, 0x00, 0x42];
        assert_eq!(buffer, expected[..]);
    }

    #[test]
    fn rejects_what_does_not_fit() {
        for message in [
            Message::Plate(PlateRecord {
                plate: "X".repeat(256),
                timestamp: 0,
            }),
            Message::IAmDispatcher(vec![1; 256]),
            Message::WantHeartbeat(Duration::from_secs(1 << 32)),
        ] {
            let mut buffer = BytesMut::new();
            assert!(MessageEncoder.encode(message, &mut buffer).is_err());
            assert!(buffer.is_empty());
        }
    }

    // TODO proptest
}
