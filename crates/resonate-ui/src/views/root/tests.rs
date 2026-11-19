use gpui::px;
use resonate_core::{AlbumId, ArtistId};

use super::{
    Following, Landing, LeftAt, Pane, PlayingRow, QueueStamp, Step, TrackId, UnderThePointer,
    in_front_of, landing, stepped_pane, unsaved,
};
use crate::{Selection, Tabs, models::Notice};

const A_SERVICE: bool = true;
const NO_SERVICE: bool = false;

const EVERY_TAB: Tabs = Tabs {
    suggestions: true,
    missing: true,
    counts: true,
};

const NO_PLAYLIST_OPEN: bool = false;

const A_PLAYLIST_OPEN: bool = true;

fn an_album() -> Selection {
    Selection::Album(AlbumId::new(3).expect("a non-zero id"))
}

fn an_artist() -> Selection {
    Selection::Artist(ArtistId::new(5).expect("a non-zero id"))
}

#[test]
fn a_settings_write_whose_writer_died_is_told_as_one_that_failed_is() {
    assert!(unsaved(Ok(Ok(()))).is_none());
    assert!(matches!(
        unsaved(Err(futures_channel::oneshot::Canceled)),
        Some(Notice::Trouble(_))
    ));
}

#[test]
fn a_hint_is_dropped_only_where_what_is_under_a_still_pointer_moved() {
    let at = gpui::point(px(40.0), px(80.0));
    let before = UnderThePointer { at, laid_out: 1 };
    let moved = UnderThePointer { at, laid_out: 2 };
    let pointer_moved = UnderThePointer {
        at: gpui::point(px(41.0), px(80.0)),
        laid_out: 2,
    };

    assert!(moved.moved_since(Some(before)));
    assert!(!before.moved_since(Some(before)));
    assert!(!pointer_moved.moved_since(Some(before)));
    assert!(!before.moved_since(None));
}

#[test]
fn a_list_whose_rows_grew_lands_on_the_row_it_was_left_at_and_as_far_into_it() {
    let left_at = LeftAt::read_off(px(-1_037.0), 36.0);
    assert_eq!(left_at.row, 28);
    assert!((left_at.into_the_row - 29.0 / 36.0).abs() < 1e-5);

    assert_eq!(left_at.offset_at(36.0), px(-1_037.0));
    let grown = f32::from(left_at.offset_at(40.0));
    assert!((grown - -(40.0 * 28.0 + 29.0 / 36.0 * 40.0)).abs() < 1e-3);
}

#[test]
fn an_album_or_an_artist_opened_stands_under_its_own_category_in_the_sidebar() {
    assert_eq!(in_front_of(Pane::Tracks, an_album()), Pane::Albums);
    assert_eq!(in_front_of(Pane::Tracks, an_artist()), Pane::Artists);
    assert_eq!(
        in_front_of(Pane::Tracks, Selection::Everything),
        Pane::Tracks
    );
    assert_eq!(in_front_of(Pane::Lyrics, an_album()), Pane::Lyrics);
}

#[test]
fn pressing_the_category_a_page_stands_under_goes_back_to_the_whole_category() {
    let album_page = in_front_of(Pane::Tracks, an_album());
    let artist_page = in_front_of(Pane::Tracks, an_artist());

    assert_eq!(
        landing(Pane::Albums, album_page, an_album(), NO_PLAYLIST_OPEN),
        Landing::Unscoped(Pane::Albums)
    );
    assert_eq!(
        landing(Pane::Artists, artist_page, an_artist(), NO_PLAYLIST_OPEN),
        Landing::Unscoped(Pane::Artists)
    );
    assert_eq!(
        landing(
            Pane::Playlists,
            Pane::Playlists,
            Selection::Everything,
            A_PLAYLIST_OPEN
        ),
        Landing::EveryPlaylist
    );
}

#[test]
fn coming_back_to_a_category_from_elsewhere_finds_the_page_it_was_left_on() {
    assert_eq!(
        landing(Pane::Albums, Pane::Lyrics, an_album(), NO_PLAYLIST_OPEN),
        Landing::Scoped
    );
    assert_eq!(
        landing(
            Pane::Playlists,
            Pane::Lyrics,
            Selection::Everything,
            A_PLAYLIST_OPEN
        ),
        Landing::On(Pane::Playlists)
    );
    assert_eq!(
        landing(Pane::Artists, Pane::Albums, an_album(), NO_PLAYLIST_OPEN),
        Landing::On(Pane::Artists)
    );
}

#[test]
fn the_tracks_row_always_lists_every_track() {
    let album_page = in_front_of(Pane::Tracks, an_album());

    assert_eq!(
        landing(Pane::Tracks, album_page, an_album(), NO_PLAYLIST_OPEN),
        Landing::Unscoped(Pane::Tracks)
    );
    assert_eq!(
        landing(
            Pane::Tracks,
            Pane::Tracks,
            Selection::Everything,
            NO_PLAYLIST_OPEN
        ),
        Landing::On(Pane::Tracks)
    );
}

const ON_SCREEN: bool = true;

const BEHIND_ANOTHER_PANE: bool = false;

fn track(id: u64) -> Option<TrackId> {
    TrackId::new(id).ok()
}

fn queue_of(rows: &[u64]) -> QueueStamp {
    QueueStamp::of(rows)
}

fn playing(row: usize, id: u64, queue: QueueStamp) -> Option<PlayingRow> {
    Some(PlayingRow {
        row,
        track: track(id),
        queue,
    })
}

#[test]
fn the_queue_is_scrolled_to_the_row_that_started_playing() {
    let queue = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let mut following = Following::default();

    assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
    assert_eq!(following.follows(playing(8, 9, queue), ON_SCREEN), Some(8));
}

#[test]
fn a_redraw_that_leaves_the_playing_row_where_it_was_scrolls_nothing() {
    let queue = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let mut following = Following::default();

    assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
    assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), None);
    assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), None);
}

#[test]
fn rows_taken_out_above_the_playing_one_move_it_without_scrolling() {
    let before = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let after = queue_of(&[1, 4, 5, 6, 7, 8]);
    let mut following = Following::default();

    assert_eq!(following.follows(playing(7, 8, before), ON_SCREEN), Some(7));
    assert_eq!(following.follows(playing(5, 8, after), ON_SCREEN), None);
    assert_eq!(following.follows(playing(5, 8, after), ON_SCREEN), None);
}

#[test]
fn a_step_onto_the_same_track_queued_twice_still_scrolls_to_it() {
    let queue = queue_of(&[1, 2, 3]);
    let mut following = Following::default();

    assert_eq!(following.follows(playing(0, 4, queue), ON_SCREEN), Some(0));
    assert_eq!(following.follows(playing(2, 4, queue), ON_SCREEN), Some(2));
}

#[test]
fn another_track_starting_as_the_queue_changes_is_scrolled_to() {
    let before = queue_of(&[1, 2, 3]);
    let after = queue_of(&[1, 3]);
    let mut following = Following::default();

    assert_eq!(following.follows(playing(1, 2, before), ON_SCREEN), Some(1));
    assert_eq!(following.follows(playing(1, 3, after), ON_SCREEN), Some(1));
}

#[test]
fn a_queue_nobody_is_looking_at_is_not_scrolled_until_it_is_drawn_again() {
    let queue = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let mut following = Following::default();

    assert_eq!(
        following.follows(playing(7, 8, queue), BEHIND_ANOTHER_PANE),
        None
    );
    assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
}

#[test]
fn a_queue_that_has_stopped_is_followed_nowhere_and_taken_up_afresh() {
    let queue = queue_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let mut following = Following::default();

    assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
    assert_eq!(following.follows(None, ON_SCREEN), None);
    assert_eq!(following.follows(playing(7, 8, queue), ON_SCREEN), Some(7));
}

#[test]
fn the_pane_keys_walk_the_sidebar_and_wrap_at_either_end() {
    let listed = Pane::BROWSE;
    let (first, last) = (listed[0], listed[listed.len() - 1]);

    assert_eq!(
        stepped_pane(first, Step::Below, EVERY_TAB, A_SERVICE),
        listed[1]
    );
    assert_eq!(
        stepped_pane(last, Step::Below, EVERY_TAB, A_SERVICE),
        first,
        "the end did not wrap"
    );
    assert_eq!(
        stepped_pane(first, Step::Above, EVERY_TAB, A_SERVICE),
        last,
        "the start did not wrap"
    );

    let walked = listed.iter().fold(first, |pane, _| {
        stepped_pane(pane, Step::Below, EVERY_TAB, A_SERVICE)
    });
    assert_eq!(
        walked, first,
        "walking the whole sidebar did not come back to where it started"
    );
}

