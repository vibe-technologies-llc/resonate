use std::time::Duration;

use resonate_core::{Resumable, Resumption, Span, TrackId};
use resonate_engine::{Command, Placement, PlayerState, QueueItem, Queued};

use crate::{Error, MobilePlayer, Result, documents::location};

const EDIT_ANSWERED_WITHIN: Duration = Duration::from_secs(10);

#[uniffi::export]
impl MobilePlayer {
    pub fn play_next(&self, ids: Vec<u64>) -> Result<()> {
        self.queue_documents(ids, |_| Placement::Next)
    }

    pub fn add_to_queue(&self, ids: Vec<u64>) -> Result<()> {
        self.queue_documents(ids, Placement::At)
    }

    pub fn remove_from_queue(&self, queue_id: u64) -> Result<()> {
        self.edit_queue(|queued, _| {
            let row = queued
                .rows
                .iter()
                .position(|item| item.id.get() == queue_id)
                .ok_or(Error::UnknownQueueRow { queue_id })?;

            Ok(Some(Command::Remove(Span::one(row))))
        })
    }

    pub fn move_in_queue(&self, from: u32, to: u32) -> Result<()> {
        self.edit_queue(|queued, _| {
            let rows = queued.rows.len();
            let moved = row_within(from, rows, rows)?;
            let before = row_within(to, rows + 1, rows)?;
            let landing = landing_before(moved, before);

            Ok((landing != moved).then_some(Command::Move {
                rows: Span::one(moved),
                to: landing,
            }))
        })
    }

    pub fn clear_queue(&self) -> Result<()> {
        self.edit_queue(|queued, _| Ok((!queued.rows.is_empty()).then_some(Command::Clear)))
    }
}

impl MobilePlayer {
    fn queue_documents(&self, ids: Vec<u64>, placed: fn(usize) -> Placement) -> Result<()> {
        let items = self.queue_items(ids)?;

        self.edit_queue(|queued, _| {
            if items.is_empty() {
                return Ok(None);
            }
            if queued.rows.is_empty() {
                return Ok(Some(Command::Resume(paused_on_the_first(items))));
            }
            Ok(Some(Command::Insert {
                items,
                at: placed(queued.rows.len()),
                play: false,
            }))
        })
    }

    fn queue_items(&self, ids: Vec<u64>) -> Result<Vec<QueueItem>> {
        ids.into_iter()
            .map(|id| {
                let track = self.catalog.track(id)?;
                Ok(QueueItem::whole(TrackId::new(id)?, location(&track.uri)?))
            })
            .collect()
    }

    pub(crate) fn edit_queue(
        &self,
        edit: impl FnOnce(&Queued, &PlayerState) -> Result<Option<Command>>,
    ) -> Result<()> {
        let outcome = {
            let held = self.player.lock();
            let player = held.as_ref().ok_or(Error::Closed)?;
            let queued = player.queued();

            let Some(command) = edit(&queued, &player.state())? else {
                return Ok(());
            };
            player.request_if_the_queue_is_still(queued.revision, command)?
        };

        Ok(outcome.wait_for(EDIT_ANSWERED_WITHIN)?)
    }
}

fn paused_on_the_first(items: Vec<QueueItem>) -> Resumption {
    Resumption {
        rows: items
            .into_iter()
            .map(|item| Resumable {
                location: item.location,
                span: item.span,
                held: Some(item.id),
            })
            .collect(),
        ..Resumption::default()
    }
}

fn row_within(row: u32, bound: usize, rows: usize) -> Result<usize> {
    usize::try_from(row)
        .ok()
        .filter(|row| *row < bound)
        .ok_or(Error::QueueRowOutOfRange {
            row,
            rows: rows as u64,
        })
}

