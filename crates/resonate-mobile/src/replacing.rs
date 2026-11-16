use std::collections::{BTreeMap, BTreeSet};

use resonate_core::Mbid;
use resonate_library::TheSong;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::{Catalog, read},
    catalog_extras::unreferenced_picture_dropped,
    collections::wanted_key,
    deleting::{SongsForgotten, picture_of},
    documents::location,
    features::SearchSong,
    search::codec_named,
    vanished::{handed_over, referring_tables},
    versions::{SongVersion, artist_of, heard_of, quality_of},
};

const HELD_WEIGHED_AT_MOST: u32 = 64;

const THE_PLAYLISTS_SONGS: &str =
    "SELECT d.* FROM playlist_rows r JOIN documents d ON d.id = r.track
     WHERE r.playlist = ?1 AND d.missing = 0 ORDER BY r.position";

const SHARING_THE_RECORDING: &str = "SELECT * FROM documents
     WHERE recording = ?1 AND id != ?2 AND missing = 0 ORDER BY id";

const WHAT_A_WANT_NEEDS: &str = "SELECT isrc, release_group FROM documents WHERE id = ?1";

const WANTED_FOR_A_REPLACEMENT: &str = "INSERT INTO wanted(identity, recording, title, artist, album, duration_ms, isrcs, release, release_group, artist_id, disc, position)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
     ON CONFLICT(identity) DO NOTHING";

const THE_WANT_NAMED: &str = "SELECT id FROM wanted WHERE identity = ?1";

const THE_WANT_REARMED: &str =
    "UPDATE wanted SET tried = NULL, misses = 0, offered_at = NULL WHERE id = ?1";

const THE_REPLACEMENT_WAITS: &str = "INSERT INTO replacements (track, want, asked)
     VALUES (?1, ?2, unixepoch())
     ON CONFLICT (track) DO UPDATE SET want = excluded.want, asked = excluded.asked";

const PENDING: &str = "SELECT EXISTS(SELECT 1 FROM replacements WHERE track = ?1)";

const A_REPLACEMENT_WANTS: &str = "SELECT EXISTS(SELECT 1 FROM replacements r
     JOIN wanted w ON w.id = r.want WHERE w.identity = ?1)";

const A_WANT_REPLACES: &str = "SELECT EXISTS(SELECT 1 FROM replacements WHERE want = ?1)";

const THE_REPLACEMENTS_WANT: &str = "SELECT want FROM replacements WHERE track = ?1";

const THE_REPLACEMENT_GOES: &str = "DELETE FROM replacements WHERE track = ?1";

const THE_WANT_GOES_WITH_ITS_LAST_REPLACEMENT: &str = "DELETE FROM wanted
     WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM replacements WHERE want = ?1)";

const THE_CODECS_OF_THE_RECORDING: &str =
    "SELECT codec FROM documents WHERE recording = ?1 AND missing = 0";

#[derive(Clone, Debug, uniffi::Record)]
pub struct LossySong {
    pub song: Track,
    pub lossless: Option<Track>,
    pub wantable: bool,
    pub pending: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LossyReplacement {
    pub lossy: Track,
    pub lossless: Track,
}

#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct LossyReplacing {
    pub held: Vec<LossyReplacement>,
    pub wanted: u32,
    pub unmatched: u32,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn lossy_songs_of(&self, playlist: u64) -> Result<Vec<LossySong>> {
        let mut found = Vec::new();
        for song in self.catalog.lossy_songs_listed_in(playlist)? {
            let lossless = self.lossless_copy_held_for(&song)?;
            found.push(LossySong {
                wantable: recording_of(&song).is_some(),
                pending: self.catalog.replacement_pending(song.id)?,
                lossless,
                song,
            });
        }
        Ok(found)
    }

    pub fn replace_lossy_songs(&self, ids: Vec<u64>) -> Result<LossyReplacing> {
        let mut replacing = LossyReplacing::default();
        let mut weighed = BTreeSet::new();
        for id in ids {
            if !weighed.insert(id) {
                continue;
            }
            let Ok(song) = self.catalog.track(id) else {
                continue;
            };
            if is_lossless(&song) {
                continue;
            }
            if let Some(lossless) = self.lossless_copy_held_for(&song)? {
                replacing.held.push(LossyReplacement {
                    lossy: song,
                    lossless,
                });
            } else if self.catalog.wanted_in_place_of(&song)? {
                replacing.wanted += 1;
            } else {
                replacing.unmatched += 1;
            }
        }
        Ok(replacing)
    }

