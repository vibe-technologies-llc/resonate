use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    path::{Component, Path},
    time::Duration,
};

use resonate_core::{MediaLocation, ZoneId};
use resonate_engine::Command;
use resonate_library::{DEFAULT_LAYOUT, Layout, Named, Naming};
use rusqlite::{OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::read,
    documents::location,
    vanished::{Vacated, referring_tables, vacated},
};

const FOLDER_ENDS: char = '/';
const CONTENT_SCHEME: &str = "content:";
const ENCODED_FOLDER_END: &str = "%2F";
const EXTENSION_STARTS: char = '.';
const YEAR_DIGITS: usize = 4;
const MOVES_FOLLOWED_WITHIN: Duration = Duration::from_secs(10);

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
        self.syncing.files_by(&layout);

        let layout = Layout::read(&layout).map_err(|source| Error::LayoutUnreadable {
            source: Box::new(source),
        })?;
        let within = within_of(&folder);
        let held = self.songs_within(&within)?;
        Ok(planned(&layout, &within, &held))
    }

    pub fn documents_moved(&self, moved: Vec<MovedDocument>) -> Result<()> {
        self.documents_moved_while(moved, |zone| self.party_following(zone))
    }
}

impl MobilePlayer {
    fn documents_moved_while(
        &self,
        moved: Vec<MovedDocument>,
        following: impl Fn(ZoneId) -> bool,
    ) -> Result<()> {
        if moved.is_empty() {
            return Ok(());
        }

        let rekeyed = self.catalog.rekeyed(&moved)?;

        let mut analyses = self.services.analyses.lock();

        for old in &rekeyed.handed_over {
            analyses.remove(old);
        }
        drop(analyses);

        if !rekeyed.relocated.is_empty() {
            self.relocated_in_every_zone(&rekeyed.relocated)?;
        }
        self.save_while(&following)
    }

    fn relocated_in_every_zone(&self, relocated: &[(MediaLocation, MediaLocation)]) -> Result<()> {
        let asked: Vec<_> = self
            .playing
            .listed()
            .into_iter()
            .map(|zone| zone.player.request(Command::Relocate(relocated.to_vec())))
            .collect();
        Ok(asked
            .into_iter()
            .map(|sent| sent.and_then(|outcome| outcome.wait_for(MOVES_FOLLOWED_WITHIN)))
            .fold(Ok(()), resonate_engine::Result::and)?)
    }

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
}

#[derive(Default)]
struct Rekeyed {
    handed_over: Vec<u64>,
    relocated: Vec<(MediaLocation, MediaLocation)>,
}

impl crate::catalog::Catalog {
    fn rekeyed(&self, moved: &[MovedDocument]) -> Result<Rekeyed> {
        let mut connection = self.0.lock();

        let referring = referring_tables(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let mut rekeyed = Rekeyed::default();

        for document in moved {
            let left = transaction
                .query_row(
                    "SELECT uri FROM documents WHERE id = ?1",
                    [document.id as i64],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Read, source))?;
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
                rekeyed.handed_over.push(old);
            }
            transaction
                .execute(
                    "UPDATE documents SET uri = ?2 WHERE id = ?1",
                    params![document.id as i64, document.uri],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            if let Some(left) = left
                && left != document.uri
                && let (Ok(from), Ok(to)) = (location(&left), location(&document.uri))
            {
                rekeyed.relocated.push((from, to));
            }
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(rekeyed)
    }
}

fn within_of(folder: &str) -> String {
    if folder.starts_with(CONTENT_SCHEME) {
        format!("{folder}{ENCODED_FOLDER_END}")
    } else {
        format!("{folder}{FOLDER_ENDS}")
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
    components_of(&layout.render(&named, Naming::Anything)?)
}

pub(crate) fn components_of(path: &Path) -> Option<Vec<String>> {
    path.components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str().map(str::to_owned),
            _ => None,
        })
        .collect()
}

pub(crate) fn given(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()).then_some(text)
}

