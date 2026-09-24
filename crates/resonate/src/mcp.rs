#[cfg(feature = "mcp")]
use std::{io, sync::Arc};

#[cfg(feature = "mcp")]
use resonate_mcp::{Lookups, OnTheBus, Server};
#[cfg(feature = "mcp")]
use resonate_mpris::PlayerName;

#[cfg(not(feature = "mcp"))]
use crate::Error;
use crate::{Result, cli::Cli, config::Config};

#[cfg(feature = "mcp")]
pub fn serve(cli: &Cli, config: &Config, player: Option<&str>) -> Result<()> {
    let server = Server::new(
        crate::open_library(cli, config)?,
        OnTheBus::named(player.map(PlayerName::new)),
    )
    .looking_up_with(Lookups {
        reference: crate::online::reference(config),
        fingerprinters: Arc::new(crate::online::fingerprinters(config)),
        providers: Arc::new(crate::providers::registered(config)),
        studies: config.studies(),
    });
    tracing::debug!("serving the Model Context Protocol on stdin and stdout");

    Ok(server.serve(io::stdin().lock(), io::stdout().lock())?)
}

#[cfg(not(feature = "mcp"))]
pub fn serve(_cli: &Cli, _config: &Config, _player: Option<&str>) -> Result<()> {
    Err(Error::NoMcp)
}
