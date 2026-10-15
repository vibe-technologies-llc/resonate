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
        self.edit_queue(|queued, state| {
            Ok(after_the_playing(queued.rows.len(), state.queue_position).map(Command::Remove))
        })?;
        self.edit_queue(
            |_, state| Ok(before_the_playing(state.queue_position).map(Command::Remove)),
        )
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

fn after_the_playing(rows: usize, playing: Option<usize>) -> Option<Span> {
    let first = playing.map_or(0, |row| row + 1);
    let last = rows.checked_sub(1)?;

    (first <= last).then(|| Span::between(first, last))
}

fn before_the_playing(playing: Option<usize>) -> Option<Span> {
    playing?.checked_sub(1).map(|last| Span::between(0, last))
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        fs::{self, File},
        os::fd::IntoRawFd,
        path::{Path, PathBuf},
        process,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        thread,
        time::Instant,
    };

    use crossbeam_channel::{Receiver, Sender, unbounded};
    use parking_lot::Mutex;
    use resonate_audio::{
        AudioSource, Backend, Error as SinkError, Opener, SinkChange, SinkId, SinkInfo, SinkResult,
        SinkStream, StreamRequest, Surveyor,
    };
    use resonate_codec::Sources;
    use resonate_core::{Frames, Gain, SampleRate};
    use resonate_engine::{EngineConfig, Player};

    use super::*;
    use crate::{
        Documents, PlaybackPreferences, Snapshot, Track, Transport, catalog::Catalog,
        documents::GrantedFiles, downloads::Downloads, features::Services, output::Routing,
        sync::Syncing,
    };

    const RATE: u32 = 44_100;
    const SECONDS_A_SONG: u32 = 2;
    const SETTLED_WITHIN: Duration = Duration::from_secs(10);
    const LOOKED_AGAIN_AFTER: Duration = Duration::from_millis(10);
    const FOLDER: &str = "content://test/";
    const SONGS: [&str; 4] = ["a", "b", "c", "d"];

    struct Folder(PathBuf);

    impl Documents for Folder {
        fn open_document(&self, uri: String) -> i32 {
            uri.strip_prefix(FOLDER)
                .and_then(|name| File::open(self.0.join(name)).ok())
                .map_or(-1, IntoRawFd::into_raw_fd)
        }
    }

    struct Silent {
        _announces: Sender<SinkChange>,
        changes: Receiver<SinkChange>,
    }

    struct Nowhere;

    impl Surveyor for Nowhere {
        fn enumerate_sinks(&self, _: Duration) -> SinkResult<Vec<SinkInfo>> {
            Ok(Vec::new())
        }
    }

    impl Opener for Nowhere {
        fn open(&self, _: &StreamRequest, _: Box<dyn AudioSource>) -> SinkResult<SinkStream> {
            Err(SinkError::NoSink)
        }
    }

    impl Backend for Silent {
        fn subscribe_sinks(&self) -> Receiver<SinkChange> {
            self.changes.clone()
        }

        fn surveyor(&self) -> Arc<dyn Surveyor> {
            Arc::new(Nowhere)
        }

        fn opener(&self) -> Arc<dyn Opener> {
            Arc::new(Nowhere)
        }

        fn set_device_volume(&self, _: SinkId, _: Gain) -> SinkResult<()> {
            Ok(())
        }

        fn set_device_mute(&self, _: SinkId, _: bool) -> SinkResult<()> {
            Ok(())
        }

        fn shutdown(self: Box<Self>) -> SinkResult<()> {
            Ok(())
        }
    }

    struct Harness {
        player: MobilePlayer,
        songs: Vec<Track>,
        folder: PathBuf,
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            if let Some(player) = self.player.player.lock().take() {
                let _ = player.shutdown();
            }
            let _ = fs::remove_dir_all(&self.folder);
        }
    }

    impl Harness {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let folder = env::temp_dir().join(format!(
                "resonate-mobile-queue-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&folder).expect("a test folder");
            for name in SONGS {
                fs::write(folder.join(format!("{name}.wav")), silence()).expect("a song");
            }

            let documents: Arc<dyn Documents> = Arc::new(Folder(folder.clone()));
            let sources = Arc::new(Sources::local().and(Arc::new(
                GrantedFiles::new(documents, "content").expect("a granted provider"),
            )));
            let engine = Player::with_sources_and_backend(
                EngineConfig::default(),
                Arc::clone(&sources),
                |_| {
                    let (announces, changes) = unbounded();
                    Ok(Box::new(Silent {
                        _announces: announces,
                        changes,
                    }))
                },
            )
            .expect("an engine with no device");
            let player = MobilePlayer {
                player: Mutex::new(Some(engine)),
                catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
                sources,
                failure: Mutex::new(None),
                saved: Mutex::new(None),
                preferences: Mutex::new(PlaybackPreferences::default()),
                services: Services::default(),
                playlist_undo: Mutex::new(Vec::new()),
                downloads: Downloads::default(),
                syncing: Syncing::under(folder.clone()),
                routing: Arc::new(Routing::new()),
                enriching: crate::enrichment::Enriching::default(),
                room: crate::room::Room::under(std::env::temp_dir()),
                tag_undo: crate::tagging::TagUndo::default(),
            };

            let songs = SONGS
                .iter()
                .map(|name| {
                    player
                        .index_document(format!("{FOLDER}{name}.wav"), format!("{name}.wav"))
                        .expect("an indexed song")
                })
                .collect();

            Self {
                player,
                songs,
                folder,
            }
        }

        fn ids(&self, names: &str) -> Vec<u64> {
            names.chars().map(|name| self.song(name).id).collect()
        }

        fn song(&self, name: char) -> &Track {
            self.songs
                .iter()
                .find(|song| song.title.starts_with(name))
                .expect("a song of that name")
        }

        fn resumed(&self, names: &str, row: usize, at_ms: u64) {
            let rows = self
                .ids(names)
                .into_iter()
                .map(|id| Resumable {
                    location: location(&self.player.catalog.track(id).expect("a song").uri)
                        .expect("a document"),
                    span: None,
                    held: Some(TrackId::new(id).expect("an id")),
                })
                .collect();
            let resumption = Resumption {
                rows,
                row,
                at: Frames::from_duration(
                    Duration::from_millis(at_ms),
                    SampleRate::new(RATE).expect("a rate"),
                ),
                ..Resumption::default()
            };

            self.player
                .player
                .lock()
                .as_ref()
                .expect("an open player")
                .request(Command::Resume(resumption))
                .expect("a resumption sent")
                .wait_for(SETTLED_WITHIN)
                .expect("a resumption");
            self.settled(|snapshot| snapshot.current.is_some());
        }

        fn queued(&self) -> String {
            self.player
                .queue()
                .expect("the queue")
                .iter()
                .map(name_of)
                .collect()
        }

        fn queue_ids(&self) -> Vec<u64> {
            self.player
                .queue()
                .expect("the queue")
                .iter()
                .map(|track| track.queue_id.expect("a queue row id"))
                .collect()
        }

        fn settled(&self, until: impl Fn(&Snapshot) -> bool) -> Snapshot {
            let began = Instant::now();
            loop {
                let snapshot = self.player.snapshot().expect("a snapshot");
                if until(&snapshot) {
                    return snapshot;
                }
                assert!(
                    began.elapsed() < SETTLED_WITHIN,
                    "never settled: {snapshot:?}"
                );
                thread::sleep(LOOKED_AGAIN_AFTER);
            }
        }

        fn playing(&self) -> (char, u32, u64) {
            let snapshot = self.player.snapshot().expect("a snapshot");
            let current = snapshot.current.expect("a current song");

            (
                name_of(&current),
                snapshot.queue_index,
                snapshot.position_ms,
            )
        }
    }

    fn name_of(track: &Track) -> char {
        track.title.chars().next().expect("a titled song")
    }

    fn silence() -> Vec<u8> {
        let channels = 2_u16;
        let bits = 16_u16;
        let block_align = channels * bits / 8;
        let data = vec![0_u8; (RATE * SECONDS_A_SONG) as usize * usize::from(block_align)];

        let mut format = Vec::new();
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&channels.to_le_bytes());
        format.extend_from_slice(&RATE.to_le_bytes());
        format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
        format.extend_from_slice(&block_align.to_le_bytes());
        format.extend_from_slice(&bits.to_le_bytes());

        let mut body = b"WAVE".to_vec();
        for (id, payload) in [(b"fmt ", &format), (b"data", &data)] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            body.extend_from_slice(payload);
        }

        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

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
}
