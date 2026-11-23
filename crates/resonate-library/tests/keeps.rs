use std::{
    collections::{BTreeMap, VecDeque},
    env, fs, io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use parking_lot::Mutex;
use resonate_codec::Writing;
use resonate_core::{Gate as MeteredGate, MediaLocation, Meter, Metering, SourceId, Span, writer};
use resonate_library::{
    CoverArt, Cut, DeliveryFolder, Direction, Error, FileTags, ImageFormat, KeepStopped,
    KeepSummary, KeptList, KeptSong, Layout, Library, OrganiseOptions, Picturing, PlaylistToKeep,
    PollOptions, PollSummary, Result, SavedQuery, Service, SongToKeep, SortOrder, Sources,
    TagSink as _, TagSource as _, Track, TrackQuery,
};
use resonate_providers::{
    Delivery, Error as ProvidedError, Extension, Identity, Obtained, Opened, Opening, Provider,
    ProviderOp, Providers, Result as ProvidedResult, WhyAway,
};

use crate::support::{
    DiesPartWay, RATE, Stall, THE_POLL_HAS_WAITED, Tree, Wav, WhenAsked, a_music_folder,
    a_music_folder_on_disk, beside, delivering_into, ffmpeg, files_under, followed, held_as,
    only_album, opened_with_a_vault, options, orbits, orbits_rows, orbits_tree, png, release_row,
    scan, scanned_orbits, songs_under, source, waited_for,
};

fn in_this_suite(test: &str) -> String {
    let (_, module) = module_path!()
        .split_once("::")
        .expect("a module of the suite");
    format!("{module}::{test}")
}

fn left_long_ago(path: &Path) {
    let long_ago = SystemTime::now() - writer::UNHELD_AND_UNTOUCHED_FOR * 2;

    fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(long_ago))
        .expect("a file left long ago");
}

const A_SECOND_OF_FRAMES: usize = RATE as usize;

const LONGER_THAN_ANY_LENGTH_AGREES: usize = 8 * RATE as usize;

const WATCH_ADDRESS: &str = "https://music.youtube.com/watch?v=";

const KEPT_BY: &str = "tube";

const KILLED_KEEP_FOLDER: &str = "RESONATE_KILLED_KEEP_FOLDER";

const UNREAD_KEEP_FOLDER: &str = "RESONATE_UNREAD_KEEP_FOLDER";

const A_KILLED_KEEP_IS_STAGED_WITHIN: Duration = Duration::from_secs(60);

const STAGED_DELIVERY: &str = ".resonate-delivery";

const A_REFUSED_WALK_HOLDS_NO_SONG_UP_FOR: Duration = Duration::from_secs(30);

const A_WALK_ENDS_AFTER_THE_LANDING: Duration = Duration::from_millis(1_500);

#[derive(Clone, Copy)]
enum TubeAnswer {
    Lasting(usize),
    Nothing,
    Gone,
    Undecodable,
    Away,
    Lapsed,
    Busy,
    DyingAway,
    Hanging,
}

struct Tube {
    source: SourceId,
    answers: Mutex<BTreeMap<String, VecDeque<TubeAnswer>>>,
    asked: Mutex<Vec<Identity>>,
    when_asked: Mutex<BTreeMap<String, WhenAsked>>,
    pictures_asked: Mutex<Vec<String>>,
}

const A_PICTURE_THE_TUBE_REFUSES: &str = "https://i.ytimg.com/gone";

const A_PICTURE_THE_TUBE_NAMES: &str = "https://i.ytimg.com/vi/";

impl Tube {
    fn named(name: &str) -> Arc<Self> {
        Arc::new(Self {
            source: source(name),
            answers: Mutex::new(BTreeMap::new()),
            asked: Mutex::new(Vec::new()),
            when_asked: Mutex::new(BTreeMap::new()),
            pictures_asked: Mutex::new(Vec::new()),
        })
    }

    fn new() -> Arc<Self> {
        Self::named(KEPT_BY)
    }

    fn when_asked_for(&self, video: &str, then: impl FnOnce() + Send + 'static) {
        self.when_asked
            .lock()
            .insert(video.to_owned(), Box::new(then));
    }

    fn answering(&self, video: &str, answers: &[TubeAnswer]) {
        self.answers
            .lock()
            .insert(video.to_owned(), answers.iter().copied().collect());
    }

    fn videos_asked(&self) -> Vec<String> {
        self.asked.lock().iter().map(video_of).collect()
    }

    fn registered(self: &Arc<Self>) -> Providers {
        Providers::none().and(Arc::clone(self) as Arc<dyn Provider>)
    }

    fn stream(&self, video: &str, opening: Opening) -> ProvidedResult<Obtained> {
        Ok(Obtained::Found(Delivery::Stream {
            key: format!("track/{video}").into(),
            extension: Extension::new("wav")?,
            opening,
        }))
    }
}

impl Provider for Tube {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn picture(&self, address: &str) -> ProvidedResult<Vec<u8>> {
        self.pictures_asked.lock().push(address.to_owned());
        if address == A_PICTURE_THE_TUBE_REFUSES {
            return Err(ProvidedError::Refused {
                provider: self.source.clone(),
                op: ProviderOp::Cover,
                status: 404,
            });
        }
        Ok(png(64))
    }

    fn picture_named_for(&self, key: &str) -> ProvidedResult<Option<String>> {
        Ok(Some(format!("{A_PICTURE_THE_TUBE_NAMES}{key}")))
    }

    fn find(&self, identity: &Identity) -> ProvidedResult<Obtained> {
        self.asked.lock().push(identity.clone());

        let video = video_of(identity);

        if let Some(then) = self.when_asked.lock().remove(&video) {
            then();
        }

        let answer = self
            .answers
            .lock()
            .get_mut(&video)
            .and_then(VecDeque::pop_front)
            .unwrap_or(TubeAnswer::Lasting(A_SECOND_OF_FRAMES));
        let bytes = |frames| std::io::Cursor::new(Wav::new().frames(frames).build());
        match answer {
            TubeAnswer::Lasting(frames) => self.stream(&video, Opening::ready(bytes(frames))),
            TubeAnswer::Nothing => Ok(Obtained::Nothing),
            TubeAnswer::Gone => self.stream(&video, Opening::new(|| Ok(Opened::Gone))),
            TubeAnswer::Undecodable => self.stream(
                &video,
                Opening::ready(std::io::Cursor::new(b"not a song at all".to_vec())),
            ),
            TubeAnswer::Away => Err(ProvidedError::Io {
                provider: self.source.clone(),
                op: ProviderOp::Search,
                source: io::Error::from(io::ErrorKind::ConnectionRefused),
            }),
            TubeAnswer::Lapsed => Err(ProvidedError::Unwelcome {
                provider: self.source.clone(),
                op: ProviderOp::Search,
                code: 401,
            }),
            TubeAnswer::Busy => Err(ProvidedError::Busy {
                provider: self.source.clone(),
                op: ProviderOp::Download,
            }),
            TubeAnswer::DyingAway => self.stream(
                &video,
                Opening::ready(DiesPartWay {
                    provider: self.source.clone(),
                    began: false,
                }),
            ),
            TubeAnswer::Hanging => self.stream(&video, Opening::ready(Stall { began: false })),
        }
    }
}

struct LastResortTube(Arc<Tube>);

impl Provider for LastResortTube {
    fn source(&self) -> &SourceId {
        self.0.source()
    }

    fn find(&self, identity: &Identity) -> ProvidedResult<Obtained> {
        self.0.find(identity)
    }

    fn last_resort(&self) -> bool {
        true
    }
}

fn video_of(identity: &Identity) -> String {
    identity
        .track_on(Service::YoutubeMusic)
        .and_then(|link| link.strip_prefix(WATCH_ADDRESS))
        .expect("a keep carries the video's address")
        .to_owned()
}

fn song_to_keep(video: &str, title: &str) -> SongToKeep {
    SongToKeep {
        source: source(KEPT_BY),
        video: video.to_owned(),
        title: title.to_owned(),
        artist: Some("The Orbiters".to_owned()),
        album: Some("Kept Songs".to_owned()),
        length: Some(Duration::from_secs(1)),
        cover: None,
    }
}

fn list_to_keep(name: &str, ordered: bool, songs: &[(&str, &str)]) -> PlaylistToKeep {
    PlaylistToKeep {
        name: name.to_owned(),
        ordered,
        songs: songs
            .iter()
            .map(|(video, title)| song_to_keep(video, title))
            .collect(),
    }
}

fn pass(library: &Library, tube: &Arc<Tube>) -> Result<KeepSummary> {
    library.keep_pending(&tube.registered(), &|| false)
}

fn titles_in(library: &Library, list: &str) -> Result<Vec<String>> {
    let Some(playlist) = library.playlist_named(list)? else {
        return Ok(Vec::new());
    };
    Ok(library
        .playlist_entries(playlist.id, None)?
        .into_iter()
        .map(|entry| entry.track.map_or_else(String::new, |track| track.title))
        .collect())
}

fn staged_under(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();

    files_under(folder, &mut found);
    found.retain(|path| path.to_string_lossy().ends_with(STAGED_DELIVERY));
    found
}

fn track_titled(library: &Library, title: &str) -> Result<Track> {
    Ok(library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|track| track.title == title)
        .expect("the track is catalogued"))
}

