use arguments::Arguments;
use clap::Parser;
use futures::{SinkExt, StreamExt};
use lrcp_codec::Frame;
use rustyline::{error::ReadlineError, history::DefaultHistory};
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
    let connect = Frame::Connect(123);
    let data = Frame::Data {
        session: 123,
        position: 0,
        data: r"some data with \/ sla\\h".to_string(),
    };
    let ack = Frame::Ack {
        session: 123,
        length: 23,
    };
    println!(
        "RON input, such as:\n{}\nor\n{}\nor\n{}",
        ron::to_string(&connect).unwrap(),
        ron::to_string(&data).unwrap(),
        ron::to_string(&ack).unwrap(),
    );
    println!("Type CTRL+V CTRL+J to insert a newline without submitting the input (might not work on some shells such as vscode integrated terminal).");
    println!("As per protocol, the application layer processes bytes only line-wise!");
    println!("CTRL+C to clear and CTRL+D to quit.");
    let mut rl = rustyline::Editor::<(), DefaultHistory>::new()?;
    loop {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }
                rl.add_history_entry(&line)?;
                let message: Result<Frame, _> = ron::from_str(&line);
                match message {
                    Ok(message) => {
                        sink.send((message, args.remote_address)).await?;
                    }
                    Err(e) => {
                        println!("{e:#?}");
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
    Ok(())
}
