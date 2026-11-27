use std::{
    io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{
    Gate as MeteredGate, MediaLocation, Meter, Metering, SourceId, TrackId, WantId,
};
use resonate_library::{
    DeliveryFolder, Layout, Library, PollHandle, PollOptions, PollProgress, PollSummary,
    RETRY_WAITS, ReleaseTrack, Removal, Result, ScanOptions, Track,
};
use resonate_providers::{
    Delivery, Extension, Identity, Obtained, Opened, Opening, Provider, Providers,
    Result as ProvidedResult, TURNED_TO_APART,
};

use crate::support::{
    Canned, Delivering, Fake, GENRE, LastResort, Offering, THE_POLL_HAS_WAITED, TITLE,
    TrashedByTheDesktop, Tree, Unanswering, Wav, a_provider_offering_slow_water, all,
    asking_in_turn, every_file_under, low_tide_fake, low_tide_release, matching, only_album,
    opened_with_a_vault, options, orbits, orbits_rows, orbits_track, orbits_tree, release_row,
    scan, scanned_orbits, slow_water_found, source, titles, unreachable_server, waited_for,
    wanted_harbour_lights,
};

struct CancelledAsItAnswers {
    source: SourceId,
    file: PathBuf,
    poll: Mutex<Option<Arc<PollProgress>>>,
}

impl Provider for CancelledAsItAnswers {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _: &Identity) -> ProvidedResult<Obtained> {
        let started = Instant::now();

        while started.elapsed() < Duration::from_secs(5) {
            if let Some(poll) = self.poll.lock().as_ref() {
                poll.cancel();
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        Ok(Obtained::Found(Delivery::File(self.file.clone())))
    }
}

struct WatchedAsItAnswers {
    source: SourceId,
    file: PathBuf,
    poll: Mutex<Option<Arc<PollProgress>>>,
    heard: Mutex<Vec<Option<WantId>>>,
}

impl Provider for WatchedAsItAnswers {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _: &Identity) -> ProvidedResult<Obtained> {
        let started = Instant::now();

        while started.elapsed() < Duration::from_secs(5) {
            if let Some(poll) = self.poll.lock().as_ref() {
                self.heard.lock().push(poll.asking());
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        Ok(Obtained::Found(Delivery::File(self.file.clone())))
    }
}

fn polled_into_a_vault(
    held: &Tree,
    delivered: PathBuf,
) -> Result<(Library, WantId, Vec<Option<WantId>>)> {
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(held)?;

    scan(&library, &options(&orbits))?;

    let want = wanted_harbour_lights(&library)?;

    let provider = Arc::new(WatchedAsItAnswers {
        source: SourceId::new("inbox").expect("a nameable source"),
        file: delivered,
        poll: Mutex::new(None),
        heard: Mutex::new(Vec::new()),
    });
    let handle = library.poll(
        Arc::new(Providers::none().and(Arc::clone(&provider) as Arc<dyn Provider>)),
        PollOptions::default(),
    )?;
    let progress = Arc::clone(handle.progress());

    *provider.poll.lock() = Some(Arc::clone(&progress));
    handle.join()?;

    assert_eq!(
        progress.asking(),
        None,
        "a finished poll still names a want"
    );

    let heard = provider.heard.lock().clone();
    Ok((library, want, heard))
}

#[test]
fn a_poll_names_the_want_it_is_asking_about_while_it_asks() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new()
            .text(TITLE, "Harbour Lights")
            .frames(8_820)
            .build(),
    );

    let (_library, want, heard) = polled_into_a_vault(&held, delivered)?;

    assert_eq!(heard, vec![Some(want)]);
    Ok(())
}

#[test]
fn deleting_a_delivered_track_takes_its_vault_object_and_its_want() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new()
            .text(TITLE, "Harbour Lights")
            .frames(8_820)
            .build(),
    );

    let (library, _want, _heard) = polled_into_a_vault(&held, delivered)?;
    let landed: Vec<TrackId> = library
        .wants()?
        .into_iter()
        .filter_map(|want| want.held)
        .collect();
    let objects = library.vault_objects()?;

    assert_eq!(landed.len(), 1);
    assert_eq!(objects.len(), 1);

    library.trash_through(Some(Arc::new(TrashedByTheDesktop {
        asked: Mutex::new(Vec::new()),
        answer: Removal::Removed,
    })));

    let deleted = library.delete_tracks(&landed)?;

    assert_eq!(deleted.tracks, 1);
    assert!(library.track(landed[0])?.is_none());
    assert!(library.wants()?.is_empty(), "the want would fetch it again");
    assert!(library.vault_objects()?.is_empty());
    assert!(!objects[0].path.exists());
    Ok(())
}

#[test]
fn a_delivery_that_landed_as_the_poll_was_cancelled_is_still_noted() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Lantern").frames(8_820).build(),
    );
    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;

    let want = wanted_harbour_lights(&library)?;

    let provider = Arc::new(CancelledAsItAnswers {
        source: SourceId::new("inbox").expect("a nameable source"),
        file: delivered,
        poll: Mutex::new(None),
    });
    let handle = library.poll(
        Arc::new(Providers::none().and(Arc::clone(&provider) as Arc<dyn Provider>)),
        PollOptions::default(),
    )?;

    *provider.poll.lock() = Some(Arc::clone(handle.progress()));

    let summary = handle.join()?;

    assert!(summary.cancelled);

    let objects = library.vault_objects()?;

    assert_eq!(objects.len(), 1, "what landed was not noted");
    assert!(objects[0].path.starts_with(vault.root()));

    let wants = library.wants()?;

    assert_eq!(wants[0].id, want);
    assert_eq!(
        wants[0].offered.as_deref(),
        Some(MediaLocation::local(&objects[0].path).to_uri().as_str())
    );
    Ok(())
}

#[test]
fn a_delivered_row_keeps_the_genre_the_gain_and_the_words_its_file_declared() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(GENRE, "Krautrock")
            .described("REPLAYGAIN_TRACK_GAIN", "-7.50 dB")
            .described("REPLAYGAIN_TRACK_PEAK", "0.891")
            .sung("Lazing in the sun on the shore")
            .frames(8_820)
            .build(),
    );

    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;

    let _want = wanted_harbour_lights(&library)?;

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));
    let summary = library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.kept, 1);

    let landed = library
        .wants()?
        .first()
        .and_then(|want| want.held)
        .expect("the delivery is paired with the want");
    let track = library.track(landed)?.expect("the delivery is a track row");

    assert_eq!(
        titles(&library.search("genre:krautrock", 10)?.tracks),
        vec!["Harbour Lights"],
        "the genre the delivered file declared does not reach a search"
    );

    let stood = library
        .stand_in()
        .stands_in(&track.location, track.span)
        .expect("the vault stands in for the delivered row");
    let gain = stood.tags.replay_gain;

    assert!(
        gain.track_gain
            .is_some_and(|gain| (gain.get() + 7.5).abs() < 1e-3),
        "the delivered row is not levelled by the gain its file declared: {gain:?}"
    );
    assert!(
        gain.track_peak
            .is_some_and(|peak| (peak - 0.891).abs() < 1e-3)
    );
    assert_eq!(
        stood.tags.lyrics.as_deref(),
        Some("Lazing in the sun on the shore")
    );
    assert_eq!(
        matching(&library, "lyrics:shore")?,
        ["Harbour Lights"],
        "the words the delivered file declared do not reach a search"
    );
    Ok(())
}

