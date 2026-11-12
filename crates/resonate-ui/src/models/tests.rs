use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use gpui::{BorrowAppContext as _, Context, Entity, TestAppContext};
use parking_lot::Mutex;
use resonate_core::{AlbumId, ArtistId, MediaLocation, SourceId};
use resonate_library::{
    AlbumFound, Direction, Found, Isrc, KeepStopped, KeepSummary, Library, Mbid, PassKind,
    PollStats, Shared, SortOrder,
};
use resonate_providers::{Identity, Obtained, Provider, Providers, WhyAway};

use super::{
    Beyond, Change, Favourited, Followed, KeepPass, LibraryModel, ListedRow, MissingRow, Notice,
    POLLS_PER_REREAD_WHILE_ASKING, Pass, Planned, Wanted,
    arranged::{
        Arranging, Reaching, albums_kept_before_the_rest, arranged, beyond_the_listing,
        elsewhere_standing, headed_by_disc, held_at, held_in, kept_before_the_rest,
        missing_track_rows, unheld_release_rows,
    },
    delivery_folder, kept,
    pictures::{landed_since, renewed},
    stopped_told,
    told::{on_the_clipboard, retried_after, sharing_told},
    watched::Watching,
};
use crate::{
    ResonateApp,
    driven::{Driven, Folder},
    held::Rows,
    served::ServedSong,
    serving::{Row, song_to_keep},
    settings::{Online, Supplying},
    toast, words,
};

const KEPT_FROM: &str = "kept-from";
const HELD_FOR_A_LOOK: Duration = Duration::from_millis(5);

struct Gated {
    source: SourceId,
    asked: Arc<Mutex<Vec<String>>>,
    released: Arc<AtomicBool>,
}

impl Provider for Gated {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> resonate_providers::Result<Obtained> {
        self.asked.lock().push(identity.title.clone());
        while !self.released.load(Ordering::Relaxed) {
            thread::sleep(HELD_FOR_A_LOOK);
        }
        Ok(Obtained::Nothing)
    }
}

struct Keeping {
    driven: Driven,
    model: Entity<LibraryModel>,
    asked: Arc<Mutex<Vec<String>>>,
    released: Arc<AtomicBool>,
    _music: Folder,
}

impl Keeping {
    fn open(cx: &mut TestAppContext, online: Online, music_folder: bool) -> Self {
        let music = Folder::new();
        let catalog = music.path().join("library.db");
        let library = Arc::new(Library::open(&catalog).expect("a catalog on disc"));
        let mut driven = Driven::open(cx, library);
        let asked = Arc::new(Mutex::new(Vec::new()));
        let released = Arc::new(AtomicBool::new(false));
        let (hearing, holding) = (Arc::clone(&asked), Arc::clone(&released));
        let path = music.path().to_path_buf();
        let model = driven.read(|root, _| root.library.clone());
        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|global, _| {
                global.online = online;
                global.music_folder = music_folder.then_some(path);
            });
            model.update(cx, |library, _| {
                library.sourcing.register = Arc::new(move |_: &Supplying<'_>| {
                    Providers::none().and(Arc::new(Gated {
                        source: SourceId::new(KEPT_FROM).expect("a source"),
                        asked: Arc::clone(&hearing),
                        released: Arc::clone(&holding),
                    }))
                });
            });
        });
        Self {
            driven,
            model,
            asked,
            released,
            _music: music,
        }
    }

    fn pressed(&mut self, song: &str) {
        let row = kept_row(song);
        let model = self.model.clone();
        self.driven
            .cx
            .update(|_, cx| model.update(cx, |library, cx| library.keep(&row, cx)));
    }

    fn until_asked(&mut self, songs: usize) {
        let asked = Arc::clone(&self.asked);
        self.driven.until(|_, _| asked.lock().len() >= songs);
    }

    fn until_the_pass_ends(&mut self) {
        let model = self.model.clone();
        self.driven
            .until(|_, cx| model.read(cx).keeps_left().is_none());
    }

    fn queued(&mut self) -> u64 {
        let model = self.model.clone();
        self.driven.cx.update(|_, cx| {
            model
                .read(cx)
                .library
                .keeps_pending()
                .expect("what is queued read")
        })
    }
}

fn kept_row(title: &str) -> Row {
    Row::Song(ServedSong {
        rating: resonate_core::titles::Rating::Unmarked,
        location: MediaLocation::from_uri(&format!("{KEPT_FROM}:track/{title}"))
            .expect("a location"),
        title: title.to_owned(),
        artist: Some("Ada".to_owned()),
        album: None,
        track: None,
        length: None,
        format: None,
        cover: None,
        favourite: None,
    })
}

fn turned_on() -> Online {
    Online {
        enabled: true,
        youtube_music: true,
        ..Online::default()
    }
}

#[gpui::test]
fn a_song_pressed_while_a_pass_runs_waits_for_it_and_is_asked_by_the_next_pass(
    cx: &mut TestAppContext,
) {
    let mut keeping = Keeping::open(cx, turned_on(), true);

    keeping.pressed("one");
    keeping.until_asked(1);
    keeping.pressed("two");
    let queued = keeping.model.clone();
    keeping.driven.until(|_, cx| queued.read(cx).keeps.owed);
    assert_eq!(keeping.queued(), 2);
    assert_eq!(
        keeping.asked.lock().clone(),
        ["one"],
        "no second pass began"
    );

    keeping.released.store(true, Ordering::Relaxed);
    keeping.until_asked(2);
    keeping.until_the_pass_ends();

    assert_eq!(keeping.asked.lock().clone(), ["one", "two"]);
    assert_eq!(keeping.queued(), 0);
}