#[test]
fn a_song_kept_lands_in_the_music_folder_tagged_as_the_service_named_it() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    assert_eq!(
        library.keep_song(song_to_keep("abc123", "San Tropez"))?,
        KeptSong::Queued
    );
    assert_eq!(library.keeps_pending()?, 1);

    let summary = pass(&library, &tube)?;

    assert_eq!(
        summary,
        KeepSummary {
            landed: 1,
            ..KeepSummary::default()
        }
    );
    assert_eq!(library.keeps_pending()?, 0);

    let asked = tube.asked.lock()[0].clone();

    assert_eq!(asked.title, "San Tropez");
    assert_eq!(asked.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(asked.album.as_deref(), Some("Kept Songs"));
    assert_eq!(asked.length, Some(Duration::from_secs(1)));
    assert_eq!(
        asked.track_on(Service::YoutubeMusic),
        Some("https://music.youtube.com/watch?v=abc123")
    );

    let track = track_titled(&library, "San Tropez")?;

    assert_eq!(track.artist.as_deref(), Some("The Orbiters"));

    let album = library
        .album(track.album_id.expect("the song names its album"))?
        .expect("the album");

    assert_eq!(album.title, "Kept Songs");
    assert_eq!(
        track.location.as_path(),
        Some(
            tree.path()
                .join("The Orbiters/Kept Songs/San Tropez.wav")
                .as_path()
        )
    );

    let tags = FileTags::over(Sources::local())
        .read(&track.location, Picturing::Whether)
        .expect("the file reads");

    assert_eq!(tags.tags.title.as_deref(), Some("San Tropez"));
    assert_eq!(tags.tags.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(tags.tags.album.as_deref(), Some("Kept Songs"));
    assert_eq!(tags.tags.album_artist, None);
    assert!(library.wants()?.is_empty(), "a keep wrote a want");
    Ok(())
}

#[test]
fn a_song_kept_lands_with_the_picture_its_service_showed_for_it() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();
    let address = "https://lh3.googleusercontent.com/cover";

    library.keep_song(SongToKeep {
        cover: Some(address.to_owned()),
        ..song_to_keep("abc123", "San Tropez")
    })?;

    assert_eq!(pass(&library, &tube)?.landed, 1);

    assert_eq!(*tube.pictures_asked.lock(), [address]);

    let track = track_titled(&library, "San Tropez")?;
    let carried = FileTags::over(Sources::local())
        .read(&track.location, Picturing::Copied)
        .expect("the file reads")
        .picture
        .into_copied()
        .expect("the file carries the picture");

    assert_eq!(carried.bytes, png(64));

    let album = track.album_id.expect("the song names its album");

    assert_eq!(
        library.cover_art(album)?.map(|art| art.bytes),
        Some(png(64)),
        "the library draws no cover for a kept song"
    );
    Ok(())
}

#[test]
fn a_song_kept_bare_before_keeps_carried_their_picture_is_given_one_once() -> Result<()> {
    let (tree, library, database) = a_music_folder_on_disk()?;
    let tube = Tube::new();

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    assert_eq!(pass(&library, &tube)?.landed, 1);

    beside(&database)
        .execute(
            "UPDATE kept_songs SET cover = NULL, picture_asked = NULL",
            [],
        )
        .expect("the keep is made one from before pictures");

    let track = track_titled(&library, "San Tropez")?;
    let bare = FileTags::over(Sources::local())
        .read(&track.location, Picturing::Copied)
        .expect("the file reads");

    assert_eq!(bare.picture.into_copied(), None);

    tube.pictures_asked.lock().clear();

    pass(&library, &tube)?;
    pass(&library, &tube)?;

    let named = format!("{A_PICTURE_THE_TUBE_NAMES}abc123");

    assert_eq!(
        *tube.pictures_asked.lock(),
        std::slice::from_ref(&named),
        "the picture was not asked for once"
    );

    let carried = FileTags::over(Sources::local())
        .read(&track.location, Picturing::Copied)
        .expect("the file reads")
        .picture
        .into_copied()
        .expect("the file carries the picture");

    assert_eq!(carried.bytes, png(64));

    let album = track_titled(&library, "San Tropez")?
        .album_id
        .expect("the song names its album");

    assert_eq!(
        library.cover_art(album)?.map(|art| art.bytes),
        Some(png(64)),
        "the library still draws no cover for the kept song"
    );

    let kept: Option<String> = beside(&database)
        .query_row("SELECT cover FROM kept_songs", [], |row| row.get(0))
        .expect("the keep reads");

    assert_eq!(kept, Some(named));
    assert!(tree.path().join("library.db").exists());
    Ok(())
}

#[test]
fn a_song_whose_picture_cannot_be_fetched_is_kept_all_the_same() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    library.keep_song(SongToKeep {
        cover: Some(A_PICTURE_THE_TUBE_REFUSES.to_owned()),
        ..song_to_keep("abc123", "San Tropez")
    })?;

    assert_eq!(pass(&library, &tube)?.landed, 1);

    let track = track_titled(&library, "San Tropez")?;
    let tags = FileTags::over(Sources::local())
        .read(&track.location, Picturing::Copied)
        .expect("the file reads");

    assert_eq!(tags.tags.title.as_deref(), Some("San Tropez"));
    assert_eq!(tags.picture.into_copied(), None);
    Ok(())
}

#[test]
fn a_song_pressed_again_keeps_the_picture_it_was_first_pressed_with() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();
    let address = "https://i.ytimg.com/first";

    library.keep_song(SongToKeep {
        cover: Some(address.to_owned()),
        ..song_to_keep("abc123", "San Tropez")
    })?;
    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    assert_eq!(pass(&library, &tube)?.landed, 1);

    assert_eq!(*tube.pictures_asked.lock(), [address]);
    Ok(())
}

#[test]
fn a_keep_is_asked_of_its_own_provider_alone() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let shop = Tube::named("shop");
    let tube = Tube::new();

    let providers = Providers::none()
        .and(Arc::clone(&shop) as Arc<dyn Provider>)
        .and(Arc::clone(&tube) as Arc<dyn Provider>);

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    let summary = library.keep_pending(&providers, &|| false)?;

    assert_eq!(summary.landed, 1);
    assert_eq!(tube.videos_asked(), ["abc123"]);
    assert!(shop.asked.lock().is_empty(), "another provider was asked");
    Ok(())
}

#[test]
fn a_last_resort_named_alone_is_asked_for_a_keep_whatever_else_is_registered() -> Result<()> {
    for beside_another in [true, false] {
        let (_tree, library) = a_music_folder()?;
        let shop = Tube::named("shop");
        let tube = Tube::new();
        let last = Arc::new(LastResortTube(Arc::clone(&tube))) as Arc<dyn Provider>;

        let providers = if beside_another {
            Providers::none()
                .and(Arc::clone(&shop) as Arc<dyn Provider>)
                .and(last)
        } else {
            Providers::none().and(last)
        };

        library.keep_song(song_to_keep("abc123", "San Tropez"))?;

        let summary = library.keep_pending(&providers, &|| false)?;

        assert_eq!(summary.landed, 1, "beside another: {beside_another}");
        assert!(shop.asked.lock().is_empty());
    }
    Ok(())
}

#[test]
fn a_keep_with_no_length_or_artist_is_still_asked_and_lands_whatever_it_lasts() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering(
        "abc123",
        &[TubeAnswer::Lasting(LONGER_THAN_ANY_LENGTH_AGREES)],
    );
    library.keep_song(SongToKeep {
        artist: None,
        album: None,
        length: None,
        ..song_to_keep("abc123", "San Tropez")
    })?;

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.landed, 1);
    assert_eq!(tube.videos_asked(), ["abc123"]);
    assert_eq!(track_titled(&library, "San Tropez")?.artist, None);
    Ok(())
}

#[test]
fn a_song_held_already_is_not_kept_again_and_its_copy_fills_the_list() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let held = held_as(&tree, "Elsewhere/held.wav", "TWO", A_SECOND_OF_FRAMES);

    scan(&library, &options(&tree))?;

    let tube = Tube::new();
    let the_held_track = track_titled(&library, "TWO")?;

    let kept =
        library.keep_playlist(list_to_keep("Mix", true, &[("one", "One"), ("two", "Two")]))?;

    assert_eq!(kept, KeptList { queued: 1, held: 1 });
    assert_eq!(titles_in(&library, "Mix")?, ["TWO"]);
    assert_eq!(library.keeps_pending()?, 1);

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.landed, 1);
    assert_eq!(tube.videos_asked(), ["one"]);
    assert_eq!(titles_in(&library, "Mix")?, ["One", "TWO"]);

    let list = library.playlist_named("Mix")?.expect("the list");
    let entries = library.playlist_entries(list.id, None)?;

    assert_eq!(entries[1].cut.location.as_path(), Some(held.as_path()));
    assert_eq!(
        songs_under(&tree).len(),
        2,
        "a lossy copy was filed beside the song held"
    );
    assert_eq!(
        library.keep_song(song_to_keep("two", "Two"))?,
        KeptSong::AlreadyHeld(the_held_track.id)
    );
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

