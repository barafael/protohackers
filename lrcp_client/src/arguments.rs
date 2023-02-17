use clap::Parser;
use std::net::SocketAddr;

#[derive(Debug, Parser)]
#[command(author, version)]
pub struct Arguments {
    /// Local address to bind to
    #[arg(short, long, default_value = "0.0.0.0:8001")]
    pub local_address: SocketAddr,

    /// Remote address to connect to
    #[arg(short, long, default_value = "0.0.0.0:8000")]
    pub remote_address: SocketAddr,
}
