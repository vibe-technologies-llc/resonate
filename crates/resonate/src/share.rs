use std::path::Path;

use resonate_core::{FrameSpan, MediaLocation};
use resonate_library::{Library, Reference};

use crate::{Error, Result, from_here, reached};

pub fn print(
    library: &Library,
    reference: Option<&dyn Reference>,
    file: Option<&Path>,
) -> Result<()> {
    let (location, span) = match file {
        Some(path) => (MediaLocation::local(from_here(path)), None),
        None => playing()?,
    };
    let held = held(library, &location, span)?;
    let shared = match reference {
        Some(reference) => held.streamed_where_asked(reference),
        None => held,
    };
    let Some(link) = shared.written() else {
        return Err(Error::NothingToShare);
    };

    said!("{link}");
    Ok(())
}

fn playing() -> Result<(MediaLocation, Option<FrameSpan>)> {
    reached(None)?
        .metadata()?
        .and_then(|described| Some((described.location?, described.span)))
        .ok_or(Error::NothingPlaying)
}

fn held(
    library: &Library,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Result<resonate_library::Shared> {
    let unheld = || Error::NotInTheCatalog {
        location: location.clone(),
    };

    let track = location
        .as_path()
        .and_then(|path| library.track_at(path, span).transpose())
        .transpose()?
        .ok_or_else(unheld)?;

    library.shareable(track.id)?.ok_or_else(unheld)
}