#[test]
fn a_song_named_alike_but_apart_in_length_or_by_another_artist_is_not_held() -> Result<()> {
    let (tree, library) = a_music_folder()?;

    held_as(&tree, "held.wav", "Two", 4 * A_SECOND_OF_FRAMES);
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.keep_song(song_to_keep("two", "Two"))?,
        KeptSong::Queued
    );
    assert_eq!(
        library.keep_song(SongToKeep {
            length: Some(Duration::from_secs(4)),
            artist: Some("The Other Orbiters".to_owned()),
            ..song_to_keep("second", "Two")
        })?,
        KeptSong::Queued
    );
    assert_eq!(
        library.keep_song(SongToKeep {
            length: None,
            ..song_to_keep("third", "Two")
        })?,
        KeptSong::Queued
    );
    assert!(matches!(
        library.keep_song(SongToKeep {
            length: Some(Duration::from_secs(5)),
            ..song_to_keep("fourth", "Two")
        })?,
        KeptSong::AlreadyHeld(_)
    ));
    Ok(())
}

#[test]
fn a_song_whose_file_is_gone_by_hand_is_not_held() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let held = held_as(&tree, "held.wav", "Two", A_SECOND_OF_FRAMES);

    scan(&library, &options(&tree))?;
    fs::remove_file(held).expect("the file goes");

    assert_eq!(
        library.keep_song(song_to_keep("two", "Two"))?,
        KeptSong::Queued
    );
    Ok(())
}

#[test]
fn a_song_kept_twice_is_queued_once() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    assert_eq!(
        library.keep_song(song_to_keep("abc123", "San Tropez"))?,
        KeptSong::Queued
    );
    assert_eq!(
        library.keep_song(song_to_keep("abc123", "San Tropez"))?,
        KeptSong::Queued
    );
    assert_eq!(library.keeps_pending()?, 1);

    pass(&library, &tube)?;

    assert_eq!(tube.videos_asked(), ["abc123"]);
    assert!(matches!(
        library.keep_song(song_to_keep("abc123", "San Tropez"))?,
        KeptSong::AlreadyHeld(_)
    ));
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

#[test]
fn a_song_kept_with_no_name_to_weigh_is_known_by_its_video_once_it_has_landed() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();
    let by_link = SongToKeep {
        artist: None,
        length: None,
        ..song_to_keep("abc123", "San Tropez")
    };

    library.keep_song(by_link.clone())?;
    pass(&library, &tube)?;

    assert!(matches!(
        library.keep_song(by_link)?,
        KeptSong::AlreadyHeld(_)
    ));
    assert_eq!(library.keeps_pending()?, 0);
    assert_eq!(tube.videos_asked(), ["abc123"]);
    Ok(())
}

#[test]
fn a_kept_song_deleted_by_hand_is_queued_again_when_pressed_again() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;
    pass(&library, &tube)?;
    for song in songs_under(&tree) {
        fs::remove_file(song).expect("the file goes");
    }

    assert_eq!(
        library.keep_song(song_to_keep("abc123", "San Tropez"))?,
        KeptSong::Queued
    );
    assert_eq!(library.keeps_pending()?, 1);
    assert_eq!(pass(&library, &tube)?.landed, 1);
    assert_eq!(songs_under(&tree).len(), 1);
    Ok(())
}

#[test]
fn a_playlist_keeps_youtubes_order_however_its_songs_land() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::Busy]);
    tube.answering("two", &[TubeAnswer::Busy, TubeAnswer::Busy]);
    library.keep_playlist(list_to_keep(
        "Mix",
        true,
        &[("one", "One"), ("two", "Two"), ("three", "Three")],
    ))?;

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["Three"]);

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["One", "Three"]);

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["One", "Two", "Three"]);
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

#[test]
fn a_playlist_told_not_to_keep_order_is_in_the_order_songs_landed() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::Busy]);
    tube.answering("two", &[TubeAnswer::Busy, TubeAnswer::Busy]);
    library.keep_playlist(list_to_keep(
        "Mix",
        false,
        &[("one", "One"), ("two", "Two"), ("three", "Three")],
    ))?;

    pass(&library, &tube)?;
    pass(&library, &tube)?;
    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["Three", "One", "Two"]);
    Ok(())
}

#[test]
fn a_song_that_never_lands_leaves_its_place_and_the_rest_keep_theirs() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("two", &[TubeAnswer::Nothing]);

    let list = list_to_keep(
        "Mix",
        true,
        &[("one", "One"), ("two", "Two"), ("three", "Three")],
    );

    library.keep_playlist(list.clone())?;

    let summary = pass(&library, &tube)?;

    assert_eq!(
        summary,
        KeepSummary {
            landed: 2,
            unkept: 1,
            ..KeepSummary::default()
        }
    );
    assert_eq!(titles_in(&library, "Mix")?, ["One", "Three"]);
    assert_eq!(library.keeps_pending()?, 0);
    assert_eq!(pass(&library, &tube)?, KeepSummary::default());
    assert_eq!(tube.videos_asked(), ["one", "two", "three"]);

    assert_eq!(
        library.keep_playlist(list)?,
        KeptList { queued: 1, held: 2 }
    );
    assert_eq!(library.keeps_pending()?, 1);

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["One", "Two", "Three"]);
    Ok(())
}

#[test]
fn a_second_press_adds_new_songs_in_their_places_and_leaves_a_removed_one_removed() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    library.keep_playlist(list_to_keep("Mix", true, &[("one", "One"), ("two", "Two")]))?;
    pass(&library, &tube)?;

    let list = library.playlist_named("Mix")?.expect("the list");

    assert!(library.remove_from_playlist(list.id, Span::between(1, 1))?);

    let kept = library.keep_playlist(list_to_keep(
        "Mix",
        true,
        &[("new", "New"), ("one", "One"), ("two", "Two")],
    ))?;

    assert_eq!(kept, KeptList { queued: 1, held: 2 });
    assert_eq!(titles_in(&library, "Mix")?, ["One"]);

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["New", "One"]);
    assert_eq!(tube.videos_asked(), ["one", "two", "new"]);
    Ok(())
}

#[test]
fn a_song_in_two_kept_lists_is_downloaded_once_and_fills_both() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();
    let by_link = SongToKeep {
        artist: None,
        length: None,
        ..song_to_keep("both", "Both")
    };

    for name in ["Mix", "Other Mix"] {
        library.keep_playlist(PlaylistToKeep {
            name: name.to_owned(),
            ordered: true,
            songs: vec![by_link.clone()],
        })?;
    }

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.landed, 2);
    assert_eq!(tube.videos_asked(), ["both"]);
    assert_eq!(songs_under(&tree).len(), 1);
    assert_eq!(titles_in(&library, "Mix")?, ["Both"]);
    assert_eq!(titles_in(&library, "Other Mix")?, ["Both"]);
    Ok(())
}

#[test]
fn a_song_that_arrived_since_it_was_queued_is_not_asked_for() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    library.keep_playlist(list_to_keep("Mix", true, &[("two", "Two")]))?;

    let held = held_as(&tree, "held.wav", "Two", A_SECOND_OF_FRAMES);

    scan(&library, &options(&tree))?;

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.landed, 1);
    assert!(tube.videos_asked().is_empty());
    assert_eq!(songs_under(&tree), [held]);
    assert_eq!(titles_in(&library, "Mix")?, ["Two"]);
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

#[test]
fn a_keep_longer_than_the_song_was_listed_is_not_kept() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering(
        "abc123",
        &[TubeAnswer::Lasting(LONGER_THAN_ANY_LENGTH_AGREES)],
    );
    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    let summary = pass(&library, &tube)?;

    assert_eq!(
        summary,
        KeepSummary {
            unkept: 1,
            ..KeepSummary::default()
        }
    );
    assert!(songs_under(&tree).is_empty(), "the file refused stayed");
    assert_eq!(library.keeps_pending()?, 0);
    assert_eq!(pass(&library, &tube)?, KeepSummary::default());
    assert_eq!(tube.videos_asked(), ["abc123"]);
    Ok(())
}

#[test]
fn bytes_that_do_not_decode_give_a_keep_up() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("abc123", &[TubeAnswer::Undecodable]);
    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.unkept, 1);
    assert!(songs_under(&tree).is_empty());
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

#[test]
fn a_video_gone_when_opened_gives_a_keep_up_and_a_second_press_asks_again() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("abc123", &[TubeAnswer::Gone]);
    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    let summary = pass(&library, &tube)?;

    assert_eq!(
        summary,
        KeepSummary {
            gone: 1,
            ..KeepSummary::default()
        }
    );
    assert_eq!(library.keeps_pending()?, 0);
    assert_eq!(
        library.keep_song(song_to_keep("abc123", "San Tropez"))?,
        KeptSong::Queued
    );
    assert_eq!(library.keeps_pending()?, 1);
    assert_eq!(pass(&library, &tube)?.landed, 1);
    Ok(())
}

#[test]
fn a_keep_with_no_delivery_folder_is_refused() -> Result<()> {
    let library = Library::open_in_memory()?;
    let tube = Tube::new();

    assert!(matches!(
        library.keep_song(song_to_keep("abc123", "San Tropez")),
        Err(Error::NoDeliveryFolder)
    ));
    assert!(matches!(
        library.keep_playlist(list_to_keep("Mix", true, &[("one", "One")])),
        Err(Error::NoDeliveryFolder)
    ));
    assert_eq!(library.keeps_pending()?, 0);

    let tree = Tree::new();

    delivering_into(&library, &tree);
    library.keep_song(song_to_keep("abc123", "San Tropez"))?;
    library.deliver_into(None);

    assert!(matches!(
        pass(&library, &tube),
        Err(Error::NoDeliveryFolder)
    ));
    assert!(tube.videos_asked().is_empty());
    assert_eq!(library.keeps_pending()?, 1);
    Ok(())
}