    pub fn fold_lossy_songs(&self, replaced: Vec<LossyReplacement>) -> Result<SongsForgotten> {
        let pairs = replaced
            .iter()
            .map(|replacement| (replacement.lossy.id, replacement.lossless.id))
            .filter(|(lossy, lossless)| lossy != lossless)
            .collect::<BTreeMap<_, _>>();
        let foldable = |lossy: u64, lossless: u64| {
            let kept = self.catalog.track(lossless).ok()?;
            let taken = self.catalog.track(lossy).ok()?;
            (is_lossless(&kept) && !is_lossless(&taken)).then_some(taken.uri)
        };
        let gone = pairs
            .iter()
            .filter_map(|(lossy, lossless)| foldable(*lossy, *lossless))
            .collect();
        let left_the_queue = self.drop_from_queue(&gone);

        let folded = self.catalog.replaced(&pairs)?;
        self.forget_analyses(pairs.keys());
        Ok(SongsForgotten {
            forgotten: counted(folded),
            left_the_queue: counted(left_the_queue),
        })
    }
}

impl MobilePlayer {
    fn lossless_copy_held_for(&self, lossy: &Track) -> Result<Option<Track>> {
        let song = TheSong::new(heard_of(lossy));
        let words = TheSong::words(&lossy.title, artist_of(lossy));
        let sharing = self.catalog.sharing_the_recording(lossy)?;
        let sharing_ids = sharing.iter().map(|copy| copy.id).collect::<BTreeSet<_>>();
        let named_alike = self.services.searching.ranked_tracks(
            &self.catalog,
            &words,
            0,
            HELD_WEIGHED_AT_MOST,
        )?;

        let mut offered: Vec<Track> = Vec::new();
        for copy in sharing.into_iter().chain(named_alike) {
            let fits = copy.id != lossy.id
                && is_lossless(&copy)
                && !offered.iter().any(|held| held.id == copy.id)
                && (sharing_ids.contains(&copy.id) || song.is_bettered_by(heard_of(&copy)));
            if fits {
                offered.push(copy);
            }
        }
        offered.sort_by_key(|copy| quality_of(copy).best_first());

        let chosen = match self.version_chosen(lossy.id)? {
            Some(SongVersion::Held { id }) => offered.iter().position(|copy| copy.id == id),
            Some(SongVersion::Served { .. }) | None => None,
        };
        Ok((!offered.is_empty()).then(|| offered.swap_remove(chosen.unwrap_or(0))))
    }