#[gpui::test]
fn a_poll_asked_for_during_a_pass_goes_first_and_the_pass_is_begun_again_after_it(
    cx: &mut TestAppContext,
) {
    let mut keeping = Keeping::open(cx, turned_on(), true);
    let model = keeping.model.clone();
    keeping.driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.library.deliver_into(delivery_folder(cx));
            for title in ["one", "two"] {
                let song = song_to_keep(kept_row(title).song().expect("a song"));
                library
                    .library
                    .keep_song(song.expect("a song to keep"))
                    .expect("the song queued");
            }
            library.keep_what_is_queued(cx);
        });
    });
    keeping.until_asked(1);
    keeping
        .driven
        .cx
        .update(|_, cx| model.update(cx, |library, cx| library.poll(cx)));
    let owing = keeping.model.clone();
    keeping
        .driven
        .until(|_, cx| owing.read(cx).running.poll_owed.is_some());

    keeping.released.store(true, Ordering::Relaxed);
    keeping.until_asked(2);
    keeping.until_the_pass_ends();

    assert_eq!(keeping.asked.lock().clone(), ["one", "two"]);
    assert_eq!(keeping.queued(), 0);
}

#[gpui::test]
fn a_pass_that_gave_way_owes_the_poll_it_gave_way_to_and_a_pass_after_it(cx: &mut TestAppContext) {
    let mut keeping = Keeping::open(cx, turned_on(), true);
    let model = keeping.model.clone();

    keeping.driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.keep_pass_answered(
                &KeepSummary {
                    yielded: true,
                    ..KeepSummary::default()
                },
                cx,
            );
        });
    });

    let (owes_a_pass, owes_a_poll) = keeping.driven.cx.update(|_, cx| {
        let library = model.read(cx);
        (library.keeps.owed, library.running.poll_owed.is_some())
    });
    assert!(owes_a_pass);
    assert!(owes_a_poll);
}

#[gpui::test]
fn a_pass_stopped_leaves_the_rest_queued_and_owes_nothing(cx: &mut TestAppContext) {
    let mut keeping = Keeping::open(cx, turned_on(), true);
    keeping.pressed("one");
    keeping.until_asked(1);
    keeping.pressed("two");
    let model = keeping.model.clone();
    keeping.driven.until(|_, cx| model.read(cx).keeps.owed);

    keeping
        .driven
        .cx
        .update(|_, cx| model.update(cx, |library, cx| library.stop_keeping(cx)));
    keeping.until_the_pass_ends();
    keeping.released.store(true, Ordering::Relaxed);
    keeping.driven.settle();

    assert_eq!(keeping.asked.lock().clone(), ["one"]);
    assert_eq!(keeping.queued(), 2);
}

#[gpui::test]
fn a_keep_with_no_music_folder_is_refused_and_nothing_is_queued(cx: &mut TestAppContext) {
    let mut keeping = Keeping::open(cx, turned_on(), false);

    keeping.pressed("one");
    keeping.driven.settle();
    keeping.until_the_pass_ends();

    assert!(keeping.asked.lock().is_empty());
    assert_eq!(keeping.queued(), 0);
}

#[gpui::test]
fn what_was_left_queued_is_taken_up_at_open_only_while_youtube_music_is_on(
    cx: &mut TestAppContext,
) {
    let mut off = Keeping::open(
        cx,
        Online {
            youtube_music: false,
            ..turned_on()
        },
        true,
    );
    let left = off.model.clone();
    off.driven.cx.update(|_, cx| {
        left.update(cx, |library, cx| {
            library.library.deliver_into(delivery_folder(cx));
            library
                .library
                .keep_song(song_to_keep(kept_row("one").song().expect("a song")).expect("kept"))
                .expect("the song queued");
            library.keep_what_was_left_queued(cx);
        });
    });
    off.driven.settle();
    assert!(off.asked.lock().is_empty());

    off.driven.cx.update(|_, cx| {
        cx.update_global::<ResonateApp, _>(|global, _| global.online = turned_on());
        left.update(cx, |library, cx| library.keep_what_was_left_queued(cx));
    });
    off.released.store(true, Ordering::Relaxed);
    off.until_asked(1);
    off.until_the_pass_ends();

    assert_eq!(off.queued(), 0);
}

#[test]
fn the_roots_are_read_once_in_a_while_and_not_at_every_look() {
    let library = Library::open_in_memory().expect("an in-memory catalog");
    let watching = Watching::default();
    assert!(watching.is_due_to_read_the_roots());

    let watching = watching.following(&library);
    assert!(!watching.is_due_to_read_the_roots());
}

#[test]
fn a_pass_says_what_it_kept_and_one_stopped_by_its_service_says_so() {
    let landed = KeepSummary {
        landed: 2,
        ..KeepSummary::default()
    };

    assert_eq!(kept(&KeepSummary::default()), None);
    assert_eq!(kept(&landed), Some(Notice::Done("Kept 2 songs".to_owned())));
    assert_eq!(
        kept(&KeepSummary {
            unkept: 1,
            ..landed.clone()
        }),
        Some(Notice::Noted("Kept 2 songs, 1 song not".to_owned()))
    );
    assert!(matches!(
        kept(&KeepSummary {
            stopped_by: SourceId::new(KEPT_FROM).ok(),
            stopped: Some(KeepStopped {
                why: WhyAway::NotAnswering,
                song: "Creep".to_owned(),
            }),
            ..landed
        }),
        Some(Notice::Trouble(_))
    ));
}