fn keeps_until_killed(folder: &Path) -> Result<()> {
    let library = Library::open(&folder.join("library.db"))?;

    library.deliver_into(Some(DeliveryFolder {
        path: folder.to_path_buf(),
        layout: Layout::default(),
    }));

    let tube = Tube::new();

    tube.answering("abc123", &[TubeAnswer::Hanging]);
    pass(&library, &tube).map(drop)
}

fn staged_by_a_keep_under_way(folder: &Path) -> PathBuf {
    let began = Instant::now();
    loop {
        if let Some(staged) = staged_under(folder)
            .into_iter()
            .find(|staged| fs::metadata(staged).is_ok_and(|file| file.len() > 0))
        {
            return staged;
        }

        assert!(
            began.elapsed() < A_KILLED_KEEP_IS_STAGED_WITHIN,
            "the keep to be killed staged nothing"
        );

        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_keep_killed_part_way_is_taken_up_again_and_its_staging_swept() -> Result<()> {
    if let Some(folder) = env::var_os(KILLED_KEEP_FOLDER) {
        return keeps_until_killed(Path::new(&folder));
    }

    let tree = Tree::new();
    let library = Library::open(&tree.path().join("library.db"))?;

    delivering_into(&library, &tree);
    library.keep_playlist(list_to_keep("Mix", true, &[("abc123", "San Tropez")]))?;

    let mut killed = Command::new(env::current_exe().expect("the test binary"))
        .args([
            in_this_suite("a_keep_killed_part_way_is_taken_up_again_and_its_staging_swept")
                .as_str(),
            "--exact",
        ])
        .env(KILLED_KEEP_FOLDER, tree.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the test binary runs");
    let staged = staged_by_a_keep_under_way(tree.path());

    killed.kill().expect("the keep is killed");
    killed.wait().expect("the keep is gone");

    assert!(
        staged.exists(),
        "the killed keep's staging file went with it"
    );
    assert_eq!(library.keeps_pending()?, 1);
    assert!(songs_under(&tree).is_empty());

    left_long_ago(&staged);

    let tube = Tube::new();
    let summary = pass(&library, &tube)?;

    assert_eq!(summary.landed, 1);
    assert!(!staged.exists(), "the killed keep's staging file was left");
    assert!(staged_under(tree.path()).is_empty());
    assert_eq!(songs_under(&tree).len(), 1);
    assert_eq!(titles_in(&library, "Mix")?, ["San Tropez"]);
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

#[test]
fn a_provider_away_stops_the_pass_and_keeps_the_queue() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::Away]);
    library.keep_playlist(list_to_keep("Mix", true, &[("one", "One"), ("two", "Two")]))?;

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.stopped_by, Some(source(KEPT_BY)));
    assert_eq!(summary.landed, 0);
    assert_eq!(tube.videos_asked(), ["one"]);
    assert_eq!(library.keeps_pending()?, 2);

    assert_eq!(pass(&library, &tube)?.landed, 2);
    assert_eq!(titles_in(&library, "Mix")?, ["One", "Two"]);
    Ok(())
}

#[test]
fn a_provider_failing_part_way_through_a_download_stops_the_pass_and_keeps_the_queue() -> Result<()>
{
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::DyingAway]);
    library.keep_song(song_to_keep("one", "One"))?;
    library.keep_song(song_to_keep("two", "Two"))?;

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.stopped_by, Some(source(KEPT_BY)));
    assert_eq!(tube.videos_asked(), ["one"]);
    assert_eq!(library.keeps_pending()?, 2);
    assert!(staged_under(tree.path()).is_empty());
    assert!(songs_under(&tree).is_empty());
    Ok(())
}

#[test]
fn a_song_that_stopped_a_pass_goes_to_the_back_and_the_next_pass_begins_elsewhere() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::Away]);
    library.keep_song(song_to_keep("one", "One"))?;
    library.keep_song(song_to_keep("two", "Two"))?;
    library.keep_song(song_to_keep("three", "Three"))?;

    assert!(pass(&library, &tube)?.stopped_by.is_some());

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.landed, 3);
    assert_eq!(tube.videos_asked(), ["one", "two", "three", "one"]);
    Ok(())
}

#[test]
fn a_song_the_provider_was_busy_for_goes_behind_a_song_not_yet_tried() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::Busy]);
    library.keep_song(song_to_keep("one", "One"))?;

    assert_eq!(pass(&library, &tube)?.unkept, 1);

    library.keep_song(song_to_keep("two", "Two"))?;

    assert_eq!(pass(&library, &tube)?.landed, 2);

    assert_eq!(tube.videos_asked(), ["one", "two", "one"]);
    Ok(())
}

#[test]
fn a_keep_that_keeps_failing_is_given_up_at_its_fifth_try_and_a_press_brings_it_back() -> Result<()>
{
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::DyingAway; 5]);
    library.keep_song(song_to_keep("one", "One"))?;

    for _ in 0..4 {
        assert!(pass(&library, &tube)?.stopped_by.is_some());
        assert_eq!(library.keeps_pending()?, 1);
    }

    assert!(pass(&library, &tube)?.stopped_by.is_some());
    assert_eq!(library.keeps_pending()?, 0, "the fifth try gives it up");
    assert_eq!(pass(&library, &tube)?, KeepSummary::default());
    assert_eq!(tube.videos_asked().len(), 5);

    tube.answering("one", &[TubeAnswer::Busy; 4]);
    library.keep_song(song_to_keep("one", "One"))?;
    for _ in 0..4 {
        assert_eq!(pass(&library, &tube)?.unkept, 1);
    }

    assert_eq!(
        library.keeps_pending()?,
        1,
        "a press gave it five tries more"
    );
    assert_eq!(pass(&library, &tube)?.landed, 1);
    Ok(())
}

#[test]
fn a_pass_stopped_says_why_and_at_which_song() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::Lapsed, TubeAnswer::Away]);
    tube.answering("two", &[TubeAnswer::Away]);
    library.keep_song(song_to_keep("one", "One"))?;
    library.keep_song(song_to_keep("two", "Two"))?;

    assert_eq!(
        pass(&library, &tube)?.stopped,
        Some(KeepStopped {
            why: WhyAway::SignInLapsed,
            song: "One".to_owned(),
        })
    );
    assert_eq!(
        pass(&library, &tube)?.stopped,
        Some(KeepStopped {
            why: WhyAway::NotAnswering,
            song: "Two".to_owned(),
        })
    );
    Ok(())
}

#[test]
fn a_lapsed_account_moves_a_keep_back_and_costs_it_no_try() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::Lapsed; 7]);
    tube.answering("two", &[TubeAnswer::Lapsed; 7]);
    library.keep_song(song_to_keep("one", "One"))?;
    library.keep_song(song_to_keep("two", "Two"))?;

    for _ in 0..14 {
        assert!(pass(&library, &tube)?.stopped_by.is_some());
    }

    assert_eq!(library.keeps_pending()?, 2);
    assert_eq!(pass(&library, &tube)?.landed, 2);
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_music_folder_that_cannot_be_written_moves_a_keep_back_and_costs_it_no_try() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    library.keep_song(song_to_keep("one", "One"))?;
    fs::set_permissions(tree.path(), fs::Permissions::from_mode(0o555))
        .expect("the music folder takes a mode");
    if fs::write(tree.path().join("probe"), b"").is_ok() {
        fs::set_permissions(tree.path(), fs::Permissions::from_mode(0o755))
            .expect("the music folder takes a mode");
        eprintln!("skipping: this user writes a folder whatever its mode");
        return Ok(());
    }

    for _ in 0..7 {
        pass(&library, &tube)?;
    }
    fs::set_permissions(tree.path(), fs::Permissions::from_mode(0o755))
        .expect("the music folder takes a mode");

    assert_eq!(library.keeps_pending()?, 1, "the folder gave the song up");
    assert_eq!(pass(&library, &tube)?.landed, 1);
    Ok(())
}

struct OffersVideo {
    source: SourceId,
    video: &'static str,
    opened: AtomicUsize,
}

impl OffersVideo {
    fn registered(video: &'static str) -> Arc<Self> {
        Arc::new(Self {
            source: source(KEPT_BY),
            video,
            opened: AtomicUsize::new(0),
        })
    }

    fn polled_by(self: &Arc<Self>, library: &Library) -> Result<PollSummary> {
        library
            .poll(
                Arc::new(Providers::none().and(Arc::clone(self) as Arc<dyn Provider>)),
                PollOptions {
                    lanes: NonZeroUsize::MIN,
                    ..PollOptions::ASKING_EVERY_WANT
                },
            )?
            .join()
    }
}

impl Provider for OffersVideo {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _: &Identity) -> ProvidedResult<Obtained> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Obtained::Found(Delivery::Stream {
            key: format!("track/{}", self.video).into(),
            extension: Extension::new("wav")?,
            opening: Opening::ready(std::io::Cursor::new(
                Wav::new().frames(A_SECOND_OF_FRAMES).build(),
            )),
        }))
    }

    fn last_resort(&self) -> bool {
        true
    }
}

