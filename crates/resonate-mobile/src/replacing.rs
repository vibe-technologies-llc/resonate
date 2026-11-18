use std::collections::BTreeSet;

use resonate_core::Mbid;
use resonate_library::TheSong;
use rusqlite::{Connection, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::{Catalog, read},
    collections::wanted_key,
    documents::location,
    features::SearchSong,
    search::codec_named,
    versions::{
        FoundVersion, HELD_WEIGHED_AT_MOST, NAMES_APART, SongVersion, VersionSearch, artist_of,
        heard_of, held_label, quality_of,
    },
    words,
};

const THE_PLAYLISTS_SONGS: &str =
    "SELECT d.* FROM playlist_rows r JOIN documents d ON d.id = r.track
     WHERE r.playlist = ?1 AND d.missing = 0 ORDER BY r.position";

const SHARING_THE_RECORDING: &str = "SELECT * FROM documents
     WHERE recording = ?1 AND id != ?2 AND missing = 0 ORDER BY id";

const A_LOSSLESS_COPY_IS_WANTED: &str = "SELECT EXISTS(SELECT 1 FROM versions_wanted v
     JOIN wanted w ON w.id = v.want WHERE w.identity = ?1 AND w.served IS NULL)";

const THE_CODECS_OF_THE_RECORDING: &str =
    "SELECT codec FROM documents WHERE recording = ?1 AND missing = 0";

#[derive(Clone, Debug, uniffi::Record)]
pub struct HigherVersion {
    pub song: Track,
    pub found: Option<FoundVersion>,
    pub chosen: bool,
    pub pending: bool,
}

#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct HigherVersions {
    pub songs: Vec<HigherVersion>,
    pub trouble: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct HigherVersionPicked {
    pub song: u64,
    pub version: SongVersion,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct HigherVersionsKept {
    pub chosen: u32,
    pub downloading: Vec<SearchSong>,
    pub passed_over: u32,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn higher_versions_of(&self, playlist: u64) -> Result<HigherVersions> {
        let services = self.services_to_ask();
        let mut refused = BTreeSet::new();
        let mut songs = Vec::new();
        for song in self.catalog.songs_listed_in(playlist)? {
            let pending = self.catalog.a_version_waits_for(song.id)?;
            let found = match self.best_copy_held_above(&song)? {
                Some(copy) => Some(FoundVersion {
                    label: held_label(VersionSearch::Better, &copy),
                    version: SongVersion::Held { id: copy.id },
                }),
                None if is_lossless(&song) || pending => None,
                None => {
                    let words = TheSong::words(&song.title, artist_of(&song));
                    let heard = TheSong::new(heard_of(&song));
                    let asked =
                        self.asked_for_versions(&services, &words, &heard, VersionSearch::Better);
                    refused.extend(asked.refused);
                    asked.found.into_iter().next()
                }
            };
            if found.is_none() && is_lossless(&song) && !pending {
                continue;
            }
            let chosen = match (&found, self.version_chosen(song.id)?) {
                (Some(found), Some(chosen)) => found.version == chosen,
                _ => false,
            };
            songs.push(HigherVersion {
                song,
                found,
                chosen,
                pending,
            });
        }
        let trouble = (!refused.is_empty()).then(|| {
            words::versions::could_not_ask(
                refused.into_iter().collect::<Vec<_>>().join(NAMES_APART),
            )
        });
        Ok(HigherVersions { songs, trouble })
    }

    pub fn keep_higher_versions(
        &self,
        picked: Vec<HigherVersionPicked>,
    ) -> Result<HigherVersionsKept> {
        let mut kept = HigherVersionsKept::default();
        let mut weighed = BTreeSet::new();
        for picked in picked {
            if !weighed.insert(picked.song) {
                continue;
            }
            let Ok(song) = self.catalog.track(picked.song) else {
                kept.passed_over += 1;
                continue;
            };
            match picked.version {
                SongVersion::Held { id } => {
                    let above = self
                        .catalog
                        .track(id)
                        .is_ok_and(|copy| quality_of(&copy).is_above(quality_of(&song)));
                    match above && id != song.id {
                        true => {
                            self.catalog.keep_a_variant(song.id, id)?;
                            kept.chosen += 1;
                        }
                        false => kept.passed_over += 1,
                    }
                }
                SongVersion::Served { uri } => match self.served_version_wanted(&song, &uri) {
                    Ok(wanted) => {
                        self.catalog.want_a_version(song.id, &wanted)?;
                        kept.downloading.push(wanted);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "a higher quality version could not be wanted");
                        kept.passed_over += 1;
                    }
                },
            }
        }
        Ok(kept)
    }
}