#[test]
fn a_streamed_delivery_lands_in_the_vault_and_leaves_nothing_in_staging() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let shop = Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/55391743",
            extension: "wav",
            bytes: Wav::new()
                .text(TITLE, "Harbour Lights")
                .frames(8_820)
                .build(),
        },
    ));
    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 1);
    assert_eq!(summary.stats.unkept, 0);

    let objects = library.vault_objects()?;

    assert_eq!(objects.len(), 1);
    assert!(objects[0].path.starts_with(vault.root()));
    assert_eq!(
        std::fs::read_dir(vault.root().join("staging"))
            .expect("the staging folder")
            .count(),
        0
    );

    let wants = library.wants()?;

    assert_eq!(
        wants[0].offered.as_deref(),
        Some(MediaLocation::local(&objects[0].path).to_uri().as_str())
    );

    let held = wants[0].held.expect("the stream is a track row");
    let track = library.track(held)?.expect("the delivered row");

    assert_eq!(track.location.as_path(), Some(objects[0].path.as_path()));

    let released = library.release_from_vault(&[])?;

    assert_eq!(
        released.released, 0,
        "a delivered row was released from the vault"
    );
    assert!(
        library
            .stand_in()
            .stands_in(&track.location, track.span)
            .is_some()
    );
    Ok(())
}

#[test]
fn a_poll_cancelled_mid_delivery_stops_reading_and_leaves_the_want_untried() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let shop = Arc::new(Offering::new("shop", Delivering::Endless));
    let polling = library.poll(shop.registered(), PollOptions::default())?;

    thread::sleep(Duration::from_millis(100));
    polling.cancel();

    let summary = polling.join()?;

    assert!(summary.cancelled);
    assert_eq!(summary.stats.kept, 0);
    assert_eq!(summary.stats.unkept, 0);
    assert!(library.vault_objects()?.is_empty());
    assert_eq!(
        std::fs::read_dir(vault.root().join("staging"))
            .expect("the staging folder")
            .count(),
        0
    );

    let wants = library.wants()?;

    assert_eq!(wants[0].tried, None);
    Ok(())
}

fn polling_endlessly() -> Result<(Tree, Tree, Library, PollHandle)> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let shop = Arc::new(Offering::new("shop", Delivering::Endless));
    let polling = library.poll(shop.registered(), PollOptions::default())?;

    thread::sleep(Duration::from_millis(100));
    Ok((held, orbits, library, polling))
}

#[test]
fn a_cancelled_poll_is_not_nudged_and_owes_nothing() -> Result<()> {
    let (_held, _orbits, _library, polling) = polling_endlessly()?;
    let progress = Arc::clone(polling.progress());

    polling.cancel();

    let nudged = progress.wants_changed();

    polling.join()?;

    assert!(
        !nudged,
        "a cancelled poll took a want it will never ask about"
    );
    assert!(!progress.owes_a_poll());
    Ok(())
}

#[test]
fn a_poll_nudged_before_it_was_cancelled_owes_another() -> Result<()> {
    let (_held, _orbits, _library, polling) = polling_endlessly()?;
    let progress = Arc::clone(polling.progress());

    assert!(progress.wants_changed());

    polling.cancel();
    polling.join()?;

    assert!(progress.owes_a_poll());
    Ok(())
}

#[test]
fn a_stream_that_stops_sending_is_given_up_as_late_and_the_want_left_due() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let shop = Arc::new(Offering::new("shop", Delivering::Stalling));
    let began = Instant::now();
    let summary = library
        .poll(
            shop.registered(),
            PollOptions {
                answers_within: Duration::from_millis(200),
                ..PollOptions::default()
            },
        )?
        .join()?;

    assert!(began.elapsed() < Duration::from_secs(5));
    assert!(!summary.cancelled);
    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.late, 1);
    assert_eq!(summary.stats.kept, 0);
    assert_eq!(summary.stats.unkept, 0);
    assert!(library.vault_objects()?.is_empty());
    assert_eq!(
        std::fs::read_dir(vault.root().join("staging"))
            .expect("the staging folder")
            .count(),
        0
    );

    let wants = library.wants()?;

    assert!(
        wants[0].tried.is_none(),
        "a delivery that went quiet is no verdict on the song"
    );
    assert_eq!(wants[0].misses, 0);
    assert_eq!(wants[0].offered, None);
    Ok(())
}

fn the_want_is_left_due_after_its_download_died(
    library: &Library,
    summary: &PollSummary,
) -> Result<()> {
    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.refused, 1);
    assert_eq!(summary.stats.unkept, 0);
    assert_eq!(summary.stats.kept, 0);

    let wants = library.wants()?;

    assert!(
        wants[0].tried.is_none(),
        "a provider's failure part way is no verdict on the song"
    );
    assert_eq!(wants[0].misses, 0);
    Ok(())
}

#[test]
fn a_download_its_provider_failed_part_way_into_the_vault_is_refused_and_no_miss() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let shop = Arc::new(Offering::new("shop", Delivering::DyingPartWay));

    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(shop.opened(), 1);
    the_want_is_left_due_after_its_download_died(&library, &summary)
}

#[test]
fn a_download_its_provider_failed_part_way_into_the_music_folder_is_refused_and_no_miss()
-> Result<()> {
    let (tree, library) = scanned_orbits()?;

    wanted_harbour_lights(&library)?;
    library.deliver_into(Some(DeliveryFolder {
        path: tree.path().to_path_buf(),
        layout: Layout::default(),
    }));

    let shop = Arc::new(Offering::new("shop", Delivering::DyingPartWay));

    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(shop.opened(), 1);
    the_want_is_left_due_after_its_download_died(&library, &summary)
}

#[test]
fn a_streamed_delivery_with_no_vault_is_counted_unkept_and_offers_nothing() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    wanted_harbour_lights(&library)?;

    let shop = Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/55391743",
            extension: "wav",
            bytes: Wav::new().frames(8_820).build(),
        },
    ));
    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 0);
    assert_eq!(summary.stats.unkept, 1);

    let wants = library.wants()?;

    assert!(wants[0].tried.is_some());
    assert_eq!(wants[0].offered, None);
    Ok(())
}

