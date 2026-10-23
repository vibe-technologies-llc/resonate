use std::{net::SocketAddr, path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use clap::{CommandFactory, Parser, error::ErrorKind};
use resonate_server::{
    Allowance, Capacity, Error, Gateway, Hifi, Limits, Minted, Server, Store, Web, gateway_tls,
    listening, pem, serving,
};
use resonate_sync::wire::LARGEST_RECORD;
use tokio::{
    net::TcpListener,
    signal::unix::{SignalKind, signal},
};
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
        help = "Mint a hifi-api key: print the secret to hand out and the digest to list in the config, then exit"
    )]
    hifi_key: bool,

    #[arg(
        long,
        default_value = "[::]:443",
        help = "The UDP address QUIC listens on"
    )]
    listen: SocketAddr,

    #[arg(
        long,
        value_name = "PEM",
        required_unless_present = "hifi_key",
        help = "The certificate chain the server presents"
    )]
    certificate: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PEM",
        required_unless_present = "hifi_key",
        help = "The private key of that certificate"
    )]
    key: Option<PathBuf>,

    #[arg(
        long,
        value_name = "FILE",
        required_unless_present = "hifi_key",
        help = "The SQLite file the records are kept in"
    )]
    store: Option<PathBuf>,

    #[arg(long, default_value_t = LARGEST_RECORD, help = "The largest one record may be, in bytes")]
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

    #[arg(
        long,
        default_value_t = 10_000,
        help = "The most spaces the server keeps"
    )]
    spaces: u64,

    #[arg(
        long,
        default_value_t = 16 * 1024 * 1024 * 1024,
        help = "The most bytes all spaces may hold together"
    )]
    total_bytes: u64,

    #[arg(
        long,
        default_value_t = 2048,
        help = "The most connections open at once, from everyone"
    )]
    connections: usize,

    #[arg(
        long,
        default_value_t = 64,
        help = "The most connections open at once from one address (an IPv6 /64 counts as one)"
    )]
    connections_per_address: usize,

    #[arg(
        long,
        default_value_t = 30,
        help = "The most connections one address may open a minute"
    )]
    attempts_per_minute: u32,

    #[arg(
        long,
        default_value_t = 5,
        help = "The most new spaces one address may make a day"
    )]
    spaces_per_address_per_day: u32,

    #[arg(
        long,
        value_name = "TOML",
        help = "The config file; its [hifi] table opens the keyed hifi-api gateway and its [web] table the WebSocket listener"
    )]
    config: Option<PathBuf>,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();
    let outcome = if args.hifi_key { mint() } else { run(args) };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the server stopped");
            ExitCode::FAILURE
        }
    }
}

fn mint() -> Result<(), Error> {
    let minted = Minted::new()?;
    println!("secret: {}", minted.secret);
    println!("digest: {}", minted.digest.hex());
    Ok(())
}

fn required(value: Option<PathBuf>, flag: &str) -> PathBuf {
    value.unwrap_or_else(|| {
        Args::command()
            .error(
                ErrorKind::MissingRequiredArgument,
                format!("--{flag} is required"),
            )
            .exit()
    })
}

fn run(args: Args) -> Result<(), Error> {
    let (chain, key) = pem(
        &required(args.certificate, "certificate"),
        &required(args.key, "key"),
    )?;
    let hifi = args
        .config
        .as_deref()
        .map(Hifi::read)
        .transpose()?
        .flatten();
    let web = args.config.as_deref().map(Web::read).transpose()?.flatten();
    let gateway_acceptor = hifi
        .as_ref()
        .map(|_| gateway_tls(chain.clone(), key.clone_key()))
        .transpose()?;
    let config = serving(chain, key)?;
    let store = Store::open(&required(args.store, "store"))?.limited(Capacity {
        spaces: args.spaces,
        bytes: args.total_bytes,
    });
    let limits = Limits {
        record_bytes: args.record_bytes,
        space_bytes: args.space_bytes,
        devices: args.devices,
        frames_per_minute: args.frames_per_minute,
        idle_for: Duration::from_secs(args.idle_days.saturating_mul(SECONDS_PER_DAY)),
        allowance: Allowance {
            connections: args.connections,
            connections_per_address: args.connections_per_address,
            attempts_per_minute: args.attempts_per_minute,
            spaces_per_address_per_day: args.spaces_per_address_per_day,
        },
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    runtime.block_on(async {
        let endpoint = listening(config, args.listen)?;
        tracing::info!(listen = %args.listen, "serving sync");
        let server = Server::new(store, limits);
        let gateway = match (hifi, gateway_acceptor) {
            (Some(hifi), Some(acceptor)) => {
                let listener = TcpListener::bind(hifi.listen).await.map_err(Error::Listen)?;
                tracing::info!(listen = %hifi.listen, upstream = %hifi.upstream, keys = hifi.keys.len(), "serving hifi-api");
                Some(Gateway::new(hifi.keys, hifi.upstream).serve(listener, acceptor))
            }
            _ => None,
        };
        let web_server = Arc::clone(&server);
        let serving_web = async {
            match web {
                Some(web) => {
                    let listener = TcpListener::bind(web.listen).await.map_err(Error::Listen)?;
                    tracing::info!(listen = %web.listen, forwarded_by = ?web.forwarded_by, "serving sync over web sockets");
                    web_server.serve_web(listener, web.forwarded_by).await;
                    Ok::<(), Error>(())
                }
                None => std::future::pending().await,
            }
        };
        let serving_hifi = async {
            match gateway {
                Some(serving) => serving.await,
                None => std::future::pending().await,
            }
        };
        let mut terminated = signal(SignalKind::terminate()).map_err(Error::Signals)?;
        tokio::select! {
            () = server.serve(endpoint.clone()) => {}
            () = serving_hifi => {}
            bound = serving_web => bound?,
            _ = tokio::signal::ctrl_c() => tracing::info!("asked to stop"),
            _ = terminated.recv() => tracing::info!("asked to stop"),
        }
        endpoint.close(0_u32.into(), b"stopping");
        endpoint.wait_idle().await;
        Ok(())
    })
}
