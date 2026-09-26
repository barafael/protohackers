use anyhow::Context;
use arguments::{parse_hex_digit, Action, Arguments, Delimiter};
use clap::Parser;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;
use std::io::{stdout, Write};
use std::net::{Shutdown, TcpStream};

mod arguments;

fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();

    let mut stream = TcpStream::connect(args.socket).context("Failed to connect to socket")?;

    if matches!(args.action, Action::Repl) {
        let mut stream_clone = stream.try_clone().context("Failed to clone socket")?;
        let printer = std::thread::spawn(move || {
            let mut stdout = stdout();
            std::io::copy(&mut stream_clone, &mut stdout).context("Failed forwarding")
        });
        let mut rl = Editor::<(), DefaultHistory>::new()?;
        loop {
            let readline = rl.readline(">> ");
            if printer.is_finished() {
                println!("Server closed the connection");
                break;
            }
            match readline {
                Ok(line) => {
                    rl.add_history_entry(line.as_str())?;
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
                    continue;
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

        // Tell the server we are done, then print whatever it still sends until it hangs up.
        // If the connection is already gone, the printer ends on its own.
        let _ = stream.shutdown(Shutdown::Write);
        match printer.join() {
            Ok(Ok(bytes)) => println!("Received {bytes} bytes"),
            Ok(Err(error)) => eprintln!("{error:#}"),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    Ok(())
}