struct WatchesTheWants {
    source: SourceId,
    library: Arc<Library>,
    held_when_opened: Arc<Mutex<Vec<usize>>>,
}

impl Provider for WatchesTheWants {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _: &Identity) -> ProvidedResult<Obtained> {
        let library = Arc::clone(&self.library);
        let held_when_opened = Arc::clone(&self.held_when_opened);
        Ok(Obtained::Found(Delivery::Stream {
            key: "track/55391743".into(),
            extension: Extension::new("wav")?,
            opening: Opening::new(move || {
                let held = library
                    .wants()
                    .expect("the wants read")
                    .iter()
                    .filter(|want| want.held.is_some())
                    .count();

                held_when_opened.lock().push(held);
                Ok(Opened::Reading {
                    reader: Box::new(io::Cursor::new(Wav::new().frames(8_820).build())),
                    length: None,
                })
            }),
        }))
    }
}

fn two_wants_filed_in_the_music_folder() -> Result<(Tree, Arc<Library>)> {
    let (tree, library) = scanned_orbits()?;
    let library = Arc::new(library);
    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Harbour Lights", Vec::new()));
    rows.push(release_row(5, "Marlow", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    for row in library.release_tracks(album.id)? {
        if row.title == "Harbour Lights" || row.title == "Marlow" {
            library.want(row.id)?;
        }
    }
    library.deliver_into(Some(DeliveryFolder {
        path: tree.path().to_path_buf(),
        layout: Layout::default(),
    }));
    Ok((tree, library))
}

#[test]
fn a_delivery_with_no_vault_is_held_before_the_lane_opens_the_next() -> Result<()> {
    let (_tree, library) = two_wants_filed_in_the_music_folder()?;
    let provider = Arc::new(WatchesTheWants {
        source: SourceId::new("shop").expect("a nameable source"),
        library: Arc::clone(&library),
        held_when_opened: Arc::new(Mutex::new(Vec::new())),
    });

    let summary = library
        .poll(
            Arc::new(Providers::none().and(Arc::clone(&provider) as Arc<dyn Provider>)),
            PollOptions {
                lanes: NonZeroUsize::MIN,
                ..PollOptions::default()
            },
        )?
        .join()?;

    assert_eq!(summary.stats.kept, 2);
    assert_eq!(*provider.held_when_opened.lock(), [0, 1]);
    assert!(library.wants()?.iter().all(|want| want.held.is_some()));
    Ok(())
}

struct NotesWhatIsAsked {
    source: SourceId,
    asked: Mutex<Vec<String>>,
}

impl Provider for NotesWhatIsAsked {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> ProvidedResult<Obtained> {
        self.asked.lock().push(identity.title.clone());
        Ok(Obtained::Nothing)
    }
}

#[test]
fn an_album_asked_for_whole_is_asked_song_by_song_in_its_order_and_before_an_album_asked_later()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.extend([
        release_row(4, "Harbour Lights", Vec::new()),
        release_row(5, "Marlow", Vec::new()),
        release_row(6, "Lantern", Vec::new()),
        release_row(7, "Childhood's End", Vec::new()),
        release_row(8, "Free Four", Vec::new()),
    ]);
    library.land_release(album.id, &orbits(rows, Vec::new()))?;

    let held = library.release_tracks(album.id)?;
    let row_of = |title: &str| {
        held.iter()
            .find(|row| row.title == title)
            .map(|row| row.id)
            .expect("the release names the song")
    };
    let noting = Arc::new(NotesWhatIsAsked {
        source: SourceId::new("shop").expect("a nameable source"),
        asked: Mutex::new(Vec::new()),
    });

    library.want_release_tracks(&[row_of("Marlow"), row_of("Harbour Lights")])?;
    library.want_release_tracks(&[row_of("Free Four"), row_of("Childhood's End")])?;
    library.want(row_of("Lantern"))?;
    library
        .poll(
            Arc::new(Providers::none().and(Arc::clone(&noting) as Arc<dyn Provider>)),
            PollOptions {
                lanes: NonZeroUsize::MIN,
                ..PollOptions::default()
            },
        )?
        .join()?;

    assert_eq!(
        *noting.asked.lock(),
        [
            "Lantern",
            "Harbour Lights",
            "Marlow",
            "Childhood's End",
            "Free Four"
        ]
    );
    Ok(())
}

struct HeldUntilAskedAgain {
    asked_again: Arc<AtomicBool>,
    overlapped: Arc<AtomicBool>,
    bytes: io::Cursor<Vec<u8>>,
    waited: bool,
}

impl io::Read for HeldUntilAskedAgain {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if !self.waited {
            self.waited = true;

            let began = Instant::now();

            while !self.asked_again.load(Ordering::SeqCst)
                && began.elapsed() < Duration::from_secs(5)
            {
                thread::sleep(Duration::from_millis(10));
            }
            self.overlapped
                .store(self.asked_again.load(Ordering::SeqCst), Ordering::SeqCst);
        }
        self.bytes.read(into)
    }
}

struct OffersOnceThenWatches {
    source: SourceId,
    asked: AtomicUsize,
    asked_again: Arc<AtomicBool>,
    overlapped: Arc<AtomicBool>,
}

impl Provider for OffersOnceThenWatches {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _: &Identity) -> ProvidedResult<Obtained> {
        if self.asked.fetch_add(1, Ordering::SeqCst) > 0 {
            self.asked_again.store(true, Ordering::SeqCst);
            return Ok(Obtained::Nothing);
        }
        Ok(Obtained::Found(Delivery::Stream {
            key: "track/55391743".into(),
            extension: Extension::new("wav")?,
            opening: Opening::ready(HeldUntilAskedAgain {
                asked_again: Arc::clone(&self.asked_again),
                overlapped: Arc::clone(&self.overlapped),
                bytes: io::Cursor::new(Wav::new().frames(8_820).build()),
                waited: false,
            }),
        }))
    }
}

#[test]
fn a_lane_asks_about_its_next_want_while_the_last_delivery_is_kept() -> Result<()> {
    let (_tree, library) = two_wants_filed_in_the_music_folder()?;
    let provider = Arc::new(OffersOnceThenWatches {
        source: SourceId::new("shop").expect("a nameable source"),
        asked: AtomicUsize::new(0),
        asked_again: Arc::new(AtomicBool::new(false)),
        overlapped: Arc::new(AtomicBool::new(false)),
    });

    let summary = library
        .poll(
            Arc::new(Providers::none().and(Arc::clone(&provider) as Arc<dyn Provider>)),
            PollOptions {
                lanes: NonZeroUsize::MIN,
                ..PollOptions::default()
            },
        )?
        .join()?;

    assert!(
        provider.overlapped.load(Ordering::SeqCst),
        "the lane waited for the keep before asking about the next want"
    );
    assert_eq!(summary.stats.asked, 2);
    assert_eq!(summary.stats.kept, 1);
    assert_eq!(summary.stats.nothing, 1);
    Ok(())
}

