use arguments::Arguments;
use clap::Parser;
use futures::{SinkExt, StreamExt};
use lrcp_codec::Frame;
use rustyline::error::ReadlineError;
use tokio::net::UdpSocket;
use tokio_util::udp::UdpFramed;

mod arguments;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();

    println!("Binding to local address: {}", args.local_address);
    let client = UdpSocket::bind(args.local_address).await?;

    println!("Binding to remote address: {}", args.remote_address);
    client.connect(args.remote_address).await?;

    let framed = UdpFramed::new(client, lrcp_codec::Lrcp);
    let (mut sink, mut stream) = framed.split();

    // reader task
    tokio::task::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            println!("{msg:?}");
        }
    });
    let ack = Frame::Ack {
        session: 123,
        length: 456,
    };
    let data = Frame::Data {
        session: 123,
        position: 5,
        data: "some/data/with/slashes".to_string(),
    };
    println!(
        "RON input, such as:\n{}\nor\n{}",
        ron::to_string(&ack).unwrap(),
        ron::to_string(&data).unwrap()
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
                let message: Result<Frame, _> = ron::from_str(&line);
                match message {
                    Ok(message) => {
                        sink.send((message, args.remote_address)).await?;
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
    Ok(())
}
