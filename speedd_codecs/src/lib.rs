#![feature(iter_array_chunks)]

pub mod camera;
pub mod client;
pub mod plate;
pub mod server;

pub type Timestamp = u32;
pub type Mile = u16;
pub type Road = u16;
pub type Limit = u16;

pub const SECONDS_PER_DAY: u32 = 86400;
//pub const SECONDS_PER_DAY: u32 = 1000;

/// The length prefix of a `str`, which holds at most 255 bytes.
fn str_len(text: &str) -> anyhow::Result<u8> {
    u8::try_from(text.len())
        .map_err(|_| anyhow::anyhow!("{} bytes do not fit in a str", text.len()))
}

#[cfg(test)]
mod test {
    use crate::{camera::Camera, plate::PlateRecord};
    use futures::StreamExt;
    use tokio_test::io::Builder;

    #[tokio::test]
    async fn codec_example() {
        let client_1 = Builder::new()
            .read(&[0x80, 0x00, 0x7b, 0x00, 0x08, 0x00, 0x3c])
            .read(&[0x20, 0x04, 0x55, 0x4e, 0x31, 0x58, 0x00, 0x00, 0x00, 0x00])
            .build();
        let mut client_1 = tokio_util::codec::FramedRead::new(
            client_1,
            crate::client::decoder::MessageDecoder,
        );

        let client_2 = Builder::new()
            .read(&[0x80, 0x00, 0x7b, 0x00, 0x09, 0x00, 0x3c])
            .read(&[0x20, 0x04, 0x55, 0x4e, 0x31, 0x58, 0x00, 0x00, 0x00, 0x2d])
            .build();
        let mut client_2 = tokio_util::codec::FramedRead::new(
            client_2,
            crate::client::decoder::MessageDecoder,
        );

        assert_eq!(
            client_1.next().await.unwrap().unwrap(),
            crate::client::Message::IAmCamera(Camera {
                road: 123,
                mile: 8,
                limit: 60,
            })
        );
        assert_eq!(
            client_1.next().await.unwrap().unwrap(),
            crate::client::Message::Plate(PlateRecord {
                plate: "UN1X".to_string(),
                timestamp: 0,
            })
        );
        assert_eq!(
            client_2.next().await.unwrap().unwrap(),
            crate::client::Message::IAmCamera(Camera {
                road: 123,
                mile: 9,
                limit: 60,
            })
        );
        assert_eq!(
            client_2.next().await.unwrap().unwrap(),
            crate::client::Message::Plate(PlateRecord {
                plate: "UN1X".to_string(),
                timestamp: 45,
            })
        );
    }
}