#[test]
fn a_delivery_with_no_vault_is_filed_in_the_music_folder_and_joins_the_album_it_was_wanted_for()
-> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;

    wanted_harbour_lights(&library)?;
    library.deliver_into(Some(DeliveryFolder {
        path: tree.path().to_path_buf(),
        layout: Layout::default(),
    }));

    let shop = Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/55391743",
            extension: "wav",
            bytes: Wav::new().frames(8_820).build(),
        },
    ));
    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 1);
    assert_eq!(summary.stats.unkept, 0);

    let wants = library.wants()?;
    let held = wants[0].held.expect("the filed file is the want's row");
    let track = library.track(held)?.expect("the filed row");
    let path = track
        .location
        .as_path()
        .expect("a local file")
        .to_path_buf();
    let root = tree.path().canonicalize().expect("the tree");

    assert!(path.starts_with(&root), "{}", path.display());
    assert!(path.is_file());
    assert_eq!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("wav")
    );
    assert!(
        !track.delivered,
        "a filed row belongs to its root, not the vault"
    );
    assert_eq!(track.title, "Harbour Lights");
    assert_eq!(track.album_id, Some(album.id));
    assert_eq!(
        wants[0].offered.as_deref(),
        Some(MediaLocation::local(&path).to_uri().as_str())
    );

    scan(&library, &options(&tree))?;

    let again = library.track(held)?.expect("the row a rescan kept");

    assert_eq!(again.album_id, Some(album.id));
    assert_eq!(library.wants()?[0].held, Some(held));
    Ok(())
}

const WHOLE_SONG_FRAMES: usize = 40_000;

fn harbour_lights_bytes() -> Vec<u8> {
    Wav::new().frames(WHOLE_SONG_FRAMES).build()
}

fn harbour_lights_wanted_into_the_music_folder() -> Result<(Tree, Library)> {
    let (tree, library) = scanned_orbits()?;

    wanted_harbour_lights(&library)?;
    library.deliver_into(Some(DeliveryFolder {
        path: tree.path().to_path_buf(),
        layout: Layout::default(),
    }));
    Ok((tree, library))
}

fn harbour_lights_offered() -> Arc<Offering> {
    Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/55391743",
            extension: "wav",
            bytes: harbour_lights_bytes(),
        },
    ))
}

#[test]
fn a_queued_download_waits_while_the_connection_is_metered_and_goes_on_when_it_is_not() -> Result<()>
{
    let (_tree, library) = harbour_lights_wanted_into_the_music_folder()?;
    let metering = Metering::new();

    metering.report(Meter::Metered);
    library.stall_downloads_by(MeteredGate::new(metering.clone(), true));

    let shop = harbour_lights_offered();

    let polling = library.poll(shop.registered(), PollOptions::default())?;
    let progress = Arc::clone(polling.progress());

    assert!(waited_for(|| progress.is_waiting_on_the_meter()));

    thread::sleep(THE_POLL_HAS_WAITED);

    assert!(progress.is_waiting_on_the_meter());
    assert!(shop.asked().is_empty(), "a stalled poll asked a provider");
    assert_eq!(progress.snapshot().offered, 0);
    assert_eq!(library.wants()?[0].tried, None);
    assert_eq!(library.wants()?[0].misses, 0);

    metering.report(Meter::Unmetered);

    let summary = polling.join()?;

    assert!(!progress.is_waiting_on_the_meter());
    assert_eq!(shop.asked().len(), 1);
    assert_eq!(summary.stats.kept, 1);
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn a_queued_download_goes_on_when_the_listener_turns_the_setting_off() -> Result<()> {
    let (_tree, library) = harbour_lights_wanted_into_the_music_folder()?;
    let metering = Metering::new();

    metering.report(Meter::Metered);

    let gate = MeteredGate::new(metering, true);

    library.stall_downloads_by(gate.clone());

    let shop = harbour_lights_offered();

    let polling = library.poll(shop.registered(), PollOptions::default())?;
    let progress = Arc::clone(polling.progress());

    assert!(waited_for(|| progress.is_waiting_on_the_meter()));

    gate.shut_while_metered(false);

    let summary = polling.join()?;

    assert_eq!(summary.stats.kept, 1);
    Ok(())
}

#[test]
fn a_connection_that_cannot_be_told_is_not_a_reason_to_hold_a_download() -> Result<()> {
    let (_tree, library) = harbour_lights_wanted_into_the_music_folder()?;

    library.stall_downloads_by(MeteredGate::new(Metering::new(), true));

    let shop = harbour_lights_offered();

    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.kept, 1);
    Ok(())
}

#[test]
fn a_metered_connection_holds_nothing_back_unless_the_setting_asks() -> Result<()> {
    let (_tree, library) = harbour_lights_wanted_into_the_music_folder()?;
    let metering = Metering::new();

    metering.report(Meter::Metered);
    library.stall_downloads_by(MeteredGate::new(metering, false));

    let shop = harbour_lights_offered();

    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.kept, 1);
    Ok(())
}

#[test]
fn a_poll_cancelled_while_it_waits_for_the_connection_ends_without_asking() -> Result<()> {
    let (_tree, library) = harbour_lights_wanted_into_the_music_folder()?;
    let metering = Metering::new();

    metering.report(Meter::Metered);
    library.stall_downloads_by(MeteredGate::new(metering, true));

    let shop = harbour_lights_offered();

    let polling = library.poll(shop.registered(), PollOptions::default())?;
    let progress = Arc::clone(polling.progress());

    assert!(waited_for(|| progress.is_waiting_on_the_meter()));

    polling.cancel();

    let summary = polling.join()?;

    assert!(summary.cancelled);
    assert!(shop.asked().is_empty());
    assert_eq!(summary.stats.kept, 0);
    Ok(())
}

enum TurnsMetered {
    WhenAsked,
    WhenToldTo,
}

struct ReadsSlowly {
    inner: io::Cursor<Vec<u8>>,
}

impl io::Read for ReadsSlowly {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        thread::sleep(Duration::from_millis(150));
        self.inner.read(into)
    }
}

struct TurnsMeteredDuring {
    source: SourceId,
    metering: Metering,
    when: TurnsMetered,
    opened: Arc<AtomicUsize>,
}

impl Provider for TurnsMeteredDuring {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _identity: &Identity) -> ProvidedResult<Obtained> {
        let bytes = harbour_lights_bytes();
        let opened = Arc::clone(&self.opened);

