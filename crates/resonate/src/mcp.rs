#[cfg(feature = "mcp")]
use std::{
    io::{self, BufReader},
    sync::Arc,
};

#[cfg(feature = "mcp")]
use resonate_mcp::{Lookups, OnTheBus, Server, stoppable};
#[cfg(feature = "mcp")]
use resonate_mpris::PlayerName;

#[cfg(not(feature = "mcp"))]
use crate::Error;
use crate::{Result, cli::Cli, config::Config};

#[cfg(feature = "mcp")]
pub fn serve(cli: &Cli, config: &Config, player: Option<&str>) -> Result<()> {
    let library = crate::open_library(cli, config)?;
    let sources = library.sources();
    let server = Server::new(library, OnTheBus::named(player.map(PlayerName::new)))
        .looking_up_with(Lookups {
            reference: crate::online::reference(config),
            fingerprinters: Arc::new(crate::online::fingerprinters(
                config,
                Arc::new(sources),
                &crate::online::by_sound(config),
            )),
            providers: Arc::new(crate::providers::registered(config)),
            studies: config.studies(),
            lyrics: config.fetches_lyrics(),
        });
    tracing::debug!("serving the Model Context Protocol on stdin and stdout");

    let (stop, stoppable) = stoppable();
    let _told = crate::signals::cancel_when_told(move || stop.stop());
    Ok(server.serve_until_stopped(BufReader::new(io::stdin()), io::stdout().lock(), stoppable)?)
}

#[cfg(not(feature = "mcp"))]
pub fn serve(_cli: &Cli, _config: &Config, _player: Option<&str>) -> Result<()> {
    Err(Error::NoMcp)
}