impl MobilePlayer {
    fn best_copy_held_above(&self, song: &Track) -> Result<Option<Track>> {
        let heard = TheSong::new(heard_of(song));
        let words = TheSong::words(&song.title, artist_of(song));
        let sharing = self.catalog.sharing_the_recording(song)?;
        let sharing_ids = sharing.iter().map(|copy| copy.id).collect::<BTreeSet<_>>();
        let named_alike = self.services.searching.ranked_tracks(
            &self.catalog,
            &words,
            0,
            HELD_WEIGHED_AT_MOST,
        )?;

        let held = quality_of(song);
        let mut above: Vec<Track> = Vec::new();
        for copy in sharing.into_iter().chain(named_alike) {
            let fits = copy.id != song.id
                && !above.iter().any(|kept| kept.id == copy.id)
                && (sharing_ids.contains(&copy.id) || heard.names(heard_of(&copy)))
                && quality_of(&copy).is_above(held);
            if fits {
                above.push(copy);
            }
        }
        above.sort_by_key(|copy| quality_of(copy).best_first());
        Ok(above.into_iter().next())
    }

    pub(crate) fn refuse_a_lossy_delivery(&self, uri: &str, song: &SearchSong) -> Result<()> {
        if !self.catalog.a_lossless_copy_is_wanted(song)? {
            return Ok(());
        }
        let info = resonate_codec::probe(&self.sources, &location(uri)?)?;
        match resonate_codec::Codec::from_id(info.codec).is_lossless() {
            true => Ok(()),
            false => Err(Error::LossyForAReplacement),
        }
    }
}

impl Catalog {
    fn songs_listed_in(&self, playlist: u64) -> Result<Vec<Track>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(THE_PLAYLISTS_SONGS)
            .map_err(read_error)?;
        let listed = query
            .query_map([playlist as i64], read)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        let mut seen = BTreeSet::new();
        Ok(listed
            .into_iter()
            .filter(|song| seen.insert(song.id))
            .collect())
    }

    fn sharing_the_recording(&self, song: &Track) -> Result<Vec<Track>> {
        let Some(recording) = recording_of(song) else {
            return Ok(Vec::new());
        };
        let connection = self.0.lock();
        let mut query = connection
            .prepare(SHARING_THE_RECORDING)
            .map_err(read_error)?;
        query
            .query_map(params![recording.to_string(), song.id as i64], read)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    fn a_lossless_copy_is_wanted(&self, song: &SearchSong) -> Result<bool> {
        self.0
            .lock()
            .query_row(A_LOSSLESS_COPY_IS_WANTED, [wanted_key(song)], |row| {
                row.get(0)
            })
            .map_err(read_error)
    }
}