    pub(crate) fn refuse_a_lossy_delivery(&self, uri: &str, song: &SearchSong) -> Result<()> {
        if !self.catalog.replaces_with(song)? {
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
    fn lossy_songs_listed_in(&self, playlist: u64) -> Result<Vec<Track>> {
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
            .filter(|song| !is_lossless(song) && seen.insert(song.id))
            .collect())
    }

    fn sharing_the_recording(&self, lossy: &Track) -> Result<Vec<Track>> {
        let Some(recording) = recording_of(lossy) else {
            return Ok(Vec::new());
        };
        let connection = self.0.lock();
        let mut query = connection
            .prepare(SHARING_THE_RECORDING)
            .map_err(read_error)?;
        query
            .query_map(params![recording.to_string(), lossy.id as i64], read)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    pub(crate) fn replacement_pending(&self, track: u64) -> Result<bool> {
        self.0
            .lock()
            .query_row(PENDING, [track as i64], |row| row.get(0))
            .map_err(read_error)
    }

    pub(crate) fn wanted_in_place_of(&self, lossy: &Track) -> Result<bool> {
        let mut connection = self.0.lock();
        let Some(song) = wanted_for(&connection, lossy)? else {
            return Ok(false);
        };
        let identity = wanted_key(&song);

        let transaction = connection.transaction().map_err(write_error)?;
        transaction
            .execute(
                WANTED_FOR_A_REPLACEMENT,
                params![
                    identity,
                    song.recording,
                    song.title,
                    song.artist,
                    song.album,
                    song.duration_ms
                        .map(|length| length.min(i64::MAX as u64) as i64),
                    song.isrcs.join("\n"),
                    song.release,
                    song.release_group,
                    song.artist_id,
                    song.disc,
                    song.position
                ],
            )
            .map_err(write_error)?;
        let want: i64 = transaction
            .query_row(THE_WANT_NAMED, [&identity], |row| row.get(0))
            .map_err(read_error)?;
        transaction
            .execute(THE_WANT_REARMED, [want])
            .map_err(write_error)?;
        transaction
            .execute(THE_REPLACEMENT_WAITS, params![lossy.id as i64, want])
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)?;
        Ok(true)
    }

    fn replaces_with(&self, song: &SearchSong) -> Result<bool> {
        self.0
            .lock()
            .query_row(A_REPLACEMENT_WANTS, [wanted_key(song)], |row| row.get(0))
            .map_err(read_error)
    }

    fn replaced(&self, pairs: &BTreeMap<u64, u64>) -> Result<usize> {
        if pairs.is_empty() {
            return Ok(0);
        }
        let mut connection = self.0.lock();
        let referring = referring_tables(&connection)?;
        let transaction = connection.transaction().map_err(write_error)?;
        let mut folded = 0;
        for (lossy, lossless) in pairs {
            let (lossy, lossless) = (*lossy as i64, *lossless as i64);
            if !may_fold(&transaction, lossy, lossless)? {
                continue;
            }
            let want: Option<i64> = transaction
                .query_row(THE_REPLACEMENTS_WANT, [lossy], |row| row.get(0))
                .optional()
                .map_err(read_error)?;
            transaction
                .execute(THE_REPLACEMENT_GOES, [lossy])
                .map_err(write_error)?;
            if let Some(want) = want {
                transaction
                    .execute(THE_WANT_GOES_WITH_ITS_LAST_REPLACEMENT, [want])
                    .map_err(write_error)?;
            }
            let picture = picture_of(&transaction, lossy).map_err(read_error)?;
            handed_over(&transaction, &referring, lossy, lossless).map_err(write_error)?;
            if let Some(picture) = picture {
                unreferenced_picture_dropped(&transaction, picture).map_err(write_error)?;
            }
            folded += 1;
        }
        transaction.commit().map_err(write_error)?;
        Ok(folded)
    }
}

pub(crate) fn replaces(connection: &Connection, want: u64) -> Result<bool> {
    connection
        .query_row(A_WANT_REPLACES, [want as i64], |row| row.get(0))
        .map_err(read_error)
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

fn may_fold(transaction: &Transaction<'_>, lossy: i64, lossless: i64) -> Result<bool> {
    let codec_of = |id: i64| {
        transaction
            .query_row("SELECT codec FROM documents WHERE id = ?1", [id], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(read_error)
    };
    Ok(match (codec_of(lossy)?, codec_of(lossless)?) {
        (Some(taken), Some(kept)) => {
            !codec_named(&taken).is_lossless() && codec_named(&kept).is_lossless()
        }
        _ => false,
    })
}

fn wanted_for(connection: &Connection, lossy: &Track) -> Result<Option<SearchSong>> {
    let Some(recording) = recording_of(lossy) else {
        return Ok(None);
    };
    let (isrc, release_group) = connection
        .query_row(WHAT_A_WANT_NEEDS, [lossy.id as i64], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })
        .map_err(read_error)?;
    let identified = |id: Option<&str>| {
        id.and_then(|id| Mbid::new(id).ok())
            .map(|id| id.to_string())
            .unwrap_or_default()
    };
    Ok(Some(SearchSong {
        recording: recording.to_string(),
        title: lossy.title.clone(),
        artist: lossy.artist.clone(),
        album: lossy.album.clone(),
        duration_ms: (lossy.duration_ms > 0).then_some(lossy.duration_ms),
        isrcs: isrc
            .and_then(|code| resonate_core::Isrc::new(&code).ok())
            .map(|code| code.to_string())
            .into_iter()
            .collect(),
        release: identified(lossy.release.as_deref()),
        release_group: identified(release_group.as_deref()),
        artist_id: identified(lossy.artist_mbid.as_deref()),
        disc: lossy.disc_number,
        position: lossy.track_number,
        held: false,
    }))
}

fn recording_of(song: &Track) -> Option<Mbid> {
    song.recording
        .as_deref()
        .and_then(|recording| Mbid::new(recording).ok())
}

fn is_lossless(song: &Track) -> bool {
    codec_named(&song.codec).is_lossless()
}

fn counted(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path, process};

    use rusqlite::functions::FunctionFlags;

    use super::*;
    use crate::{
        catalog::{LOSSY_SONGS_WAIT_TO_BE_REPLACED, MIGRATIONS},
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
        alarm: Track,
        held: Track,
    }

    fn indexed(catalog: &Catalog, name: &str, codec: &str, recording: Option<&str>) -> Track {
        let (title, extension) = name.split_once('.').expect("a name with an extension");
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
        assert!(!extension.is_empty());
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET recording = ?2 WHERE id = ?1",
                params![track.id as i64, recording],
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
        let alarm = indexed(catalog, "Alarm Call.ogg", "Vorbis", None);
        let held = indexed(catalog, "Bachelorette.flac", "FLAC", None);
        let playlist = harness
            .player
            .make_playlist(
                "Homogenic".into(),
                vec![joga.id, hunter.id, joga.id, held.id, alarm.id],
            )
            .expect("a playlist");
        Shelf {
            harness,
            playlist,
            joga,
            joga_lossless,
            hunter,
            alarm,
            held,
        }
    }

    fn listed(found: &[LossySong]) -> Vec<(u64, Option<u64>, bool, bool)> {
        found
            .iter()
            .map(|lossy| {
                (
                    lossy.song.id,
                    lossy.lossless.as_ref().map(|copy| copy.id),
                    lossy.wantable,
                    lossy.pending,
                )
            })
            .collect()
    }

    #[test]
    fn a_playlists_lossy_songs_are_listed_once_each_with_the_lossless_copy_held() {
        let shelf = shelf();

        let found = shelf
            .harness
            .player
            .lossy_songs_of(shelf.playlist)
            .expect("the lossy songs");

        assert_eq!(
            listed(&found),
            [
                (shelf.joga.id, Some(shelf.joga_lossless.id), true, false),
                (shelf.hunter.id, None, true, false),
                (shelf.alarm.id, None, false, false),
            ]
        );
    }

    #[test]
    fn the_songs_chosen_are_folded_into_the_copy_held_or_wanted_and_the_rest_are_left() {
        let shelf = shelf();
        let player = &shelf.harness.player;

        let replacing = player
            .replace_lossy_songs(vec![shelf.joga.id, shelf.hunter.id, shelf.held.id])
            .expect("replaced");

        assert_eq!(
            replacing
                .held
                .iter()
                .map(|held| (held.lossy.id, held.lossless.id))
                .collect::<Vec<_>>(),
            [(shelf.joga.id, shelf.joga_lossless.id)]
        );
        assert_eq!((replacing.wanted, replacing.unmatched), (1, 0));

        let wants = player.wanted_songs(0, 10).expect("the wants");

        assert_eq!(wants.len(), 1);
        assert_eq!(wants[0].song.recording, HUNTER);
        assert!(!wants[0].held);
        assert_eq!(
            player
                .poll_wanted_due()
                .expect("the wants due")
                .iter()
                .map(|wanted| wanted.song.title.as_str())
                .collect::<Vec<_>>(),
            ["Hunter"]
        );

        let told = player
            .fold_lossy_songs(replacing.held)
            .expect("the copy folded");
        let rows = player
            .playlist_tracks(shelf.playlist, 0, 10)
            .expect("the playlist");

        assert_eq!(told.forgotten, 1);
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [
                shelf.joga_lossless.id,
                shelf.hunter.id,
                shelf.joga_lossless.id,
                shelf.held.id,
                shelf.alarm.id
            ]
        );
        assert!(player.catalog.track(shelf.joga.id).is_err());
        assert_eq!(
            listed(&player.lossy_songs_of(shelf.playlist).expect("the rest")),
            [
                (shelf.hunter.id, None, true, true),
                (shelf.alarm.id, None, false, false),
            ]
        );
    }