const fn landing_before(moved: usize, before: usize) -> usize {
    if before > moved { before - 1 } else { before }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Arc, atomic::Ordering},
        time::Duration,
    };

    use crossbeam_channel::unbounded;
    use resonate_core::Frames;
    use resonate_engine::{Event, PlaybackState, PlayerState};

    use super::*;
    use crate::{Track, Transport, harness::*};

    #[test]
    fn play_next_lands_after_the_playing_row_and_add_appends_under_a_row_of_its_own() {
        let harness = Harness::new();
        harness.resumed("abc", 1, 400);

        harness
            .player
            .play_next(harness.ids("d"))
            .expect("a play next");

        assert_eq!(harness.queued(), "abdc");
        assert_eq!(harness.playing(), ('b', 1, 400));

        harness
            .player
            .add_to_queue(harness.ids("a"))
            .expect("an append");
        let ids = harness.queue_ids();

        assert_eq!(harness.queued(), "abdca");
        assert_eq!(harness.playing(), ('b', 1, 400));
        assert_ne!(ids[0], ids[4]);
    }

    #[test]
    fn the_playing_song_names_its_queue_row_as_the_queue_does() {
        let harness = Harness::new();
        harness.resumed("abca", 0, 400);
        let ids = harness.queue_ids();

        let resumed = harness.settled(|snapshot| snapshot.current.is_some());

        assert_eq!(
            resumed.current.and_then(|current| current.queue_id),
            Some(ids[0])
        );

        harness.player.next().expect("a skip");
        let skipped = harness.settled(|snapshot| {
            snapshot
                .current
                .as_ref()
                .is_some_and(|current| current.id == harness.song('b').id)
        });

        assert_eq!(
            skipped.current.and_then(|current| current.queue_id),
            Some(ids[1])
        );
    }

    #[test]
    fn a_row_still_opening_is_the_song_a_snapshot_names() {
        let harness = Harness::new();
        harness.resumed("abc", 0, 0);
        harness.settled(|snapshot| snapshot.current.is_some());

        let rows = harness
            .player
            .player
            .lock()
            .as_ref()
            .expect("an open player")
            .queued()
            .rows;
        let opening = |playback| PlayerState {
            playback,
            queue_position: Some(1),
            ..PlayerState::default()
        };

        assert_eq!(
            crate::row_being_opened(&opening(PlaybackState::Buffering), &rows),
            Some(rows[1].id)
        );
        assert_eq!(
            crate::row_being_opened(&opening(PlaybackState::Paused), &rows),
            Some(rows[1].id)
        );
        assert_eq!(
            crate::row_being_opened(&opening(PlaybackState::Stopped), &rows),
            None
        );
        assert_eq!(
            crate::row_being_opened(
                &PlayerState {
                    queue_position: Some(rows.len()),
                    ..opening(PlaybackState::Buffering)
                },
                &rows
            ),
            None
        );
    }

    #[test]
    fn songs_whose_files_hold_the_same_bytes_are_found_and_one_alike_in_size_alone_is_not() {
        let harness = Harness::new();
        let mut differing = silence();
        if let Some(last) = differing.last_mut() {
            *last ^= 1;
        }
        fs::write(harness.folder.join("d.wav"), differing).expect("a song differing by a byte");

        let found = harness
            .player
            .identical_songs()
            .expect("the identical songs");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kept.id, harness.song('a').id);
        assert_eq!(
            found[0]
                .copies
                .iter()
                .map(|copy| copy.id)
                .collect::<Vec<_>>(),
            harness.ids("bc")
        );
    }

    #[test]
    fn the_copy_playing_is_the_one_kept() {
        let harness = Harness::new();
        harness.resumed("ab", 1, 0);

        let found = harness
            .player
            .identical_songs()
            .expect("the identical songs");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kept.id, harness.song('b').id);
    }

    #[test]
    fn folding_a_copy_hands_its_favourite_and_playlist_rows_to_the_one_kept() {
        let harness = Harness::new();
        let [kept, copy] = [harness.song('a').id, harness.song('b').id];
        harness.player.favourite(copy, true).expect("a favourite");
        let playlist = harness
            .player
            .create_playlist("Evening".into())
            .expect("a playlist");
        harness
            .player
            .add_to_playlist(playlist, vec![copy])
            .expect("a playlist row");

        let folded = harness
            .player
            .fold_identical_songs(kept, vec![copy])
            .expect("a fold");

        assert_eq!(folded.forgotten, 1);
        assert!(matches!(
            harness.player.catalog.track(copy),
            Err(Error::UnknownTrack)
        ));
        assert!(
            harness
                .player
                .catalog
                .track(kept)
                .expect("the kept song")
                .favourite
        );
        assert_eq!(
            harness
                .player
                .playlist_tracks(playlist, 0, 10)
                .expect("the playlist")
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            vec![kept]
        );
    }

    #[test]
    fn a_deleted_song_leaves_the_catalog_the_queue_and_its_playlists() {
        let harness = Harness::new();
        harness.resumed("abc", 0, 0);
        let gone = harness.song('b').id;
        let playlist = harness
            .player
            .create_playlist("Evening".into())
            .expect("a playlist");
        harness
            .player
            .add_to_playlist(playlist, vec![gone, harness.song('c').id])
            .expect("playlist rows");

        let forgotten = harness
            .player
            .forget_deleted_songs(vec![gone])
            .expect("a deletion");

        assert_eq!(forgotten.forgotten, 1);
        assert_eq!(forgotten.left_the_queue, 1);
        assert_eq!(harness.queued(), "ac");
        assert!(matches!(
            harness.player.catalog.track(gone),
            Err(Error::UnknownTrack)
        ));
        assert_eq!(
            harness
                .player
                .playlist_tracks(playlist, 0, 10)
                .expect("the playlist")
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            harness.ids("c")
        );
    }

    #[test]
    fn an_empty_queue_takes_what_is_queued_paused_on_its_first_row() {
        let harness = Harness::new();

        harness
            .player
            .play_next(harness.ids("ba"))
            .expect("a play next");
        let snapshot = harness.settled(|snapshot| snapshot.current.is_some());

        assert_eq!(harness.queued(), "ba");
        assert!(matches!(snapshot.transport, Transport::Paused));
        assert_eq!(harness.playing(), ('b', 0, 0));

        harness
            .player
            .add_to_queue(Vec::new())
            .expect("nothing queued");

        assert_eq!(harness.queued(), "ba");
    }

    #[test]
    fn removing_a_row_keeps_the_playing_one_and_removing_the_playing_one_moves_on() {
        let harness = Harness::new();
        harness.resumed("abcd", 2, 400);
        let ids = harness.queue_ids();

        harness.player.remove_from_queue(ids[0]).expect("a removal");

        assert_eq!(harness.queued(), "bcd");
        assert_eq!(harness.playing(), ('c', 1, 400));

        harness
            .player
            .remove_from_queue(ids[2])
            .expect("the playing row's removal");
        let snapshot = harness.settled(|snapshot| {
            snapshot
                .current
                .as_ref()
                .is_some_and(|current| current.id == harness.song('d').id)
        });

        assert_eq!(harness.queued(), "bd");
        assert_eq!(snapshot.queue_index, 1);
        assert_eq!(snapshot.position_ms, 0);
        assert!(matches!(
            harness.player.remove_from_queue(ids[0]),
            Err(Error::UnknownQueueRow { queue_id }) if queue_id == ids[0]
        ));
    }

    #[test]
    fn songs_whose_files_are_gone_leave_the_queue_and_the_playing_one_stays() {
        let harness = Harness::new();
        harness.resumed("abcd", 2, 400);
        for name in ["b", "d"] {
            fs::remove_file(harness.folder.join(format!("{name}.wav"))).expect("a deletion");
        }

        let changed = harness
            .player
            .hide_missing_songs(vec![FOLDER.into()])
            .expect("hidden");

        assert_eq!(changed.hidden, 2);
        assert_eq!(changed.left_the_queue, 2);
        assert_eq!(harness.queued(), "ac");
        assert_eq!(harness.playing(), ('c', 1, 400));
    }

    #[test]
    fn a_document_the_listing_shows_is_not_opened_and_the_rest_are() {
        let harness = Harness::new();
        harness.resumed("abcd", 2, 400);
        for name in ["b", "d"] {
            fs::remove_file(harness.folder.join(format!("{name}.wav"))).expect("a deletion");
        }
        let listed = ['a', 'b', 'c']
            .map(|name| harness.song(name).uri.clone())
            .into_iter()
            .collect();

        let changed = harness
            .player
            .hide_unlisted_songs(vec![FOLDER.into()], listed)
            .expect("hidden");

        assert_eq!(changed.hidden, 1);
        assert_eq!(changed.left_the_queue, 1);
        assert_eq!(harness.queued(), "abc");
    }

    #[test]
    fn a_file_the_provider_refuses_is_not_a_file_that_is_gone() {
        use std::os::unix::fs::PermissionsExt;

        let harness = Harness::new();
        harness.resumed("abcd", 2, 400);
        let refused = harness.folder.join("b.wav");
        fs::set_permissions(&refused, fs::Permissions::from_mode(0o000)).expect("a mode");

        let changed = harness
            .player
            .hide_missing_songs(vec![FOLDER.into()])
            .expect("a pass");

        fs::set_permissions(&refused, fs::Permissions::from_mode(0o644)).expect("a mode");
        assert_eq!(changed.hidden, 0);
        assert_eq!(changed.left_the_queue, 0);
        assert_eq!(harness.queued(), "abcd");
    }

    #[test]
    fn a_folder_removed_hides_its_songs_though_every_file_is_still_there() {
        let harness = Harness::new();
        harness.resumed("abcd", 2, 400);

        let changed = harness
            .player
            .hide_songs_under(FOLDER.into())
            .expect("hidden");

        assert_eq!(changed.hidden, 4);
        assert_eq!(changed.restored, 0);
        assert_eq!(changed.left_the_queue, 4);
        assert_eq!(harness.queued(), "");
        assert_eq!(
            harness
                .player
                .hide_songs_under(FOLDER.into())
                .expect("hidden again")
                .hidden,
            0
        );
    }

    #[test]
    fn a_move_names_the_row_it_lands_before_as_swiftui_does_in_both_directions() {
        let harness = Harness::new();
        harness.resumed("abcd", 1, 400);

        harness.player.move_in_queue(0, 3).expect("a move down");

        assert_eq!(harness.queued(), "bcad");
        assert_eq!(harness.playing(), ('b', 0, 400));

        harness.player.move_in_queue(3, 0).expect("a move up");

        assert_eq!(harness.queued(), "dbca");
        assert_eq!(harness.playing(), ('b', 1, 400));

        harness
            .player
            .move_in_queue(1, 4)
            .expect("a move to the end");

        assert_eq!(harness.queued(), "dcab");
        assert_eq!(harness.playing(), ('b', 3, 400));

        harness.player.move_in_queue(2, 2).expect("a move nowhere");
        harness
            .player
            .move_in_queue(2, 3)
            .expect("another move nowhere");

        assert_eq!(harness.queued(), "dcab");
        assert!(matches!(
            harness.player.move_in_queue(4, 0),
            Err(Error::QueueRowOutOfRange { row: 4, rows: 4 })
        ));
        assert!(matches!(
            harness.player.move_in_queue(0, 5),
            Err(Error::QueueRowOutOfRange { row: 5, rows: 4 })
        ));
    }

    #[test]
    fn clearing_keeps_the_playing_row_alone() {
        let harness = Harness::new();
        harness.resumed("abcd", 2, 400);

        harness.player.clear_queue().expect("a clear");

        assert_eq!(harness.queued(), "c");
        assert_eq!(harness.playing(), ('c', 0, 400));
    }

    #[test]
    fn a_save_after_an_edit_keeps_the_edited_queue() {
        let harness = Harness::new();
        harness.resumed("abc", 1, 400);
        harness.player.save().expect("a first save");

        harness
            .player
            .play_next(harness.ids("d"))
            .expect("a play next");
        harness.player.move_in_queue(0, 4).expect("a move");
        harness.player.save().expect("a save of the edit");

        assert_eq!(harness.queued(), "bdca");
        assert_eq!(
            harness.player.catalog.resume().expect("the saved queue"),
            (harness.ids("bdca"), 0, 400)
        );
    }

    #[test]
    fn a_closed_player_refuses_every_edit() {
        let harness = Harness::new();
        harness.resumed("ab", 0, 0);
        if let Some(player) = harness.player.player.lock().take() {
            player.shutdown().expect("a shutdown");
        }

        assert!(matches!(
            harness.player.play_next(harness.ids("c")),
            Err(Error::Closed)
        ));
        assert!(matches!(
            harness.player.add_to_queue(harness.ids("c")),
            Err(Error::Closed)
        ));
        assert!(matches!(
            harness.player.remove_from_queue(1),
            Err(Error::Closed)
        ));
        assert!(matches!(
            harness.player.move_in_queue(0, 1),
            Err(Error::Closed)
        ));
        assert!(matches!(harness.player.clear_queue(), Err(Error::Closed)));
    }

    #[test]
    fn a_document_scanned_unchanged_is_not_read_again_and_a_changed_one_is() {
        let harness = Harness::new();
        let uri = format!("{FOLDER}a.wav");
        harness
            .player
            .scan_document(uri.clone(), "a.wav".into(), 100, 5)
            .expect("a first scan");
        let settled = harness.player.catalog.revision();
        fs::remove_file(harness.folder.join("a.wav")).expect("a file removed");

        let again = harness
            .player
            .scan_document(uri.clone(), "a.wav".into(), 100, 5)
            .expect("an unchanged document");
        let touched = harness
            .player
            .scan_document(uri.clone(), "a.wav".into(), 100, 6);
        let unstamped = harness.player.scan_document(uri, "a.wav".into(), 0, 0);

        assert_eq!(again.title, harness.song('a').title);
        assert_eq!(harness.player.catalog.revision(), settled);
        assert!(touched.is_err());
        assert!(unstamped.is_err());
    }

    #[test]
    fn a_hidden_document_is_indexed_again_though_its_stamp_matches() {
        let harness = Harness::new();
        let uri = format!("{FOLDER}b.wav");
        let song = harness
            .player
            .scan_document(uri.clone(), "b.wav".into(), 100, 5)
            .expect("a first scan");
        harness
            .player
            .catalog
            .mark_missing(&[song.id as i64], &[])
            .expect("hidden");

        harness
            .player
            .scan_document(uri, "b.wav".into(), 100, 5)
            .expect("a second scan");

        let shown = harness.player.catalog.tracks("", 0, 10).expect("listed");
        assert!(shown.iter().any(|track| track.id == song.id));
    }

    #[test]
    fn a_snapshot_says_there_is_no_device_to_play_through_until_the_transport_stops() {
        let harness = Harness::new();
        let told = "No speakers or headphones to play through — playing on once one is there";
        harness.resumed("ab", 0, 0);

        *harness.player.waiting.lock() = Some(told.to_owned());
        let paused = harness.player.snapshot().expect("a snapshot");
        let again = harness.player.snapshot().expect("a snapshot");
        harness.player.stop().expect("a stop sent");
        let stopped = harness.settled(|snapshot| matches!(snapshot.transport, Transport::Stopped));
        let after = harness.player.snapshot().expect("a snapshot");

        assert_eq!(paused.waiting.as_deref(), Some(told));
        assert_eq!(again.waiting.as_deref(), Some(told));
        assert_eq!(stopped.waiting, None);
        assert_eq!(after.waiting, None);
    }

    #[test]
    fn a_snapshot_keeps_the_playing_song_until_the_catalog_moves() {
        let harness = Harness::new();
        harness.resumed("ab", 0, 0);

        let before = harness.player.snapshot().expect("a snapshot");
        let again = harness.player.snapshot().expect("a snapshot");
        harness
            .player
            .catalog
            .upsert(Track {
                title: "Renamed".into(),
                ..harness.song('a').clone()
            })
            .expect("a retag");
        let after = harness.player.snapshot().expect("a snapshot");

        assert_eq!(
            before.current.map(|track| track.title),
            again.current.map(|track| track.title)
        );
        assert_eq!(
            after.current.map(|track| track.title).as_deref(),
            Some("Renamed")
        );
    }

    #[test]
    fn a_saved_queue_resumes_in_its_order_at_its_row() {
        let harness = Harness::new();
        harness.resumed("cab", 1, 0);

        harness.player.save().expect("a save");
        let (ids, row, _) = harness.player.catalog.resume().expect("a saved queue");

        assert_eq!(ids, harness.ids("cab"));
        assert_eq!(row, 1);
    }

    #[test]
    fn a_snapshot_waiting_on_the_catalog_does_not_hold_up_the_transport() {
        let harness = Harness::new();
        harness.resumed("ab", 0, 0);
        let held_for = Duration::from_millis(1500);
        let transport_within = Duration::from_millis(1000);
        let player = &harness.player;
        let asked = std::thread::scope(|scope| {
            let (held, holding) = std::sync::mpsc::channel();
            scope.spawn(move || {
                let _connection = player.catalog.0.lock();
                held.send(()).expect("a signal");
                std::thread::sleep(held_for);
            });
            holding.recv().expect("the catalog held");
            scope.spawn(move || player.snapshot());
            std::thread::sleep(Duration::from_millis(200));

            let began = std::time::Instant::now();
            player.seek(0).expect("a seek");
            began.elapsed()
        });

        assert!(asked < transport_within, "the seek waited {asked:?}");
    }

    struct Told(Arc<std::sync::atomic::AtomicUsize>);

    impl crate::PlayerWatch for Told {
        fn moved(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn engine_events_that_change_what_is_shown_tell_the_watch_once_however_many_arrive() {
        let harness = Harness::new();
        let told = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        harness
            .player
            .watch_player(Box::new(Told(Arc::clone(&told))));
        let (sends, arrives) = unbounded();
        sends.send(Event::QueueFinished).expect("an event sent");
        sends
            .send(Event::TrackStarted(TrackId::new(1).expect("an id")))
            .expect("an event sent");

        harness.player.absorbed(Event::QueueFinished, &arrives);

        assert_eq!(told.load(Ordering::Relaxed), 1);
        assert!(arrives.is_empty());
    }

    #[test]
    fn an_underrun_alone_does_not_wake_the_watch() {
        let harness = Harness::new();
        let told = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        harness
            .player
            .watch_player(Box::new(Told(Arc::clone(&told))));
        let (_sends, arrives) = unbounded();

        harness
            .player
            .absorbed(Event::Underrun { missing: Frames(1) }, &arrives);

        assert_eq!(told.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_watch_over_a_player_no_longer_held_ends() {
        let (sends, arrives) = unbounded();
        sends.send(Event::QueueFinished).expect("an event sent");

        crate::watching::watch(&std::sync::Weak::new(), &arrives);

        assert_eq!(arrives.len(), 0);
    }

    #[test]
    fn counting_what_is_heard_reads_the_engine_and_answers_its_transport() {
        let harness = Harness::new();
        harness.resumed("ab", 0, 0);

        let playback = harness.player.counted_now().expect("a count");

        assert_eq!(playback, PlaybackState::Paused);
    }
}