#[test]
fn a_pass_stopped_names_the_song_and_says_what_to_do() {
    let said = [
        (WhyAway::SignInLapsed, "Sign in"),
        (WhyAway::SubscriptionLapsed, "Premium"),
        (WhyAway::ToolMissing, "Install"),
        (WhyAway::NotAnswering, "stopped answering"),
    ]
    .map(|(why, words)| {
        let told = stopped_told(&KeepStopped {
            why,
            song: "Creep".to_owned(),
        });
        assert!(told.contains("Creep"), "{told}");
        assert!(told.contains(words), "{told}");
        told
    });

    assert!(said.windows(2).all(|pair| pair[0] != pair[1]));
}

#[gpui::test]
fn a_pass_that_gave_way_is_told_with_the_pass_that_ends_and_loses_none_of_its_failures(
    cx: &mut TestAppContext,
) {
    let mut keeping = Keeping::open(cx, turned_on(), true);
    let model = keeping.model.clone();

    let told_of_the_yield = keeping.driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.keep_pass_answered(
                &KeepSummary {
                    landed: 1,
                    gone: 1,
                    yielded: true,
                    ..KeepSummary::default()
                },
                cx,
            );
        });
        toast::showing_text(cx)
    });
    assert_eq!(told_of_the_yield, None);

    let told_at_the_end = keeping.driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.keep_pass_answered(
                &KeepSummary {
                    landed: 2,
                    unkept: 1,
                    ..KeepSummary::default()
                },
                cx,
            );
        });
        toast::showing_text(cx)
    });
    assert_eq!(
        told_at_the_end.as_deref(),
        Some("Kept 3 songs, 2 songs not")
    );
}

fn the_pass_ends_once(keeping: &mut Keeping, turned: impl FnOnce(&mut Online)) {
    keeping.pressed("one");
    keeping.until_asked(1);
    keeping.pressed("two");
    let model = keeping.model.clone();
    keeping.driven.until(|_, cx| model.read(cx).keeps.owed);

    keeping.driven.cx.update(|_, cx| {
        cx.update_global::<ResonateApp, _>(|global, _| turned(&mut global.online));
    });
    keeping.until_the_pass_ends();
    keeping.released.store(true, Ordering::Relaxed);
    keeping.driven.settle();

    assert_eq!(keeping.asked.lock().clone(), ["one"]);
    assert_eq!(keeping.queued(), 2);
}

#[gpui::test]
fn turning_youtube_music_off_ends_a_pass_and_leaves_the_rest_queued(cx: &mut TestAppContext) {
    let mut keeping = Keeping::open(cx, turned_on(), true);

    the_pass_ends_once(&mut keeping, |online| online.youtube_music = false);
}

#[gpui::test]
fn turning_online_off_ends_a_pass_and_leaves_the_rest_queued(cx: &mut TestAppContext) {
    let mut keeping = Keeping::open(cx, turned_on(), true);

    the_pass_ends_once(&mut keeping, |online| online.enabled = false);
}

#[gpui::test]
fn a_stop_is_not_undone_by_a_pass_that_gave_way_or_was_turned_away(cx: &mut TestAppContext) {
    let mut keeping = Keeping::open(cx, turned_on(), true);
    let model = keeping.model.clone();
    let ended_stopped = |library: &mut LibraryModel,
                         ended: resonate_library::Result<KeepSummary>,
                         cx: &mut Context<LibraryModel>| {
        library.keeps.pass = Some(KeepPass {
            stop: Arc::new(AtomicBool::new(true)),
            left: 0,
        });
        library.keep_pass_ended(Ok(ended), cx);
        library.keeps.owed
    };

    let (after_a_yield, after_a_refusal) = keeping.driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            (
                ended_stopped(
                    library,
                    Ok(KeepSummary {
                        yielded: true,
                        ..KeepSummary::default()
                    }),
                    cx,
                ),
                ended_stopped(
                    library,
                    Err(resonate_library::Error::AlreadyAsking {
                        pass: PassKind::Poll,
                    }),
                    cx,
                ),
            )
        })
    });

    assert!(!after_a_yield);
    assert!(!after_a_refusal);
}

fn untitled_row() -> Row {
    let Row::Song(song) = kept_row("one") else {
        unreachable!("a kept row is a song");
    };
    Row::Song(ServedSong {
        title: " ".to_owned(),
        ..song
    })
}

#[gpui::test]
fn a_song_with_no_title_is_told_it_cannot_be_kept(cx: &mut TestAppContext) {
    let mut keeping = Keeping::open(cx, turned_on(), true);
    let model = keeping.model.clone();
    let row = untitled_row();

    keeping
        .driven
        .cx
        .update(|_, cx| model.update(cx, |library, cx| library.keep(&row, cx)));

    keeping
        .driven
        .until(|_, cx| toast::showing_text(cx).is_some_and(|said| said.contains("no title")));
    assert_eq!(keeping.queued(), 0);
}

#[gpui::test]
fn a_list_says_how_many_of_its_rows_were_left_out_for_want_of_a_title(cx: &mut TestAppContext) {
    let mut keeping = Keeping::open(cx, turned_on(), true);
    keeping.released.store(true, Ordering::Relaxed);
    let model = keeping.model.clone();
    let rows = [kept_row("one"), untitled_row()];

    keeping.driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.keep_mix("Road trip".to_owned(), &rows, cx);
        });
    });

    keeping
        .driven
        .until(|_, cx| toast::showing_text(cx).is_some_and(|said| said.contains("1 left out")));
}