    #[test]
    fn a_wanted_replacement_is_held_only_once_a_lossless_copy_lands_and_goes_once_folded() {
        let shelf = shelf();
        let player = &shelf.harness.player;
        player
            .replace_lossy_songs(vec![shelf.hunter.id])
            .expect("wanted");

        let landed = indexed(&player.catalog, "Hunter.flac", "FLAC", Some(HUNTER));
        let wants = player.wanted_songs(0, 10).expect("the wants");

        assert!(wants[0].held);
        assert!(player.poll_wanted_due().expect("nothing due").is_empty());

        let replacing = player
            .replace_lossy_songs(vec![shelf.hunter.id])
            .expect("the copy held");
        let told = player
            .fold_lossy_songs(replacing.held)
            .expect("the copy folded");

        assert_eq!(told.forgotten, 1);
        assert!(player.wanted_songs(0, 10).expect("the wants").is_empty());
        assert!(
            player
                .playlist_tracks(shelf.playlist, 0, 10)
                .expect("the playlist")
                .iter()
                .any(|row| row.id == landed.id)
        );
    }

    #[test]
    fn dismissing_a_replacements_want_cancels_it_and_leaves_the_lossy_song() {
        let shelf = shelf();
        let player = &shelf.harness.player;
        player
            .replace_lossy_songs(vec![shelf.hunter.id])
            .expect("wanted");
        let want = player.wanted_songs(0, 10).expect("the wants")[0].id;

        player.remove_wanted(want).expect("dismissed");

        assert!(
            !player
                .catalog
                .replacement_pending(shelf.hunter.id)
                .expect("an answer")
        );
        assert!(player.catalog.track(shelf.hunter.id).is_ok());
    }