fn one_want_of(library: &Library, title: &str) -> Result<()> {
    let album = only_album(library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, title, Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    for row in library.release_tracks(album.id)? {
        if row.title == title {
            library.want(row.id)?;
        }
    }
    Ok(())
}

fn named_otherwise(video: &str) -> SongToKeep {
    SongToKeep {
        artist: Some("Someone Else Entirely".to_owned()),
        ..song_to_keep(video, "A Name No Want Gave")
    }
}

#[test]
fn a_video_a_want_filed_in_the_music_folder_is_not_fetched_again_for_a_keep() -> Result<()> {
    let (tree, library) = scanned_orbits()?;

    delivering_into(&library, &tree);
    one_want_of(&library, "San Tropez")?;

    assert_eq!(
        OffersVideo::registered("abc123")
            .polled_by(&library)?
            .stats
            .kept,
        1
    );

    let tube = Tube::new();

    let kept = library.keep_song(named_otherwise("abc123"))?;
    let summary = pass(&library, &tube)?;

    assert!(matches!(kept, KeptSong::AlreadyHeld(_)), "{kept:?}");
    assert_eq!(summary, KeepSummary::default());
    assert!(
        tube.videos_asked().is_empty(),
        "the video was fetched twice"
    );
    Ok(())
}

#[test]
fn a_video_a_want_kept_in_the_vault_is_not_fetched_again_for_a_keep() -> Result<()> {
    let tree = orbits_tree();
    let music = Tree::new();
    let held = Tree::new();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    delivering_into(&library, &music);
    one_want_of(&library, "San Tropez")?;

    assert_eq!(
        OffersVideo::registered("abc123")
            .polled_by(&library)?
            .stats
            .kept,
        1
    );

    let tube = Tube::new();

    let kept = library.keep_song(named_otherwise("abc123"))?;

    pass(&library, &tube)?;

    assert!(matches!(kept, KeptSong::AlreadyHeld(_)), "{kept:?}");
    assert!(
        tube.videos_asked().is_empty(),
        "the video was fetched twice"
    );
    Ok(())
}

#[test]
fn a_video_a_keep_landed_is_declined_by_the_poll_while_its_file_stands() -> Result<()> {
    let (tree, library) = scanned_orbits()?;

    delivering_into(&library, &tree);
    one_want_of(&library, "San Tropez")?;

    let tube = Tube::new();

    library.keep_song(named_otherwise("abc123"))?;

    assert_eq!(pass(&library, &tube)?.landed, 1);

    let kept = track_titled(&library, "A Name No Want Gave")?;

    let offers = OffersVideo::registered("abc123");
    let declined = offers.polled_by(&library)?;

    assert_eq!(declined.stats.kept, 0);

    fs::remove_file(kept.location.as_path().expect("a file")).expect("the file removed");

    let again = offers.polled_by(&library)?;

    assert_eq!(again.stats.kept, 1, "a file gone declines nothing");
    Ok(())
}

fn opus_in_webm() -> Option<Vec<u8>> {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build an Opus WebM");
        return None;
    }

    let held = Tree::new();
    let source = held.write("tone.wav", &Wav::new().frames(A_SECOND_OF_FRAMES).build());
    let target = held.path().join("served.webm");

    let encoded = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(&source)
        .args(["-c:a", "libopus", "-b:a", "96k", "-f", "webm"])
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if !encoded.is_ok_and(|status| status.success()) {
        eprintln!("skipped: ffmpeg would not encode Opus into WebM");
        return None;
    }
    fs::read(target).ok()
}

struct ServesWebm {
    source: SourceId,
    bytes: Vec<u8>,
}

impl Provider for ServesWebm {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn picture(&self, _: &str) -> ProvidedResult<Vec<u8>> {
        Ok(png(64))
    }

    fn find(&self, _: &Identity) -> ProvidedResult<Obtained> {
        Ok(Obtained::Found(Delivery::Stream {
            key: "track/abc123".into(),
            extension: Extension::new("mka")?,
            opening: Opening::ready(std::io::Cursor::new(self.bytes.clone())),
        }))
    }
}

#[test]
fn opus_in_webm_kept_as_mka_is_tagged_and_keeps_its_names_through_a_rescan() -> Result<()> {
    let Some(bytes) = opus_in_webm() else {
        return Ok(());
    };
    let (tree, library) = a_music_folder()?;
    let providers = Providers::none().and(Arc::new(ServesWebm {
        source: source(KEPT_BY),
        bytes,
    }) as Arc<dyn Provider>);

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    assert_eq!(library.keep_pending(&providers, &|| false)?.landed, 1);

    let path = tree.path().join("The Orbiters/Kept Songs/San Tropez.mka");

    assert!(path.is_file(), "{}", path.display());

    let read = FileTags::over(Sources::local())
        .read(&MediaLocation::local(&path), Picturing::Whether)
        .expect("the file reads");

    assert_eq!(read.tags.title.as_deref(), Some("San Tropez"));
    assert_eq!(read.tags.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(read.tags.album.as_deref(), Some("Kept Songs"));

    scan(&library, &options(&tree))?;

    assert_eq!(
        track_titled(&library, "San Tropez")?.location,
        MediaLocation::local(&path)
    );

    let afresh = Library::open_in_memory()?;

    scan(&afresh, &options(&tree))?;

    let read_afresh = track_titled(&afresh, "San Tropez")?;

    assert_eq!(read_afresh.artist.as_deref(), Some("The Orbiters"));

    let album = afresh
        .album(read_afresh.album_id.expect("the song names its album"))?
        .expect("the album");

    assert_eq!(album.title, "Kept Songs");
    Ok(())
}

fn pictured_by_its_maker(bytes: &[u8], picture: &CoverArt) -> Vec<u8> {
    let held = Tree::new();
    let path = held.write("made.mka", bytes);

    FileTags::over(Sources::local())
        .write(
            &MediaLocation::local(&path),
            Writing {
                edits: &[],
                taken: &[],
                picture: Some(picture),
                unpictured: false,
                popularity: None,
            },
        )
        .expect("the maker's picture attached");
    fs::read(&path).expect("the pictured file")
}

#[test]
fn a_kept_matroska_whose_own_cover_refuses_the_one_fetched_still_lands_with_its_names() -> Result<()>
{
    let Some(served) = opus_in_webm() else {
        return Ok(());
    };
    let its_own = CoverArt {
        format: ImageFormat::Png,
        bytes: png(16),
    };
    let (tree, library) = a_music_folder()?;

    let providers = Providers::none().and(Arc::new(ServesWebm {
        source: source(KEPT_BY),
        bytes: pictured_by_its_maker(&served, &its_own),
    }) as Arc<dyn Provider>);

    library.keep_song(SongToKeep {
        cover: Some("https://lh3.googleusercontent.com/cover".to_owned()),
        ..song_to_keep("abc123", "San Tropez")
    })?;

    assert_eq!(library.keep_pending(&providers, &|| false)?.landed, 1);

    let path = tree.path().join("The Orbiters/Kept Songs/San Tropez.mka");
    let read = FileTags::over(Sources::local())
        .read(&MediaLocation::local(&path), Picturing::Copied)
        .expect("the file reads");

    assert_eq!(read.tags.title.as_deref(), Some("San Tropez"));
    assert_eq!(read.tags.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(read.tags.album.as_deref(), Some("Kept Songs"));
    assert_eq!(read.picture.into_copied(), Some(its_own));
    Ok(())
}

#[test]
fn a_busy_provider_leaves_the_song_queued_and_the_pass_goes_on() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("one", &[TubeAnswer::Busy]);
    library.keep_song(song_to_keep("one", "One"))?;
    library.keep_song(song_to_keep("two", "Two"))?;

    let summary = pass(&library, &tube)?;

    assert_eq!(
        summary,
        KeepSummary {
            landed: 1,
            unkept: 1,
            ..KeepSummary::default()
        }
    );
    assert_eq!(library.keeps_pending()?, 1);
    assert_eq!(pass(&library, &tube)?.landed, 1);
    Ok(())
}

#[test]
fn a_registry_without_the_provider_leaves_the_queue_as_it_stands() -> Result<()> {
    let (_tree, library) = a_music_folder()?;

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;
    library.deliver_into(None);

    let summary = library.keep_pending(&Tube::named("shop").registered(), &|| false)?;
    let none = library.keep_pending(&Providers::none(), &|| false)?;

    assert_eq!(summary, KeepSummary::default());
    assert_eq!(none, KeepSummary::default());
    assert_eq!(library.keeps_pending()?, 1);
    Ok(())
}

#[test]
fn a_keep_waits_while_the_connection_is_metered_and_goes_on_when_it_is_not() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let metering = Metering::new();

    metering.report(Meter::Metered);
    library.stall_downloads_by(MeteredGate::new(metering.clone(), true));

    let tube = Tube::new();

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    let summary = thread::scope(|scope| {
        let keeping = scope.spawn(|| pass(&library, &tube));

        thread::sleep(THE_POLL_HAS_WAITED);

        assert!(!keeping.is_finished());
        assert!(tube.videos_asked().is_empty(), "a stalled keep asked");
        assert_eq!(library.keeps_pending().expect("it reads"), 1);

        metering.report(Meter::Unmetered);
        keeping.join().expect("the pass ends")
    })?;

    assert_eq!(summary.landed, 1);
    assert_eq!(tube.videos_asked(), ["abc123"]);
    Ok(())
}

