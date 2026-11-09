#[cfg(feature = "ui")]
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "ui")]
use resonate_codec::{CoverArt, ImageFormat};
use resonate_engine::NodeName;
use resonate_listen::{CLIP_BY_DEFAULT, Hearing, Listener, Listening};
#[cfg(feature = "ui")]
use resonate_listen::{Heard, PictureFormat};
#[cfg(feature = "ui")]
use resonate_mpris::{Teller, Told};

use crate::{Error, Result, config::Config, info::pairs, online, words};

const APP_NAME: &str = "Resonate";

pub fn run(
    config: &Config,
    microphone: Option<&str>,
    seconds: Option<u64>,
    microphones: bool,
) -> Result<()> {
    let listener = Listener::new(APP_NAME);
    if microphones {
        for found in listener.microphones()? {
            let marked = if found.is_default { "*" } else { " " };
            said!("{marked} {}  {}", found.name, found.description);
        }
        return Ok(());
    }

    let recognisers = online::recognisers(config);
    if recognisers.is_empty() {
        return Err(Error::NoRecogniser);
    }
    let from = match microphone {
        None => config.listen_from.clone().unwrap_or_default(),
        Some("") => Listening::Microphone(None),
        Some(name) => Listening::Microphone(Some(NodeName::new(name.to_owned()))),
    };
    let length = seconds.map_or_else(
        || config.listen_for.unwrap_or(CLIP_BY_DEFAULT),
        Duration::from_secs,
    );
    let heard_from = match &from {
        Listening::Desktop => words::listen::what_the_desktop_plays().to_owned(),
        Listening::Microphone(None) => words::listen::the_default_microphone().to_owned(),
        Listening::Microphone(Some(name)) => name.to_string(),
    };
    told!("{}", words::listen::listening(heard_from, length.as_secs()));

    let clip = listener.record(&from, length, &Hearing::new())?;
    let recognition = recognisers.recognise(&clip)?;
    let Some(heard) = recognition.heard else {
        let asked: Vec<String> = recognisers
            .names()
            .iter()
            .map(ToString::to_string)
            .collect();
        said!("{}", words::listen::nothing_known(asked.join(", ")));
        return Ok(());
    };
    let told = |value: Option<String>| value.unwrap_or_else(|| "—".to_owned());
    pairs(vec![
        (words::listen::title(), heard.title),
        (words::listen::artist(), told(heard.artist)),
        (words::listen::album(), told(heard.album)),
        (
            words::listen::year(),
            told(heard.year.map(|year| year.to_string())),
        ),
        (
            words::listen::isrc(),
            told(heard.isrc.map(|isrc| isrc.to_string())),
        ),
        (
            words::listen::recording(),
            told(heard.recording.map(|mbid| mbid.to_string())),
        ),
        (words::listen::link(), told(heard.link)),
        (words::listen::heard_by(), heard.by.to_string()),
    ]);
    Ok(())
}

#[cfg(feature = "ui")]
pub fn in_the_window(config: &Config, teller: Option<Teller>) -> resonate_ui::Listens {
    resonate_ui::Listens {
        listener: Listener::new(APP_NAME),
        recognisers: Arc::new(online::recognisers(config)),
        tell: Arc::new(move |heard: &Heard| {
            if let Some(teller) = &teller {
                teller.tell(told_of(heard));
            }
        }),
        from: config.listen_from.clone().unwrap_or_default(),
        length: config.listen_for.unwrap_or(CLIP_BY_DEFAULT),
    }
}

#[cfg(feature = "ui")]
fn told_of(heard: &Heard) -> Told {
    let body = [heard.artist.as_deref(), heard.album.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    Told {
        summary: heard.title.clone(),
        body,
        picture: heard.picture.as_ref().map(|picture| CoverArt {
            format: match picture.format {
                PictureFormat::Jpeg => ImageFormat::Jpeg,
                PictureFormat::Png => ImageFormat::Png,
            },
            bytes: picture.bytes.clone(),
        }),
    }
}
