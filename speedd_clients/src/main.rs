use arguments::{Arguments, Mode};
use clap::Parser;
use futures::{SinkExt, StreamExt};
use rustyline::{error::ReadlineError, history::DefaultHistory};
use speedd_codecs::{
    camera::Camera,
    client::{self, encoder::MessageEncoder as Encoder},
    plate::PlateRecord,
    server::decoder::MessageDecoder as Decoder,
};
use std::time::Duration;
use tokio::{
    net::{tcp::OwnedReadHalf, TcpStream},
    task::JoinHandle,
};
use tokio_util::codec::{FramedRead, FramedWrite};

mod arguments;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();

    let client = TcpStream::connect(args.address).await?;
    let (reader, writer) = client.into_split();

    let reader = FramedRead::new(reader, Decoder);
    let mut writer = FramedWrite::new(writer, Encoder);

    if !args.interval.is_zero() {
        writer
            .send(client::Message::WantHeartbeat(args.interval.into()))
            .await?;
    }

    match args.mode {
        Mode::Client => {
            let printer = tokio::spawn(print_all(reader));
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
            let mut rl = rustyline::Editor::<(), DefaultHistory>::new()?;
            loop {
                let readline = rl.readline(">> ");
                if printer.is_finished() {
                    break;
                }
                match readline {
                    Ok(line) => {
                        if line.is_empty() {
                            continue;
                        }
                        rl.add_history_entry(&line)?;
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
                        continue;
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
            stop(printer).await;
        }
        Mode::Dispatcher { roads } => {
            let mut reader = reader;
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
            let printer = tokio::spawn(print_all(reader));
            writer
                .send(client::Message::IAmCamera(Camera { road, mile, limit }))
                .await?;

            let mut rl = rustyline::Editor::<(), DefaultHistory>::new()?;
            loop {
                let readline = rl.readline(">> ");
                if printer.is_finished() {
                    break;
                }
                match readline {
                    Ok(line) => {
                        let mut tokens = line.split_whitespace();
                        match (tokens.next(), tokens.next()) {
                            (Some(plate), Some(timestamp)) => {
                                if let Ok(timestamp) = timestamp.parse() {
                                    rl.add_history_entry(&line)?;
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
                        continue;
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
            stop(printer).await;
        }
    }
    Ok(())
}

/// Print what the server sends, until it hangs up.
async fn print_all(mut reader: FramedRead<OwnedReadHalf, Decoder>) {
    loop {
        match reader.next().await {
            Some(Ok(msg)) => println!("{msg:?}"),
            Some(Err(e)) => {
                eprintln!("{e:?}");
                break;
            }
            None => {
                println!("Server closed the connection");
                break;
            }
        }
    }
}

/// Stop printing, without swallowing a panic of the printer.
async fn stop(printer: JoinHandle<()>) {
    printer.abort();
    if let Err(error) = printer.await {
        if error.is_panic() {
            std::panic::resume_unwind(error.into_panic());
        }
    }
}