        let opening = match self.when {
            TurnsMetered::WhenAsked => {
                self.metering.report(Meter::Metered);
                Opening::new(move || {
                    opened.fetch_add(1, Ordering::SeqCst);
                    Opening::ready(io::Cursor::new(bytes)).open()
                })
            }
            TurnsMetered::WhenToldTo => Opening::new(move || {
                opened.fetch_add(1, Ordering::SeqCst);
                Opening::ready(ReadsSlowly {
                    inner: io::Cursor::new(bytes),
                })
                .open()
            }),
        };
        Ok(Obtained::Found(Delivery::Stream {
            key: "track/55391743".into(),
            extension: Extension::new("wav")?,
            opening,
        }))
    }
}

struct TurningMetered {
    _tree: Tree,
    library: Library,
    metering: Metering,
    opened: Arc<AtomicUsize>,
    providers: Arc<Providers>,
}

fn turning_metered(when: TurnsMetered) -> Result<TurningMetered> {
    let (tree, library) = harbour_lights_wanted_into_the_music_folder()?;
    let metering = Metering::new();

    library.stall_downloads_by(MeteredGate::new(metering.clone(), true));

    let opened = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(TurnsMeteredDuring {
        source: SourceId::new("shop").expect("a nameable source"),
        metering: metering.clone(),
        when,
        opened: Arc::clone(&opened),
    });
    let providers = Arc::new(Providers::none().and(provider as Arc<dyn Provider>));
    Ok(TurningMetered {
        _tree: tree,
        library,
        metering,
        opened,
        providers,
    })
}

#[test]
fn an_offer_found_as_the_connection_turned_metered_is_not_opened_until_it_is_not() -> Result<()> {
    let TurningMetered {
        _tree,
        library,
        metering,
        opened,
        providers,
    } = turning_metered(TurnsMetered::WhenAsked)?;

    let polling = library.poll(providers, PollOptions::default())?;
    let progress = Arc::clone(polling.progress());

    assert!(waited_for(|| progress.is_waiting_on_the_meter()));

    thread::sleep(THE_POLL_HAS_WAITED);

    assert_eq!(opened.load(Ordering::SeqCst), 0);

    metering.report(Meter::Unmetered);

    let summary = polling.join()?;

    assert_eq!(opened.load(Ordering::SeqCst), 1);
    assert_eq!(summary.stats.kept, 1);
    Ok(())
}

#[test]
fn a_download_under_way_stops_reading_when_the_connection_turns_metered_and_finishes_when_it_is_not()
-> Result<()> {
    let TurningMetered {
        _tree,
        library,
        metering,
        providers,
        ..
    } = turning_metered(TurnsMetered::WhenToldTo)?;

    let polling = library.poll(providers, PollOptions::default())?;
    let progress = Arc::clone(polling.progress());
    let began = Instant::now();

    while progress.received() == 0 {
        assert!(began.elapsed() < Duration::from_secs(10), "nothing arrived");

        thread::sleep(Duration::from_millis(2));
    }
    metering.report(Meter::Metered);

    assert!(waited_for(|| progress.is_waiting_on_the_meter()));

    let received = progress.received();

    assert!(
        received > 0,
        "nothing was read before the connection turned"
    );
    assert!(received < harbour_lights_bytes().len() as u64);

    thread::sleep(THE_POLL_HAS_WAITED);

    assert_eq!(progress.received(), received);

    metering.report(Meter::Unmetered);

    let summary = polling.join()?;

    assert_eq!(summary.stats.kept, 1);
    assert_eq!(summary.stats.late, 0);
    Ok(())
}

fn wanted_lasting(library: &Library, length: Option<Duration>) -> Result<WantId> {
    let album = only_album(library)?;
    let mut rows = orbits_rows();

    rows.push(ReleaseTrack {
        length,
        ..release_row(4, "Harbour Lights", Vec::new())
    });
    library.land_release(album.id, &orbits(rows, Vec::new()))?;

    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "Harbour Lights")
        .expect("the release holds the row");
    library.want(missing.id)
}

fn handed_over(poll: &Mutex<Option<Arc<PollProgress>>>) -> Arc<PollProgress> {
    let started = Instant::now();
    loop {
        if let Some(progress) = poll.lock().as_ref() {
            return Arc::clone(progress);
        }

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the poll was never handed to the provider"
        );

        thread::sleep(Duration::from_millis(5));
    }
}

struct Watching {
    source: SourceId,
    delivers: Option<Vec<u8>>,
    poll: Mutex<Option<Arc<PollProgress>>>,
    named: Arc<Mutex<Vec<Option<SourceId>>>>,
    received: Arc<Mutex<Vec<u64>>>,
}

struct ReadWatched {
    bytes: io::Cursor<Vec<u8>>,
    progress: Arc<PollProgress>,
    received: Arc<Mutex<Vec<u64>>>,
}

impl io::Read for ReadWatched {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.received.lock().push(self.progress.received());
        io::Read::read(&mut self.bytes, into)
    }
}

impl Provider for Watching {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _: &Identity) -> ProvidedResult<Obtained> {
        let progress = handed_over(&self.poll);

        self.named.lock().push(progress.asking_provider());

        Ok(match &self.delivers {
            None => Obtained::Nothing,
            Some(bytes) => Obtained::Found(Delivery::Stream {
                key: "track/1".into(),
                extension: Extension::new("wav")?,
                opening: Opening::ready(ReadWatched {
                    bytes: io::Cursor::new(bytes.clone()),
                    progress,
                    received: Arc::clone(&self.received),
                }),
            }),
        })
    }
}

#[test]
fn a_poll_names_the_provider_it_is_asking_and_counts_what_it_received() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, None)?;

    let bytes = Wav::new()
        .text(TITLE, "Harbour Lights")
        .frames(400_000)
        .build();
    let size = bytes.len() as u64;
    let named = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::new(Mutex::new(Vec::new()));

    let watching = |name: &str, delivers: Option<Vec<u8>>| {
        Arc::new(Watching {
            source: source(name),
            delivers,
            poll: Mutex::new(None),
            named: Arc::clone(&named),
            received: Arc::clone(&received),
        })
    };
    let quiet = watching("quiet", None);
    let shop = watching("shop", Some(bytes));

    let handle = library.poll(
        Arc::new(
            Providers::none()
                .and(Arc::clone(&quiet) as Arc<dyn Provider>)
                .and(Arc::clone(&shop) as Arc<dyn Provider>),
        ),
        PollOptions::default(),
    )?;
    let progress = Arc::clone(handle.progress());

    *quiet.poll.lock() = Some(Arc::clone(&progress));
    *shop.poll.lock() = Some(Arc::clone(&progress));

    let summary = handle.join()?;

    assert_eq!(summary.stats.kept, 1);
    assert_eq!(
        *named.lock(),
        [Some(source("quiet")), Some(source("shop"))],
        "a provider was not named while it was asked"
    );

    let most = received.lock().iter().copied().max().unwrap_or_default();

    assert!(
        most > 0 && most <= size,
        "{most} of {size} bytes were counted as the stream was read"
    );
    assert_eq!(progress.asking_provider(), None);
    assert_eq!(progress.received(), 0);
    Ok(())
}

