use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Debug, Parser)]
#[command(author, version)]
pub struct Arguments {
    #[command(subcommand)]
    pub mode: Mode,
}

#[derive(Debug, Subcommand)]
pub enum Mode {
    Generate {
        /// Landscape description - how many roads, tickets, violations, ...
        #[arg(short, long, default_value = "landscape.toml")]
        input: PathBuf,

        /// Generator output
        #[arg(short, long, default_value = "sequence.ron")]
        output: PathBuf,
    },
    Replay {
        /// TCP server socket to connect to
        #[arg(short, long, default_value = "0.0.0.0:8000")]
        server: SocketAddr,

        /// Input file
        #[arg(short, long, default_value = "sequence.ron")]
        instance: PathBuf,
    },
}