pub(crate) fn year_of(date: &str) -> Option<i32> {
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
            artist: "Example Band".into(),
            album: "The Pier".into(),
            album_artist: "Example Band".into(),
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
                "Downloads/in%20the%20pier.flac",
                "In the Pier?",
                1,
                Some(1),
            ),
            song(
                2,
                "Example%20Band/The%20Pier/2-01%20Small%20Mercies.flac",
                "Small Mercies",
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
                    "Example Band".into(),
                    "The Pier".into(),
                    "1-01 In the Pier?.flac".into()
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
                "Example%20Band/The%20Pier/06%20Goodbye.flac",
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
    fn a_granted_tree_holds_its_songs_beneath_the_encoded_id_of_their_folder() {
        const TREE: &str = "content://com.android.externalstorage.documents/tree/primary%3AMusic/document/primary%3AMusic";

        let within = within_of(TREE);
        let layout = Layout::default();
        let held = [
            Track {
                uri: format!("{within}in%20the%20pier.flac"),
                ..song(1, "in the pier.flac", "In the Pier?", 1, None)
            },
            Track {
                uri: format!("{within}Example%20Band%2FThe%20Pier%2F06%20Goodbye.flac"),
                ..song(2, "06 Goodbye.flac", "Goodbye", 6, None)
            },
        ];

        let plan = planned(&layout, &within, &held);

        assert_eq!(within, format!("{TREE}%2F"));
        assert_eq!(plan.unchanged, 1);
        assert_eq!(
            plan.moves,
            vec![FiledSong {
                id: 1,
                from: held[0].uri.clone(),
                to: vec![
                    "Example Band".into(),
                    "The Pier".into(),
                    "01 In the Pier?.flac".into()
                ],
            }]
        );
        assert_eq!(
            within_of("resonate-folder://local"),
            "resonate-folder://local/"
        );
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
                artist: "Example Band".into(),
                album: "The Pier".into(),
                duration_ms: 200_000,
                ..Track::default()
            })
            .expect("upserted")
    }

    #[test]
    fn a_move_onto_the_place_a_hidden_other_song_holds_succeeds_and_keeps_that_song_hidden() {
        let (folder, catalog) = catalogued("other");
        let gone = filed(&catalog, "Example%20Band/The%20Pier/01.flac", "Mother");
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

        assert!(handed.handed_over.is_empty());
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
        let gone = filed(&catalog, "Example%20Band/The%20Pier/01.flac", "Mother");
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

        assert_eq!(handed.handed_over, [gone.id]);
        assert!(catalog.track(gone.id).is_err());

        let song = catalog.track(moving.id).expect("the song");

        assert_eq!(song.uri, gone.uri);
        assert!(song.favourite);

        let _ = std::fs::remove_dir_all(&folder);
    }

    mod through_the_queue {
        use std::fs;

        use resonate_engine::{PlaybackState, PlayerState};

        use super::*;
        use crate::{
            Reached,
            documents::document_uri,
            harness::{FOLDER, Harness},
            place_saved,
        };

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

            while harness
                .player
                .queue()
                .map_or(true, |queue| queue.iter().any(|track| track.id == 0))
            {
                assert!(
                    began.elapsed() < crate::harness::SETTLED_WITHIN,
                    "the queue never named the moved song"
                );

                std::thread::sleep(crate::harness::LOOKED_AGAIN_AFTER);
            }
            harness.settled(|snapshot| snapshot.position_ms == before.2);

            assert_eq!(harness.queued(), "abcd");
            assert_eq!(harness.playing(), before);
        }

        #[test]
        fn a_moved_song_in_a_queue_that_is_playing_leaves_it_playing_at_the_same_row() {
            use crate::Transport;

            let harness = Harness::sounding();

            harness.resumed("abcd", 2, 500);
            harness.player.play().expect("play");
            harness.settled(|snapshot| snapshot.transport == Transport::Playing);

            let id = harness.song('c').id;

            let moving = moved(&harness, 'c', "e.wav");

            harness
                .player
                .documents_moved(vec![moving])
                .expect("documents moved");

            let after = harness.settled(|snapshot| {
                snapshot.transport == Transport::Playing && snapshot.current.is_some()
            });

            assert_eq!(after.queue_index, 2);
            assert_eq!(harness.queued(), "abcd");
            assert_eq!(
                harness.player.catalog.track(id).expect("the song").uri,
                format!("{FOLDER}e.wav")
            );
        }

        #[test]
        fn a_moved_song_in_a_queue_a_party_is_followed_through_moves_there_and_the_kept_queue_stays_as_saved()
         {
            let harness = Harness::sounding();

            harness.resumed("abcd", 2, 500);
            harness.player.play().expect("play");
            harness.settled(|snapshot| snapshot.transport == crate::Transport::Playing);
            harness.player.save().expect("saved");
            harness.resumed("dcba", 1, 0);
            harness.player.play().expect("play");
            harness.settled(|snapshot| snapshot.transport == crate::Transport::Playing);

            let before = harness.settled(|snapshot| snapshot.current.is_some());
            let id = harness.song('b').id;
            let saved = harness.ids("abcd");

            let moving = moved(&harness, 'b', "e.wav");

            harness
                .player
                .documents_moved_while(vec![moving], |_| true)
                .expect("documents moved");

            let player = harness.player.playing.player().expect("an open player");
            let after = harness.player.snapshot().expect("a snapshot");
            let landed: Vec<String> = player
                .queued()
                .rows
                .iter()
                .map(|row| document_uri(&row.location).expect("a document"))
                .collect();
            let (kept, _) = harness
                .player
                .catalog
                .resume_rows_of(ZoneId::FIRST)
                .expect("the kept queue");
            let kept: Vec<u64> = kept
                .rows
                .iter()
                .filter_map(|laid| match laid.row {
                    crate::catalog::KeptRow::Document(id) => Some(id),
                    crate::catalog::KeptRow::Served(_) => None,
                })
                .collect();

            assert_eq!(harness.queued(), "dcba");
            assert_eq!(
                landed,
                ["d.wav", "c.wav", "e.wav", "a.wav"].map(|name| format!("{FOLDER}{name}"))
            );
            assert_eq!(after.queue_index, before.queue_index);
            assert_eq!(after.transport, crate::Transport::Playing);
            assert_eq!(kept, saved);
            assert_eq!(
                harness.player.catalog.track(id).expect("the song").uri,
                format!("{FOLDER}e.wav")
            );
        }

        #[test]
        fn a_zone_a_party_is_followed_in_saves_no_place_and_a_row_opening_saves_its_row_alone() {
            let harness = Harness::new();

            harness.resumed("abc", 0, 0);

            let player = harness.player.playing.player().expect("an open player");
            let queue = player.queued();
            let state = |playback| PlayerState {
                playback,
                queue_position: Some(1),
                ..PlayerState::default()
            };

            let following = place_saved(
                true,
                &state(PlaybackState::Stopped),
                &queue.rows,
                queue.revision,
            );
            let opening = place_saved(
                false,
                &state(PlaybackState::Buffering),
                &queue.rows,
                queue.revision,
            );
            let stopped = place_saved(
                false,
                &state(PlaybackState::Stopped),
                &queue.rows,
                queue.revision,
            );

            assert!(following.is_none());
            assert_eq!(
                opening.map(|place| place.reached),
                Some(Reached::StillOpening)
            );
            assert_eq!(stopped.map(|place| place.reached), Some(Reached::At(0)));
        }

        #[test]
        fn a_zone_opening_a_row_as_its_songs_move_plays_them_where_they_went_and_saves_them() {
            let harness = Harness::new();
            let player = harness.player.playing.player().expect("an open player");
            let ids = harness.ids("abcd");

            let opening = || {
                let state = player.state();
                let queue = player.queued();
                queue.rows.len() == ids.len()
                    && place_saved(false, &state, &queue.rows, queue.revision)
                        .is_some_and(|place| place.reached == Reached::StillOpening)
            };

            harness.hold_opens();
            harness
                .player
                .load(ids.clone(), 1, true)
                .expect("a queue loaded");
            settled_on(opening, "the zone never began opening its row");

            let opened = moved(&harness, 'b', "e.wav");
            let later = moved(&harness, 'c', "f.wav");

            harness
                .player
                .documents_moved(vec![opened, later])
                .expect("documents moved");
            harness.let_opens_through();

            let heard = harness.settled(|snapshot| snapshot.current.is_some());

            player
                .request(Command::Stop)
                .expect("a stop sent")
                .wait_for(crate::harness::SETTLED_WITHIN)
                .expect("stopped");
            harness.player.save().expect("saved");

            let landed: Vec<String> = player
                .queued()
                .rows
                .iter()
                .map(|row| document_uri(&row.location).expect("a document"))
                .collect();
            let (kept, _) = harness
                .player
                .catalog
                .resume_rows_of(ZoneId::FIRST)
                .expect("the kept queue");
            let kept: Vec<u64> = kept
                .rows
                .iter()
                .filter_map(|laid| match laid.row {
                    crate::catalog::KeptRow::Document(id) => Some(id),
                    crate::catalog::KeptRow::Served(_) => None,
                })
                .collect();

            assert_eq!(heard.queue_index, 1);
            assert_eq!(heard.current.map(|song| song.id), Some(ids[1]));
            assert_eq!(
                landed,
                ["a.wav", "e.wav", "f.wav", "d.wav"].map(|name| format!("{FOLDER}{name}"))
            );
            assert_eq!(harness.queued(), "abcd");
            assert_eq!(kept, ids);
        }

        fn settled_on(until: impl Fn() -> bool, never: &str) {
            let began = std::time::Instant::now();
            while !until() {
                assert!(began.elapsed() < crate::harness::SETTLED_WITHIN, "{never}");

                std::thread::sleep(crate::harness::LOOKED_AGAIN_AFTER);
            }
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