#[test]
fn a_file_delivered_into_the_vault_counts_the_bytes_it_was_as_received() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let bytes = Wav::new()
        .text(TITLE, "Harbour Lights")
        .frames(4_000_000)
        .build();
    let size = bytes.len() as u64;

    let delivered = tree.write("delivered.wav", &bytes);

    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, None)?;

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));
    let handle = library.poll(inbox.registered(), PollOptions::default())?;
    let progress = Arc::clone(handle.progress());
    let sampled = Arc::new(AtomicBool::new(false));
    let counted = thread::spawn({
        let sampled = Arc::clone(&sampled);
        move || {
            let mut most = 0;

            while !sampled.load(Ordering::Relaxed) {
                most = most.max(progress.received());
                thread::sleep(Duration::from_micros(200));
            }
            most
        }
    });
    let summary = handle.join()?;

    sampled.store(true, Ordering::Relaxed);

    let most = counted.join().expect("the sampling thread");

    assert_eq!(summary.stats.kept, 1);
    assert_eq!(most, size, "a file delivered was never counted as received");
    Ok(())
}

#[test]
fn a_poll_asking_the_inbox_alone_leaves_what_it_lacks_untried_and_lands_what_it_holds() -> Result<()>
{
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new()
            .text(TITLE, "Harbour Lights")
            .frames(8_820)
            .build(),
    );
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;

    let want = wanted_harbour_lights(&library)?;

    let empty = Arc::new(Offering::new("inbox", Delivering::Nothing));
    let server = Arc::new(Offering::new("server", Delivering::File(delivered.clone())));

    let every_provider = Providers::none()
        .and(Arc::clone(&empty) as Arc<dyn Provider>)
        .and(Arc::clone(&server) as Arc<dyn Provider>);
    let summary = library
        .poll(
            Arc::new(every_provider.only(&source("inbox"))),
            PollOptions::ASKING_EVERY_WANT,
        )?
        .join()?;

    assert_eq!(summary.stats.asked, 1);
    assert_eq!(summary.stats.nothing, 0);
    assert_eq!(empty.asked().len(), 1);
    assert!(server.asked().is_empty(), "the network was asked");

    let untried = library.wants()?[0].clone();

    assert_eq!((untried.tried, untried.misses), (None, 0));
    assert!(library.is_a_want_due(PollOptions::default())?);

    let holding = Arc::new(Offering::new("inbox", Delivering::File(delivered)));
    let every_provider = Providers::none()
        .and(Arc::clone(&holding) as Arc<dyn Provider>)
        .and(Arc::clone(&server) as Arc<dyn Provider>);
    let summary = library
        .poll(
            Arc::new(every_provider.only(&source("inbox"))),
            PollOptions::ASKING_EVERY_WANT,
        )?
        .join()?;

    assert_eq!(summary.stats.kept, 1);
    assert!(server.asked().is_empty());

    let landed = library.wants()?[0].clone();

    assert_eq!(landed.id, want);
    assert!(landed.held.is_some(), "what the inbox held was not landed");
    Ok(())
}

fn half_of_harbour_lights() -> Arc<Offering> {
    Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/half",
            extension: "wav",
            bytes: Wav::new()
                .text(TITLE, "Harbour Lights")
                .frames(8_820)
                .build(),
        },
    ))
}

#[test]
fn a_delivery_not_as_long_as_the_wanted_track_is_refused_and_waits_as_a_miss_does() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(60)))?;

    let summary = library
        .poll(
            half_of_harbour_lights().registered(),
            PollOptions::default(),
        )?
        .join()?;

    assert_eq!(
        (
            summary.stats.offered,
            summary.stats.kept,
            summary.stats.unkept
        ),
        (1, 0, 1)
    );
    assert!(
        library.vault_objects()?.is_empty(),
        "a delivery of the wrong length was noted"
    );
    assert!(
        all(&library)?
            .iter()
            .all(|track| track.title != "Harbour Lights")
    );

    let refused = library.wants()?[0].clone();

    assert_eq!((refused.held, refused.offered), (None, None));
    assert_eq!(refused.misses, 1);
    assert!(!library.is_a_want_due(PollOptions::default())?);
    Ok(())
}

fn the_whole_of_harbour_lights(provider: &str) -> Arc<Offering> {
    Arc::new(Offering::new(
        provider,
        Delivering::Bytes {
            key: "track/whole",
            extension: "wav",
            bytes: Wav::new()
                .text(TITLE, "Harbour Lights")
                .frames(441_000)
                .build(),
        },
    ))
}

#[test]
fn a_delivery_refused_from_one_provider_is_asked_of_the_next() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let wrong = half_of_harbour_lights();
    let right = the_whole_of_harbour_lights("store");

    let summary = library
        .poll(asking_in_turn(&[&wrong, &right]), PollOptions::default())?
        .join()?;

    assert_eq!(
        (
            summary.stats.offered,
            summary.stats.kept,
            summary.stats.unkept
        ),
        (2, 1, 1)
    );
    assert_eq!(wrong.asked.lock().len(), 1);
    assert_eq!(right.asked.lock().len(), 1);
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn an_offer_that_cannot_be_opened_falls_through_to_the_next_provider() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let broken = Arc::new(Offering::new("broken", Delivering::Unopenable));
    let gone = Arc::new(Offering::new("gone", Delivering::GoneWhenOpened));
    let right = the_whole_of_harbour_lights("store");

    let summary = library
        .poll(
            asking_in_turn(&[&broken, &gone, &right]),
            PollOptions::default(),
        )?
        .join()?;

    assert_eq!(
        (
            summary.stats.offered,
            summary.stats.kept,
            summary.stats.refused
        ),
        (3, 1, 1)
    );
    assert_eq!((broken.opened(), gone.opened(), right.opened()), (1, 1, 1));
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn an_offer_found_gone_is_told_to_its_provider_which_is_asked_again_without_it() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let only = Arc::new(Offering::new(
        "only",
        Delivering::GoneUntilTold {
            bytes: Wav::new()
                .text(TITLE, "Harbour Lights")
                .frames(441_000)
                .build(),
        },
    ));
    let linked = MediaLocation::new(
        SourceId::new("only").expect("a nameable source"),
        "track/linked",
    );

    let summary = library
        .poll(asking_in_turn(&[&only]), PollOptions::default())?
        .join()?;

    let asked = only.asked();

    assert_eq!(asked.len(), 2);
    assert!(asked[0].gone.is_empty());
    assert_eq!(asked[1].gone, [linked]);
    assert_eq!((summary.stats.offered, summary.stats.kept), (2, 1));
    assert_eq!(summary.stats.nothing, 0);
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn an_offer_the_race_did_not_take_is_never_opened() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let first = Arc::new(
        Arc::into_inner(the_whole_of_harbour_lights("first"))
            .expect("one holder")
            .answering_after(TURNED_TO_APART + Duration::from_millis(500)),
    );
    let second = the_whole_of_harbour_lights("second");

    let summary = library
        .poll(asking_in_turn(&[&first, &second]), PollOptions::default())?
        .join()?;

    assert_eq!((summary.stats.offered, summary.stats.kept), (1, 1));
    assert_eq!(
        second.asked().len(),
        1,
        "the later provider was never raced"
    );
    assert_eq!((first.opened(), second.opened()), (1, 0));
    Ok(())
}