#[test]
fn providers_that_keep_refusing_are_asked_less_and_less_often() {
    let waits: Vec<u64> = (0..8).map(|polls| retried_after(polls).as_secs()).collect();

    assert_eq!(waits, [30, 60, 120, 240, 480, 960, 960, 960]);
}

#[test]
fn a_poll_that_moved_is_read_again_while_it_runs_and_a_landing_rereads_everything() {
    let mut followed = Followed::default();
    let answered = PollStats {
        asked: 2,
        nothing: 1,
        ..PollStats::default()
    };
    let landed = PollStats {
        kept: 1,
        ..answered
    };
    let mut rereads = Vec::new();

    for _ in 0..POLLS_PER_REREAD_WHILE_ASKING {
        rereads.push(followed.reread(answered));
    }
    for _ in 0..POLLS_PER_REREAD_WHILE_ASKING {
        rereads.push(followed.reread(answered));
    }
    for _ in 0..POLLS_PER_REREAD_WHILE_ASKING {
        rereads.push(followed.reread(landed));
    }
    let reread: Vec<Wanted> = rereads.into_iter().flatten().collect();

    assert_eq!(reread, [Wanted::ThePlaylists, Wanted::Everything]);
}

#[test]
fn a_read_asked_for_while_another_runs_covers_what_both_would_have_read() {
    let every = [
        Wanted::Everything,
        Wanted::TheSearch,
        Wanted::ThePage,
        Wanted::ThePlaylists,
    ];
    for one in every {
        for other in every {
            let both = one.with(other);
            assert_eq!(both, other.with(one));
            for part in [one, other] {
                assert!(!part.reads_the_listing() || both.reads_the_listing());
                assert!(!part.reads_what_stands() || both.reads_what_stands());
                assert!(!part.reads_the_shelves() || both.reads_the_shelves());
                assert!(!part.reads_the_wants() || both.reads_the_wants());
                assert!(
                    part != Wanted::ThePage || both.reads_the_listing() || both == Wanted::ThePage,
                    "{one:?} with {other:?} dropped the page"
                );
            }
        }
    }
    assert_eq!(Wanted::ThePage.with(Wanted::ThePage), Wanted::ThePage);
    assert!(!Wanted::TheSearch.reads_what_stands());
    assert!(!Wanted::TheSearch.reads_the_wants());
}

#[test]
fn the_roots_are_looked_at_every_second_while_seen_and_once_a_minute_while_unseen() {
    let just = Some(std::time::Instant::now());
    let a_minute_ago = std::time::Instant::now().checked_sub(super::ROOTS_LOOKED_AT_EVERY_UNSEEN);

    assert!(super::watched::roots_looked_at_now(true, just));
    assert!(!super::watched::roots_looked_at_now(false, just));
    assert!(super::watched::roots_looked_at_now(false, a_minute_ago));
    assert!(super::watched::roots_looked_at_now(false, None));
}

#[test]
fn a_list_read_again_the_same_is_left_where_it_stood() {
    let mut held: std::sync::Arc<[u32]> = vec![1, 2, 3].into();
    let before = std::sync::Arc::clone(&held);

    assert!(!renewed(&mut held, vec![1, 2, 3]));
    assert!(std::sync::Arc::ptr_eq(&held, &before));
    assert!(renewed(&mut held, vec![1, 2]));
    assert_eq!(*held, [1, 2]);
}

#[test]
fn a_page_read_past_what_is_held_is_added_to_it_only_where_it_carries_on_from_the_last_row() {
    let mut held: Rows<u32> = vec![1, 2, 3].into();
    let before = held.clone();

    assert_eq!(
        super::watched::grew(&mut held, 3, vec![3]),
        super::Grew::Same
    );
    assert!(held.same_as(&before));
    assert_eq!(
        super::watched::grew(&mut held, 3, vec![3, 4, 5]),
        super::Grew::Renewed
    );
    assert_eq!(held.to_vec(), [1, 2, 3, 4, 5]);
    assert_eq!(
        held.chunks().next().map(<[u32]>::as_ptr),
        before.chunks().next().map(<[u32]>::as_ptr),
        "the rows held are kept as they were, not copied, when a page is added"
    );
    assert_eq!(
        super::watched::grew(&mut held, 5, vec![9, 6]),
        super::Grew::Moved
    );
    assert_eq!(
        super::watched::grew(&mut held, 4, vec![4, 5, 6]),
        super::Grew::Moved
    );
    assert_eq!(
        super::watched::grew(&mut held, 5, Vec::new()),
        super::Grew::Moved
    );
    assert_eq!(held.to_vec(), [1, 2, 3, 4, 5]);
    assert_eq!(
        super::watched::grew(&mut held, 0, vec![7, 8]),
        super::Grew::Renewed
    );
    assert_eq!(held.to_vec(), [7, 8]);
}

#[test]
fn a_list_drawn_far_past_what_it_holds_reaches_as_far_as_it_was_drawn_in_one_read() {
    assert_eq!(
        super::watched::reach_covering(super::PAGE, super::PAGE),
        2 * super::PAGE
    );
    assert_eq!(
        super::watched::reach_covering(super::PAGE, 10),
        2 * super::PAGE
    );
    assert_eq!(
        super::watched::reach_covering(super::PAGE, 49_999),
        (49_999 + super::LOOK_AHEAD).div_ceil(super::PAGE) * super::PAGE
    );
}

