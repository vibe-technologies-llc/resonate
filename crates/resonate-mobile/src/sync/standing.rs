use std::{num::NonZeroU32, path::Path};

use resonate_codec::{Sources, TagSet, probe};
use resonate_core::MediaLocation;
use resonate_library::{Layout, Named, Naming};
use unicode_normalization::UnicodeNormalization;

use super::{StandingDocument, Waiting, named_alike};
use crate::{
    features::SearchSong,
    organising::{components_of, given, year_of},
};

const FILED_COPIES_WEIGHED_AT_MOST: usize = 64;

const NAMINGS: [Naming; 2] = [Naming::Anything, Naming::Portable];

#[derive(Clone)]
struct Spelled {
    album_artist: Option<String>,
    artist: String,
    album: String,
    title: String,
    year: Option<i32>,
    disc: Option<NonZeroU32>,
    discs: u32,
    track: Option<u32>,
}

impl Spelled {
    fn named<'a>(&'a self, extension: &'a str) -> Named<'a> {
        Named {
            album_artist: self.album_artist.as_deref().and_then(given),
            artist: given(&self.artist),
            album: given(&self.album),
            title: &self.title,
            year: self.year,
            disc: self.disc,
            discs: self.discs,
            track: self.track,
            extension: Some(extension),
        }
    }
}

pub(super) fn folders_filed_in(
    sources: &Sources,
    waiting: &Waiting,
    layout: &str,
    synced: &[String],
) -> Vec<Vec<String>> {
    let tags = probe(sources, &MediaLocation::local(waiting.path.clone()))
        .map(|info| info.tags)
        .unwrap_or_default();
    let spellings = spellings_of(tags, &waiting.received.song);
    let mut layouts: Vec<Layout> = Vec::new();

    for read in [layout]
        .into_iter()
        .chain(synced.iter().map(String::as_str))
        .filter_map(|text| Layout::read(text).ok())
        .chain([Layout::default()])
    {
        if !layouts.contains(&read) {
            layouts.push(read);
        }
    }

    let mut folders: Vec<Vec<String>> = Vec::new();

    for layout in layouts {
        for naming in NAMINGS {
            for spelled in &spellings {
                let Some(folder) = folder_of(&layout, spelled, &waiting.received.extension, naming)
                else {
                    continue;
                };
                if !folders.contains(&folder) {
                    folders.push(folder);
                }
            }
        }
    }
    folders
}

pub(super) fn weighed<'a>(
    waiting: &Waiting,
    folders: &[Vec<String>],
    standing: &'a [StandingDocument],
) -> Vec<&'a StandingDocument> {
    let name = &waiting.received.name;
    let at_the_root = standing
        .iter()
        .filter(|document| document.folder.is_empty() && named_alike(name, &document.name));
    let mut filed = standing
        .iter()
        .filter(|document| !document.folder.is_empty() && among(folders, &document.folder))
        .filter(|document| {
            named_alike(name, &document.name)
                || of_the_extension(&document.name, &waiting.received.extension)
        })
        .collect::<Vec<_>>();

    filed.sort_by_key(|document| !named_alike(name, &document.name));
    filed.truncate(FILED_COPIES_WEIGHED_AT_MOST);
    at_the_root.chain(filed).collect()
}

fn spellings_of(tags: TagSet, song: &SearchSong) -> Vec<Spelled> {
    let or_else = |tag: Option<String>, sent: &str| {
        tag.filter(|text| given(text).is_some())
            .unwrap_or_else(|| sent.to_owned())
    };
    let disc = tags.disc_number.or(song.disc).and_then(NonZeroU32::new);

    let as_indexed = Spelled {
        album_artist: tags.album_artist.filter(|text| given(text).is_some()),
        artist: or_else(tags.artist, &song.artist),
        album: or_else(tags.album, &song.album),
        title: or_else(tags.title, &song.title),
        year: tags.date.as_deref().and_then(year_of),
        disc,
        discs: tags
            .disc_total
            .unwrap_or_default()
            .max(disc.map_or(1, NonZeroU32::get)),
        track: tags.track_number.or(song.position),
    };

    if as_indexed.album_artist.is_some() {
        return vec![as_indexed];
    }

    let under_the_artist = Spelled {
        album_artist: Some(as_indexed.artist.clone()),
        ..as_indexed.clone()
    };
    vec![as_indexed, under_the_artist]
}

fn folder_of(
    layout: &Layout,
    spelled: &Spelled,
    extension: &str,
    naming: Naming,
) -> Option<Vec<String>> {
    let rendered = layout.render(&spelled.named(extension), naming)?;
    let folder = components_of(rendered.parent()?)?;
    let composed = folder
        .iter()
        .map(|part| part.nfc().collect::<String>())
        .collect::<Vec<_>>();
    (!composed.is_empty()).then_some(composed)
}

fn among(folders: &[Vec<String>], folder: &[String]) -> bool {
    folders.iter().any(|filed| {
        filed.len() == folder.len()
            && filed
                .iter()
                .zip(folder)
                .all(|(ours, theirs)| ours.chars().eq(theirs.nfc()))
    })
}

fn of_the_extension(name: &str, extension: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|found| found.to_str())
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::sync::ReceivedSong;

    const LANDING: &str = "Sölvi - Wavebreak.wav";

    fn waiting() -> Waiting {
        Waiting {
            received: ReceivedSong {
                id: 1,
                name: LANDING.to_owned(),
                extension: "wav".to_owned(),
                song: SearchSong::default(),
            },
            path: PathBuf::from("staged.part"),
            file: None,
        }
    }

    fn document(name: &str, folder: &[&str]) -> StandingDocument {
        StandingDocument {
            uri: format!("content://test/{name}"),
            name: name.to_owned(),
            folder: folder.iter().map(|part| (*part).to_owned()).collect(),
        }
    }

    #[test]
    fn a_filed_folder_has_its_named_alike_weighed_first_and_never_more_than_the_bound() {
        let seabrook = ["Sölvi", "Seabrook"];
        let mut standing = (0..100)
            .map(|nth| document(&format!("{nth:02} Song.WAV"), &seabrook))
            .collect::<Vec<_>>();

        standing.push(document("cover.jpg", &seabrook));
        standing.push(document(LANDING, &["So\u{308}lvi", "Seabrook"]));
        standing.push(document(LANDING, &["Sölvi", "Early Days"]));
        standing.push(document("Sölvi - Wavebreak (1).wav", &[]));
        standing.push(document("01 Wavebreak.wav", &[]));

        let folders = vec![seabrook.map(str::to_owned).to_vec()];

        let weighed = weighed(&waiting(), &folders, &standing);

        assert_eq!(weighed.len(), 1 + FILED_COPIES_WEIGHED_AT_MOST);
        assert_eq!(weighed[0].name, "Sölvi - Wavebreak (1).wav");
        assert_eq!(weighed[1].name, LANDING);
        assert_eq!(weighed[1].folder, ["So\u{308}lvi", "Seabrook"]);
        assert!(weighed.iter().all(|document| {
            document.name != "cover.jpg"
                && document.name != "01 Wavebreak.wav"
                && document
                    .folder
                    .get(1)
                    .is_none_or(|album| album == "Seabrook")
        }));
    }
}