    #[test]
    fn a_fold_is_refused_where_the_kept_copy_is_lossy_or_the_taken_one_lossless() {
        let shelf = shelf();
        let player = &shelf.harness.player;

        let told = player
            .fold_lossy_songs(vec![
                LossyReplacement {
                    lossy: shelf.joga.clone(),
                    lossless: shelf.hunter.clone(),
                },
                LossyReplacement {
                    lossy: shelf.held.clone(),
                    lossless: shelf.joga_lossless.clone(),
                },
            ])
            .expect("nothing folded");

        assert_eq!(told.forgotten, 0);
        assert!(player.catalog.track(shelf.joga.id).is_ok());
        assert!(player.catalog.track(shelf.held.id).is_ok());
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
    fn a_lossy_delivery_is_refused_only_where_it_answers_a_replacement() {
        let harness = Harness::new();
        let player = &harness.player;
        let lossy = indexed(&player.catalog, "a.mp3", "MP3", Some(JOGA));
        let wanted = wanted_for(&player.catalog.0.lock(), &lossy)
            .expect("read")
            .expect("a want");
        fs::write(harness.folder.join("adpcm.wav"), adpcm()).expect("a lossy file");
        let lossless = format!("{}a.wav", crate::harness::FOLDER);
        let adpcm = format!("{}adpcm.wav", crate::harness::FOLDER);

        assert!(player.refuse_a_lossy_delivery(&adpcm, &wanted).is_ok());

        assert!(player.catalog.wanted_in_place_of(&lossy).expect("wanted"));

        assert!(player.catalog.replaces_with(&wanted).expect("an answer"));
        assert!(player.refuse_a_lossy_delivery(&lossless, &wanted).is_ok());
        assert!(matches!(
            player.refuse_a_lossy_delivery(&adpcm, &wanted),
            Err(Error::LossyForAReplacement)
        ));
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
    fn a_replacement_waiting_is_kept_across_a_reopen() {
        let path = unique_path("replacing");
        let catalog = Catalog::open(&path).expect("a catalog");
        let lossy = indexed(&catalog, "Hunter.m4a", "AAC", Some(HUNTER));
        let wanted = wanted_for(&catalog.0.lock(), &lossy)
            .expect("read")
            .expect("a want");

        assert!(catalog.wanted_in_place_of(&lossy).expect("wanted"));
        drop(catalog);

        let reopened = Catalog::open(&path).expect("reopened");

        assert!(reopened.replacement_pending(lossy.id).expect("an answer"));
        assert!(reopened.replaces_with(&wanted).expect("an answer"));
        drop(reopened);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn a_catalog_of_before_lossy_songs_were_replaced_keeps_its_wants_and_gains_the_table() {
        let path = unique_path("before-replacing");
        let step = || {
            MIGRATIONS
                .iter()
                .position(|laid| *laid == LOSSY_SONGS_WAIT_TO_BE_REPLACED)
                .expect("a step of the chain")
        };
        let connection = rusqlite::Connection::open(&path).expect("a database");
        connection
            .create_scalar_function(
                "folded",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
            )
            .expect("the fold");
        for laid in &MIGRATIONS[..step()] {
            connection.execute_batch(laid).expect("an earlier version");
        }
        connection
            .execute_batch(
                "INSERT INTO documents (id, uri, name, title, artist, album, duration_ms, rate, codec)
                     VALUES (4, 'content://a/4', 'Hunter.m4a', 'Hunter', 'Björk', 'Homogenic', 1000, 44100, 'AAC');
                 INSERT INTO wanted (id, identity, recording, title, artist, album, isrcs)
                     VALUES (9, 'recording:x', 'x', 'Hunter', 'Björk', 'Homogenic', '');",
            )
            .expect("a catalog of before");
        connection
            .pragma_update(
                None,
                "user_version",
                u32::try_from(step()).expect("a count"),
            )
            .expect("its stamp");
        drop(connection);

        let opened = Catalog::open(Path::new(&path)).expect("migrated");
        let connection = opened.0.lock();
        let count = |table: &str| -> i64 {
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("the table")
        };

        assert_eq!(
            (count("documents"), count("wanted"), count("replacements")),
            (1, 1, 0)
        );

        connection
            .execute(
                "INSERT INTO replacements (track, want, asked) VALUES (4, 9, 0)",
                [],
            )
            .expect("a replacement");
        connection
            .execute("DELETE FROM wanted WHERE id = 9", [])
            .expect("the want dismissed");

        assert_eq!(count("replacements"), 0);
        drop(connection);
        drop(opened);
        fs::remove_file(&path).ok();
    }
}
