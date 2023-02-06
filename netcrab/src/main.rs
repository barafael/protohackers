use anyhow::Context;
use arguments::{parse_hex_digit, Action, Arguments, Delimiter};
use clap::Parser;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::io::{stdout, Write};
use std::net::TcpStream;

mod arguments;

fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();

    let mut stream = TcpStream::connect(args.socket).context("Failed to connect to socket")?;

    if let Action::Repl = args.action {
        let mut stream_clone = stream.try_clone().unwrap();
        std::thread::spawn(move || {
            let mut stdout = stdout();
            std::io::copy(&mut stream_clone, &mut stdout)
                .context("Failed forwarding")
                .unwrap();
        });
        let mut rl = Editor::<()>::new()?;
        loop {
            let readline = rl.readline(">> ");
            match readline {
                Ok(line) => {
                    rl.add_history_entry(line.as_str());
                    if args.hex {
                        let line = line
                            .split_whitespace()
                            .map(parse_hex_digit)
                            .collect::<anyhow::Result<Vec<u8>>>()?;
                        stream.write_all(&line)?;
                    } else {
                        stream.write_all(line.as_bytes()).unwrap();
                    }
                    match args.delimiter {
                        Delimiter::Newline => {
                            stream.write_all(b"\n").unwrap();
                        }
                        Delimiter::CrLf => {
                            stream.write_all(b"\r\n").unwrap();
                        }
                        Delimiter::None => {}
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Error: {err:?}");
                    break;
                }
            }
        }
    }
    Ok(())
}