#[test]
fn a_keep_cancelled_before_it_begins_asks_nothing() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    let summary = library.keep_pending(&tube.registered(), &|| true)?;

    assert_eq!(summary, KeepSummary::default());
    assert!(tube.videos_asked().is_empty());
    assert_eq!(library.keeps_pending()?, 1);
    Ok(())
}

#[test]
fn a_keep_cancelled_part_way_through_a_download_leaves_the_song_queued() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("abc123", &[TubeAnswer::Hanging]);
    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    let told = AtomicBool::new(false);
    let cancelled = || told.load(Ordering::SeqCst);
    let providers = tube.registered();

    let summary = thread::scope(|scope| {
        let keeping = scope.spawn(|| library.keep_pending(&providers, &cancelled));

        staged_by_a_keep_under_way(tree.path());
        told.store(true, Ordering::SeqCst);
        keeping.join().expect("the pass ends")
    })?;

    assert_eq!(summary, KeepSummary::default());
    assert_eq!(library.keeps_pending()?, 1);
    assert!(staged_under(tree.path()).is_empty());
    assert!(songs_under(&tree).is_empty());
    Ok(())
}

#[test]
fn a_kept_song_filed_elsewhere_since_still_seats_what_lands_after_it() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    tube.answering("two", &[TubeAnswer::Busy]);
    library.keep_playlist(list_to_keep(
        "Mix",
        true,
        &[("one", "One"), ("two", "Two"), ("three", "Three")],
    ))?;
    pass(&library, &tube)?;

    let summary = library
        .organise(OrganiseOptions {
            layout: Layout::read("Filed/{title}")?,
            apply: true,
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert_eq!(summary.stats.moved, 2);
    assert!(tree.path().join("Filed/Three.wav").exists());

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["One", "Two", "Three"]);
    Ok(())
}

#[test]
fn a_kept_list_named_as_a_saved_query_stands_beside_it_as_one_list() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();
    let query = library.save_query(
        "Mix",
        &SavedQuery {
            text: Some("one".to_owned()),
            sort: SortOrder::Title,
            reading: Direction::Ascending,
            limit: None,
        },
    )?;

    library.keep_playlist(list_to_keep("Mix", true, &[("one", "One"), ("two", "Two")]))?;

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix (copy)")?, ["One", "Two"]);
    assert_eq!(
        library.playlist_named("Mix (copy 2)")?.map(|list| list.id),
        None
    );
    assert_eq!(
        library.playlist_named("Mix")?.map(|list| list.id),
        Some(query)
    );
    Ok(())
}

#[test]
fn a_song_with_no_album_is_filed_under_its_artist_and_one_with_neither_at_the_top() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();

    library.keep_song(SongToKeep {
        album: None,
        ..song_to_keep("artist", "Artist Alone")
    })?;
    library.keep_song(SongToKeep {
        artist: None,
        length: None,
        album: None,
        ..song_to_keep("neither", "Neither")
    })?;

    pass(&library, &tube)?;

    assert_eq!(
        songs_under(&tree),
        [
            tree.path().join("Neither.wav"),
            tree.path().join("The Orbiters/Artist Alone.wav"),
        ]
    );
    Ok(())
}

#[test]
fn a_blank_title_is_refused_at_the_press_and_nothing_is_queued() -> Result<()> {
    let (_tree, library) = a_music_folder()?;

    assert!(matches!(
        library.keep_song(song_to_keep("abc123", "  ")),
        Err(Error::KeptWithNoTitle)
    ));
    assert!(matches!(
        library.keep_playlist(list_to_keep("Mix", true, &[("one", "One"), ("blank", "")])),
        Err(Error::KeptWithNoTitle)
    ));
    assert_eq!(library.keeps_pending()?, 0);
    assert_eq!(library.playlist_named("Mix")?.map(|list| list.id), None);
    Ok(())
}

fn answered_with_held(held_title: &str, pressed_title: &str) -> Result<KeptSong> {
    let (tree, library) = a_music_folder()?;

    held_as(&tree, "held.wav", held_title, A_SECOND_OF_FRAMES);
    scan(&library, &options(&tree))?;

    library.keep_song(song_to_keep("pressed", pressed_title))
}

#[test]
fn a_held_copy_differing_only_in_a_version_a_guest_or_a_spelling_is_held_either_way_round()
-> Result<()> {
    for (held, pressed) in [
        ("Song (Remastered)", "Song"),
        ("Song", "Song (Remastered)"),
        ("Song (feat. Someone)", "Song"),
        ("Song", "Song (feat. Someone)"),
        ("Song - 2011 Remaster", "Song"),
        ("Song", "Song - 2011 Remaster"),
        ("Song (feat. Someone)", "Song - 2011 Remaster"),
        ("Dont Stop", "Don't Stop"),
        ("Don't Stop", "Dont Stop"),
        ("Dont", "Don\u{2019}t"),
        ("Don't", "Dont"),
        ("Don't Stop (Remastered)", "Dont Stop"),
        ("S.O.S.", "SOS"),
        ("SOS", "S.O.S."),
        ("Hifi", "Hi-Fi"),
        ("Hi-Fi", "Hifi"),
    ] {
        assert!(
            matches!(answered_with_held(held, pressed)?, KeptSong::AlreadyHeld(_)),
            "{held:?} did not stand for {pressed:?}"
        );
    }
    Ok(())
}

#[test]
fn a_live_remix_acoustic_or_instrumental_take_is_another_song() -> Result<()> {
    for take in [
        "Song (Live)",
        "Song (Remix)",
        "Song (Acoustic)",
        "Song (Instrumental)",
        "Song - Live",
    ] {
        assert_eq!(
            answered_with_held(take, "Song")?,
            KeptSong::Queued,
            "{take:?} stood for the song"
        );
        assert_eq!(
            answered_with_held("Song", take)?,
            KeptSong::Queued,
            "the song stood for {take:?}"
        );
    }

    assert_eq!(
        answered_with_held("Song And More", "Song")?,
        KeptSong::Queued
    );
    Ok(())
}

#[test]
fn a_held_clean_take_stands_only_for_a_clean_one() -> Result<()> {
    assert_eq!(
        answered_with_held("Song (Clean)", "Song (Explicit)")?,
        KeptSong::Queued
    );
    assert_eq!(
        answered_with_held("Song (Clean)", "Song")?,
        KeptSong::Queued
    );
    assert!(matches!(
        answered_with_held("Song (Clean)", "Song (Clean)")?,
        KeptSong::AlreadyHeld(_)
    ));
    assert!(matches!(
        answered_with_held("Song (Explicit)", "Song (Clean)")?,
        KeptSong::AlreadyHeld(_)
    ));
    assert!(matches!(
        answered_with_held("Song (Explicit)", "Song")?,
        KeptSong::AlreadyHeld(_)
    ));
    Ok(())
}

fn on_disc(tree: &Tree) -> Result<(PathBuf, Library)> {
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    delivering_into(&library, tree);
    Ok((database, library))
}

fn walk_held_by_another_process(database: &Path) -> fs::File {
    let mut named = database.as_os_str().to_owned();

    named.push(".walk");

    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(PathBuf::from(named))
        .expect("the walk's lock opens");

    lock.try_lock().expect("the walk's lock is free");
    lock
}

#[test]
fn a_landing_the_walk_refused_is_read_before_the_pass_ends() -> Result<()> {
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let tube = Tube::new();

    library.keep_playlist(list_to_keep("Mix", true, &[("one", "One"), ("two", "Two")]))?;

    let held = walk_held_by_another_process(&database);

    tube.when_asked_for("two", move || drop(held));

    let began = Instant::now();
    let summary = pass(&library, &tube)?;

    assert!(
        began.elapsed() < A_REFUSED_WALK_HOLDS_NO_SONG_UP_FOR,
        "the next song waited out the walk"
    );
    assert_eq!(summary.landed, 2);
    assert_eq!(titles_in(&library, "Mix")?, ["One", "Two"]);
    Ok(())
}

#[test]
fn a_file_landed_that_no_scan_reached_is_read_at_the_head_of_the_next_pass() -> Result<()> {
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let tube = Tube::new();
    let by_link = SongToKeep {
        artist: None,
        length: None,
        ..song_to_keep("abc123", "San Tropez")
    };

    library.keep_playlist(PlaylistToKeep {
        name: "Mix".to_owned(),
        ordered: true,
        songs: vec![by_link.clone()],
    })?;
    pass(&library, &tube)?;
    beside(&database)
        .execute("DELETE FROM tracks", [])
        .expect("the row a scan never wrote");

    assert_eq!(titles_in(&library, "Mix")?, [""]);

    assert_eq!(library.keeps_pending()?, 1, "the read owed was not counted");
    assert_eq!(library.keep_song(by_link.clone())?, KeptSong::Queued);
    assert_eq!(library.keeps_pending()?, 2, "the press queued nothing");

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.landed, 1);
    assert_eq!(titles_in(&library, "Mix")?, ["San Tropez"]);
    assert_eq!(
        tube.videos_asked(),
        ["abc123"],
        "the song was fetched again"
    );
    assert_eq!(songs_under(&tree).len(), 1);
    assert_eq!(library.keeps_pending()?, 0);
    assert!(matches!(
        library.keep_song(by_link)?,
        KeptSong::AlreadyHeld(_)
    ));
    Ok(())
}

