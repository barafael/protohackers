use anyhow::Context;
use clap::{Parser, Subcommand};
use futures::{SinkExt, StreamExt};
use rustyline::error::ReadlineError;
use std::{net::SocketAddr, time::Duration};
use tokio::net::TcpStream;
use tokio_util::codec::{FramedRead, FramedWrite};

use speedd_codecs::{
    camera::Camera,
    client::{self, encoder::MessageEncoder as Encoder},
    plate::PlateRecord,
    server::decoder::MessageDecoder as Decoder,
};

fn parse_hex_digit(s: &str) -> anyhow::Result<u16> {
    u16::from_str_radix(s, 16).context("Failed to parse hex byte")
}

#[derive(Debug, Parser)]
#[command(author, version)]
pub struct Args {
    /// Address to connect to
    #[arg(short, long, default_value = "127.0.0.1:8000")]
    address: SocketAddr,

    /// Heartbeat interval duration (off by default)
    #[arg(long, short, default_value_t = Duration::ZERO.into())]
    interval: humantime::Duration,

    #[command(subcommand)]
    mode: Mode,
}

#[derive(Debug, Subcommand)]
pub enum Mode {
    Dispatcher {
        #[arg(num_args = 1.., value_delimiter = ' ', value_parser = parse_hex_digit)]
        roads: Vec<u16>,
    },
    Camera {
        /// Road ID
        #[arg(short, long)]
        road: u16,

        /// Camera position
        #[arg(short, long)]
        mile: u16,

        /// Speed limits in `mp/h x 100` (yes, I know, but that's the problem statetment)
        #[arg(short, long)]
        limit: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let client = TcpStream::connect(args.address).await?;
    let (reader, writer) = client.into_split();

    let mut reader = FramedRead::new(reader, Decoder::default());
    let mut writer = FramedWrite::new(writer, Encoder::default());

    if !args.interval.is_zero() {
        writer
            .send(client::Message::WantHeartbeat(args.interval.into()))
            .await?;
    }

    match args.mode {
        Mode::Dispatcher { roads } => {
            println!("Registering as dispatcher");
            writer.send(client::Message::IAmDispatcher(roads)).await?;

            println!("Start listening loop");
            loop {
                match reader.next().await {
                    Some(Ok(next)) => {
                        println!("{next:?}");
                    }
                    Some(Err(e)) => println!("{e:?}"),
                    None => {
                        println!("Leaving listening loop");
                        break;
                    }
                }
            }
            println!("Finished listening loop");
        }
        Mode::Camera { road, mile, limit } => {
            writer
                .send(client::Message::IAmCamera(Camera { road, mile, limit }))
                .await?;

            let mut rl = rustyline::Editor::<()>::new()?;
            loop {
                let readline = rl.readline(">> ");
                match readline {
                    Ok(line) => {
                        let mut tokens = line.split_whitespace();
                        match (tokens.next(), tokens.next()) {
                            (Some(plate), Some(timestamp)) => {
                                if let Ok(timestamp) = timestamp.parse() {
                                    let message = client::Message::Plate(PlateRecord {
                                        plate: plate.to_string(),
                                        timestamp,
                                    });
                                    writer.send(message).await.unwrap();
                                } else {
                                    println!("Invalid timestamp");
                                }
                            }
                            x => println!("Invalid plate and timestamp: {x:?}"),
                        };
                    }
                    Err(ReadlineError::Interrupted) => {
                        println!("CTRL+C");
                        break;
                    }
                    Err(ReadlineError::Eof) => {
                        println!("CTRL+D");
                        break;
                    }
                    Err(e) => {
                        anyhow::bail!("{e:?}");
                    }
                }
            }
        }
    }
    drop(reader);
    drop(writer);
    Ok(())
}
