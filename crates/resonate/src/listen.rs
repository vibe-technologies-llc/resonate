#[cfg(feature = "ui")]
use std::sync::Arc;
use std::{num::NonZeroU64, time::Duration};

#[cfg(feature = "ui")]
use resonate_codec::{CoverArt, ImageFormat};
use resonate_listen::{CLIP_BY_DEFAULT, Hearing, Listener, Listening};
#[cfg(feature = "ui")]
use resonate_listen::{Heard, PictureFormat};
#[cfg(feature = "ui")]
use resonate_mpris::{Teller, Told};
use resonate_pipewire::NodeName;

use crate::{Error, Result, config::Config, info::pairs, online};

const APP_NAME: &str = "Resonate";

pub fn run(
    config: &Config,
    microphone: Option<&str>,
    seconds: Option<NonZeroU64>,
    microphones: bool,
) -> Result<()> {
    let listener = Listener::new(APP_NAME);
    if microphones {
        for found in listener.microphones()? {
            let marked = if found.is_default { "*" } else { " " };
            println!("{marked} {}  {}", found.name, found.description);
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
        |seconds| Duration::from_secs(seconds.get()),
    );
    let heard_from = match &from {
        Listening::Desktop => "what the desktop plays".to_owned(),
        Listening::Microphone(None) => "the default microphone".to_owned(),
        Listening::Microphone(Some(name)) => name.to_string(),
    };
    eprintln!("listening to {heard_from} for {} s", length.as_secs());

    let clip = listener.record(&from, length, &Hearing::new())?;
    let recognition = recognisers.recognise(&clip)?;
    let Some(heard) = recognition.heard else {
        let asked: Vec<String> = recognisers
            .names()
            .iter()
            .map(ToString::to_string)
            .collect();
        println!("nothing {} knows was heard", asked.join(", "));
        return Ok(());
    };
    let told = |value: Option<String>| value.unwrap_or_else(|| "—".to_owned());
    pairs(vec![
        ("title", heard.title),
        ("artist", told(heard.artist)),
        ("album", told(heard.album)),
        ("year", told(heard.year.map(|year| year.to_string()))),
        ("isrc", told(heard.isrc.map(|isrc| isrc.to_string()))),
        (
            "recording",
            told(heard.recording.map(|mbid| mbid.to_string())),
        ),
        ("link", told(heard.link)),
        ("heard by", heard.by.to_string()),
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