#[test]
fn a_lookup_that_kept_lyrics_says_how_many_tracks_it_kept_them_for() {
    let summary = |lyrics| resonate_library::EnrichSummary {
        stats: resonate_library::EnrichStats {
            releases: 2,
            artists: 1,
            lyrics,
            ..resonate_library::EnrichStats::default()
        },
        cancelled: false,
        stopped_by: None,
    };

    assert_eq!(
        super::told::looked_up(&summary(0), super::Prompted::ByHand),
        Some(super::Notice::Noted(
            "2 albums and 1 artist answered".to_owned()
        ))
    );
    assert_eq!(
        super::told::looked_up(&summary(12), super::Prompted::ByHand),
        Some(super::Notice::Noted(
            "2 albums and 1 artist answered · lyrics for 12 tracks".to_owned()
        ))
    );
}

#[test]
fn only_a_playlist_edit_is_told_as_one_when_it_fails() {
    let not_playlists = [
        Change::Want,
        Change::Unwant,
        Change::Dismiss,
        Change::BringBack,
        Change::Favour { favourite: true },
        Change::Favour { favourite: false },
        Change::Hide { hidden: true },
        Change::Hide { hidden: false },
        Change::ForgetDelivered,
        Change::Undo,
        Change::Redo,
    ];

    assert!(Change::Playlist.doing().contains("playlist"));
    for change in not_playlists {
        assert!(!change.doing().contains("playlist"), "{change:?}");
    }
}

const SEARCHED: Reaching = Reaching {
    held: 2,
    whole: true,
    found: 2,
};

#[test]
fn an_artists_page_lists_what_its_discography_lacks_after_what_the_library_holds() {
    let rows = beyond_the_listing(SEARCHED);

    assert_eq!(
        rows,
        vec![
            ListedRow::Held(0),
            ListedRow::Held(1),
            ListedRow::NotHeld(2),
            ListedRow::Found(0),
            ListedRow::Found(1),
        ]
    );
    assert_eq!(held_at(&rows, 1), Some(1));
    assert_eq!(held_at(&rows, 2), None);
    assert_eq!(held_at(&rows, 5), None);
}

#[test]
fn nothing_is_listed_beyond_a_listing_until_the_whole_of_it_has_been_read() {
    assert!(
        beyond_the_listing(Reaching {
            whole: false,
            ..SEARCHED
        })
        .is_empty()
    );
    assert!(
        beyond_the_listing(Reaching {
            found: 0,
            ..SEARCHED
        })
        .is_empty()
    );
}

#[test]
fn a_search_still_being_asked_elsewhere_says_so() {
    assert_eq!(elsewhere_standing(0, true, false), Some(Beyond::Asking));
    assert_eq!(elsewhere_standing(0, false, false), None);
    assert_eq!(
        elsewhere_standing(3, false, false),
        Some(Beyond::Elsewhere(3))
    );
}

fn a_song(recording: &str, title: &str, artist: &str) -> Found {
    Found {
        recording: Mbid::new(recording).expect("a well-formed mbid"),
        title: title.to_owned(),
        artist: artist.to_owned(),
        length: None,
        release: None,
        releases: Vec::new(),
        performer: None,
        isrcs: Vec::new(),
    }
}

#[test]
fn songs_kept_from_a_discography_come_first_and_musicbrainz_does_not_repeat_them() {
    let daybreak = a_song(
        "83d91898-7763-47d7-b03b-b92132375c47",
        "Daybreak",
        "The Orbiters",
    );
    let kept = [daybreak.clone()];
    let answered = [
        a_song(
            "5b11f4ce-a62d-471e-81fc-a69a8278c7da",
            "DAYBREAK",
            "the orbiters",
        ),
        daybreak.clone(),
        a_song(
            "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
            "Daybreak",
            "Someone Else",
        ),
    ];

    let shown = kept_before_the_rest(&kept, &answered);

    assert_eq!(shown, vec![daybreak, answered[2].clone()]);
    assert_eq!(kept_before_the_rest(&[], &answered), answered.to_vec());
}

#[test]
fn a_kept_song_no_code_names_gives_way_in_its_place_to_musicbrainzs_take_that_carries_one() {
    let kept_take = a_song(
        "f5505d60-b054-4538-bd92-75b003774810",
        "Hint Kumaşı",
        "maNga",
    );
    let other = a_song(
        "83d91898-7763-47d7-b03b-b92132375c47",
        "Bir Kadın Çizeceksin",
        "maNga",
    );
    let coded = Found {
        isrcs: vec![Isrc::new("TR0391400015").expect("an isrc")],
        ..a_song(
            "b4ee9f32-b746-4b5a-bb8e-a1bf55857a6b",
            "Hint Kumaşı",
            "maNga",
        )
    };
    let uncoded_elsewhere = a_song(
        "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
        "Hint Kumaşı",
        "maNga",
    );

    let shown = kept_before_the_rest(
        &[kept_take.clone(), other.clone()],
        &[uncoded_elsewhere, coded.clone()],
    );
    let unanswered = kept_before_the_rest(std::slice::from_ref(&kept_take), &[]);

    assert_eq!(shown, vec![coded, other]);
    assert_eq!(unanswered, vec![kept_take]);
}

