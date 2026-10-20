use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    path::Component,
};

use resonate_engine::{Command, PlaybackState};
use resonate_library::{DEFAULT_LAYOUT, Layout, Named, Naming};
use rusqlite::{OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::read,
    documents::document_uri,
    vanished::{Vacated, referring_tables, vacated},
};

const FOLDER_ENDS: char = '/';
const EXTENSION_STARTS: char = '.';
const YEAR_DIGITS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FiledSong {
    pub id: u64,
    pub from: String,
    pub to: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct OrganisePlan {
    pub moves: Vec<FiledSong>,
    pub unchanged: u32,
    pub unplaced: u32,
    pub taken: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MovedDocument {
    pub id: u64,
    pub uri: String,
}

#[uniffi::export]
pub fn default_organise_layout() -> String {
    DEFAULT_LAYOUT.to_owned()
}

#[uniffi::export]
pub fn organise_layout_problem(layout: String) -> Option<String> {
    Layout::read(&layout).err().map(|error| error.to_string())
}

#[uniffi::export]
impl MobilePlayer {
    pub fn plan_organise(&self, folder: String, layout: String) -> Result<OrganisePlan> {
        let layout = Layout::read(&layout).map_err(|source| Error::LayoutUnreadable {
            source: Box::new(source),
        })?;
        let within = format!("{folder}{FOLDER_ENDS}");
        let held = self.songs_within(&within)?;
        Ok(planned(&layout, &within, &held))
    }

    pub fn documents_moved(&self, moved: Vec<MovedDocument>) -> Result<()> {
        if moved.is_empty() {
            return Ok(());
        }
        let queued = self.queued_ids()?;
        let requeued = moved.iter().any(|document| queued.contains(&document.id));
        let playing = requeued
            && self
                .player
                .lock()
                .as_ref()
                .is_some_and(|player| player.state().playback == PlaybackState::Playing);
        if requeued {
            self.save()?;
        }

        let handed_over = self.catalog.rekeyed(&moved)?;
        let mut analyses = self.services.analyses.lock();
        for old in handed_over {
            analyses.remove(&old);
        }
        drop(analyses);

        if requeued {
            *self.saved.lock() = None;
            self.resume_saved()?;
            if playing {
                self.command(Command::Play)?;
            }
        }
        Ok(())
    }
}

impl MobilePlayer {
    fn songs_within(&self, within: &str) -> Result<Vec<Track>> {
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare(
                "SELECT * FROM documents WHERE missing = 0 AND substr(uri, 1, length(?1)) = ?1 ORDER BY id",
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        query
            .query_map([within], read)
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn queued_ids(&self) -> Result<BTreeSet<u64>> {
        let held = self.player.lock();
        let Some(player) = held.as_ref() else {
            return Ok(BTreeSet::new());
        };
        Ok(player
            .queued()
            .rows
            .iter()
            .filter_map(|item| document_uri(&item.location).ok())
            .filter_map(|uri| self.catalog.track_uri(&uri).ok())
            .map(|track| track.id)
            .collect())
    }
}

impl crate::catalog::Catalog {
    fn rekeyed(&self, moved: &[MovedDocument]) -> Result<Vec<u64>> {
        let mut connection = self.0.lock();
        let referring = referring_tables(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let mut handed_over = Vec::new();
        for document in moved {
            let holder = transaction
                .query_row(
                    "SELECT id FROM documents WHERE uri = ?2 AND id != ?1",
                    params![document.id as i64, document.uri],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Read, source))?;
            if let Some(holder) = holder
                && let Vacated::HandedOver(old) =
                    vacated(&transaction, &referring, holder, document.id as i64)?
            {
                handed_over.push(old);
            }
            transaction
                .execute(
                    "UPDATE documents SET uri = ?2 WHERE id = ?1",
                    params![document.id as i64, document.uri],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(handed_over)
    }
}

fn planned(layout: &Layout, within: &str, held: &[Track]) -> OrganisePlan {
    let mut discs: BTreeMap<String, u32> = BTreeMap::new();
    for track in held {
        let discs_held = discs.entry(album_of(track)).or_default();
        *discs_held = (*discs_held).max(track.disc_number.unwrap_or(1));
    }

    let mut plan = OrganisePlan::default();
    let standing: BTreeSet<String> = held
        .iter()
        .filter_map(|track| relative(&track.uri, within))
        .collect();
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    for track in held {
        let Some(now) = relative(&track.uri, within) else {
            plan.unplaced += 1;
            continue;
        };
        let Some(to) = destination(
            layout,
            track,
            discs.get(&album_of(track)).copied().unwrap_or(1),
        ) else {
            plan.unplaced += 1;
            continue;
        };
        let joined = to.join("/");
        if joined == now {
            plan.unchanged += 1;
            claimed.insert(joined);
            continue;
        }
        if standing.contains(&joined) || !claimed.insert(joined) {
            plan.taken += 1;
            continue;
        }
        plan.moves.push(FiledSong {
            id: track.id,
            from: track.uri.clone(),
            to,
        });
    }
    plan
}

fn destination(layout: &Layout, track: &Track, discs: u32) -> Option<Vec<String>> {
    let named = Named {
        album_artist: given(&track.album_artist),
        artist: given(&track.artist),
        album: given(&track.album),
        title: &track.title,
        year: year_of(&track.date),
        disc: track.disc_number.and_then(NonZeroU32::new),
        discs,
        track: track.track_number,
        extension: track
            .name
            .rsplit_once(EXTENSION_STARTS)
            .map(|(_, extension)| extension),
    };
    let path = layout.render(&named, Naming::Anything)?;
    path.components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str().map(str::to_owned),
            _ => None,
        })
        .collect()
}

fn given(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()).then_some(text)
}

fn year_of(date: &str) -> Option<i32> {
    date.get(..YEAR_DIGITS)?.parse().ok()
}

fn album_of(track: &Track) -> String {
    format!(
        "{}\u{1f}{}",
        track.album.trim().to_lowercase(),
        given(&track.album_artist)
            .unwrap_or(&track.artist)
            .trim()
            .to_lowercase()
    )
}

fn relative(uri: &str, within: &str) -> Option<String> {
    let held = uri.strip_prefix(within)?;
    Some(
        held.split(FOLDER_ENDS)
            .map(unescaped)
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn unescaped(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut kept = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%')
            .then(|| bytes.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                kept.push(byte);
                at += 3;
            }
            None => {
                kept.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITHIN: &str = "resonate-folder://local/";

    fn song(id: u64, path: &str, title: &str, track_number: u32, disc: Option<u32>) -> Track {
        Track {
            id,
            uri: format!("{WITHIN}{path}"),
            name: path.rsplit('/').next().unwrap_or(path).to_owned(),
            title: title.to_owned(),
            artist: "Pink Floyd".into(),
            album: "The Wall".into(),
            album_artist: "Pink Floyd".into(),
            track_number: Some(track_number),
            disc_number: disc,
            ..Track::default()
        }
    }

    #[test]
    fn a_song_is_filed_under_its_album_artist_and_album_with_its_disc_where_the_album_has_two() {
        let layout = Layout::default();
        let held = [
            song(
                1,
                "Downloads/in%20the%20flesh.flac",
                "In the Flesh?",
                1,
                Some(1),
            ),
            song(
                2,
                "Pink%20Floyd/The%20Wall/2-01%20Hey%20You.flac",
                "Hey You",
                1,
                Some(2),
            ),
        ];

        let plan = planned(&layout, WITHIN, &held);

        assert_eq!(plan.unchanged, 1);
        assert_eq!(
            plan.moves,
            vec![FiledSong {
                id: 1,
                from: held[0].uri.clone(),
                to: vec![
                    "Pink Floyd".into(),
                    "The Wall".into(),
                    "1-01 In the Flesh?.flac".into()
                ],
            }]
        );
    }

    #[test]
    fn a_place_another_song_holds_or_claims_first_is_not_taken() {
        let layout = Layout::default();
        let held = [
            song(1, "a.flac", "Mother", 5, None),
            song(2, "b.flac", "Mother", 5, None),
            song(
                3,
                "Pink%20Floyd/The%20Wall/06%20Goodbye.flac",
                "Goodbye",
                6,
                None,
            ),
            song(4, "c.flac", "Goodbye", 6, None),
        ];

        let plan = planned(&layout, WITHIN, &held);

        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.moves[0].id, 1);
        assert_eq!(plan.taken, 2);
        assert_eq!(plan.unchanged, 1);
    }

    #[test]
    fn a_layout_that_does_not_read_says_why() {
        assert!(organise_layout_problem(default_organise_layout()).is_none());
        assert!(organise_layout_problem("{albumartist}/{nonsense}".into()).is_some());
    }

    fn catalogued(name: &str) -> (std::path::PathBuf, crate::catalog::Catalog) {
        let folder =
            std::env::temp_dir().join(format!("resonate-organise-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        let catalog =
            crate::catalog::Catalog::open(&folder.join("catalog.sqlite")).expect("opened");
        (folder, catalog)
    }

    fn filed(catalog: &crate::catalog::Catalog, path: &str, title: &str) -> Track {
        catalog
            .upsert(Track {
                uri: format!("{WITHIN}{path}"),
                name: path.into(),
                title: title.into(),
                artist: "Pink Floyd".into(),
                album: "The Wall".into(),
                duration_ms: 200_000,
                ..Track::default()
            })
            .expect("upserted")
    }

    #[test]
    fn a_move_onto_the_place_a_hidden_other_song_holds_succeeds_and_keeps_that_song_hidden() {
        let (folder, catalog) = catalogued("other");
        let gone = filed(&catalog, "Pink%20Floyd/The%20Wall/01.flac", "Mother");
        let moving = filed(&catalog, "a.flac", "Goodbye");
        catalog.favourite(gone.id, true).expect("favoured");
        catalog
            .mark_missing(&[gone.id as i64], &[])
            .expect("hidden");

        let handed = catalog
            .rekeyed(&[MovedDocument {
                id: moving.id,
                uri: gone.uri.clone(),
            }])
            .expect("rekeyed");

        assert!(handed.is_empty());
        assert_eq!(catalog.track(moving.id).expect("the song").uri, gone.uri);
        assert!(catalog.track(gone.id).is_ok());
        assert!(
            catalog
                .tracks("", 0, 10)
                .expect("listed")
                .iter()
                .all(|song| song.id != gone.id)
        );

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_move_onto_the_place_the_same_song_held_hidden_takes_over_its_history() {
        let (folder, catalog) = catalogued("same");
        let gone = filed(&catalog, "Pink%20Floyd/The%20Wall/01.flac", "Mother");
        let moving = filed(&catalog, "a.flac", "Mother");
        catalog.favourite(gone.id, true).expect("favoured");
        catalog
            .mark_missing(&[gone.id as i64], &[])
            .expect("hidden");

        let handed = catalog
            .rekeyed(&[MovedDocument {
                id: moving.id,
                uri: gone.uri.clone(),
            }])
            .expect("rekeyed");

        assert_eq!(handed, [gone.id]);
        assert!(catalog.track(gone.id).is_err());
        let song = catalog.track(moving.id).expect("the song");
        assert_eq!(song.uri, gone.uri);
        assert!(song.favourite);

        let _ = std::fs::remove_dir_all(&folder);
    }

    mod through_the_queue {
        use std::fs;

        use super::*;
        use crate::harness::{FOLDER, Harness};

        fn moved(harness: &Harness, name: char, to: &str) -> MovedDocument {
            let song = harness.song(name);
            fs::rename(
                harness.folder.join(format!("{name}.wav")),
                harness.folder.join(to),
            )
            .expect("a file moved");
            MovedDocument {
                id: song.id,
                uri: format!("{FOLDER}{to}"),
            }
        }

        #[test]
        fn a_moved_song_in_the_queue_is_requeued_at_its_row_and_position_under_its_new_name() {
            let harness = Harness::new();
            harness.resumed("abcd", 1, 500);
            let before = harness.playing();
            let id = harness.song('b').id;

            let moving = moved(&harness, 'b', "e.wav");
            harness
                .player
                .documents_moved(vec![moving])
                .expect("documents moved");

            assert_eq!(
                harness.player.catalog.track(id).expect("the song").uri,
                format!("{FOLDER}e.wav")
            );
            let began = std::time::Instant::now();
            while harness.player.queue().is_err() {
                assert!(
                    began.elapsed() < crate::harness::SETTLED_WITHIN,
                    "the queue never named the moved song"
                );
                std::thread::sleep(crate::harness::LOOKED_AGAIN_AFTER);
            }
            assert_eq!(harness.queued(), "abcd");
            assert_eq!(harness.playing(), before);
        }

        #[test]
        fn a_moved_song_the_queue_does_not_hold_leaves_the_queue_as_it_stood() {
            let harness = Harness::new();
            harness.resumed("ab", 1, 500);
            let revision = harness
                .player
                .snapshot()
                .expect("a snapshot")
                .queue_revision;

            let moving = moved(&harness, 'd', "e.wav");
            harness
                .player
                .documents_moved(vec![moving])
                .expect("documents moved");

            assert_eq!(harness.queued(), "ab");
            assert_eq!(
                harness
                    .player
                    .snapshot()
                    .expect("a snapshot")
                    .queue_revision,
                revision
            );
        }

        #[test]
        fn nothing_moved_changes_nothing() {
            let harness = Harness::new();
            harness.resumed("ab", 0, 0);
            let revision = harness
                .player
                .snapshot()
                .expect("a snapshot")
                .queue_revision;

            harness
                .player
                .documents_moved(Vec::new())
                .expect("documents moved");

            assert_eq!(
                harness
                    .player
                    .snapshot()
                    .expect("a snapshot")
                    .queue_revision,
                revision
            );
        }

        #[test]
        fn a_song_taking_the_place_of_its_hidden_twin_lets_the_twins_analysis_go() {
            let harness = Harness::new();
            let (taker, twin) = (harness.song('a').id, harness.song('d').id);
            harness.player.analyse_track(taker).expect("an analysis");
            harness.player.analyse_track(twin).expect("an analysis");
            harness
                .player
                .catalog
                .0
                .lock()
                .execute(
                    "UPDATE documents SET title = 'Same', artist = 'Band' WHERE id IN (?1, ?2)",
                    [taker as i64, twin as i64],
                )
                .expect("twins named alike");
            harness
                .player
                .catalog
                .mark_missing(&[twin as i64], &[])
                .expect("hidden");
            let twin_uri = harness.player.catalog.track(twin).expect("the twin").uri;

            harness
                .player
                .documents_moved(vec![MovedDocument {
                    id: taker,
                    uri: twin_uri,
                }])
                .expect("documents moved");

            let held: Vec<u64> = harness
                .player
                .services
                .analyses
                .lock()
                .keys()
                .copied()
                .collect();
            assert_eq!(held, [taker]);
            assert!(harness.player.catalog.track(twin).is_err());
        }
    }
}
