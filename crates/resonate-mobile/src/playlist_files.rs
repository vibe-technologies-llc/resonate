use std::path::{Path, PathBuf};

use resonate_core::{Locator, MediaLocation, folded_letters, text};
use resonate_library::{Described, PlaylistFormat, SheetRow, read_sheet, written_sheet};
use rusqlite::{Connection, params};

use crate::{Error, MobilePlayer, Result, StoreOp};

const PLAYLIST_FILE_AT_MOST: usize = 8 * 1024 * 1024;

const ROWS_AT_MOST: usize = 10_000;

const NAMES_TRIED: u32 = 99;

const FOLDER_ROOT: &str = "/";

pub(crate) const HELD_AT: &str =
    "SELECT id, uri FROM documents WHERE missing = 0 AND folded(name) = folded(?1) ORDER BY id";

pub(crate) const PLAYLIST_FILE_EXTENSIONS: [&str; 4] = ["m3u", "m3u8", "pls", "xspf"];

#[uniffi::export]
pub fn playlist_file_extensions() -> Vec<String> {
    PLAYLIST_FILE_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_owned())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PlaylistFileFormat {
    M3u,
    Pls,
    Xspf,
}

impl PlaylistFileFormat {
    fn library(self) -> PlaylistFormat {
        match self {
            Self::M3u => PlaylistFormat::M3u,
            Self::Pls => PlaylistFormat::Pls,
            Self::Xspf => PlaylistFormat::Xspf,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ImportedPlaylistFile {
    pub id: u64,
    pub name: String,
    pub matched: u32,
    pub missing: u32,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn import_playlist_file(
        &self,
        bytes: Vec<u8>,
        file_name: String,
    ) -> Result<ImportedPlaylistFile> {
        if bytes.len() > PLAYLIST_FILE_AT_MOST {
            return Err(Error::PlaylistFileTooLarge {
                bytes: bytes.len() as u64,
            });
        }
        let (text, _) = text::decoded(&bytes);
        let (sheet, _) = read_sheet(&text, Path::new(FOLDER_ROOT));
        let listed = sheet.locations.len() + sheet.elsewhere;
        if listed == 0 {
            return Err(Error::PlaylistFileEmpty);
        }

        let tracks = sheet
            .locations
            .iter()
            .take(ROWS_AT_MOST)
            .map(|listed| match listed.location.as_path() {
                Some(path) => held_at(&self.catalog.0.lock(), path),
                None => Ok(None),
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if tracks.is_empty() {
            return Err(Error::NoLocalMatches);
        }

        let matched = tracks.len() as u32;
        let wanted = sheet
            .declared
            .unwrap_or_else(|| stem_of(&file_name).to_owned());
        let (id, name) = self.make_playlist_named_free(&wanted, tracks)?;

        Ok(ImportedPlaylistFile {
            id,
            name,
            matched,
            missing: listed.min(ROWS_AT_MOST) as u32 - matched,
        })
    }

    pub fn export_playlist(&self, id: u64, format: PlaylistFileFormat) -> Result<String> {
        let connection = self.catalog.0.lock();
        let name: String = connection
            .query_row(
                "SELECT name FROM playlists WHERE id = ?1",
                [id as i64],
                |row| row.get(0),
            )
            .map_err(|source| match source {
                rusqlite::Error::QueryReturnedNoRows => Error::UnknownPlaylist,
                source => Error::store(StoreOp::Read, source),
            })?;
        let mut query = connection
            .prepare("SELECT d.uri, d.title, d.artist, d.duration_ms FROM playlist_rows p JOIN documents d ON d.id = p.track WHERE p.playlist = ?1 AND d.missing = 0 ORDER BY p.position")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = query
            .query_map([id as i64], |row| {
                let artist: String = row.get(2)?;
                let milliseconds: i64 = row.get(3)?;
                Ok((
                    row.get::<_, String>(0)?,
                    Described {
                        seconds: (milliseconds > 0).then_some(milliseconds as u64 / 1000),
                        artist: (!artist.is_empty()).then_some(artist),
                        title: row.get(1)?,
                    },
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        let files = rows
            .iter()
            .filter_map(|(uri, _)| path_in_folder(uri))
            .collect::<Vec<_>>();
        let written = rows
            .into_iter()
            .zip(&files)
            .map(|((_, described), file)| SheetRow {
                file,
                described,
                timed: None,
            })
            .collect::<Vec<_>>();

        written_sheet(format.library(), &name, &written, Path::new(FOLDER_ROOT)).map_err(|source| {
            Error::PlaylistFileUnwritten {
                source: Box::new(source),
            }
        })
    }
}

impl MobilePlayer {
    fn make_playlist_named_free(&self, wanted: &str, tracks: Vec<u64>) -> Result<(u64, String)> {
        let mut attempt = 1;
        loop {
            let name = if attempt == 1 {
                wanted.trim().to_owned()
            } else {
                format!("{} ({attempt})", wanted.trim())
            };
            match self.make_playlist(name.clone(), tracks.clone()) {
                Err(Error::DuplicatePlaylistName) if attempt < NAMES_TRIED => attempt += 1,
                made => return made.map(|id| (id, name)),
            }
        }
    }
}

fn held_at(connection: &Connection, listed: &Path) -> Result<Option<u64>> {
    let Some(name) = listed.file_name().and_then(|name| name.to_str()) else {
        return Ok(None);
    };
    let mut query = connection
        .prepare(HELD_AT)
        .map_err(|source| Error::store(StoreOp::Read, source))?;
    let candidates = query
        .query_map(params![name], |row| {
            Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?))
        })
        .map_err(|source| Error::store(StoreOp::Read, source))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|source| Error::store(StoreOp::Read, source))?;
    if let [(id, _)] = candidates.as_slice() {
        return Ok(Some(*id));
    }

    let listed = folded_components(
        listed
            .components()
            .filter_map(|part| part.as_os_str().to_str()),
    );
    let mut shared = candidates
        .into_iter()
        .map(|(id, uri)| (shared_tail(&listed, &components_of_uri(&uri)), id))
        .collect::<Vec<_>>();
    shared.sort_by(|one, other| other.0.cmp(&one.0).then(one.1.cmp(&other.1)));

    Ok(match shared.as_slice() {
        [(best, id), (next, _), ..] if best > next => Some(*id),
        [(_, id)] => Some(*id),
        _ => None,
    })
}

fn shared_tail(listed: &[String], held: &[String]) -> usize {
    listed
        .iter()
        .rev()
        .zip(held.iter().rev())
        .take_while(|(listed, held)| listed == held)
        .count()
}

fn folded_components<'a>(parts: impl Iterator<Item = &'a str>) -> Vec<String> {
    parts
        .filter(|part| *part != FOLDER_ROOT)
        .map(folded_letters)
        .collect()
}

fn components_of_uri(uri: &str) -> Vec<String> {
    match MediaLocation::from_uri(uri)
        .as_ref()
        .map(MediaLocation::locator)
    {
        Some(Locator::Key(key)) => {
            folded_components(key.split('/').filter(|part| !part.is_empty()))
        }
        _ => Vec::new(),
    }
}

fn path_in_folder(uri: &str) -> Option<PathBuf> {
    let location = MediaLocation::from_uri(uri)?;
    let Locator::Key(key) = location.locator() else {
        return None;
    };
    let mut parts = key.split('/').filter(|part| !part.is_empty());
    parts.next()?;
    let mut path = PathBuf::from(FOLDER_ROOT);
    path.extend(parts);
    (path != Path::new(FOLDER_ROOT)).then_some(path)
}

fn stem_of(file_name: &str) -> &str {
    Path::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Track, playlists::tests::player};

    fn song(player: &MobilePlayer, uri: &str, title: &str, artist: &str) -> u64 {
        player
            .catalog
            .upsert(Track {
                uri: uri.into(),
                name: uri
                    .rsplit('/')
                    .next()
                    .expect("a name")
                    .replace("%20", " ")
                    .replace("%27", "'"),
                title: title.into(),
                artist: artist.into(),
                duration_ms: 215_000,
                ..Track::default()
            })
            .expect("a song")
            .id
    }

    #[test]
    fn the_ios_playlist_type_declares_what_an_import_reads() {
        let plist = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ios/Resonate/Info.plist"),
        )
        .expect("the iOS Info.plist");
        let (_, declared) = plist
            .split_once(
                "<string>org.resonate.playlist</string>\n\t\t\t<key>UTTypeDescription</key>",
            )
            .expect("the playlist type");
        let (_, declared) = declared
            .split_once("<key>public.filename-extension</key>")
            .expect("the declared extensions");
        let (listed, _) = declared.split_once("</array>").expect("the extension list");
        let extensions = listed
            .split("<string>")
            .skip(1)
            .filter_map(|entry| entry.split_once("</string>"))
            .map(|(extension, _)| extension.to_owned())
            .collect::<Vec<_>>();

        assert_eq!(extensions, playlist_file_extensions());
    }

    #[test]
    fn a_playlist_written_in_each_format_reads_back_onto_the_same_songs() {
        let player = player();
        let one = song(
            &player,
            "resonate-folder://local/Air/Moon%20Safari/01%20La%20femme%20d%27argent.flac",
            "La femme d'argent",
            "Air",
        );
        let two = song(
            &player,
            "resonate-folder://local/Air/Moon%20Safari/02%20Sexy%20Boy.flac",
            "Sexy Boy",
            "Air",
        );
        let id = player.create_playlist("Evening".into()).expect("a list");
        player
            .add_to_playlist(id, vec![two, one, two])
            .expect("three rows");

        for format in [
            PlaylistFileFormat::M3u,
            PlaylistFileFormat::Pls,
            PlaylistFileFormat::Xspf,
        ] {
            let text = player.export_playlist(id, format).expect("a written list");
            assert!(text.contains("Safari"), "{text}");

            let imported = player
                .import_playlist_file(text.into_bytes(), "Evening.file".into())
                .expect("the list read back");
            let rows = player
                .playlist_tracks(imported.id, 0, 100)
                .expect("the rows")
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>();

            assert_eq!((imported.matched, imported.missing), (3, 0));
            assert_eq!(rows, vec![two, one, two]);
        }
    }

    #[test]
    fn a_file_name_held_twice_is_matched_by_its_folders_and_left_alone_when_they_tie() {
        let player = player();
        let first = song(
            &player,
            "resonate-folder://local/Air/Moon%20Safari/01%20Intro.flac",
            "Intro",
            "Air",
        );
        let second = song(
            &player,
            "resonate-folder://local/Beck/Odelay/01%20Intro.flac",
            "Intro",
            "Beck",
        );
        let sheet = "#EXTM3U\nAir/Moon Safari/01 Intro.flac\n01 Intro.flac\nGone/Elsewhere.flac\nhttp://radio.example/stream\n";

        let imported = player
            .import_playlist_file(sheet.as_bytes().to_vec(), "Mixed.m3u8".into())
            .expect("the list");
        let rows = player
            .playlist_tracks(imported.id, 0, 100)
            .expect("the rows")
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>();

        assert_eq!(imported.name, "Mixed");
        assert_eq!(rows, vec![first]);
        assert_eq!(imported.missing, 3);
        assert_ne!(first, second);
    }

    #[test]
    fn a_taken_name_gets_a_number_and_a_list_of_strangers_is_refused() {
        let player = player();
        song(&player, "resonate-folder://local/a.flac", "A", "X");
        player.create_playlist("Mix".into()).expect("a list");
        let sheet = b"#EXTM3U\n#PLAYLIST:Mix\na.flac\n".to_vec();

        let imported = player
            .import_playlist_file(sheet, "whatever.m3u".into())
            .expect("a numbered list");

        assert_eq!(imported.name, "Mix (2)");
        assert!(matches!(
            player.import_playlist_file(b"#EXTM3U\nnothing.flac\n".to_vec(), "x.m3u".into()),
            Err(Error::NoLocalMatches)
        ));
        assert!(matches!(
            player.import_playlist_file(b"#EXTM3U\n".to_vec(), "x.m3u".into()),
            Err(Error::PlaylistFileEmpty)
        ));
    }
}