pub(crate) fn held_losslessly(connection: &Connection, recording: &str) -> Result<bool> {
    let mut query = connection
        .prepare_cached(THE_CODECS_OF_THE_RECORDING)
        .map_err(read_error)?;
    let codecs = query
        .query_map([recording], |row| row.get::<_, String>(0))
        .map_err(read_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(read_error)?;
    Ok(codecs.iter().any(|codec| codec_named(codec).is_lossless()))
}

fn recording_of(song: &Track) -> Option<Mbid> {
    song.recording
        .as_deref()
        .and_then(|recording| Mbid::new(recording).ok())
}

fn is_lossless(song: &Track) -> bool {
    codec_named(&song.codec).is_lossless()
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path, process};

    use rusqlite::functions::FunctionFlags;

    use super::*;
    use crate::{
        catalog::{A_VERSION_IS_KEPT_BESIDE_ITS_SONG, MIGRATIONS},
        harness::Harness,
    };

    const JOGA: &str = "0a5c6e2b-8b25-4b9a-9d0f-2f3c2f0e7a11";
    const HUNTER: &str = "1b6d7f3c-9c36-4cab-8e10-3f4d3f1f8b22";

    struct Shelf {
        harness: Harness,
        playlist: u64,
        joga: Track,
        joga_lossless: Track,
        hunter: Track,
        bachelorette: Track,
        bachelorette_hires: Track,
    }

    fn indexed(catalog: &Catalog, name: &str, codec: &str, recording: Option<&str>) -> Track {
        indexed_at(catalog, name, codec, recording, None)
    }

    fn indexed_at(
        catalog: &Catalog,
        name: &str,
        codec: &str,
        recording: Option<&str>,
        bits: Option<u32>,
    ) -> Track {
        let title = name.split_once('.').expect("a name with an extension").0;
        let title = title.trim_end_matches(" (hi-res)");
        let track = catalog
            .upsert(Track {
                uri: format!("content://shelf/{name}"),
                name: name.into(),
                title: title.into(),
                artist: "Björk".into(),
                album: "Homogenic".into(),
                duration_ms: 305_000,
                rate: 44_100,
                codec: codec.into(),
                ..Track::default()
            })
            .expect("an indexed song");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET recording = ?2, bits = ?3 WHERE id = ?1",
                params![track.id as i64, recording, bits],
            )
            .expect("its recording");
        catalog.track(track.id).expect("the song")
    }

    fn shelf() -> Shelf {
        let harness = Harness::new();
        let catalog = &harness.player.catalog;
        let joga = indexed(catalog, "Joga.mp3", "MP3", Some(JOGA));
        let joga_lossless = indexed(catalog, "Joga.flac", "FLAC", None);
        let hunter = indexed(catalog, "Hunter.m4a", "AAC", Some(HUNTER));
        let bachelorette = indexed_at(catalog, "Bachelorette.flac", "FLAC", None, Some(16));
        let bachelorette_hires = indexed_at(
            catalog,
            "Bachelorette (hi-res).flac",
            "FLAC",
            None,
            Some(24),
        );
        let alike = indexed_at(catalog, "Alarm Call.flac", "FLAC", None, Some(16));
        let playlist = harness
            .player
            .make_playlist(
                "Homogenic".into(),
                vec![joga.id, hunter.id, joga.id, bachelorette.id, alike.id],
            )
            .expect("a playlist");
        Shelf {
            harness,
            playlist,
            joga,
            joga_lossless,
            hunter,
            bachelorette,
            bachelorette_hires,
        }
    }

    fn listed(found: &HigherVersions) -> Vec<(u64, Option<SongVersion>, bool, bool)> {
        found
            .songs
            .iter()
            .map(|higher| {
                (
                    higher.song.id,
                    higher.found.as_ref().map(|found| found.version.clone()),
                    higher.chosen,
                    higher.pending,
                )
            })
            .collect()
    }

    fn count(catalog: &Catalog, table: &str) -> i64 {
        catalog
            .0
            .lock()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("the table")
    }

    #[test]
    fn a_playlists_songs_are_offered_the_best_copy_held_above_them_once_each() {
        let shelf = shelf();

        let found = shelf
            .harness
            .player
            .higher_versions_of(shelf.playlist)
            .expect("the higher versions");

        assert_eq!(
            listed(&found),
            [
                (
                    shelf.joga.id,
                    Some(SongVersion::Held {
                        id: shelf.joga_lossless.id
                    }),
                    false,
                    false
                ),
                (shelf.hunter.id, None, false, false),
                (
                    shelf.bachelorette.id,
                    Some(SongVersion::Held {
                        id: shelf.bachelorette_hires.id
                    }),
                    false,
                    false
                ),
            ]
        );
        assert_eq!(found.trouble, None);
    }

    #[test]
    fn the_versions_ticked_are_kept_beside_their_songs_and_nothing_is_deleted() {
        let shelf = shelf();
        let player = &shelf.harness.player;
        let documents = count(&player.catalog, "documents");

        let kept = player
            .keep_higher_versions(vec![
                HigherVersionPicked {
                    song: shelf.joga.id,
                    version: SongVersion::Held {
                        id: shelf.joga_lossless.id,
                    },
                },
                HigherVersionPicked {
                    song: shelf.bachelorette.id,
                    version: SongVersion::Held {
                        id: shelf.bachelorette_hires.id,
                    },
                },
                HigherVersionPicked {
                    song: shelf.joga_lossless.id,
                    version: SongVersion::Held { id: shelf.joga.id },
                },
            ])
            .expect("the versions kept");

        assert_eq!(
            (kept.chosen, kept.downloading.len(), kept.passed_over),
            (2, 0, 1)
        );
        assert_eq!(count(&player.catalog, "documents"), documents);
        assert_eq!(
            player.version_chosen(shelf.joga.id).expect("the choice"),
            Some(SongVersion::Held {
                id: shelf.joga_lossless.id
            })
        );
        assert!(
            player
                .higher_versions_of(shelf.playlist)
                .expect("again")
                .songs
                .iter()
                .filter(|higher| higher.found.is_some())
                .all(|higher| higher.chosen)
        );
        assert_eq!(
            player
                .variants_of(shelf.bachelorette.id)
                .expect("the variants")
                .len(),
            2
        );
    }

    #[test]
    fn a_service_version_of_no_service_is_passed_over_and_wants_nothing() {
        let shelf = shelf();
        let player = &shelf.harness.player;

        let kept = player
            .keep_higher_versions(vec![HigherVersionPicked {
                song: shelf.hunter.id,
                version: SongVersion::Served {
                    uri: "tidal:track/7".into(),
                },
            }])
            .expect("nothing kept");

        assert_eq!(kept.passed_over, 1);
        assert_eq!(count(&player.catalog, "wanted"), 0);
    }

    fn adpcm() -> Vec<u8> {
        let block_align = 256_u16;
        let samples_a_block = 505_u16;
        let mut format = Vec::new();
        format.extend_from_slice(&0x11_u16.to_le_bytes());
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&crate::harness::RATE.to_le_bytes());
        format.extend_from_slice(&22_355_u32.to_le_bytes());
        format.extend_from_slice(&block_align.to_le_bytes());
        format.extend_from_slice(&4_u16.to_le_bytes());
        format.extend_from_slice(&2_u16.to_le_bytes());
        format.extend_from_slice(&samples_a_block.to_le_bytes());
        let frames = u32::from(samples_a_block) * 8;
        let data = vec![0_u8; usize::from(block_align) * 8];

        let mut body = b"WAVE".to_vec();
        for (id, payload) in [
            (b"fmt ", format),
            (b"fact", frames.to_le_bytes().to_vec()),
            (b"data", data),
        ] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            body.extend_from_slice(&payload);
        }
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn a_lossy_delivery_is_refused_only_where_it_answers_a_lossless_copy_wanted() {
        let harness = Harness::new();
        let player = &harness.player;
        let lossy = indexed(&player.catalog, "a.mp3", "MP3", Some(JOGA));
        let wanted = SearchSong {
            recording: JOGA.into(),
            title: "a".into(),
            artist: "Björk".into(),
            ..SearchSong::default()
        };
        fs::write(harness.folder.join("adpcm.wav"), adpcm()).expect("a lossy file");
        let lossless = format!("{}a.wav", crate::harness::FOLDER);
        let adpcm = format!("{}adpcm.wav", crate::harness::FOLDER);

        assert!(player.refuse_a_lossy_delivery(&adpcm, &wanted).is_ok());

        player
            .catalog
            .want_a_version(lossy.id, &wanted)
            .expect("wanted");

        assert!(player.refuse_a_lossy_delivery(&lossless, &wanted).is_ok());
        assert!(matches!(
            player.refuse_a_lossy_delivery(&adpcm, &wanted),
            Err(Error::LossyForAReplacement)
        ));

        let served = SearchSong {
            served: Some("tidal:track/7".into()),
            ..wanted
        };
        player
            .catalog
            .want_a_version(lossy.id, &served)
            .expect("a served version wanted");

        assert!(player.refuse_a_lossy_delivery(&adpcm, &served).is_ok());
    }

    fn unique_path(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        env::temp_dir().join(format!(
            "resonate-mobile-{name}-{}-{unique}.sqlite",
            process::id()
        ))
    }

    #[test]
    fn a_version_waiting_is_kept_across_a_reopen() {
        let path = unique_path("versions-waiting");
        let catalog = Catalog::open(&path).expect("a catalog");
        let lossy = indexed(&catalog, "Hunter.m4a", "AAC", Some(HUNTER));
        let wanted = SearchSong {
            title: "Hunter".into(),
            artist: "Björk".into(),
            served: Some("tidal:track/7".into()),
            ..SearchSong::default()
        };

        catalog.want_a_version(lossy.id, &wanted).expect("wanted");
        drop(catalog);

        let reopened = Catalog::open(&path).expect("reopened");

        assert!(reopened.a_version_waits_for(lossy.id).expect("an answer"));
        drop(reopened);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn a_catalog_of_before_versions_were_kept_beside_their_songs_carries_its_choices_and_waits() {
        let path = unique_path("before-versions-held");
        let step = MIGRATIONS
            .iter()
            .position(|laid| *laid == A_VERSION_IS_KEPT_BESIDE_ITS_SONG)
            .expect("a step of the chain");
        let connection = rusqlite::Connection::open(&path).expect("a database");
        connection
            .create_scalar_function(
                "folded",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
            )
            .expect("the fold");
        for laid in &MIGRATIONS[..step] {
            connection.execute_batch(laid).expect("an earlier version");
        }
        connection
            .execute_batch(
                "INSERT INTO documents (id, uri, name, title, artist, album, duration_ms, rate, codec)
                     VALUES (1, 'content://a/1', 'Joga.mp3', 'Joga', 'Björk', 'Homogenic', 1000, 44100, 'MP3'),
                            (2, 'content://a/2', 'Joga.flac', 'Joga', 'Björk', 'Homogenic', 1000, 44100, 'FLAC'),
                            (3, 'content://a/3', 'Hunter.m4a', 'Hunter', 'Björk', 'Homogenic', 1000, 44100, 'AAC'),
                            (4, 'content://a/4', 'Alarm.ogg', 'Alarm Call', 'Björk', 'Homogenic', 1000, 44100, 'Vorbis');
                 INSERT INTO versions_chosen (track, held, served, chosen) VALUES (1, 2, NULL, 5), (4, NULL, 'tidal:track/7', 6);
                 INSERT INTO wanted (id, identity, recording, title, artist, album, isrcs)
                     VALUES (9, 'recording:x', 'x', 'Hunter', 'Björk', 'Homogenic', '');
                 INSERT INTO replacements (track, want, asked) VALUES (3, 9, 7);",
            )
            .expect("a catalog of before");
        connection
            .pragma_update(None, "user_version", u32::try_from(step).expect("a count"))
            .expect("its stamp");
        drop(connection);

        let opened = Catalog::open(Path::new(&path)).expect("migrated");

        assert_eq!(
            (
                count(&opened, "documents"),
                count(&opened, "versions_chosen"),
                count(&opened, "versions_held"),
                count(&opened, "versions_wanted"),
                count(&opened, "replacements"),
                count(&opened, "wanted"),
            ),
            (4, 1, 1, 1, 0, 1)
        );
        assert!(opened.a_version_waits_for(3).expect("a wait"));
        assert!(!opened.a_version_waits_for(4).expect("none"));

        let kept: (i64, i64, i64) = opened
            .0
            .lock()
            .query_row("SELECT track, held, kept FROM versions_held", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .expect("the version kept");

        assert_eq!(kept, (1, 2, 5));

        opened
            .0
            .lock()
            .execute("DELETE FROM documents WHERE id = 2", [])
            .expect("the copy forgotten");

        assert_eq!(count(&opened, "versions_held"), 0);
        assert_eq!(count(&opened, "versions_chosen"), 0);
        drop(opened);
        fs::remove_file(&path).ok();
    }
}
