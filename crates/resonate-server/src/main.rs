use std::{net::SocketAddr, path::PathBuf, process::ExitCode, time::Duration};

use clap::Parser;
use resonate_server::{Error, Limits, Server, Store, listening, tls};
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Parser)]
#[command(
    name = "resonate-server",
    version,
    about = "Keep the sealed sync records of Resonate's devices and introduce them to each other"
)]
struct Args {
    #[arg(
        long,
        default_value = "[::]:443",
        help = "The UDP address QUIC listens on"
    )]
    listen: SocketAddr,

    #[arg(
        long,
        value_name = "PEM",
        help = "The certificate chain the server presents"
    )]
    certificate: PathBuf,

    #[arg(long, value_name = "PEM", help = "The private key of that certificate")]
    key: PathBuf,

    #[arg(
        long,
        value_name = "FILE",
        help = "The SQLite file the records are kept in"
    )]
    store: PathBuf,

    #[arg(long, default_value_t = 256 * 1024, help = "The largest one record may be, in bytes")]
    record_bytes: usize,

    #[arg(long, default_value_t = 64 * 1024 * 1024, help = "The most one space may hold, in bytes")]
    space_bytes: u64,

    #[arg(
        long,
        default_value_t = 16,
        help = "The most devices of one space connected at once"
    )]
    devices: usize,

    #[arg(
        long,
        default_value_t = 240,
        help = "The most frames one connection may send a minute"
    )]
    frames_per_minute: u32,

    #[arg(
        long,
        default_value_t = 365,
        help = "Days a space may go untouched before it is swept"
    )]
    idle_days: u64,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the server stopped");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), Error> {
    let config = tls(&args.certificate, &args.key)?;
    let store = Store::open(&args.store)?;
    let limits = Limits {
        record_bytes: args.record_bytes,
        space_bytes: args.space_bytes,
        devices: args.devices,
        frames_per_minute: args.frames_per_minute,
        idle_for: Duration::from_secs(args.idle_days.saturating_mul(SECONDS_PER_DAY)),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    runtime.block_on(async {
        let endpoint = listening(config, args.listen)?;
        tracing::info!(listen = %args.listen, "serving sync");
        let server = Server::new(store, limits);
        let mut terminated = signal(SignalKind::terminate()).map_err(Error::Signals)?;
        tokio::select! {
            () = server.serve(endpoint.clone()) => {}
            _ = tokio::signal::ctrl_c() => tracing::info!("asked to stop"),
            _ = terminated.recv() => tracing::info!("asked to stop"),
        }
        endpoint.close(0_u32.into(), b"stopping");
        endpoint.wait_idle().await;
        Ok(())
    })
}