fn an_album(group: &str, title: &str, artist: &str) -> AlbumFound {
    AlbumFound {
        group: Mbid::new(group).expect("a well-formed mbid"),
        title: title.to_owned(),
        artist: artist.to_owned(),
        kind: Some("Album".to_owned()),
        first_released: None,
    }
}

#[test]
fn albums_kept_from_a_discography_come_first_and_musicbrainz_does_not_repeat_them() {
    let hours = an_album(
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "Hours",
        "The Orbiters",
    );
    let kept = [hours.clone()];
    let answered = [
        hours.clone(),
        an_album(
            "5b11f4ce-a62d-471e-81fc-a69a8278c7da",
            "HOURS",
            "the orbiters",
        ),
        an_album(
            "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
            "Hours",
            "Someone Else",
        ),
    ];

    let shown = albums_kept_before_the_rest(&kept, &answered);

    assert_eq!(shown, vec![hours, answered[2].clone()]);
    assert_eq!(
        albums_kept_before_the_rest(&[], &answered),
        answered.to_vec()
    );
}

#[test]
fn songs_still_listed_while_more_are_asked_for_say_the_rest_is_coming() {
    assert_eq!(
        elsewhere_standing(1, true, false),
        Some(Beyond::Refining(1))
    );
    assert_eq!(elsewhere_standing(1, true, true), Some(Beyond::Refining(1)));
}

#[test]
fn a_search_musicbrainz_did_not_answer_says_so() {
    assert_eq!(
        elsewhere_standing(0, false, true),
        Some(Beyond::Unreached(0))
    );
}

#[test]
fn a_search_musicbrainz_did_not_answer_says_so_beside_the_songs_kept() {
    assert_eq!(
        elsewhere_standing(2, false, true),
        Some(Beyond::Unreached(2))
    );
}

#[test]
fn a_name_held_under_an_id_answers_only_for_the_row_it_was_read_for() {
    use resonate_core::{FrameSpan, Frames, MediaLocation, TrackId};
    use resonate_engine::QueueItem;

    use super::arranged::Named;

    let id = TrackId::new(u64::MAX).expect("a non-zero id");
    let held = Named {
        location: MediaLocation::local("/music/first.flac"),
        span: None,
        track: None,
    };
    let row = |path: &str, span| QueueItem {
        id,
        location: MediaLocation::local(path),
        span,
    };

    assert!(held.names(&row("/music/first.flac", None)));
    assert!(!held.names(&row("/music/second.flac", None)));
    assert!(!held.names(&row(
        "/music/first.flac",
        Some(FrameSpan::starting(Frames(44_100)))
    )));
}

#[test]
fn a_row_of_an_album_names_the_track_it_draws_and_a_heading_names_none() {
    let rows = [
        ListedRow::Disc(1),
        ListedRow::Held(0),
        ListedRow::Missing(0),
        ListedRow::Disc(2),
        ListedRow::Held(1),
    ];

    assert_eq!(held_at(&rows, 0), None);
    assert_eq!(held_at(&rows, 1), Some(0));
    assert_eq!(held_at(&rows, 2), None);
    assert_eq!(held_at(&rows, 4), Some(1));
    assert_eq!(held_at(&rows, 5), None);
}

fn placed(rows: &[(u32, u32, usize)]) -> Vec<((u32, u32), ListedRow)> {
    rows.iter()
        .map(|(disc, position, held)| ((*disc, *position), ListedRow::Held(*held)))
        .collect()
}

const BY_SEAT: Arranging = Arranging {
    sort: SortOrder::AlbumThenTrack,
    reading: Direction::Ascending,
};

#[test]
fn an_album_in_its_own_order_is_drawn_and_played_by_its_seats() {
    let held = placed(&[(1, 3, 0), (1, 1, 1)]);
    let missing = vec![((1, 2), ListedRow::Missing(0))];

    let rows = arranged(held, missing, BY_SEAT);
    assert_eq!(
        rows,
        vec![
            ListedRow::Held(1),
            ListedRow::Missing(0),
            ListedRow::Held(0),
        ]
    );
    assert_eq!(
        held_in(&rows),
        vec![1, 0],
        "an album was played in an order other than the one drawn"
    );
}

#[test]
fn an_album_turned_round_is_drawn_from_its_last_seat() {
    let held = placed(&[(1, 1, 0), (2, 1, 1)]);
    let reversed = Arranging {
        reading: Direction::Descending,
        ..BY_SEAT
    };

    assert_eq!(
        arranged(held, Vec::new(), reversed),
        vec![
            ListedRow::Disc(2),
            ListedRow::Held(1),
            ListedRow::Disc(1),
            ListedRow::Held(0),
        ]
    );
}

#[test]
fn an_album_sorted_another_way_is_drawn_in_that_sort_with_what_it_lacks_after() {
    let held = placed(&[(1, 3, 0), (1, 1, 1)]);
    let missing = vec![
        ((1, 4), ListedRow::Missing(1)),
        ((1, 2), ListedRow::Missing(0)),
    ];
    let by_title = Arranging {
        sort: SortOrder::Title,
        reading: Direction::Ascending,
    };

    assert_eq!(
        arranged(held, missing, by_title),
        vec![
            ListedRow::Held(0),
            ListedRow::Held(1),
            ListedRow::Missing(0),
            ListedRow::Missing(1),
        ]
    );
}