#[test]
fn an_empty_queue_still_reads_a_landed_file_no_scan_reached() -> Result<()> {
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let tube = Tube::new();

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;
    pass(&library, &tube)?;
    beside(&database)
        .execute("DELETE FROM tracks", [])
        .expect("the row a scan never wrote");

    let summary = pass(&library, &tube)?;

    assert_eq!(summary, KeepSummary::default());
    assert_eq!(track_titled(&library, "San Tropez")?.title, "San Tropez");
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

fn refused_a_poll(library: &Library) -> bool {
    matches!(
        library.poll(Arc::new(Providers::none()), PollOptions::default()),
        Err(Error::AlreadyAsking { .. })
    )
}

#[test]
fn a_poll_refused_by_a_keep_ends_the_pass_at_the_next_song_and_never_mid_download() -> Result<()> {
    let tree = Tree::new();
    let (_database, library) = on_disc(&tree)?;
    let library = Arc::new(library);
    let tube = Tube::new();

    library.keep_playlist(list_to_keep(
        "Mix",
        true,
        &[("one", "One"), ("two", "Two"), ("three", "Three")],
    ))?;

    let refused = Arc::new(AtomicBool::new(false));
    let asking = Arc::clone(&library);
    let told = Arc::clone(&refused);

    tube.when_asked_for("one", move || {
        told.store(refused_a_poll(&asking), Ordering::SeqCst);
    });

    let summary = pass(&library, &tube)?;

    assert!(refused.load(Ordering::SeqCst), "the poll was not refused");
    assert_eq!(
        summary,
        KeepSummary {
            landed: 1,
            yielded: true,
            ..KeepSummary::default()
        }
    );
    assert_eq!(tube.videos_asked(), ["one"]);
    assert_eq!(library.keeps_pending()?, 2);

    library
        .poll(Arc::new(Providers::none()), PollOptions::default())?
        .join()?;

    let again = pass(&library, &tube)?;

    assert_eq!(again.landed, 2);
    assert!(!again.yielded);
    assert_eq!(titles_in(&library, "Mix")?, ["One", "Two", "Three"]);
    Ok(())
}

#[test]
fn a_poll_refused_in_another_process_leaves_a_keep_to_go_on() -> Result<()> {
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let elsewhere = Library::open(&database)?;
    let tube = Tube::new();

    library.keep_song(song_to_keep("one", "One"))?;
    library.keep_song(song_to_keep("two", "Two"))?;

    let refused = Arc::new(AtomicBool::new(false));
    let told = Arc::clone(&refused);

    tube.when_asked_for("one", move || {
        told.store(refused_a_poll(&elsewhere), Ordering::SeqCst);
    });

    let summary = pass(&library, &tube)?;

    assert!(refused.load(Ordering::SeqCst), "the poll was not refused");
    assert_eq!(summary.landed, 2);
    assert!(!summary.yielded);
    Ok(())
}

#[test]
fn a_poll_refused_while_a_keep_waits_on_the_meter_ends_the_wait() -> Result<()> {
    let tree = Tree::new();
    let (_database, library) = on_disc(&tree)?;
    let metering = Metering::new();

    metering.report(Meter::Metered);
    library.stall_downloads_by(MeteredGate::new(metering, true));

    let tube = Tube::new();

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;

    let providers = tube.registered();
    let begun = AtomicBool::new(false);
    let heeded = || {
        begun.store(true, Ordering::SeqCst);
        false
    };

    let summary = thread::scope(|scope| {
        let keeping = scope.spawn(|| library.keep_pending(&providers, &heeded));

        assert!(
            waited_for(|| begun.load(Ordering::SeqCst)),
            "the keep never began"
        );
        assert!(refused_a_poll(&library), "the poll was not refused");
        keeping.join().expect("the pass ends")
    })?;

    assert!(summary.yielded);
    assert!(tube.videos_asked().is_empty());
    assert_eq!(library.keeps_pending()?, 1);
    Ok(())
}

#[test]
fn a_kept_list_deleted_is_made_again_by_a_second_press() -> Result<()> {
    let (_tree, library) = a_music_folder()?;
    let tube = Tube::new();
    let list = list_to_keep("Mix", true, &[("one", "One"), ("two", "Two")]);

    library.keep_playlist(list.clone())?;
    pass(&library, &tube)?;

    let made = library.playlist_named("Mix")?.expect("the list");

    assert!(library.remove_playlist(made.id)?);

    let kept = library.keep_playlist(list)?;

    assert_eq!(kept, KeptList { queued: 0, held: 2 });
    assert_eq!(titles_in(&library, "Mix")?, ["One", "Two"]);
    assert_eq!(tube.videos_asked(), ["one", "two"]);
    Ok(())
}

#[test]
fn a_kept_list_deleted_with_a_file_of_its_gone_asks_for_that_song_again() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let tube = Tube::new();
    let list = PlaylistToKeep {
        name: "Mix".to_owned(),
        ordered: true,
        songs: vec![
            song_to_keep("one", "One"),
            SongToKeep {
                artist: None,
                length: None,
                album: None,
                ..song_to_keep("two", "Two")
            },
        ],
    };

    library.keep_playlist(list.clone())?;
    pass(&library, &tube)?;

    let made = library.playlist_named("Mix")?.expect("the list");

    assert!(library.remove_playlist(made.id)?);

    fs::remove_file(tree.path().join("Two.wav")).expect("the file goes");

    let kept = library.keep_playlist(list)?;

    assert_eq!(kept, KeptList { queued: 1, held: 1 });

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["One", "Two"]);
    Ok(())
}

#[test]
fn a_press_seats_the_rows_it_added_and_leaves_the_order_the_listener_gave_the_rest() -> Result<()> {
    let (tree, library) = a_music_folder()?;
    let one = held_as(&tree, "one.wav", "One", A_SECOND_OF_FRAMES);
    let two = held_as(&tree, "two.wav", "Two", A_SECOND_OF_FRAMES);

    held_as(&tree, "three.wav", "Three", A_SECOND_OF_FRAMES);
    scan(&library, &options(&tree))?;

    let theirs = library.create_playlist("Mix")?;

    library.add_to_playlist(
        theirs,
        &[
            Cut::whole(MediaLocation::local(&two)),
            Cut::whole(MediaLocation::local(&one)),
        ],
    )?;

    let kept = library.keep_playlist(list_to_keep(
        "Mix",
        true,
        &[("one", "One"), ("two", "Two"), ("three", "Three")],
    ))?;

    assert_eq!(kept, KeptList { queued: 0, held: 3 });
    assert_eq!(titles_in(&library, "Mix")?, ["Two", "One", "Three"]);
    Ok(())
}

fn the_listeners_again(database: &Path) {
    beside(database)
        .execute("DELETE FROM playlist_shares", [])
        .expect("the list is the listener's again");
}

#[test]
fn a_list_followed_since_the_press_asks_for_nothing_and_holds_nothing_up() -> Result<()> {
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let tube = Tube::new();

    library.create_playlist("Mix")?;
    library.keep_playlist(list_to_keep("Mix", true, &[("one", "One")]))?;
    library.keep_song(song_to_keep("two", "Two"))?;
    followed(&database, "Mix");

    let summary = pass(&library, &tube)?;

    assert_eq!(
        summary,
        KeepSummary {
            landed: 1,
            unkept: 1,
            ..KeepSummary::default()
        }
    );
    assert_eq!(pass(&library, &tube)?.unkept, 1);
    assert_eq!(tube.videos_asked(), ["two"]);
    assert_eq!(songs_under(&tree).len(), 1);
    assert_eq!(library.keeps_pending()?, 1, "the song reads as removed");

    the_listeners_again(&database);

    assert_eq!(pass(&library, &tube)?.landed, 1);

    assert_eq!(titles_in(&library, "Mix")?, ["One"]);
    assert_eq!(tube.videos_asked(), ["two", "one"]);
    assert_eq!(songs_under(&tree).len(), 2);
    Ok(())
}

#[test]
fn a_list_followed_as_its_song_was_fetched_holds_nothing_up_and_fetches_nothing_twice() -> Result<()>
{
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let tube = Tube::new();

    library.create_playlist("Mix")?;
    library.keep_playlist(list_to_keep("Mix", true, &[("one", "One")]))?;
    library.keep_song(song_to_keep("two", "Two"))?;

    let following = database.clone();

    tube.when_asked_for("one", move || followed(&following, "Mix"));

    let summary = pass(&library, &tube)?;

    assert_eq!(
        summary,
        KeepSummary {
            landed: 1,
            unkept: 1,
            ..KeepSummary::default()
        }
    );
    assert_eq!(pass(&library, &tube)?.unkept, 1);
    assert_eq!(tube.videos_asked(), ["one", "two"]);
    assert_eq!(library.keeps_pending()?, 1, "the song reads as removed");

    the_listeners_again(&database);

    assert_eq!(pass(&library, &tube)?.landed, 1);

    assert_eq!(titles_in(&library, "Mix")?, ["One"]);
    assert_eq!(tube.videos_asked(), ["one", "two"]);
    assert_eq!(songs_under(&tree).len(), 2);
    Ok(())
}

