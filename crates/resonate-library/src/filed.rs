use std::{
    fs::{self, File},
    io::{self, Read, Write},
    num::NonZeroU32,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use resonate_codec::{
    DecodeStatus, Decoder, FileTags, Sources, TagEdit, TagField, TagSink as _, Writing,
};
use resonate_core::{AudioBuffer, Frames, Isrc, MediaLocation, SampleRate, naming};

use crate::{
    Error, Library, Want,
    organise::{self, Layout, Named, Naming},
    scan, store, take_in,
};

pub(crate) const STAGED_SUFFIX: &str = "resonate-delivery";
const LARGEST_FILED: u64 = 4 * 1024 * 1024 * 1024;

static STAGED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryFolder {
    pub path: PathBuf,
    pub layout: Layout,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AlbumToFile {
    pub(crate) title: String,
    pub(crate) owner: Option<String>,
    pub(crate) year: Option<i32>,
    pub(crate) discs: u32,
}

#[derive(Debug)]
pub(crate) enum Unfiled {
    NoFolder,
    Unnamed,
    TooLarge,
    Undecodable,
    NotAsLongAsWanted { heard: Duration },
    Io(io::Error),
    Catalog(Error),
}

impl From<io::Error> for Unfiled {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Filed {
    pub(crate) path: PathBuf,
    pub(crate) root: PathBuf,
}

pub(crate) fn filed(
    library: &Library,
    into: &DeliveryFolder,
    want: &Want,
    reader: &mut dyn Read,
    extension: &str,
) -> std::result::Result<Filed, Unfiled> {
    let folder = into
        .path
        .canonicalize()
        .ok()
        .filter(|folder| folder.is_dir())
        .ok_or(Unfiled::NoFolder)?;
    let album = library
        .album_to_file(want.album)
        .map_err(Unfiled::Catalog)?;
    let root = library
        .root_reaching(&folder)
        .map_err(Unfiled::Catalog)?
        .unwrap_or_else(|| folder.clone());

    let named = Named {
        album_artist: album.owner.as_deref(),
        artist: want.artist.as_deref(),
        album: Some(&album.title),
        title: &want.title,
        year: album.year,
        disc: NonZeroU32::new(want.disc),
        discs: album.discs,
        track: Some(want.position),
        extension: Some(extension),
    };
    let relative = into
        .layout
        .render(&named, Naming::of(&folder))
        .ok_or(Unfiled::Unnamed)?;
    let whole = folder.join(relative);
    let parent = whole.parent().unwrap_or(&folder);
    fs::create_dir_all(parent)?;

    let staged = staged_beside(&whole);
    library.staging(&staged).map_err(Unfiled::Catalog)?;
    let landed = staged_from(reader, &staged).and_then(|()| placed(&staged, &whole));
    staged_away(library, &staged);
    let path = landed?;

    if let Err(unfiled) = weighed(&path, want) {
        let _ = fs::remove_file(&path);
        return Err(unfiled);
    }
    tagged(&path, want, &album);

    library
        .claim_album_keys(want.album, &keys_for(&path, &root, &album, want))
        .map_err(Unfiled::Catalog)?;
    Ok(Filed { path, root })
}

fn weighed(path: &Path, want: &Want) -> std::result::Result<(), Unfiled> {
    let location = MediaLocation::local(path);
    if want.length.is_none() {
        return resonate_codec::probe(&Sources::local(), &location)
            .map(drop)
            .map_err(|_| Unfiled::Undecodable);
    }

    let (heard, rate) = heard_whole(&location).ok_or(Unfiled::Undecodable)?;
    if want.lasts_as_long_as(heard, rate) {
        Ok(())
    } else {
        Err(Unfiled::NotAsLongAsWanted {
            heard: heard.to_duration(rate),
        })
    }
}

fn heard_whole(location: &MediaLocation) -> Option<(Frames, SampleRate)> {
    let (mut decoder, info) = Decoder::open(&Sources::local(), location).ok()?;
    let mut block = AudioBuffer::empty(info.spec);
    let mut frames = 0_u64;

    loop {
        match decoder.next_block(&mut block).ok()? {
            DecodeStatus::Decoded => frames += block.frames() as u64,
            DecodeStatus::EndOfStream => return Some((Frames(frames), info.spec.rate)),
        }
    }
}

fn staged_away(library: &Library, staged: &Path) {
    match organise::swept(staged) {
        Ok(()) => {
            if let Err(error) = library.staged_away(staged) {
                tracing::warn!(%error, "a delivery's staging file taken away is still noted as left behind");
            }
        }
        Err(error) => {
            tracing::warn!(%error, staged = %staged.display(), "a delivery's staging file could not be taken away");
        }
    }
}

fn staged_beside(whole: &Path) -> PathBuf {
    let stamped = format!(
        ".{}-{}.{STAGED_SUFFIX}",
        process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    );
    whole.with_file_name(naming::named_within(
        ".",
        whole.file_name().unwrap_or_default(),
        &stamped,
    ))
}

fn staged_from(reader: &mut dyn Read, staged: &Path) -> std::result::Result<(), Unfiled> {
    let mut file = File::create(staged)?;
    let copied = io::copy(&mut reader.take(LARGEST_FILED + 1), &mut file)?;
    if copied > LARGEST_FILED {
        return Err(Unfiled::TooLarge);
    }
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn placed(staged: &Path, whole: &Path) -> std::result::Result<PathBuf, Unfiled> {
    for candidate in take_in::candidates(whole) {
        if fs::symlink_metadata(&candidate).is_ok() {
            continue;
        }
        match fs::hard_link(staged, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) if candidate.exists() => {}
            Err(_) => {
                fs::rename(staged, &candidate)?;
                return Ok(candidate);
            }
        }
    }
    Err(Unfiled::Io(io::Error::from(io::ErrorKind::AlreadyExists)))
}

fn edits(want: &Want, album: &AlbumToFile) -> Vec<TagEdit> {
    let edit = |field, value: &str| {
        let value = value.trim();
        (!value.is_empty()).then(|| TagEdit {
            field,
            value: value.to_owned(),
        })
    };

    [
        edit(TagField::Title, &want.title),
        want.artist
            .as_deref()
            .and_then(|artist| edit(TagField::Artist, artist)),
        edit(TagField::Album, &album.title),
        album
            .owner
            .as_deref()
            .and_then(|owner| edit(TagField::AlbumArtist, owner)),
        edit(TagField::TrackNumber, &want.position.to_string()),
        edit(TagField::DiscNumber, &want.disc.to_string()),
        album
            .year
            .and_then(|year| edit(TagField::Date, &year.to_string())),
        want.recording
            .as_ref()
            .and_then(|recording| edit(TagField::MusicBrainzTrackId, recording.as_str())),
        want.track
            .as_ref()
            .and_then(|track| edit(TagField::MusicBrainzReleaseTrackId, track.as_str())),
        want.release
            .as_ref()
            .and_then(|release| edit(TagField::MusicBrainzAlbumId, release.as_str())),
        want.isrc
            .as_ref()
            .map(Isrc::as_str)
            .and_then(|isrc| edit(TagField::Isrc, isrc)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn tagged(path: &Path, want: &Want, album: &AlbumToFile) {
    let tags = FileTags::over(Sources::local());
    let location = MediaLocation::local(path);
    if !tags.writes(&location) {
        tracing::debug!(path = %path.display(), "a filed delivery is in a format this build writes no tags into");
        return;
    }
    let edits = edits(want, album);
    let writing = Writing {
        edits: &edits,
        taken: &[],
        picture: None,
        unpictured: false,
        popularity: None,
    };
    if let Err(error) = tags.write(&location, writing) {
        tracing::warn!(%error, path = %path.display(), "a filed delivery could not be tagged");
    }
}

fn keys_for(path: &Path, root: &Path, album: &AlbumToFile, want: &Want) -> Vec<String> {
    if let Some(release) = &want.release {
        return vec![store::release_key(release.as_str())];
    }

    let mut keys = Vec::new();
    if let Some(owner) = album.owner.as_deref() {
        keys.push(store::album_key(&album.title, Some(owner)));
    }
    if let Some(folder) = scan::sleeve(path, root) {
        keys.push(store::sleeve_key(&album.title, &folder));
    }
    if keys.is_empty() {
        keys.push(store::album_key(&album.title, want.artist.as_deref()));
    }
    keys
}

pub(crate) fn roots_of(filed: &[Filed]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = filed.iter().map(|filed| filed.root.clone()).collect();
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use resonate_core::{AlbumId, Mbid, ReleaseTrackId, WantId};

    use super::*;

    fn want() -> Want {
        Want {
            id: WantId::new(1).expect("an id"),
            release_track: ReleaseTrackId::new(1).expect("an id"),
            album: AlbumId::new(1).expect("an id"),
            album_title: "Meddle".to_owned(),
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            recording: Some(Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("an mbid")),
            track: None,
            release: None,
            isrc: Some(Isrc::new("GBN9Y1100089").expect("an isrc")),
            length: None,
            disc: 1,
            position: 6,
            wanted: SystemTime::UNIX_EPOCH,
            tried: None,
            offered: None,
            misses: 0,
            held: None,
            links: Vec::new(),
            release_links: Vec::new(),
        }
    }

    fn meddle() -> AlbumToFile {
        AlbumToFile {
            title: "Meddle".to_owned(),
            owner: Some("Pink Floyd".to_owned()),
            year: Some(1971),
            discs: 1,
        }
    }

    #[test]
    fn a_filed_delivery_is_tagged_with_everything_the_want_and_its_album_say() {
        let edits = edits(&want(), &meddle());
        let said = |field| {
            edits
                .iter()
                .find(|edit| edit.field == field)
                .map(|edit| edit.value.as_str())
        };

        assert_eq!(said(TagField::Title), Some("Echoes"));
        assert_eq!(said(TagField::AlbumArtist), Some("Pink Floyd"));
        assert_eq!(said(TagField::Album), Some("Meddle"));
        assert_eq!(said(TagField::TrackNumber), Some("6"));
        assert_eq!(said(TagField::Date), Some("1971"));
        assert_eq!(said(TagField::Isrc), Some("GBN9Y1100089"));
        assert_eq!(
            said(TagField::MusicBrainzTrackId),
            Some("83d91898-7763-47d7-b03b-b92132375c47")
        );
        assert_eq!(said(TagField::MusicBrainzAlbumId), None);
    }

    #[test]
    fn a_release_names_the_album_alone_and_otherwise_its_owner_and_folder_do() {
        let path = Path::new("/music/Pink Floyd/Meddle/06 Echoes.flac");
        let root = Path::new("/music");

        let released = Want {
            release: Some(Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("an mbid")),
            ..want()
        };

        assert_eq!(
            keys_for(path, root, &meddle(), &released),
            vec![store::release_key("b84ee12a-09ef-421b-82de-0441a926375b")]
        );
        assert_eq!(
            keys_for(path, root, &meddle(), &want()),
            vec![
                store::album_key("Meddle", Some("Pink Floyd")),
                store::sleeve_key("Meddle", Path::new("/music/Pink Floyd/Meddle")),
            ]
        );
    }
}
