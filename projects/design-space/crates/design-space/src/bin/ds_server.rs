use anyhow::{Result, bail};
use clap::Parser;
use design_space::server::{RemoteServer, serve_http, serve_stdio};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "ds-server", about = "Serve a design-space remote")]
struct Args {
    /// Remote storage root.
    #[arg(long, default_value = ".")]
    root: PathBuf,

    /// HTTP listen address (ignored with --stdio).
    #[arg(long, default_value = "127.0.0.1:7700")]
    listen: SocketAddr,

    /// Require `Authorization: Bearer <token>`; token is read from this file.
    #[arg(long)]
    token_file: Option<PathBuf>,

    /// Serve the SSH transport protocol on stdin/stdout.
    #[arg(long)]
    stdio: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ds-server: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let mut server = RemoteServer::open(args.root)?;
    if args.stdio {
        if args.token_file.is_some() {
            bail!("--token-file is only used by HTTP; SSH authenticates the connection");
        }
        serve_stdio(&server)
    } else {
        if let Some(path) = args.token_file {
            server = server.with_token_file(&path)?;
        }
        serve_http(server, args.listen)
    }
}