#[test]
fn a_kept_list_named_as_a_followed_list_stands_beside_it() -> Result<()> {
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let tube = Tube::new();

    held_as(&tree, "held.wav", "Held", A_SECOND_OF_FRAMES);
    scan(&library, &options(&tree))?;
    library.create_playlist("Mix")?;
    followed(&database, "Mix");

    let list = list_to_keep("Mix", true, &[("held", "Held"), ("one", "One")]);

    let kept = library.keep_playlist(list.clone())?;

    assert_eq!(kept, KeptList { queued: 1, held: 1 });

    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix (copy)")?, ["Held", "One"]);
    assert_eq!(titles_in(&library, "Mix")?, Vec::<String>::new());
    assert_eq!(library.keeps_pending()?, 0);

    the_listeners_again(&database);

    let kept = library.keep_playlist(list)?;

    assert_eq!(kept, KeptList { queued: 0, held: 2 });
    assert_eq!(titles_in(&library, "Mix")?, ["Held", "One"]);
    assert_eq!(tube.videos_asked(), ["one"]);
    Ok(())
}

#[test]
fn a_folder_forgotten_stays_forgotten_whatever_landed_in_it() -> Result<()> {
    let tree = Tree::new();
    let (_database, library) = on_disc(&tree)?;
    let tube = Tube::new();

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;
    pass(&library, &tube)?;

    assert_eq!(library.roots()?.len(), 1);

    assert!(library.remove_root(tree.path())?);

    assert_eq!(
        library.keeps_pending()?,
        0,
        "a file outside every root owed"
    );
    assert_eq!(pass(&library, &tube)?, KeepSummary::default());
    assert_eq!(library.roots()?, Vec::<PathBuf>::new());
    Ok(())
}

#[test]
fn a_landed_file_outside_every_root_owes_no_pass_once_the_music_folder_moved() -> Result<()> {
    let tree = Tree::new();
    let elsewhere = Tree::new();
    let (_database, library) = on_disc(&tree)?;
    let tube = Tube::new();

    library.keep_song(song_to_keep("abc123", "San Tropez"))?;
    pass(&library, &tube)?;

    assert!(library.remove_root(tree.path())?);

    delivering_into(&library, &elsewhere);

    assert_eq!(library.keeps_pending()?, 0);
    assert_eq!(pass(&library, &tube)?, KeepSummary::default());
    assert_eq!(library.roots()?, Vec::<PathBuf>::new());
    Ok(())
}

fn keeps_with_the_walk_refused(folder: &Path) -> Result<()> {
    let library = Library::open(&folder.join("library.db"))?;

    library.deliver_into(Some(DeliveryFolder {
        path: folder.to_path_buf(),
        layout: Layout::default(),
    }));
    pass(&library, &Tube::new()).map(drop)
}

fn root_left_by_a_keep_under_way(library: &Library) -> Result<()> {
    let began = Instant::now();

    while library.roots()?.is_empty() {
        assert!(
            began.elapsed() < A_KILLED_KEEP_IS_STAGED_WITHIN,
            "the landing left no root"
        );

        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[test]
fn a_folder_never_scanned_is_read_by_the_next_run_though_the_walk_refused_the_landing() -> Result<()>
{
    if let Some(folder) = env::var_os(UNREAD_KEEP_FOLDER) {
        return keeps_with_the_walk_refused(Path::new(&folder));
    }

    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;

    library.keep_playlist(list_to_keep("Mix", true, &[("abc123", "San Tropez")]))?;

    assert_eq!(library.roots()?, Vec::<PathBuf>::new());

    let walk = walk_held_by_another_process(&database);

    let mut killed = Command::new(env::current_exe().expect("the test binary"))
        .args([
            in_this_suite(
                "a_folder_never_scanned_is_read_by_the_next_run_though_the_walk_refused_the_landing",
            )
            .as_str(),
            "--exact",
        ])
        .env(UNREAD_KEEP_FOLDER, tree.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the test binary runs");
    let root_left = root_left_by_a_keep_under_way(&library);

    killed.kill().expect("the keep is killed");
    killed.wait().expect("the keep is gone");
    root_left?;
    drop(walk);

    assert_eq!(titles_in(&library, "Mix")?, [""]);
    assert_eq!(library.keeps_pending()?, 1, "the read owed was not counted");
    assert_eq!(library.keeps_queued()?, 0, "a landed song is not queued");

    let tube = Tube::new();

    assert_eq!(pass(&library, &tube)?, KeepSummary::default());

    assert_eq!(titles_in(&library, "Mix")?, ["San Tropez"]);
    assert_eq!(library.keeps_pending()?, 0);
    assert!(tube.videos_asked().is_empty());
    Ok(())
}

#[test]
fn a_press_of_a_song_landed_in_a_forgotten_folder_brings_the_folder_back() -> Result<()> {
    let tree = Tree::new();
    let (_database, library) = on_disc(&tree)?;
    let tube = Tube::new();
    let by_link = SongToKeep {
        artist: None,
        length: None,
        ..song_to_keep("abc123", "San Tropez")
    };

    library.keep_song(by_link.clone())?;
    pass(&library, &tube)?;

    assert!(library.remove_root(tree.path())?);

    assert_eq!(library.keep_song(by_link.clone())?, KeptSong::Queued);
    assert_eq!(library.keeps_pending()?, 1);

    pass(&library, &tube)?;

    assert_eq!(library.roots()?.len(), 1);
    assert_eq!(
        tube.videos_asked(),
        ["abc123"],
        "the song was fetched again"
    );
    assert!(matches!(
        library.keep_song(by_link)?,
        KeptSong::AlreadyHeld(_)
    ));
    Ok(())
}

#[test]
fn a_song_of_a_new_list_landed_in_a_forgotten_folder_is_read_by_the_pass() -> Result<()> {
    let tree = Tree::new();
    let (_database, library) = on_disc(&tree)?;
    let tube = Tube::new();
    let by_link = SongToKeep {
        artist: None,
        length: None,
        ..song_to_keep("abc123", "San Tropez")
    };

    library.keep_song(by_link.clone())?;
    pass(&library, &tube)?;

    assert!(library.remove_root(tree.path())?);

    library.keep_playlist(PlaylistToKeep {
        name: "Mix".to_owned(),
        ordered: true,
        songs: vec![by_link],
    })?;
    pass(&library, &tube)?;

    assert_eq!(titles_in(&library, "Mix")?, ["San Tropez"]);
    assert_eq!(
        tube.videos_asked(),
        ["abc123"],
        "the song was fetched again"
    );
    Ok(())
}

#[test]
fn the_last_landing_the_walk_refused_is_read_before_the_pass_ends() -> Result<()> {
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let tube = Tube::new();

    library.keep_playlist(list_to_keep("Mix", true, &[("one", "One")]))?;

    let held = walk_held_by_another_process(&database);

    tube.when_asked_for("one", move || {
        thread::spawn(move || {
            thread::sleep(A_WALK_ENDS_AFTER_THE_LANDING);
            drop(held);
        });
    });

    let summary = pass(&library, &tube)?;

    assert_eq!(summary.landed, 1);
    assert_eq!(titles_in(&library, "Mix")?, ["One"]);
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

#[test]
fn a_kept_song_folded_into_its_twin_is_still_known_by_its_video() -> Result<()> {
    let tree = Tree::new();
    let trash = Tree::new();
    let (database, library) = on_disc(&tree)?;

    library.trash_into(Some(trash.path().to_path_buf()));

    let tube = Tube::new();
    let by_link = SongToKeep {
        artist: None,
        length: None,
        album: None,
        ..song_to_keep("abc123", "San Tropez")
    };

    library.keep_song(by_link.clone())?;
    pass(&library, &tube)?;

    let twin = tree.path().join("San Tropez.wav");
    let copy = tree.path().join("San Tropez (1).wav");

    fs::copy(&twin, &copy).expect("a copy");
    scan(&library, &options(&tree))?;
    beside(&database)
        .execute(
            "UPDATE kept_songs SET landed = ?1",
            [copy.to_str().expect("the fixture path is UTF-8")],
        )
        .expect("the song landed as the copy");

    let found = library.identical_copies()?;

    assert_eq!(found.identical.len(), 1);

    library.fold_copies(&found.identical)?;

    assert!(matches!(
        library.keep_song(by_link)?,
        KeptSong::AlreadyHeld(_)
    ));
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}

#[test]
fn a_song_kept_by_its_link_on_a_drive_unplugged_is_not_fetched_again() -> Result<()> {
    let tree = Tree::new();
    let (database, library) = on_disc(&tree)?;
    let drive = tree
        .path()
        .canonicalize()
        .expect("the fixture tree has a canonical path")
        .join("drive");

    fs::create_dir(&drive).expect("the fixture drive");
    library.deliver_into(Some(DeliveryFolder {
        path: drive.clone(),
        layout: Layout::default(),
    }));

    let tube = Tube::new();
    let by_link = SongToKeep {
        artist: None,
        length: None,
        ..song_to_keep("abc123", "San Tropez")
    };

    library.keep_song(by_link.clone())?;
    pass(&library, &tube)?;
    beside(&database)
        .execute(
            "INSERT OR IGNORE INTO volumes (path) VALUES (?1)",
            [drive.to_str().expect("the fixture path is UTF-8")],
        )
        .expect("the catalog takes a volume");
    fs::remove_dir_all(&drive).expect("the drive is unplugged");

    assert!(matches!(
        library.keep_song(by_link)?,
        KeptSong::AlreadyHeld(_)
    ));
    assert_eq!(library.keeps_pending()?, 0);
    Ok(())
}