#[test]
fn an_offer_held_back_by_the_race_is_opened_when_the_one_taken_cannot_be() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let first = Arc::new(
        Offering::new("first", Delivering::Unopenable)
            .answering_after(TURNED_TO_APART + Duration::from_millis(500)),
    );
    let second = the_whole_of_harbour_lights("second");

    let summary = library
        .poll(asking_in_turn(&[&first, &second]), PollOptions::default())?
        .join()?;

    assert_eq!((summary.stats.offered, summary.stats.kept), (2, 1));
    assert_eq!(
        (first.asked().len(), second.asked().len()),
        (1, 1),
        "a provider was asked again for an offer it had made"
    );
    assert_eq!((first.opened(), second.opened()), (1, 1));
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn an_offer_refused_as_not_the_song_is_declined_by_the_next_poll_which_starts_elsewhere()
-> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let wrong = half_of_harbour_lights();

    library
        .poll(wrong.registered(), PollOptions::default())?
        .join()?;

    let right = the_whole_of_harbour_lights("store");

    let summary = library
        .poll(
            asking_in_turn(&[&wrong, &right]),
            PollOptions::ASKING_EVERY_WANT,
        )?
        .join()?;

    assert_eq!((summary.stats.offered, summary.stats.kept), (1, 1));
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn a_refused_offer_is_offered_again_once_the_want_is_asked_for_again() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let wrong = half_of_harbour_lights();

    library
        .poll(wrong.registered(), PollOptions::default())?
        .join()?;

    let row = library.wants()?[0].release_track;

    let declined = library
        .poll(wrong.registered(), PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    library.want(row)?;

    let offered_again = library
        .poll(wrong.registered(), PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert_eq!(declined.stats.offered, 0);
    assert_eq!(offered_again.stats.offered, 1);
    Ok(())
}

#[test]
fn a_filing_not_as_long_as_the_wanted_track_is_taken_away_and_waits_as_a_miss_does() -> Result<()> {
    let (tree, library) = scanned_orbits()?;

    wanted_lasting(&library, Some(Duration::from_secs(60)))?;
    library.deliver_into(Some(DeliveryFolder {
        path: tree.path().to_path_buf(),
        layout: Layout::default(),
    }));

    let before = every_file_under(tree.path());

    let summary = library
        .poll(
            half_of_harbour_lights().registered(),
            PollOptions::default(),
        )?
        .join()?;

    assert_eq!(
        (
            summary.stats.offered,
            summary.stats.kept,
            summary.stats.unkept
        ),
        (1, 0, 1)
    );
    assert_eq!(
        every_file_under(tree.path()),
        before,
        "a filing of the wrong length was left in the music folder"
    );
    assert!(
        all(&library)?
            .iter()
            .all(|track| track.title != "Harbour Lights")
    );

    let refused = library.wants()?[0].clone();

    assert_eq!((refused.held, refused.offered), (None, None));
    assert_eq!(refused.misses, 1);
    assert!(!library.is_a_want_due(PollOptions::default())?);
    Ok(())
}

struct PairedAsItAnswers {
    source: SourceId,
    file: PathBuf,
    library: Arc<Library>,
    scanning: ScanOptions,
}

impl Provider for PairedAsItAnswers {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _: &Identity) -> ProvidedResult<Obtained> {
        scan(&self.library, &self.scanning).expect("the listener's own file is scanned");

        let album = only_album(&self.library).expect("the album the file joined");
        let paired = self
            .library
            .rematch(album.id)
            .expect("the album's rows are paired");

        assert!(paired > 0, "the listener's own file was not paired");
        Ok(Obtained::Found(Delivery::File(self.file.clone())))
    }
}

#[test]
fn a_delivery_landing_after_the_wanted_row_was_paired_with_the_listeners_own_file_leaves_it_paired()
-> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new()
            .text(TITLE, "Harbour Lights")
            .frames(8_820)
            .build(),
    );
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;
    let library = Arc::new(library);

    scan(&library, &options(&orbits))?;

    let want = wanted_harbour_lights(&library)?;

    orbits.write("4.wav", &orbits_track("Harbour Lights", 4).build());

    let provider = Arc::new(PairedAsItAnswers {
        source: source("inbox"),
        file: delivered,
        library: Arc::clone(&library),
        scanning: options(&orbits),
    });
    let summary = library
        .poll(
            Arc::new(Providers::none().and(provider as Arc<dyn Provider>)),
            PollOptions::default(),
        )?
        .join()?;

    assert_eq!((summary.stats.kept, summary.stats.unkept), (0, 1));

    let harbour: Vec<Track> = all(&library)?
        .into_iter()
        .filter(|track| track.title == "Harbour Lights")
        .collect();

    assert_eq!(
        harbour.len(),
        1,
        "the delivery became a row beside the file"
    );
    assert!(!harbour[0].delivered);

    let wants = library.wants()?;

    assert_eq!(wants[0].id, want);
    assert_eq!(
        wants[0].held,
        Some(harbour[0].id),
        "the delivery took the pairing from the listener's own file"
    );
    assert!(library.vault_objects()?.is_empty());
    Ok(())
}

#[test]
fn a_forgotten_delivery_is_not_fetched_again_and_another_providers_is_landed_instead() -> Result<()>
{
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Lantern").frames(8_820).build(),
    );
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));

    library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    let object = library.vault_objects()?[0].path.clone();

    assert!(library.forget_delivered(&object)?);

    let forgotten = library.wants()?[0].clone();

    assert_eq!(
        (forgotten.held, forgotten.offered, forgotten.tried),
        (None, None, None)
    );
    assert!(
        library.is_a_want_due(PollOptions::default())?,
        "a want whose delivery was forgotten waits out an offer's six hours"
    );

    let want = wanted_harbour_lights(&library)?;
    let summary = library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(inbox.asked().len(), 2);
    assert_eq!((summary.stats.offered, summary.stats.kept), (0, 0));
    assert_eq!(summary.stats.nothing, 1);
    assert_eq!(library.wants()?[0].held, None);

    let shop = Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/55391743",
            extension: "wav",
            bytes: Wav::new()
                .text(TITLE, "Harbour Lights")
                .frames(4_410)
                .build(),
        },
    ));
    let providers = Arc::new(
        Providers::none()
            .and(Arc::clone(&inbox) as Arc<dyn Provider>)
            .and(Arc::clone(&shop) as Arc<dyn Provider>),
    );
    let summary = library
        .poll(providers, PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert_eq!(inbox.asked().len(), 3);
    assert_eq!(shop.asked().len(), 1);
    assert_eq!(summary.stats.kept, 1);

    let landed = library.wants()?[0].clone();

    assert_eq!(landed.id, want);

    let row = library
        .track(landed.held.expect("the second provider's delivery is held"))?
        .expect("the delivered row");
    let object = library
        .vault_objects()?
        .into_iter()
        .find(|object| row.location.as_path() == Some(object.path.as_path()))
        .expect("the row's object");

    assert_eq!(object.taken_from, Path::new("shop:track/55391743"));
    Ok(())
}