#[test]
fn an_album_on_one_disc_is_listed_without_a_heading_over_it() {
    let rows = placed(&[(1, 1, 0), (1, 2, 1), (1, 3, 2)]);

    assert_eq!(
        headed_by_disc(rows),
        vec![ListedRow::Held(0), ListedRow::Held(1), ListedRow::Held(2)]
    );
}

#[test]
fn a_set_of_two_discs_carries_a_heading_over_each_run_of_rows() {
    let rows = placed(&[(1, 1, 0), (1, 2, 1), (2, 1, 2), (2, 2, 3)]);

    assert_eq!(
        headed_by_disc(rows),
        vec![
            ListedRow::Disc(1),
            ListedRow::Held(0),
            ListedRow::Held(1),
            ListedRow::Disc(2),
            ListedRow::Held(2),
            ListedRow::Held(3),
        ]
    );
}

#[test]
fn a_disc_the_rows_skip_is_headed_by_the_number_the_rows_carry() {
    let rows = placed(&[(1, 1, 0), (3, 1, 1)]);

    assert_eq!(
        headed_by_disc(rows),
        vec![
            ListedRow::Disc(1),
            ListedRow::Held(0),
            ListedRow::Disc(3),
            ListedRow::Held(1),
        ]
    );
}

#[test]
fn a_missing_row_is_headed_the_way_a_held_one_is() {
    let rows = vec![
        ((1, 1), ListedRow::Held(0)),
        ((2, 1), ListedRow::Missing(0)),
    ];

    assert_eq!(
        headed_by_disc(rows),
        vec![
            ListedRow::Disc(1),
            ListedRow::Held(0),
            ListedRow::Disc(2),
            ListedRow::Missing(0),
        ]
    );
}

fn album(id: u64) -> AlbumId {
    AlbumId::new(id).expect("a non-zero id")
}

fn artist(id: u64) -> ArtistId {
    ArtistId::new(id).expect("a non-zero id")
}

#[test]
fn each_run_of_an_albums_missing_tracks_is_headed_by_the_album_once() {
    let rows = missing_track_rows(
        [
            (album(1), 1),
            (album(1), 1),
            (album(2), 1),
            (album(2), 1),
            (album(2), 1),
        ]
        .into_iter(),
    );

    assert_eq!(
        rows,
        vec![
            MissingRow::Album(0),
            MissingRow::Track(0),
            MissingRow::Track(1),
            MissingRow::Album(2),
            MissingRow::Track(2),
            MissingRow::Track(3),
            MissingRow::Track(4),
        ]
    );
}

#[test]
fn each_run_of_an_artists_unheld_releases_is_headed_by_the_artist_once() {
    let rows = unheld_release_rows([artist(3), artist(3), artist(5)].into_iter());

    assert_eq!(
        rows,
        vec![
            MissingRow::Artist(0),
            MissingRow::Release(0),
            MissingRow::Release(1),
            MissingRow::Artist(2),
            MissingRow::Release(2),
        ]
    );
}

#[test]
fn an_album_missing_rows_from_two_discs_is_headed_by_each_of_them() {
    let rows = missing_track_rows(
        [(album(1), 1), (album(1), 2), (album(1), 2), (album(2), 3)].into_iter(),
    );

    assert_eq!(
        rows,
        vec![
            MissingRow::Album(0),
            MissingRow::Disc(0),
            MissingRow::Track(0),
            MissingRow::Disc(1),
            MissingRow::Track(1),
            MissingRow::Track(2),
            MissingRow::Album(3),
            MissingRow::Track(3),
        ]
    );
}

#[test]
fn a_catalog_short_of_nothing_lists_no_row_and_no_heading() {
    assert!(missing_track_rows(std::iter::empty()).is_empty());
    assert!(unheld_release_rows(std::iter::empty()).is_empty());
}

#[test]
fn an_album_listed_twice_apart_is_headed_twice_because_the_rows_arrive_grouped() {
    let rows = missing_track_rows([(album(1), 1), (album(2), 1), (album(1), 1)].into_iter());

    assert_eq!(
        rows,
        vec![
            MissingRow::Album(0),
            MissingRow::Track(0),
            MissingRow::Album(1),
            MissingRow::Track(1),
            MissingRow::Album(2),
            MissingRow::Track(2),
        ]
    );
}

#[test]
fn the_sidebar_counts_every_favourite_of_all_three_kinds() {
    let held = Favourited {
        artists: 2,
        albums: 1,
        tracks: 9,
    };

    assert_eq!(held.held(), 12);
    assert_eq!(Favourited::default().held(), 0);
}

#[test]
fn a_favourites_pane_with_nothing_kept_in_it_is_empty() {
    assert!(Favourited::default().is_empty());
    assert!(
        !Favourited {
            artists: 0,
            albums: 0,
            tracks: 1,
        }
        .is_empty()
    );
}

#[test]
fn the_favourites_heading_counts_all_three_in_the_order_they_are_stacked() {
    let held = Favourited {
        artists: 2,
        albums: 1,
        tracks: 40,
    };

    assert_eq!(held.counted(), "2 artists · 1 album · 40 tracks");
}

#[test]
fn the_favourites_heading_leaves_out_what_there_is_none_of() {
    let held = Favourited {
        artists: 0,
        albums: 0,
        tracks: 3,
    };

    assert_eq!(held.counted(), "3 tracks");
    assert_eq!(Favourited::default().counted(), "");
}

#[test]
fn what_a_share_put_on_the_clipboard_is_named_by_its_artist_and_title() {
    let shared = Shared {
        title: "Echoes".to_owned(),
        artist: Some("Pink Floyd".to_owned()),
        ..Shared::default()
    };

    assert_eq!(
        on_the_clipboard(&shared),
        "Pink Floyd — Echoes is on the clipboard"
    );
}

