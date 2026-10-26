use std::time::Duration;

use resonate_providers::Listed as Weighed;
use serde::Deserialize;

use crate::{
    hifi::{id_spelt, listings_readable},
    played::TrackId,
};

const TRACKS: &str = "tracks";
const ARTISTS: &str = "artists";

#[derive(Deserialize)]
pub(crate) struct Searched {
    #[serde(default)]
    data: Option<Results>,
    #[serde(default, deserialize_with = "listings_readable")]
    included: Vec<Resource>,
}

#[derive(Deserialize)]
struct Results {
    #[serde(default)]
    relationships: Option<Related>,
}

#[derive(Deserialize)]
struct Related {
    #[serde(default)]
    tracks: Option<Identifiers>,
    #[serde(default)]
    artists: Option<Identifiers>,
}

#[derive(Deserialize)]
struct Identifiers {
    #[serde(default, deserialize_with = "listings_readable")]
    data: Vec<Identifier>,
}

#[derive(Deserialize)]
struct Identifier {
    #[serde(deserialize_with = "id_spelt")]
    id: u64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Resource {
    #[serde(deserialize_with = "id_spelt")]
    id: u64,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    attributes: Attributes,
    #[serde(default)]
    relationships: Option<Related>,
}

#[derive(Default, Deserialize)]
struct Attributes {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    isrc: Option<String>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

pub(crate) struct Track {
    pub(crate) id: TrackId,
    title: String,
    version: Option<String>,
    isrc: Option<String>,
    length: Option<Duration>,
    artists: Vec<String>,
}

impl Track {
    pub(crate) fn weighed(&self) -> Weighed<'_> {
        Weighed {
            isrcs: self.isrc.as_deref().into_iter().collect(),
            title: &self.title,
            version: self.version.as_deref(),
            artists: self.artists.iter().map(String::as_str).collect(),
            length: self.length,
        }
    }
}

impl Searched {
    fn included(&self, kind: &str, id: u64) -> Option<&Resource> {
        self.included
            .iter()
            .find(|resource| resource.kind == kind && resource.id == id)
    }

    fn artists_of(&self, track: &Resource) -> Vec<String> {
        track
            .relationships
            .as_ref()
            .and_then(|related| related.artists.as_ref())
            .into_iter()
            .flat_map(|artists| &artists.data)
            .filter(|artist| artist.kind == ARTISTS)
            .filter_map(|artist| self.included(ARTISTS, artist.id))
            .filter_map(|artist| artist.attributes.name.clone())
            .collect()
    }

    pub(crate) fn tracks(&self) -> Vec<Track> {
        self.data
            .iter()
            .filter_map(|results| results.relationships.as_ref())
            .filter_map(|related| related.tracks.as_ref())
            .flat_map(|tracks| &tracks.data)
            .filter(|identifier| identifier.kind == TRACKS)
            .filter_map(|identifier| self.included(TRACKS, identifier.id))
            .filter_map(|track| {
                Some(Track {
                    id: TrackId(track.id),
                    title: track.attributes.title.clone()?,
                    version: track.attributes.version.clone(),
                    isrc: track.attributes.isrc.clone(),
                    length: track.attributes.duration.as_deref().and_then(lasting),
                    artists: self.artists_of(track),
                })
            })
            .collect()
    }
}

fn lasting(text: &str) -> Option<Duration> {
    let clock = text
        .trim()
        .strip_prefix("PT")
        .filter(|clock| !clock.is_empty())?;
    let mut total = Duration::ZERO;
    let mut number = String::new();
    for letter in clock.chars() {
        if letter.is_ascii_digit() || letter == '.' {
            number.push(letter);
            continue;
        }
        let seconds_in_a_unit = match letter {
            'H' => 3600.0,
            'M' => 60.0,
            'S' => 1.0,
            _ => return None,
        };
        let amount: f64 = number.parse().ok()?;
        number.clear();
        total = total.checked_add(Duration::try_from_secs_f64(amount * seconds_in_a_unit).ok()?)?;
    }
    number.is_empty().then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_read_as_iso_8601_spells_it() {
        for (text, seconds) in [
            ("PT23M16S", 23.0 * 60.0 + 16.0),
            ("PT1H2M3.5S", 3723.5),
            ("PT45S", 45.0),
            ("PT3M", 180.0),
        ] {
            assert_eq!(
                lasting(text),
                Some(Duration::from_secs_f64(seconds)),
                "{text}"
            );
        }
        for unread in ["", "PT", "23M16S", "PT1D", "PT5", "PTMS", "PT-5S"] {
            assert_eq!(lasting(unread), None, "{unread}");
        }
    }
}
