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
    use std::{path::PathBuf, sync::Arc};

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
            party_offer,
            playlists::Held,
            reorder::{Reach, Shift},
            root::{Deleting, Magnified, Pane, RootView},
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
        driven.read(|root, _| root.reach.clone())
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

    #[gpui::test]
    fn backspace_after_a_jump_has_lapsed_takes_nothing_out_of_the_queue(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = queue_open(cx, &folder, &["alpha", "bravo", "charlie"]);

        driven.cx.simulate_keystrokes("c->c");
        driven.until(|root, _| root.reach == Some(Reach::at(Shift::Queue, 2)));
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
        driven.until(|root, _| root.reach == Some(Reach::at(Shift::Queue, 2)));
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
                .typed
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
            driven.read(|root, _| root.settings_shown.category),
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
}