#[test]
fn a_pane_the_sidebar_does_not_list_steps_onto_the_first_one_it_does() {
    assert_eq!(
        stepped_pane(Pane::Queue, Step::Below, EVERY_TAB, A_SERVICE),
        Pane::BROWSE[1]
    );
    assert_eq!(
        stepped_pane(Pane::Settings, Step::Below, EVERY_TAB, A_SERVICE),
        Pane::BROWSE[1]
    );
}

#[test]
fn the_pane_keys_step_onto_the_services_row_last_in_the_collection() {
    assert_eq!(
        stepped_pane(Pane::Missing, Step::Below, EVERY_TAB, A_SERVICE),
        Pane::Server
    );
    assert_eq!(
        stepped_pane(Pane::Server, Step::Below, EVERY_TAB, A_SERVICE),
        Pane::Lyrics
    );
    assert_eq!(
        stepped_pane(Pane::Lyrics, Step::Above, EVERY_TAB, A_SERVICE),
        Pane::Server
    );
}

#[test]
fn the_pane_keys_pass_over_the_services_row_where_no_service_is_set() {
    assert_eq!(
        stepped_pane(Pane::Missing, Step::Below, EVERY_TAB, NO_SERVICE),
        Pane::Lyrics
    );
    assert_eq!(
        stepped_pane(Pane::Lyrics, Step::Above, EVERY_TAB, NO_SERVICE),
        Pane::Missing
    );
}

#[test]
fn every_pane_is_listed_once_and_read_back_from_the_name_it_is_stored_under() {
    for pane in Pane::ALL {
        assert_eq!(Pane::parse(pane.as_str()), Some(pane));
        assert_eq!(Pane::ALL.iter().filter(|other| **other == pane).count(), 1);
    }
    for pane in Pane::BROWSE
        .into_iter()
        .chain([Pane::Queue, Pane::Settings])
    {
        assert!(Pane::ALL.contains(&pane));
    }
    assert_eq!(Pane::ALL.len(), Pane::BROWSE.len() + 2);
}

#[test]
fn the_services_pane_is_named_for_the_listener_and_stored_under_the_key_it_always_was() {
    assert_eq!(Pane::Server.label(), "Services");
    assert_eq!(Pane::Server.as_str(), "server");
    assert_eq!(Pane::parse("server"), Some(Pane::Server));
}

#[test]
fn the_pane_keys_step_over_a_tab_that_is_hidden() {
    let hidden = Tabs {
        suggestions: false,
        missing: false,
        counts: true,
    };

    assert_eq!(
        stepped_pane(Pane::Playlists, Step::Below, hidden, NO_SERVICE),
        Pane::Party
    );
    assert_eq!(
        stepped_pane(Pane::Party, Step::Below, hidden, NO_SERVICE),
        Pane::Lyrics
    );
    assert_eq!(
        stepped_pane(Pane::Lyrics, Step::Above, hidden, NO_SERVICE),
        Pane::Party
    );
    assert_eq!(
        stepped_pane(Pane::Playlists, Step::Below, EVERY_TAB, NO_SERVICE),
        Pane::Suggestions
    );
}

#[test]
fn the_missing_tab_is_hidden_until_it_is_asked_for() {
    assert!(Pane::Suggestions.is_shown(Tabs::AS_BUILT));
    assert!(!Pane::Missing.is_shown(Tabs::AS_BUILT));
    assert!(Pane::Missing.is_shown(EVERY_TAB));
}