#[test]
fn a_share_of_a_track_with_no_artist_is_named_by_its_title_alone() {
    let shared = Shared {
        title: "Track 07".to_owned(),
        ..Shared::default()
    };

    assert_eq!(on_the_clipboard(&shared), "Track 07 is on the clipboard");
}

fn linked(title: &str, recording: &str) -> Shared {
    Shared {
        title: title.to_owned(),
        recording: Some(Mbid::new(recording).expect("a well-formed mbid")),
        ..Shared::default()
    }
}

const ECHOES: &str = "6bcb2fc4-38b2-4b2c-b8ee-5d7dbf9a4a1b";
const MONEY: &str = "0d1d8d7e-72f6-4e72-9b0c-3a3b6c2f0e1a";

#[test]
fn several_shared_links_are_copied_one_to_a_line_and_counted() {
    let sharing = sharing_told(vec![
        Ok(Some(linked("Echoes", ECHOES))),
        Ok(Some(linked("Money", MONEY))),
    ]);

    assert_eq!(
        sharing.link.as_deref().map(|link| link.lines().count()),
        Some(2)
    );
    assert_eq!(
        sharing.notice,
        Notice::Done("2 links are on the clipboard".to_owned())
    );
}

#[test]
fn a_choice_where_one_track_has_no_link_says_how_many_were_copied() {
    let sharing = sharing_told(vec![
        Ok(Some(linked("Echoes", ECHOES))),
        Ok(None),
        Ok(Some(Shared::default())),
    ]);

    assert_eq!(
        sharing.link.as_deref().map(|link| link.lines().count()),
        Some(1)
    );
    assert_eq!(
        sharing.notice,
        Notice::Done("1 of 3 links are on the clipboard".to_owned())
    );
}

#[test]
fn a_choice_with_no_link_among_it_copies_nothing_and_says_so() {
    let sharing = sharing_told(vec![Ok(None), Ok(Some(Shared::default()))]);

    assert_eq!(sharing.link, None);
    assert_eq!(
        sharing.notice,
        Notice::Noted(words::models::no_link_to_share_any().to_owned())
    );
}

#[test]
fn a_file_dropped_in_the_inbox_after_the_last_poll_is_what_the_window_opens_to_ask_about() {
    let folder = std::env::temp_dir().join(format!("resonate-inbox-{}", std::process::id()));
    std::fs::create_dir_all(folder.join("nested")).expect("a writable temporary folder");
    let before = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    let after = std::time::SystemTime::now() + std::time::Duration::from_secs(60);

    assert!(
        !landed_since(&folder, before),
        "a folder holding only a folder"
    );
    std::fs::write(folder.join("an-mbid.flac"), b"audio").expect("a file dropped in");

    assert!(landed_since(&folder, before));
    assert!(!landed_since(&folder, after));

    let kept_its_time = folder.join("an-isrc.flac");
    std::fs::write(&kept_its_time, b"audio").expect("a file copied in");
    std::fs::File::options()
        .write(true)
        .open(&kept_its_time)
        .and_then(|file| file.set_modified(before - std::time::Duration::from_secs(3_600)))
        .expect("the copy keeps the time it had elsewhere");
    std::fs::remove_file(folder.join("an-mbid.flac")).expect("the first file goes");
    assert!(
        landed_since(&folder, before),
        "a file copied in with an old modification time was not seen landing"
    );
    assert!(!landed_since(&folder.join("gone"), before));
    std::fs::remove_dir_all(&folder).expect("the temporary folder goes away");
}

#[test]
fn a_preview_stands_until_what_it_read_moves_and_an_apply_leaves_none() {
    let library = Library::open_in_memory().expect("an in-memory catalog");
    let read_at = library.plans_stamp();

    assert_eq!(Planned::after(Pass::Apply, false, read_at), Planned::Not);
    assert_eq!(Planned::after(Pass::Preview, true, read_at), Planned::Not);
    let shown = Planned::after(Pass::Preview, false, read_at);
    assert!(shown.is_shown());
    assert!(!shown.outdated_at(library.plans_stamp()));

    library
        .add_root(&std::env::temp_dir())
        .expect("the catalog takes a root");
    assert!(
        shown.outdated_at(library.plans_stamp()),
        "a root added under a preview left it standing"
    );
    assert!(!Planned::Outdated.outdated_at(library.plans_stamp()));
    assert!(!Planned::Not.outdated_at(library.plans_stamp()));
}

#[test]
fn a_lookup_begun_on_its_own_tells_nothing_unless_the_reference_stopped_it() {
    let answered = resonate_library::EnrichSummary {
        stats: resonate_library::EnrichStats {
            releases: 3,
            artists: 3,
            ..resonate_library::EnrichStats::default()
        },
        cancelled: false,
        stopped_by: None,
    };
    let unreached = resonate_library::EnrichSummary {
        stopped_by: Some(resonate_library::LookupOp::Release),
        ..answered
    };

    assert!(super::told::looked_up(&answered, super::Prompted::OnItsOwn).is_none());
    assert!(matches!(
        super::told::looked_up(&answered, super::Prompted::ByHand),
        Some(Notice::Noted(_))
    ));
    assert!(matches!(
        super::told::looked_up(&unreached, super::Prompted::OnItsOwn),
        Some(Notice::Trouble(_))
    ));
}
