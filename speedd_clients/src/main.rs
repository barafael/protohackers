use arguments::{Arguments, Mode};
use clap::Parser;
use futures::{SinkExt, StreamExt};
use rustyline::error::ReadlineError;
use speedd_codecs::{
    camera::Camera,
    client::{self, encoder::MessageEncoder as Encoder},
    plate::PlateRecord,
    server::decoder::MessageDecoder as Decoder,
};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_util::codec::{FramedRead, FramedWrite};

mod arguments;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();

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
        Mode::Client => {
            tokio::task::spawn(async move {
                while let Some(Ok(msg)) = reader.next().await {
                    println!("{msg:?}");
                }
            });
            let wanthb = client::Message::WantHeartbeat(Duration::from_secs(1));
            let iam = client::Message::IAmCamera(Camera {
                limit: 1,
                mile: 2,
                road: 3,
            });
            println!(
                "RON input, such as:\n{}\nor\n{}",
                ron::to_string(&wanthb).unwrap(),
                ron::to_string(&iam).unwrap()
            );
            let mut rl = rustyline::Editor::<()>::new()?;
            loop {
                let readline = rl.readline(">> ");
                match readline {
                    Ok(line) => {
                        if line.is_empty() {
                            continue;
                        }
                        rl.add_history_entry(&line);
                        let message: Result<client::Message, _> = ron::from_str(&line);
                        match message {
                            Ok(message) => {
                                writer.send(message).await?;
                            }
                            Err(e) => {
                                eprintln!("{e:#?}");
                            }
                        }
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
            tokio::task::spawn(async move {
                while let Some(Ok(msg)) = reader.next().await {
                    println!("{msg:?}");
                }
            });
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
                                    rl.add_history_entry(&line);
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
    Ok(())
}