mod driven {
    use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc, time::Duration};

    use gpui::{OperatingSystem, PlatformInfo, PlatformTarget, TestAppContext};
    use resonate_core::{Span, Volume};
    use resonate_library::{Cut, Direction, Library, PlaylistOrder};

    use crate::{
        Followed, ListOffered, Notice, ResonateApp, SettingsCategory,
        app::About,
        driven::{Driven, Folder},
        toast,
        views::{
            menu::Menu,
            part::Region,
            party_offer,
            playlists::Held,
            reorder::{Reach, Shift},
            root::{Deleting, Magnified, Pane, RootView},
            sharing::Seen,
        },
    };

    fn catalog() -> Arc<Library> {
        Arc::new(Library::open_in_memory().expect("a catalog in memory"))
    }

    fn queued(folder: &Folder, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| folder.tone(&format!("{name}.wav"), 1))
            .collect()
    }

    fn queue_open(cx: &mut TestAppContext, folder: &Folder, names: &[&str]) -> Driven {
        let files = queued(folder, names);
        let mut driven = Driven::opened_in(cx, catalog(), folder);
        driven.play(&files);
        driven.click("queue");
        driven.focus_the_window();
        driven
    }

    fn reach(driven: &mut Driven) -> Option<Reach> {
        driven.read(|root, cx| root.reach(cx).cloned())
    }

    impl Driven {
        fn focus_the_window(&mut self) {
            let root = self.root.clone();
            self.cx.update(|window, cx| {
                let focus = root.read(cx).focus.clone();
                window.focus(&focus, cx);
            });
            self.settle();
        }

        fn lapse_the_type_ahead(&mut self) {
            let root = self.root.clone();
            self.cx.update(|_, cx| {
                root.update(cx, |root, cx| root.stop_typing(cx));
            });
            self.settle();
            assert!(self.read(|root, _| root.typing.ahead.typed().is_none()));
        }
    }

    #[gpui::test]
    fn deleting_the_last_rows_leaves_the_reach_on_the_row_now_last_and_delete_takes_that(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three", "four"]);
        let before = driven.queue();

        driven.cx.simulate_keystrokes("end shift-up delete");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(reach(&mut driven), Some(Reach::at(Shift::Queue, 1)));

        driven.cx.simulate_keystrokes("delete");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

        assert_eq!(driven.queue(), [before[0]]);
        assert_eq!(reach(&mut driven), Some(Reach::at(Shift::Queue, 0)));

        driven.cx.simulate_keystrokes("delete");
        driven.until(|root, cx| root.player.read(cx).queue().is_empty());

        assert_eq!(reach(&mut driven), None);
    }

    fn edited_behind_the_window(driven: &mut Driven, command: resonate_engine::Command) {
        let before = driven.read(|root, cx| root.player.read(cx).queued().revision);
        driven.read(|root, cx| {
            root.player
                .read(cx)
                .engine()
                .send(command)
                .expect("the engine takes a command");
        });
        driven.until(|root, cx| !root.player.read(cx).is_queued_at(before));
    }

    fn reached_song(driven: &mut Driven) -> Option<resonate_core::TrackId> {
        driven.read(|root, cx| {
            let row = root.reach(cx)?.row;
            root.player.read(cx).queue().get(row).map(|item| item.id)
        })
    }

    #[gpui::test]
    fn a_queue_row_reached_before_a_sort_landed_from_elsewhere_names_the_same_song_delete_takes(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three", "four"]);
        let before = driven.queue();

        driven.cx.simulate_keystrokes("end up");
        driven.until(|root, cx| root.reach(cx).cloned() == Some(Reach::at(Shift::Queue, 2)));
        edited_behind_the_window(
            &mut driven,
            resonate_engine::Command::Order(vec![2, 0, 3, 1]),
        );

        assert_eq!(reach(&mut driven), Some(Reach::at(Shift::Queue, 0)));
        assert_eq!(reached_song(&mut driven), Some(before[2]));

        driven.cx.simulate_keystrokes("delete");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 3);

        assert_eq!(driven.queue(), [before[0], before[3], before[1]]);
    }

    #[gpui::test]
    fn queue_rows_reached_before_a_row_landed_above_them_still_name_their_songs(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three", "four"]);
        let before = driven.queue();
        let landing = driven.read(|root, cx| {
            let queued = root.player.read(cx).queue();
            let mut landing = resonate_engine::QueueItem::clone(&queued.to_vec()[3]);
            landing.id = resonate_engine::unclaimed_id(queued.iter().map(AsRef::as_ref));
            landing
        });

        driven.cx.simulate_keystrokes("end up shift-up");
        driven
            .until(|root, cx| root.reach(cx).cloned() == Some(Reach::spanning(Shift::Queue, 2, 1)));
        edited_behind_the_window(
            &mut driven,
            resonate_engine::Command::Insert {
                items: vec![landing],
                at: resonate_engine::Placement::At(0),
                play: false,
            },
        );

        assert_eq!(
            reach(&mut driven),
            Some(Reach::spanning(Shift::Queue, 3, 2))
        );
        assert_eq!(reached_song(&mut driven), Some(before[1]));

        driven.cx.simulate_keystrokes("delete");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 3);

        assert_eq!(driven.queue()[1..], [before[0], before[3]]);
    }

    #[gpui::test]
    fn a_queue_row_raised_twice_by_its_keys_stays_reached_as_it_climbs(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three", "four"]);
        let before = driven.queue();

        driven.cx.simulate_keystrokes("end");
        driven.until(|root, cx| root.reach(cx).cloned() == Some(Reach::at(Shift::Queue, 3)));
        driven.cx.simulate_keystrokes("alt-up");
        driven.until(|root, cx| {
            root.player.read(cx).queue().get(2).map(|item| item.id) == Some(before[3])
        });
        driven.cx.simulate_keystrokes("alt-up");
        driven.until(|root, cx| {
            root.player.read(cx).queue().get(1).map(|item| item.id) == Some(before[3])
        });

        assert_eq!(driven.queue(), [before[0], before[3], before[1], before[2]]);
        assert_eq!(reach(&mut driven), Some(Reach::at(Shift::Queue, 1)));
        assert_eq!(reached_song(&mut driven), Some(before[3]));
    }

    #[gpui::test]
    fn backspace_after_a_jump_has_lapsed_takes_nothing_out_of_the_queue(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["alpha", "bravo", "charlie"]);

        driven.cx.simulate_keystrokes("c->c");
        driven.until(|root, cx| root.reach(cx).cloned() == Some(Reach::at(Shift::Queue, 2)));
        driven.lapse_the_type_ahead();

        driven.cx.simulate_keystrokes("backspace");
        driven.settle();

        assert_eq!(driven.queue().len(), 3);

        driven.cx.simulate_keystrokes("delete");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);
    }

    #[gpui::test]
    fn a_row_reached_by_hand_after_a_jump_is_taken_out_by_backspace(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["alpha", "bravo", "charlie"]);

        driven.cx.simulate_keystrokes("c->c");
        driven.until(|root, cx| root.reach(cx).cloned() == Some(Reach::at(Shift::Queue, 2)));
        driven.lapse_the_type_ahead();

        driven.cx.simulate_keystrokes("up backspace");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);
    }

    fn nothing_behind_the_sheet_answers(driven: &mut Driven, rows: usize) {
        nothing_behind_the_sheet_answers_to(
            driven,
            rows,
            "delete enter backspace alt-up down secondary-z",
        );
    }

    fn nothing_behind_the_sheet_answers_to(driven: &mut Driven, rows: usize, keys: &str) {
        let queue = driven.queue();

        driven.cx.simulate_keystrokes(keys);
        driven.cx.simulate_keystrokes("t->t");
        driven.settle();

        assert_eq!(driven.queue(), queue);
        assert_eq!(queue.len(), rows);
        assert_eq!(reach(driven), Some(Reach::at(Shift::Queue, 1)));
        assert!(driven.read(|root, cx| {
            root.typing.ahead.typed().is_none() && root.typed.search.read(cx).text().is_empty()
        }));
    }

    #[gpui::test]
    fn the_playlist_picker_holds_the_keys_back_from_the_queue_behind(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
        driven.cx.simulate_keystrokes("down down");
        let root = driven.root.clone();

        driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                let rows: Arc<[Cut]> = Arc::from(Vec::new());
                root.adding = Some(Held::of(rows));
                window.focus(&root.focus, cx);
                cx.notify();
            });
        });
        driven.settle();
        nothing_behind_the_sheet_answers(&mut driven, 3);

        driven.cx.simulate_keystrokes("escape");
        assert!(driven.read(|root, _| root.adding.is_none()));
    }

    #[gpui::test]
    fn the_magnified_cover_holds_the_keys_back_from_the_queue_behind(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
        driven.cx.simulate_keystrokes("down down");
        let location = driven.read(|root, cx| root.player.read(cx).queue()[0].location.clone());
        let root = driven.root.clone();

        driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                root.magnify(Magnified::File(location), cx);
                window.focus(&root.focus, cx);
            });
        });
        driven.settle();
        nothing_behind_the_sheet_answers(&mut driven, 3);
    }

    #[gpui::test]
    fn the_listen_sheet_holds_the_keys_back_from_the_queue_behind(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
        driven.cx.simulate_keystrokes("down down");
        let root = driven.root.clone();

        driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                root.listening_open = true;
                window.focus(&root.focus, cx);
                cx.notify();
            });
        });
        driven.settle();
        nothing_behind_the_sheet_answers_to(
            &mut driven,
            3,
            "delete backspace alt-up down secondary-z",
        );
    }

    #[gpui::test]
    fn the_delete_sheet_holds_the_keys_back_from_the_queue_behind(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
        driven.cx.simulate_keystrokes("down down");
        let track = driven.queue()[0];
        let root = driven.root.clone();

        driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                root.deleting = Some(Deleting {
                    tracks: vec![track],
                    title: "one".to_owned(),
                    artist: None,
                    a_cut: false,
                    everywhere: false,
                });
                window.focus(&root.focus, cx);
                cx.notify();
            });
        });
        driven.settle();
        nothing_behind_the_sheet_answers_to(
            &mut driven,
            3,
            "delete backspace alt-up down secondary-z",
        );

        driven.cx.simulate_keystrokes("escape");
        assert!(driven.read(|root, _| root.deleting.is_none()));
    }

    fn asking_to_delete(driven: &mut Driven) {
        let track = driven.queue()[0];
        let root = driven.root.clone();

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.deleting = Some(Deleting {
                    tracks: vec![track],
                    title: "one".to_owned(),
                    artist: None,
                    a_cut: false,
                    everywhere: false,
                });
                cx.notify();
            });
        });
        driven.settle();
        driven.settle();
    }

    fn focused_is(driven: &mut Driven, named: &'static str) -> bool {
        let root = driven.root.clone();
        driven.cx.update(|window, cx| {
            root.read(cx)
                .controls
                .handle(named)
                .is_some_and(|handle| handle.is_focused(window))
        })
    }

    #[gpui::test]
    fn focus_enters_the_delete_sheet_on_the_control_that_keeps_the_song(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two"]);

        asking_to_delete(&mut driven);

        assert!(focused_is(&mut driven, "keep-the-track"));
        assert!(!focused_is(&mut driven, "delete-the-track"));

        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert!(driven.read(|root, _| root.deleting.is_none()));
        assert_eq!(
            driven.queue().len(),
            2,
            "no key confirms a deletion, enter included"
        );
    }

    #[gpui::test]
    fn tab_stays_inside_the_delete_sheet_in_both_directions(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two"]);
        asking_to_delete(&mut driven);

        for keys in [
            "tab",
            "tab",
            "tab",
            "tab",
            "shift-tab",
            "shift-tab",
            "shift-tab",
        ] {
            driven.cx.simulate_keystrokes(keys);
            driven.settle();

            let inside = driven.read(|root, _| root.front_panel().is_some());
            let root = driven.root.clone();
            let within = driven.cx.update(|window, cx| {
                root.read(cx)
                    .front_panel()
                    .is_some_and(|panel| panel.contains_focused(window, cx))
            });

            assert!(inside, "the sheet has a panel to keep the focus in");
            assert!(within, "{keys} took the focus out of the delete sheet");
        }
    }

    #[gpui::test]
    fn escape_gives_the_focus_back_to_the_control_the_sheet_was_opened_from(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two"]);
        let root = driven.root.clone();
        let opener = driven.cx.update(|window, cx| {
            let opener = root
                .read(cx)
                .standing_controls
                .handle(Pane::Albums.label())
                .expect("the Albums tab is in the ring");
            window.focus(&opener, cx);
            opener
        });
        driven.settle();

        asking_to_delete(&mut driven);
        assert!(focused_is(&mut driven, "keep-the-track"));

        driven.cx.simulate_keystrokes("escape");
        driven.settle();
        driven.settle();

        assert!(driven.read(|root, _| root.deleting.is_none()));
        assert!(driven.cx.update(|window, _| opener.is_focused(window)));
    }

    const A_PARTY_LINK: &str = "resonate://party/ABCDE";

    fn offering_a_party(driven: &mut Driven) {
        let root = driven.root.clone();

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.follow(Followed::PartyOffered(A_PARTY_LINK.to_owned()), cx);
            });
        });
        driven.settle();
        driven.settle();
    }

    fn together_over(driven: &mut Driven, together: Arc<dyn crate::Together>) {
        driven.cx.update(|_, cx| {
            cx.global_mut::<ResonateApp>().together = Some(together);
        });
    }

    #[gpui::test]
    fn what_is_typed_into_the_party_chat_is_said_and_a_reaction_pressed_is_sent(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        *joining.at.lock() = Some(crate::together::PartySeen {
            zone: resonate_core::ZoneId::FIRST,
            hosting: false,
            link: "resonate://party/ABCDE".to_owned(),
            reach: crate::together::PartyReached::Here,
            people: Vec::new(),
            standing: crate::together::PartyStanding::InStep,
            song: None,
            asked_for: Vec::new(),
            round_trip: None,
            apart: None,
            chat: vec![crate::together::PartyLineSeen {
                id: 7,
                name: "Ada".to_owned(),
                me: false,
                saying: crate::together::PartySaying::Words("Lovely".to_owned()),
            }],
            handing: None,
        });
        together_over(&mut driven, joining.clone());

        driven
            .root
            .update(&mut driven.cx, |root, cx| root.set_pane(Pane::Party, cx));
        driven.settle();
        driven.focus_on(|root, cx| root.party.read(cx).chat.clone());
        driven.cx.simulate_input("hello there");
        driven.cx.simulate_keystrokes("enter");
        driven.settle();
        driven.click("party-react-1");
        driven.settle();

        assert_eq!(*joining.said.lock(), vec!["hello there".to_owned()]);
        assert_eq!(
            *joining.reacted.lock(),
            vec![crate::together::PartyReaction::Fire]
        );
        assert!(driven.read(|root, cx| root.party.read(cx).chat.read(cx).text().is_empty()));
    }

    fn a_party_seated_on(zone: resonate_core::ZoneId) -> crate::together::PartySeen {
        crate::together::PartySeen {
            zone,
            hosting: false,
            link: A_PARTY_LINK.to_owned(),
            reach: crate::together::PartyReached::Here,
            people: Vec::new(),
            standing: crate::together::PartyStanding::InStep,
            song: None,
            asked_for: Vec::new(),
            round_trip: None,
            apart: None,
            chat: Vec::new(),
            handing: None,
        }
    }

    #[gpui::test]
    fn a_zone_taken_away_from_under_a_party_leaves_it_and_one_beside_it_does_not(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one"]);
        let root = driven.root.clone();
        let joining = Arc::new(party_offer::joined::Joining::default());
        let second = resonate_core::ZoneId::new(2).expect("a second zone");
        let third = resonate_core::ZoneId::new(3).expect("a third zone");
        together_over(&mut driven, joining.clone());

        for _ in 0..2 {
            driven
                .cx
                .update(|_, cx| root.update(cx, |root, cx| root.add_a_zone(cx)));
        }
        driven.until(|root, _| root.zones.len() == 3);
        *joining.at.lock() = Some(a_party_seated_on(third));
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.remove_the_zone(second, cx)));
        driven.until(|_, cx| cx.global::<ResonateApp>().zones.count() == 2);

        assert_eq!(*joining.left.lock(), 0);

        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.remove_the_zone(third, cx)));
        driven.until(|_, cx| cx.global::<ResonateApp>().zones.count() == 1);

        assert_eq!(*joining.left.lock(), 1);
        assert!(joining.at.lock().is_none());
    }

    #[gpui::test]
    fn the_host_hands_the_party_to_someone_only_on_a_second_press(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        let ada = [3; 16];
        *joining.at.lock() = Some(crate::together::PartySeen {
            zone: resonate_core::ZoneId::FIRST,
            hosting: true,
            link: "resonate://party/ABCDE".to_owned(),
            reach: crate::together::PartyReached::Here,
            people: vec![
                crate::together::SharedPerson {
                    id: [1; 16],
                    name: "Host".to_owned(),
                    me: true,
                },
                crate::together::SharedPerson {
                    id: ada,
                    name: "Ada".to_owned(),
                    me: false,
                },
            ],
            standing: crate::together::PartyStanding::InStep,
            song: None,
            asked_for: Vec::new(),
            round_trip: None,
            apart: None,
            chat: Vec::new(),
            handing: None,
        });
        together_over(&mut driven, joining.clone());

        driven
            .root
            .update(&mut driven.cx, |root, cx| root.set_pane(Pane::Party, cx));
        driven.settle();
        driven.click("party-hand-over-1");
        driven.settle();

        assert!(joining.handed.lock().is_empty(), "one press only asks");
        assert_eq!(
            driven.read(|root, cx| root.party.read(cx).handing_armed),
            Some(ada)
        );

        driven.click("party-hand-over-1");
        driven.settle();

        assert_eq!(*joining.handed.lock(), vec![ada]);
        assert_eq!(
            driven.read(|root, cx| root.party.read(cx).handing_armed),
            None
        );
    }

    fn hosting_with_requests(
        asked_for: Vec<crate::together::PartyRequestSeen>,
    ) -> crate::together::PartySeen {
        crate::together::PartySeen {
            zone: resonate_core::ZoneId::FIRST,
            hosting: true,
            link: "resonate://party/ABCDE".to_owned(),
            reach: crate::together::PartyReached::Here,
            people: Vec::new(),
            standing: crate::together::PartyStanding::InStep,
            song: None,
            asked_for,
            round_trip: None,
            apart: None,
            chat: Vec::new(),
            handing: None,
        }
    }

    fn requested(id: u8, title: &str) -> crate::together::PartyRequestSeen {
        crate::together::PartyRequestSeen {
            id: crate::together::PartyRequestId([id; 32]),
            title: title.to_owned(),
            artist: "Artist".to_owned(),
        }
    }

    #[gpui::test]
    fn a_request_pressed_is_named_by_its_id_and_not_by_the_row_it_was_drawn_at(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        *joining.at.lock() = Some(hosting_with_requests(vec![
            requested(1, "Echoes"),
            requested(2, "Meddle"),
            requested(3, "Time"),
        ]));
        together_over(&mut driven, joining.clone());

        driven
            .root
            .update(&mut driven.cx, |root, cx| root.set_pane(Pane::Party, cx));
        driven.settle();
        driven.click("party-play-next-1");
        driven.settle();
        driven.click("party-dismiss-2");
        driven.settle();

        assert_eq!(
            *joining.played_next.lock(),
            vec![crate::together::PartyRequestId([2; 32])]
        );
        assert_eq!(
            *joining.dismissed.lock(),
            vec![crate::together::PartyRequestId([3; 32])]
        );
    }

    #[gpui::test]
    fn a_party_link_from_outside_asks_first_and_focus_lands_on_the_control_that_declines(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        together_over(&mut driven, joining.clone());

        offering_a_party(&mut driven);

        assert!(driven.read(|root, _| root.social.party_offered.is_some()));
        assert!(focused_is(&mut driven, "not-joining-the-party"));
        assert!(!focused_is(&mut driven, "join-the-party"));
        assert!(joining.joined.lock().is_empty(), "nothing joins unasked");

        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert!(driven.read(|root, _| root.social.party_offered.is_none()));
        assert!(
            joining.joined.lock().is_empty(),
            "no key confirms a join, enter included"
        );
    }

    #[gpui::test]
    fn escape_declines_a_party_link_and_the_sheet_holds_the_keys_back(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        together_over(&mut driven, joining.clone());

        driven.cx.simulate_keystrokes("down down");
        offering_a_party(&mut driven);
        driven.focus_the_window();
        nothing_behind_the_sheet_answers_to(&mut driven, 3, "backspace alt-up down secondary-z");

        driven.cx.simulate_keystrokes("escape");
        driven.settle();

        assert!(driven.read(|root, _| root.social.party_offered.is_none()));
        assert!(joining.joined.lock().is_empty());
    }

    #[gpui::test]
    fn joining_an_offered_party_hands_the_link_and_the_zone_shown_to_the_seam(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        together_over(&mut driven, joining.clone());
        let zone = driven.read(|root, _| root.switcher.selected());

        offering_a_party(&mut driven);
        driven.click("join-the-party");
        driven.settle();
        driven.settle();

        assert!(driven.read(|root, _| root.social.party_offered.is_none()));
        assert_eq!(
            *joining.joined.lock(),
            vec![(A_PARTY_LINK.to_owned(), zone)]
        );
        assert_eq!(driven.read(|root, _| root.pane), Pane::Party);
    }

    const A_LIST_LINK: &str = "resonate://shared/E-ABCDE?server=wss%3A%2F%2Fstranger.example%2F";

    fn offering_a_list(driven: &mut Driven) {
        let root = driven.root.clone();

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.follow(
                    Followed::ListOffered(ListOffered {
                        link: A_LIST_LINK.to_owned(),
                        server: "wss://stranger.example/".to_owned(),
                    }),
                    cx,
                );
            });
        });
        driven.settle();
        driven.settle();
    }

    #[gpui::test]
    fn a_list_on_a_stranger_server_from_outside_asks_first_and_focus_lands_on_the_control_that_declines(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        together_over(&mut driven, joining.clone());

        offering_a_list(&mut driven);

        assert!(driven.read(|root, _| root.social.list_offered.is_some()));
        assert!(focused_is(&mut driven, "not-joining-the-list"));
        assert!(!focused_is(&mut driven, "join-the-list"));
        assert!(joining.accepted.lock().is_empty(), "nothing joins unasked");

        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert!(driven.read(|root, _| root.social.list_offered.is_none()));
        assert!(
            joining.accepted.lock().is_empty(),
            "no key confirms a join, enter included"
        );
    }

    #[gpui::test]
    fn escape_declines_a_list_on_another_server_and_the_sheet_holds_the_keys_back(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two", "three"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        together_over(&mut driven, joining.clone());

        driven.cx.simulate_keystrokes("down down");
        offering_a_list(&mut driven);
        driven.focus_the_window();
        nothing_behind_the_sheet_answers_to(&mut driven, 3, "backspace alt-up down secondary-z");

        driven.cx.simulate_keystrokes("escape");
        driven.settle();

        assert!(driven.read(|root, _| root.social.list_offered.is_none()));
        assert!(joining.accepted.lock().is_empty());
    }

    #[gpui::test]
    fn joining_an_offered_list_hands_the_link_to_the_seam(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two"]);
        let joining = Arc::new(party_offer::joined::Joining::default());
        together_over(&mut driven, joining.clone());

        offering_a_list(&mut driven);
        driven.click("join-the-list");
        driven.settle();
        driven.settle();

        assert!(driven.read(|root, _| root.social.list_offered.is_none()));
        assert_eq!(*joining.accepted.lock(), vec![A_LIST_LINK.to_owned()]);
    }

    #[gpui::test]
    fn a_party_that_will_not_be_joined_is_told_and_the_pane_stays(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one"]);
        together_over(&mut driven, party_offer::joined::Joining::refusing());

        offering_a_party(&mut driven);
        driven.click("join-the-party");
        driven.settle();
        driven.settle();

        assert_eq!(driven.read(|root, _| root.pane), Pane::Queue);
        assert!(driven.read(|_, cx| RootView::noticed(cx)));
    }

    #[gpui::test]
    fn a_toast_stays_while_the_pointer_is_on_it_and_goes_once_it_leaves(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one"]);

        driven.cx.update(|_, cx| {
            toast::tell(Notice::Done("Queued".to_owned()), cx);
        });
        driven.settle();
        let on_the_toast = driven.centre_of("toast");
        driven.hover(on_the_toast);

        for _ in 0..30 {
            driven
                .cx
                .executor()
                .advance_clock(std::time::Duration::from_secs(1));
            driven.settle();
        }
        assert!(
            driven.read(|_, cx| toast::is_showing(cx)),
            "a toast under the pointer went"
        );

        driven.hover(gpui::point(gpui::px(2.0), gpui::px(2.0)));
        for _ in 0..12 {
            driven
                .cx
                .executor()
                .advance_clock(std::time::Duration::from_secs(1));
            driven.settle();
        }
        assert!(driven.read(|_, cx| !toast::is_showing(cx)));
    }

    #[gpui::test]
    fn escape_closes_an_open_menu_before_it_takes_a_toast_down(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one"]);
        let root = driven.root.clone();

        driven.cx.update(|_, cx| {
            toast::tell(Notice::Done("Queued".to_owned()), cx);
            root.update(cx, |root, cx| {
                root.menu = Some(Menu::at(gpui::point(gpui::px(40.0), gpui::px(40.0))));
                cx.notify();
            });
        });
        driven.settle();

        driven.cx.simulate_keystrokes("escape");
        assert!(driven.read(|root, cx| root.menu.is_none() && RootView::noticed(cx)));

        driven.cx.simulate_keystrokes("escape");
        assert!(driven.read(|_, cx| !RootView::noticed(cx)));
    }

    const ON_A_MAC: PlatformInfo = PlatformInfo {
        os: OperatingSystem::Mac,
        target: PlatformTarget::Native,
    };

    #[gpui::test]
    fn on_a_mac_command_steers_the_window_and_control_is_left_to_the_chords_command_cannot_take(
        cx: &mut TestAppContext,
    ) {
        cx.set_platform_info(ON_A_MAC);
        let mut driven = Driven::open(cx, catalog());
        driven.focus_the_window();
        let behind = driven.read(|root, _| root.pane);

        driven.cx.simulate_keystrokes("ctrl-u");
        assert_eq!(driven.read(|root, _| root.pane), behind);

        driven.cx.simulate_keystrokes("cmd-u");
        assert_eq!(driven.read(|root, _| root.pane), Pane::Queue);

        driven.cx.simulate_keystrokes("secondary-u");
        assert_eq!(driven.read(|root, _| root.pane), behind);

        driven.cx.simulate_keystrokes("cmd-,");
        assert_eq!(driven.read(|root, _| root.pane), Pane::Settings);
        assert!(driven.cx.update(|window, cx| {
            driven
                .root
                .read(cx)
                .settings_fields
                .read(cx)
                .finding
                .read(cx)
                .is_focused(window)
        }));
    }

    #[gpui::test]
    fn the_menus_about_opens_the_about_category_whatever_stood_there(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());
        driven.focus_the_window();

        driven.cx.dispatch_action(About);

        assert_eq!(driven.read(|root, _| root.pane), Pane::Settings);
        assert_eq!(
            driven.read(|root, cx| root.settings_pane.read(cx).shown.category),
            SettingsCategory::About
        );
    }

    #[gpui::test]
    fn a_key_opens_the_queue_and_the_same_key_goes_back_to_the_pane_it_covered(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        driven.focus_the_window();
        let behind = driven.read(|root, _| root.pane);

        driven.cx.simulate_keystrokes("secondary-u");
        assert_eq!(driven.read(|root, _| root.pane), Pane::Queue);

        driven.cx.simulate_keystrokes("secondary-u");
        assert_eq!(driven.read(|root, _| root.pane), behind);
    }

    #[gpui::test]
    fn a_key_mutes_and_the_same_key_brings_back_the_level_it_was_heard_at(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one"]);
        let heard_at = driven.read(|root, cx| root.player.read(cx).state().volume);

        driven.cx.simulate_keystrokes("ctrl-m");
        driven.until(|root, cx| root.player.read(cx).state().volume == Volume::MUTE);

        driven.cx.simulate_keystrokes("ctrl-m");
        driven.until(|root, cx| root.player.read(cx).state().volume == heard_at);
    }

    #[gpui::test]
    fn two_edits_asked_for_at_once_both_land(cx: &mut TestAppContext) {
        let library = catalog();
        let mut driven = Driven::open(cx, Arc::clone(&library));
        let model = driven.read(|root, _| root.library.clone());

        driven.cx.update(|_, cx| {
            model.update(cx, |model, cx| {
                model.create_playlist("Meddle".to_owned(), Vec::new(), cx);
                model.create_playlist("Animals".to_owned(), Vec::new(), cx);
            });
        });
        driven.until(|_, _| {
            library
                .playlist_lists(PlaylistOrder::Name, Direction::Ascending)
                .is_ok_and(|lists| lists.len() == 2)
        });
    }

    #[gpui::test]
    fn the_party_pane_redraws_the_window_only_while_it_is_shown(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        let beat = Duration::from_millis(500);
        let root = driven.root.clone();

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.set_pane(Pane::Party, cx));
        });
        driven.settle();
        driven.cx.executor().advance_clock(beat);
        driven.settle();

        assert!(driven.read(|root, cx| root.party.read(cx).is_redrawing()));

        driven.click("tab-albums");
        for _ in 0..3 {
            driven.cx.executor().advance_clock(beat);
            driven.settle();
        }

        assert!(!driven.read(|root, cx| root.party.read(cx).is_redrawing()));
    }

    fn regions_told(driven: &mut Driven) -> Rc<RefCell<Vec<Region>>> {
        let told = Rc::new(RefCell::new(Vec::new()));
        let parts = driven.read(|root, _| root.parts.each());
        driven.cx.update(|_, cx| {
            for (region, part) in parts {
                let telling = Rc::clone(&told);
                cx.observe(&part, move |_, _| telling.borrow_mut().push(region))
                    .detach();
            }
        });
        told
    }

    #[gpui::test]
    fn the_party_panes_pulse_redraws_the_pane_and_leaves_the_other_regions_alone(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        let beat = Duration::from_millis(500);
        let root = driven.root.clone();

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.set_pane(Pane::Party, cx));
        });
        driven.settle();
        let told = regions_told(&mut driven);
        for _ in 0..3 {
            driven.cx.executor().advance_clock(beat);
            driven.cx.run_until_parked();
        }
        let told = told.borrow().clone();

        assert!(told.contains(&Region::Pane));
        assert!(
            told.iter().all(|region| *region == Region::Pane),
            "{told:?}"
        );
    }

    #[gpui::test]
    fn the_sharing_sheet_reads_what_is_shared_each_second_while_open_and_never_as_it_draws(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = catalog();
        let playlist = library.create_playlist("Mix").expect("a playlist");
        let mut driven = Driven::opened_in(cx, library, &folder);
        let joining = Arc::new(party_offer::joined::Joining::default());
        let shared = crate::together::SharedSeen {
            name: "Mix".to_owned(),
            may_edit: true,
            edit_link: None,
            view_link: "resonate://shared/E-ABCDE".to_owned(),
            people: Vec::new(),
            unheld: Vec::new(),
            reach: crate::together::SharedReach::Reached,
        };
        *joining.sharing.lock() = Some(shared.clone());
        together_over(&mut driven, joining.clone());
        let root = driven.root.clone();
        let asked = || *joining.seen_asked.lock();

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.open_sharing(playlist, cx));
        });
        driven.cx.run_until_parked();
        let on_opening = asked();
        let seen_on_opening = driven.read(|root, _| {
            root.social
                .sharing
                .as_ref()
                .map(|sharing| sharing.seen.clone())
        });
        for _ in 0..5 {
            driven.cx.update(|window, _| window.refresh());
            driven.cx.run_until_parked();
        }
        let while_drawn = asked();
        driven.cx.executor().advance_clock(Duration::from_secs(1));
        driven.cx.run_until_parked();
        let a_second_on = asked();

        assert_eq!(on_opening, 1);
        assert_eq!(seen_on_opening, Some(Seen::Shared(shared)));
        assert_eq!(while_drawn, 1);
        assert_eq!(a_second_on, 2);
        assert!(driven.read(|root, _| root.social.reading_the_shares.is_some()));

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.close_sharing(cx));
        });
        driven.cx.executor().advance_clock(Duration::from_secs(3));
        driven.cx.run_until_parked();

        assert_eq!(asked(), 2);
        assert!(driven.read(|root, _| root.social.reading_the_shares.is_none()));
    }

    #[gpui::test]
    fn deleting_every_row_leaves_nothing_reached(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["one", "two"]);
        let root = driven.root.clone();

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.drop_rows(Shift::Queue, Span::between(0, 1), cx);
            });
        });
        driven.until(|root, cx| root.player.read(cx).queue().is_empty());

        assert_eq!(reach(&mut driven), None);
    }

    fn encoded_as_echoes(folder: &Folder, name: &str, codec: &[&str]) -> Option<PathBuf> {
        folder.encoded_titled(name, codec, "Echoes")
    }

    fn playlist_row(playlist: resonate_core::PlaylistId) -> &'static str {
        Box::leak(format!("playlist-{playlist}").into_boxed_str())
    }

    fn the_menu_offers(driven: &mut Driven, label: &str) -> Option<usize> {
        driven.read(|root, _| {
            root.menu
                .as_ref()
                .and_then(|menu| menu.labels().iter().position(|offered| offered == label))
        })
    }

    fn opened_the_sheet_for(driven: &mut Driven, playlist: resonate_core::PlaylistId) {
        driven.right_click(playlist_row(playlist));
        let at = the_menu_offers(driven, crate::words::replacing::replace_lossy_songs())
            .expect("the menu offers to replace the lossy songs");
        driven.click(Box::leak(format!("menu-entry-{at}").into_boxed_str()));
        driven.until(|root, _| {
            root.replacing
                .as_ref()
                .is_some_and(|sheet| sheet.rows.is_some())
        });
    }

    fn queued_at(driven: &mut Driven) -> Vec<resonate_core::MediaLocation> {
        driven.read(|root, cx| {
            root.player
                .read(cx)
                .queue()
                .iter()
                .map(|item| item.location.clone())
                .collect()
        })
    }

    #[gpui::test]
    fn a_lossy_song_plays_from_its_lossless_copy_once_kept_beside_it(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (Some(opus), Some(flac)) = (
            encoded_as_echoes(&folder, "A.opus", &["-c:a", "libopus", "-b:a", "96k"]),
            encoded_as_echoes(&folder, "A.flac", &["-c:a", "flac"]),
        ) else {
            return;
        };
        let trash = Folder::new();
        let library = catalog();
        library.trash_into(Some(trash.path().to_path_buf()));
        Driven::scanned(&library, &folder);
        let lossy = resonate_core::MediaLocation::local(&opus);
        let road_trip = library
            .start_playlist("Road trip", &[Cut::whole(lossy.clone())])
            .expect("a playlist holding the lossy copy");
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.play(std::slice::from_ref(&opus));
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);
        assert_eq!(queued_at(&mut driven), [lossy]);

        let rows = library
            .songs_to_better_in(road_trip)
            .expect("the playlist read");
        let model = driven.read(|root, _| root.library.clone());
        driven.cx.update(|_, cx| {
            model.update(cx, |library, cx| library.replace_lossy_songs(rows, cx));
        });
        let song = library
            .track_at(&opus, None)
            .expect("the catalog read")
            .expect("the song is held")
            .id;
        driven.until(|root, cx| root.library.read(cx).has_a_version_chosen(song));

        assert!(opus.exists(), "the lossy copy went");
        driven.root.update(&mut driven.cx, |root, cx| {
            let held = root.library.read(cx).held_tracks(&[song]);
            root.play(&held, 0, cx);
        });
        let lossless = resonate_core::MediaLocation::local(&flac);
        driven.until(|root, cx| {
            root.player
                .read(cx)
                .queue()
                .iter()
                .all(|item| item.location == lossless)
        });
        assert_eq!(queued_at(&mut driven), [lossless]);
    }

    const ECHOES_RECORDING: &str = "b1a9c0de-1111-4222-8333-444455556666";
    const MEDDLE_RELEASE: &str = "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f";

    struct Unanswering(resonate_core::SourceId);

    impl resonate_providers::Provider for Unanswering {
        fn source(&self) -> &resonate_core::SourceId {
            &self.0
        }

        fn find(
            &self,
            _: &resonate_providers::Identity,
        ) -> resonate_providers::Result<resonate_providers::Obtained> {
            Ok(resonate_providers::Obtained::Nothing)
        }
    }

    struct OfferingLossless(resonate_core::SourceId);

    impl resonate_providers::Provider for OfferingLossless {
        fn source(&self) -> &resonate_core::SourceId {
            &self.0
        }

        fn find(
            &self,
            _: &resonate_providers::Identity,
        ) -> resonate_providers::Result<resonate_providers::Obtained> {
            Ok(resonate_providers::Obtained::Found(
                resonate_providers::Delivery::Stream {
                    key: "track/1".into(),
                    extension: resonate_providers::Extension::new("flac")?,
                    opening: resonate_providers::Opening::ready(std::io::empty()),
                },
            ))
        }
    }

    fn a_shop_offering_it_lossless() -> crate::Registering {
        Arc::new(|_: &crate::Supplying<'_>| {
            resonate_providers::Providers::none().and(Arc::new(OfferingLossless(
                resonate_core::SourceId::new("shop").expect("a source name"),
            )))
        })
    }

    fn a_shop_answering_nothing() -> crate::Registering {
        Arc::new(|_: &crate::Supplying<'_>| {
            resonate_providers::Providers::none().and(Arc::new(Unanswering(
                resonate_core::SourceId::new("shop").expect("a source name"),
            )))
        })
    }

    fn matched_on_musicbrainz(library: &Library, song: &std::path::Path) {
        let mbid = |id: &str| resonate_library::Mbid::new(id).expect("an mbid");
        let track = library
            .track_at(song, None)
            .expect("the catalog read")
            .expect("the song is catalogued");
        let album = track.album_id.expect("the song files under an album");
        library
            .land_release(
                album,
                &resonate_library::Release {
                    id: mbid(MEDDLE_RELEASE),
                    group: None,
                    title: "Meddle".to_owned(),
                    credit: Vec::new(),
                    date: None,
                    country: None,
                    label: None,
                    catalog_number: None,
                    barcode: None,
                    kind: None,
                    disambiguation: None,
                    has_front_cover: false,
                    links: Vec::new(),
                    media: vec![resonate_library::Medium {
                        position: 1,
                        format: None,
                        title: None,
                        tracks: vec![resonate_library::ReleaseTrack {
                            position: 1,
                            number: "1".to_owned(),
                            title: "Echoes".to_owned(),
                            artist: None,
                            recording: Some(mbid(ECHOES_RECORDING)),
                            track: None,
                            length: Some(Duration::from_secs(2)),
                            isrcs: Vec::new(),
                            links: Vec::new(),
                        }],
                    }],
                },
            )
            .expect("the release lands");
        library.rematch(album).expect("the release pairs");
    }

    #[gpui::test]
    fn a_lossy_song_wanted_again_stands_in_the_downloads_and_its_cancel_keeps_the_song(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let Some(opus) = encoded_as_echoes(&folder, "A.opus", &["-c:a", "libopus", "-b:a", "96k"])
        else {
            return;
        };
        let library = catalog();
        Driven::scanned(&library, &folder);
        matched_on_musicbrainz(&library, &opus);
        let road_trip = library
            .start_playlist(
                "Road trip",
                &[Cut::whole(resonate_core::MediaLocation::local(&opus))],
            )
            .expect("a playlist holding the lossy copy");
        let rows = library
            .songs_to_better_in(road_trip)
            .expect("the playlist read");
        assert!(
            rows.iter().all(|row| row.release_track.is_some()),
            "{rows:?}"
        );
        let mut driven = Driven::provided_by(
            cx,
            Arc::clone(&library),
            &folder,
            a_shop_answering_nothing(),
        );

        let model = driven.read(|root, _| root.library.clone());
        driven.cx.update(|_, cx| {
            model.update(cx, |library, cx| library.replace_lossy_songs(rows, cx));
        });
        driven.until(|root, cx| {
            let library = root.library.read(cx);
            library.downloads().first().is_some_and(|download| {
                library.replaces_a_song(download) && library.fetching(download).can_be_cancelled()
            })
        });
        assert!(
            library
                .songs_to_better_in(road_trip)
                .expect("the playlist read")
                .iter()
                .all(|row| row.pending)
        );
        driven.click("downloads");
        driven.bounds_of("download-0");

        driven.click("cancel-download-0");

        driven.until(|root, cx| root.library.read(cx).downloads().is_empty());
        driven.until(|_, _| {
            library
                .songs_to_better_in(road_trip)
                .is_ok_and(|rows| rows.len() == 1 && rows.iter().all(|row| !row.pending))
        });
        assert!(library.wants().expect("the wants read").is_empty());
        assert!(opus.exists(), "a cancelled replacement took the lossy copy");
    }

    #[gpui::test]
    fn a_long_title_in_the_lossy_songs_sheet_ends_in_an_ellipsis_before_why_it_stands(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let long = "Echoes ".repeat(40);
        let Some(opus) =
            folder.encoded_titled("A.opus", &["-c:a", "libopus", "-b:a", "96k"], long.trim())
        else {
            return;
        };
        let library = catalog();
        Driven::scanned(&library, &folder);
        let road_trip = library
            .start_playlist(
                "Road trip",
                &[Cut::whole(resonate_core::MediaLocation::local(&opus))],
            )
            .expect("a playlist holding the lossy copy");
        let track = library
            .track_at(&opus, None)
            .expect("the catalog read")
            .expect("the song is catalogued")
            .id;
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-playlists");
        driven.until(|root, cx| root.library.read(cx).playlist_name(road_trip).is_some());

        opened_the_sheet_for(&mut driven, road_trip);

        let sheet = driven.bounds_of("replacing-sheet");
        let standing = driven.bounds_of(Box::leak(
            format!("replacing-standing-{track}").into_boxed_str(),
        ));
        assert!(
            standing.right() <= sheet.right(),
            "why the song stands was pushed past the sheet: {standing:?} beyond {sheet:?}"
        );
        assert!(standing.size.width > gpui::px(0.0));
    }

    #[gpui::test]
    fn the_lossy_songs_sheet_of_a_playlist_the_shelves_miss_is_headed_by_its_name(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let Some(opus) = encoded_as_echoes(&folder, "A.opus", &["-c:a", "libopus", "-b:a", "96k"])
        else {
            return;
        };
        let library = catalog();
        Driven::scanned(&library, &folder);
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        let road_trip = library
            .start_playlist(
                "Road trip",
                &[Cut::whole(resonate_core::MediaLocation::local(&opus))],
            )
            .expect("a playlist holding the lossy copy");
        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).playlist_name(road_trip)),
            None,
            "the shelves already hold the playlist"
        );

        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.open_replacing(road_trip, cx));
        });
        driven.until(|root, _| {
            root.replacing
                .as_ref()
                .is_some_and(|sheet| sheet.rows.is_some())
        });

        assert_eq!(
            driven.read(|root, _| root.replacing.as_ref().map(|sheet| sheet.name.clone())),
            Some("Road trip".to_owned())
        );
    }

    #[gpui::test]
    fn the_services_are_asked_which_lossy_songs_they_hold_and_one_none_holds_stays_unticked(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let Some(opus) = encoded_as_echoes(&folder, "A.opus", &["-c:a", "libopus", "-b:a", "96k"])
        else {
            return;
        };
        let library = catalog();
        Driven::scanned(&library, &folder);
        matched_on_musicbrainz(&library, &opus);
        let road_trip = library
            .start_playlist(
                "Road trip",
                &[Cut::whole(resonate_core::MediaLocation::local(&opus))],
            )
            .expect("a playlist holding the lossy copy");
        let mut driven = Driven::provided_by(
            cx,
            Arc::clone(&library),
            &folder,
            a_shop_answering_nothing(),
        );
        driven.click("tab-playlists");
        driven.until(|root, cx| root.library.read(cx).playlist_name(road_trip).is_some());
        opened_the_sheet_for(&mut driven, road_trip);
        let ticked_before = driven.read(|root, _| {
            root.replacing
                .as_ref()
                .map_or(0, |sheet| sheet.ticked.len())
        });
        assert_eq!(ticked_before, 1, "the matched song starts ticked");

        driven.click("look-for-lossless-copies");
        driven.until(|root, _| {
            root.replacing.as_ref().is_some_and(|sheet| {
                !sheet.is_looking()
                    && sheet
                        .rows
                        .iter()
                        .flatten()
                        .all(resonate_library::Replaceable::is_offered_nowhere)
            })
        });

        assert!(
            driven.read(|root, _| root
                .replacing
                .as_ref()
                .is_some_and(|sheet| sheet.ticked.is_empty())),
            "a song no service holds stayed ticked"
        );
        assert!(
            library.wants().expect("the wants read").is_empty(),
            "checking wanted the song"
        );

        driven.cx.simulate_keystrokes("escape");
        driven.settle();
        opened_the_sheet_for(&mut driven, road_trip);
        assert!(
            driven.read(|root, _| root.replacing.as_ref().is_some_and(|sheet| {
                sheet.ticked.is_empty()
                    && sheet
                        .rows
                        .iter()
                        .flatten()
                        .all(resonate_library::Replaceable::is_offered_nowhere)
            })),
            "what the services answered was not kept"
        );
    }

    #[gpui::test]
    fn a_song_matched_nowhere_is_found_by_its_tags_and_ticked_to_be_downloaded_and_swapped(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let Some(opus) = encoded_as_echoes(&folder, "A.opus", &["-c:a", "libopus", "-b:a", "96k"])
        else {
            return;
        };
        let library = catalog();
        Driven::scanned(&library, &folder);
        let road_trip = library
            .start_playlist(
                "Road trip",
                &[Cut::whole(resonate_core::MediaLocation::local(&opus))],
            )
            .expect("a playlist holding the lossy copy");
        let mut driven = Driven::provided_by(
            cx,
            Arc::clone(&library),
            &folder,
            a_shop_offering_it_lossless(),
        );
        driven.click("tab-playlists");
        driven.until(|root, cx| root.library.read(cx).playlist_name(road_trip).is_some());
        opened_the_sheet_for(&mut driven, road_trip);
        assert!(
            driven.read(|root, _| root
                .replacing
                .as_ref()
                .is_some_and(|sheet| sheet.ticked.is_empty())),
            "a song matched nowhere started ticked before anything was found"
        );

        driven.click("look-for-lossless-copies");
        driven.until(|root, _| {
            root.replacing.as_ref().is_some_and(|sheet| {
                !sheet.is_looking() && sheet.rows.iter().flatten().all(|row| row.offer().is_some())
            })
        });

        assert_eq!(
            driven.read(|root, _| root.replacing.as_ref().map(|sheet| sheet.ticked.len())),
            Some(1),
            "the song found lossless was not ticked"
        );
    }

    #[gpui::test]
    fn a_long_playlists_lossy_songs_are_drawn_where_the_sheet_shows_them(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut songs = Vec::new();
        for at in 0..40 {
            let Some(song) = folder.encoded_titled(
                &format!("{at}.opus"),
                &["-c:a", "libopus", "-b:a", "96k"],
                &format!("Song {at}"),
            ) else {
                return;
            };
            songs.push(song);
        }
        let library = catalog();
        Driven::scanned(&library, &folder);
        let cuts: Vec<Cut> = songs
            .iter()
            .map(|song| Cut::whole(resonate_core::MediaLocation::local(song)))
            .collect();
        let road_trip = library
            .start_playlist("Road trip", &cuts)
            .expect("a playlist holding the lossy copies");
        let mut driven = Driven::provided_by(
            cx,
            Arc::clone(&library),
            &folder,
            a_shop_offering_it_lossless(),
        );
        driven.click("tab-playlists");
        driven.until(|root, cx| root.library.read(cx).playlist_name(road_trip).is_some());
        opened_the_sheet_for(&mut driven, road_trip);
        let first = driven.read(|root, _| {
            root.replacing
                .as_ref()
                .and_then(|sheet| sheet.rows.as_ref())
                .and_then(|rows| rows.first())
                .map(|row| row.track.id)
                .expect("the first row")
        });
        let standing = format!("replacing-standing-{first}");
        let sheet = driven.bounds_of("replacing-sheet");
        let drawn = driven.bounds_of(Box::leak(standing.clone().into_boxed_str()));
        assert!(
            drawn.top() >= sheet.top() && drawn.bottom() <= sheet.bottom(),
            "the first row is drawn outside the sheet before checking: {drawn:?} in {sheet:?}"
        );
        let line = driven.bounds_of(Box::leak(
            format!("replacing-line-{first}").into_boxed_str(),
        ));
        assert!(
            line.size.height >= drawn.size.height,
            "the rows were squeezed to fit the list rather than scrolled: {line:?} holding {drawn:?}"
        );

        driven.click("look-for-lossless-copies");
        driven.until(|root, _| {
            root.replacing
                .as_ref()
                .is_some_and(|sheet| !sheet.is_looking())
        });

        let sheet = driven.bounds_of("replacing-sheet");
        let drawn = driven.bounds_of(Box::leak(standing.into_boxed_str()));
        assert!(
            drawn.top() >= sheet.top() && drawn.bottom() <= sheet.bottom(),
            "the first row is drawn outside the sheet after checking: {drawn:?} in {sheet:?}"
        );
    }

    #[gpui::test]
    fn a_playlists_higher_quality_versions_are_kept_from_its_menu_and_the_sheet_closes_with_a_toast(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (Some(opus), Some(flac)) = (
            encoded_as_echoes(&folder, "A.opus", &["-c:a", "libopus", "-b:a", "96k"]),
            encoded_as_echoes(&folder, "A.flac", &["-c:a", "flac"]),
        ) else {
            return;
        };
        let trash = Folder::new();
        let library = catalog();
        library.trash_into(Some(trash.path().to_path_buf()));
        Driven::scanned(&library, &folder);
        let whole = |path: &PathBuf| Cut::whole(resonate_core::MediaLocation::local(path));
        let road_trip = library
            .start_playlist("Road trip", &[whole(&opus)])
            .expect("a playlist holding the lossy copy");
        let bright = library
            .start_playlist("Bright", &[whole(&flac)])
            .expect("a playlist holding the lossless copy");
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-playlists");
        driven.until(|root, cx| {
            let library = root.library.read(cx);
            library.playlists().len() == 2 && library.playlist_name(road_trip).is_some()
        });

        driven.right_click(playlist_row(bright));
        assert!(
            the_menu_offers(&mut driven, crate::words::replacing::replace_lossy_songs()).is_some(),
            "every playlist is offered its higher quality versions"
        );
        driven.cx.simulate_keystrokes("escape");
        driven.settle();

        opened_the_sheet_for(&mut driven, road_trip);
        driven.bounds_of("replacing-sheet");
        let (listed, ticked) = driven.read(|root, _| {
            let sheet = root.replacing.as_ref().expect("the sheet stands");
            let rows = sheet.rows.clone().unwrap_or_default();
            (
                rows.iter()
                    .map(|row| row.track.title.clone())
                    .collect::<Vec<_>>(),
                rows.iter().all(|row| sheet.ticked.contains(&row.track.id)),
            )
        });
        assert_eq!(listed, ["Echoes"]);
        assert!(ticked, "the song that can be replaced starts ticked");

        driven.cx.simulate_keystrokes("escape");
        driven.settle();
        assert!(driven.read(|root, _| root.replacing.is_none()));
        assert_eq!(
            library
                .songs_to_better_in(road_trip)
                .expect("the playlist read")
                .len(),
            1,
            "escape replaced nothing"
        );
        assert!(opus.exists());

        opened_the_sheet_for(&mut driven, road_trip);
        driven.click("replace-the-lossy-songs");
        driven.until(|_, _| {
            library
                .songs_to_better_in(road_trip)
                .is_ok_and(|rows| rows.is_empty())
        });
        driven.until_said(|told| told.contains("1 song plays a higher quality version"));

        assert!(driven.read(|root, _| root.replacing.is_none()));
        assert!(opus.exists() && flac.exists(), "a file went");
    }

    fn queued_on(driven: &mut Driven, zone: resonate_core::ZoneId) -> usize {
        driven.read(|root, cx| {
            root.zones
                .iter()
                .find(|shown| shown.id == zone)
                .map_or(0, |shown| shown.player.read(cx).queue().len())
        })
    }

    fn a_second_zone_behind_the_first(driven: &mut Driven) -> resonate_core::ZoneId {
        let root = driven.root.clone();
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.add_a_zone(cx)));
        driven.until(|root, _| root.zones.len() == 2);
        let second = driven.read(|root, _| root.switcher.selected());
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.show_zone(resonate_core::ZoneId::FIRST, cx);
            });
        });
        driven.settle();
        second
    }

    #[gpui::test]
    fn the_whole_library_played_on_one_zone_lands_there_though_another_is_shown_before_it_is_read(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        for name in ["one", "two", "three"] {
            folder.tone(&format!("{name}.wav"), 1);
        }
        let library = catalog();
        Driven::scanned(&library, &folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        let root = driven.root.clone();
        let second = a_second_zone_behind_the_first(&mut driven);

        driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                root.with_the_whole_library(window, cx, |this, tracks, _, cx| {
                    this.play(&tracks, 0, cx);
                });
                root.show_zone(second, cx);
            });
        });
        driven.until(|root, cx| {
            root.zones
                .iter()
                .any(|shown| !shown.player.read(cx).queue().is_empty())
        });

        assert_eq!(driven.read(|root, _| root.switcher.selected()), second);
        assert_eq!(queued_on(&mut driven, resonate_core::ZoneId::FIRST), 3);
        assert_eq!(
            queued_on(&mut driven, second),
            0,
            "the zone turned to took the play"
        );
    }

    #[gpui::test]
    fn a_playlist_queued_on_one_zone_lands_there_though_another_is_shown_before_it_is_read(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let songs = queued(&folder, &["one", "two"]);
        let library = catalog();
        Driven::scanned(&library, &folder);
        let playlist = library.create_playlist("Evening").expect("a playlist");
        library
            .add_to_playlist(
                playlist,
                &songs
                    .iter()
                    .map(|song| Cut::whole(resonate_core::MediaLocation::local(song)))
                    .collect::<Vec<_>>(),
            )
            .expect("the rows appended");
        let mut driven = Driven::opened_in(cx, library, &folder);
        let root = driven.root.clone();
        let second = a_second_zone_behind_the_first(&mut driven);

        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.queue_the_playlists(
                    vec![playlist],
                    resonate_engine::Placement::Queued,
                    false,
                    cx,
                );
                root.show_zone(second, cx);
            });
        });
        driven.until(|root, cx| {
            root.zones
                .iter()
                .any(|shown| !shown.player.read(cx).queue().is_empty())
        });

        assert_eq!(queued_on(&mut driven, resonate_core::ZoneId::FIRST), 2);
        assert_eq!(
            queued_on(&mut driven, second),
            0,
            "the zone turned to took the rows"
        );
    }
}