#[test]
fn a_delivery_forgotten_once_a_second_provider_brought_the_same_audio_is_declined_from_both()
-> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let song = Wav::new()
        .text(TITLE, "Harbour Lights")
        .frames(4_410)
        .build();

    let delivered = tree.write("delivered.wav", &song);
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));
    let shop = Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/55391743",
            extension: "wav",
            bytes: song,
        },
    ));

    let providers = Arc::new(
        Providers::none()
            .and(Arc::clone(&inbox) as Arc<dyn Provider>)
            .and(Arc::clone(&shop) as Arc<dyn Provider>),
    );

    library
        .poll(Arc::clone(&providers), PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    let object = library.vault_objects()?[0].path.clone();

    assert!(library.forget_delivered(&object)?);

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert_eq!(
        summary.stats.kept, 1,
        "the second provider's delivery was not kept"
    );
    assert_eq!(shop.asked().len(), 1);
    assert_eq!(
        library.vault_objects()?.len(),
        1,
        "the same audio was kept twice"
    );
    assert!(library.forget_delivered(&object)?);

    let summary = library
        .poll(providers, PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert_eq!(
        summary.stats.kept, 0,
        "a provider whose delivery was forgotten delivered it again"
    );
    assert_eq!(library.wants()?[0].held, None);
    Ok(())
}

#[test]
fn a_file_put_back_in_the_inbox_after_its_delivery_was_forgotten_is_delivered_again() -> Result<()>
{
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Lantern").frames(8_820).build(),
    );
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered.clone())));

    library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    let object = library.vault_objects()?[0].path.clone();

    assert!(library.forget_delivered(&object)?);

    thread::sleep(Duration::from_millis(50));
    tree.write(
        "delivered.wav",
        &Wav::new()
            .text(TITLE, "Harbour Lights")
            .frames(6_615)
            .build(),
    );

    let summary = library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.kept, 1, "the file put back was declined");
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn a_delivery_the_vault_refuses_waits_longer_before_it_is_fetched_again() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_harbour_lights(&library)?;

    let shop = Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/silent",
            extension: "wav",
            bytes: Wav::new().frames(0).build(),
        },
    ));

    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!((summary.stats.offered, summary.stats.unkept), (1, 1));

    let refused = library.wants()?[0].clone();

    assert_eq!(refused.misses, 1);
    assert_eq!(
        refused.due_at(),
        refused.tried.map(|tried| tried + RETRY_WAITS[0])
    );

    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(
        summary.stats.asked, 0,
        "a refused delivery was fetched again at once"
    );
    assert_eq!(shop.asked().len(), 1);
    Ok(())
}

#[test]
fn a_last_resort_is_not_asked_in_a_later_round_after_an_earlier_provider_refused() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let broken = Unanswering::new("broken", unreachable_server);
    let wrong = half_of_harbour_lights();
    let tube = the_whole_of_harbour_lights("tube");

    let providers = Arc::new(
        Providers::none()
            .and(Arc::clone(&broken) as Arc<dyn Provider>)
            .and(Arc::clone(&wrong) as Arc<dyn Provider>)
            .and(Arc::new(LastResort(Arc::clone(&tube))) as Arc<dyn Provider>),
    );

    let summary = library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 0);
    assert!(tube.asked().is_empty());

    let wants = library.wants()?;

    assert!(wants[0].held.is_none());
    assert_eq!(wants[0].tried, None);
    Ok(())
}

#[test]
fn a_last_resort_is_asked_after_an_earlier_offer_was_not_the_song() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_lasting(&library, Some(Duration::from_secs(10)))?;

    let wrong = half_of_harbour_lights();
    let tube = the_whole_of_harbour_lights("tube");

    let providers = Arc::new(
        Providers::none()
            .and(Arc::clone(&wrong) as Arc<dyn Provider>)
            .and(Arc::new(LastResort(Arc::clone(&tube))) as Arc<dyn Provider>),
    );

    let summary = library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!((summary.stats.offered, summary.stats.kept), (2, 1));
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn a_last_resort_is_never_foreseen_nor_started_by_a_search() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;

    let fake = low_tide_fake();
    let found = library.found_elsewhere(&fake, "lantern")?;
    let empty = Arc::new(Offering::new("shop", Delivering::Nothing));
    let tube = a_provider_offering_slow_water("tube");

    let providers = Providers::none()
        .and(Arc::clone(&empty) as Arc<dyn Provider>)
        .and(Arc::new(LastResort(Arc::clone(&tube))) as Arc<dyn Provider>);

    assert!(!library.foresee(&providers, &found[0]));
    assert!(tube.asked().is_empty());
    Ok(())
}

#[test]
fn a_refusal_heard_while_foreseeing_bars_the_last_resort_and_the_miss_from_the_poll() -> Result<()>
{
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;

    let mut release = low_tide_release();

    for row in &mut release.media[0].tracks {
        row.length = Some(Duration::from_secs(1_411));
    }

    let fake = Fake::new(Canned {
        found_songs: vec![slow_water_found()],
        releases: vec![release],
        ..Canned::default()
    });
    let found = library.found_elsewhere(&fake, "lantern")?;
    let broken = Unanswering::new("broken", unreachable_server);
    let short = a_provider_offering_slow_water("shop");
    let tube = a_provider_offering_slow_water("tube");

    let providers = Arc::new(
        Providers::none()
            .and(Arc::clone(&broken) as Arc<dyn Provider>)
            .and(Arc::clone(&short) as Arc<dyn Provider>)
            .and(Arc::new(LastResort(Arc::clone(&tube))) as Arc<dyn Provider>),
    );

    assert!(library.foresee(&providers, &found[0]));

    library.want_found(&fake, &found[0])?;

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;

    assert!(tube.asked().is_empty());
    assert_eq!(summary.stats.refused, 1);

    let wants = library.wants()?;

    assert_eq!(wants[0].tried, None);
    assert_eq!(wants[0].misses, 0);
    assert!(wants[0].held.is_none());
    Ok(())
}
