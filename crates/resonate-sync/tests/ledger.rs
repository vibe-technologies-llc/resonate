use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process, thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::{
    Answer, Figure, Link, Mbid, Recording, SharedBandPeaks, SharedCredit, SharedLyrics,
    SharedMedium, SharedRelease, SharedStudy, SharedTrack, WholeFile,
};
use resonate_sync::{
    Catalog, Change, Follow, Ledger, LocalListen, LocalPlaylistId, Me, Platform, SpaceId, Welcomed,
    wire::Incarnation,
};
use rusqlite::Connection;

use crate::shelf::{Shelf, song};

const SPACE: SpaceId = SpaceId([5; 16]);
const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
const A_DAY_IN_NANOS: i64 = 24 * 60 * 60 * 1_000_000_000;

fn days_ago(days: i64) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch")
        .as_nanos();
    i64::try_from(now).expect("a stamp") - days * A_DAY_IN_NANOS
}

fn identified(title: &str) -> Recording {
    Recording {
        mbid: Some(Mbid::new(ECHOES).expect("an mbid")),
        ..song(title)
    }
}

fn favourites_as_held(device: &Device) {
    let mut inside = device.shelf.inside.lock();

    let held = inside
        .songs
        .iter()
        .map(|(recording, _)| recording.clone())
        .collect::<Vec<_>>();

    for favourite in &mut inside.favourites {
        if let Some(own) = held
            .iter()
            .find(|own| own.names_the_same_song_as(favourite))
        {
            favourite.clone_from(own);
        }
    }
    drop(inside);
    device.shelf.moved();
}

trait EditedHere {
    fn edit_playlist(&self, id: i64, titles: &[&str]);
    fn drop_playlist(&self, id: i64);
    fn playlist_names(&self) -> Vec<String>;
    fn listen_here(&self, title: &str, at: i64);
}

impl EditedHere for Shelf {
    fn edit_playlist(&self, id: i64, titles: &[&str]) {
        let mut inside = self.inside.lock();
        let entries = titles.iter().map(|title| song(title)).collect();

        inside.playlists.get_mut(&id).expect("a playlist").2 = entries;
        drop(inside);
        self.moved();
    }

    fn drop_playlist(&self, id: i64) {
        self.inside.lock().playlists.remove(&id);
        self.moved();
    }

    fn playlist_names(&self) -> Vec<String> {
        let inside = self.inside.lock();
        inside
            .playlists
            .values()
            .map(|(_, name, _)| name.clone())
            .collect()
    }

    fn listen_here(&self, title: &str, at: i64) {
        let mut inside = self.inside.lock();

        let id = u64::try_from(inside.listens.len()).expect("a count") + 1;

        inside.listens.push(LocalListen {
            id,
            recording: song(title),
            at,
            heard: Duration::from_secs(100),
        });
        drop(inside);
        self.moved();
    }
}

struct Device {
    ledger: Ledger,
    shelf: Shelf,
    me: Me,
}

impl Device {
    fn new(name: &str, titles: &[&str]) -> Self {
        let mut ledger = Ledger::in_memory().expect("a ledger");

        ledger.enter_space(SPACE).expect("a space");
        Self {
            ledger,
            shelf: Shelf::holding(titles),
            me: Me {
                name: name.into(),
                platform: Platform::Linux,
            },
        }
    }

    fn look(&mut self) {
        self.ledger.look(&self.shelf, &self.me).expect("looked");
    }

    fn send(&mut self) -> Vec<Change> {
        self.look();

        let bundles = self.ledger.owed().expect("owed");
        let changes = bundles.into_iter().flatten().collect::<Vec<_>>();

        self.ledger.sent(&changes).expect("sent");
        changes
    }

    fn send_edit(&mut self, id: i64, titles: &[&str]) -> Vec<Change> {
        self.shelf.edit_playlist(id, titles);
        self.send()
    }

    fn receive(&mut self, changes: &[Change]) {
        self.ledger.arrived(&self.shelf, changes).expect("arrived");
    }
}

fn sync(from: &mut Device, to: &mut Device) {
    let changes = from.send();

    to.receive(&changes);
}

fn titles(device: &Device, name: &str) -> Vec<String> {
    device
        .shelf
        .playlist_named(name)
        .unwrap_or_else(|| panic!("{name} on {}", device.me.name))
        .1
}

fn scratch_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "resonate-ledger-{name}-{}-{}",
        process::id(),
        days_ago(0)
    ));

    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn device_over_a_file(name: &str, titles: &[&str], path: &Path) -> Device {
    let mut device = Device::new(name, titles);

    device.ledger = Ledger::open(path).expect("a ledger");
    device.ledger.enter_space(SPACE).expect("a space");
    device
}

fn reopened_as_an_older_build_left_it(device: &mut Device, path: &Path) {
    device.ledger = Ledger::in_memory().expect("a ledger standing in while the file is written");

    let connection = Connection::open(path).expect("the file");
    let laid: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the steps laid");

    connection
        .execute("UPDATE playlists SET nanos = ?1", [i64::MAX])
        .expect("a playlist stamped near the end of time");
    connection
        .execute("UPDATE state SET clock = ?1", [i64::MAX])
        .expect("a clock frozen there");
    connection
        .execute_batch("ALTER TABLE state DROP COLUMN answers_after")
        .expect("the answers of one instant not yet told apart");
    connection
        .execute_batch("ALTER TABLE state DROP COLUMN held_after")
        .expect("the songs of one second not yet told apart");
    connection
        .execute_batch("DROP TABLE unread")
        .expect("no change kept unread yet");
    connection
        .execute_batch("ALTER TABLE state DROP COLUMN incarnation")
        .expect("no incarnation kept yet");
    connection
        .pragma_update(None, "user_version", laid - 5)
        .expect("the steps not yet laid");
    drop(connection);
    device.ledger = Ledger::open(path).expect("the ledger opened by the build that restamps");
}

fn stamped_ahead(changes: Vec<Change>, ahead: i64) -> Vec<Change> {
    changes
        .into_iter()
        .map(|change| match change {
            Change::Favourite(mut note) => {
                note.stamp.nanos += ahead;
                Change::Favourite(note)
            }
            Change::Deletion(mut note) => {
                note.stamp.nanos += ahead;
                Change::Deletion(note)
            }
            other => other,
        })
        .collect()
}

fn reopened_after_its_clock_ran_ahead(device: &mut Device, path: &Path, ahead: i64) {
    device.ledger = Ledger::in_memory().expect("a ledger standing in while the file is written");

    let connection = Connection::open(path).expect("the file");

    connection
        .execute("UPDATE favourites SET nanos = nanos + ?1", [ahead])
        .expect("a favourite stamped while the clock ran ahead");
    connection
        .execute("UPDATE state SET clock = clock + ?1", [ahead])
        .expect("the clock kept while it ran ahead");
    drop(connection);
    device.ledger = Ledger::open(path).expect("the ledger opened once the clock is set right");
}

#[test]
fn a_title_past_the_longest_text_is_cut_and_never_wedges_the_ledger() {
    let overlong = "é".repeat(3000);
    let mut desk = Device::new("Desk", &[overlong.as_str(), "B"]);
    let mut phone = Device::new("Phone", &["B"]);

    desk.shelf.make_playlist("Long", &[overlong.as_str(), "B"]);

    sync(&mut desk, &mut phone);
    desk.shelf.make_playlist("Short", &["B"]);
    sync(&mut desk, &mut phone);

    assert_eq!(titles(&phone, "Long"), ["B"]);
    assert_eq!(titles(&phone, "Short"), ["B"]);
}

#[test]
fn a_playlist_made_on_one_device_arrives_on_the_other_and_is_not_sent_back() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &["A", "B", "C"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);

    sync(&mut desk, &mut phone);

    assert_eq!(titles(&phone, "Summer"), ["A", "B"]);

    let echoed = phone.send();

    assert!(
        echoed
            .iter()
            .all(|change| !matches!(change, Change::Playlist(_))),
        "{echoed:?}"
    );
}

#[test]
fn songs_a_device_lacks_survive_its_edit_in_their_places() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &["A", "C"]);
    let summer = desk.shelf.make_playlist("Summer", &["A", "B", "C"]);

    sync(&mut desk, &mut phone);

    assert_eq!(titles(&phone, "Summer"), ["A", "C"]);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");

    phone.shelf.edit_playlist(on_phone, &["C", "A"]);
    sync(&mut phone, &mut desk);

    assert_eq!(titles(&desk, "Summer"), ["C", "A", "B"]);
    assert_eq!(desk.shelf.playlist_named("Summer").expect("kept").0, summer);
}

#[test]
fn the_songs_missing_from_a_playlist_are_its_own_and_none_for_one_the_ledger_does_not_hold() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);
    desk.shelf.make_playlist("Winter", &["A", "C"]);
    sync(&mut desk, &mut phone);

    let missing = |name: &str| {
        let (id, _) = phone.shelf.playlist_named(name).expect("arrived");
        phone
            .ledger
            .missing_from(LocalPlaylistId {
                id,
                created: id * 1000,
            })
            .expect("missing")
            .into_iter()
            .map(|recording| recording.title)
            .collect::<Vec<_>>()
    };

    assert_eq!(missing("Summer"), ["B"]);
    assert_eq!(missing("Winter"), ["C"]);
    assert!(
        phone
            .ledger
            .missing_from(LocalPlaylistId {
                id: 9_999,
                created: 9_999_000,
            })
            .expect("nothing missing")
            .is_empty()
    );
}

#[test]
fn a_song_arriving_later_takes_its_place_in_the_playlist() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);
    sync(&mut desk, &mut phone);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");
    let at = LocalPlaylistId {
        id: on_phone,
        created: on_phone * 1000,
    };

    assert_eq!(
        phone
            .ledger
            .missing_from(at)
            .expect("missing")
            .iter()
            .map(|recording| recording.title.as_str())
            .collect::<Vec<_>>(),
        ["B"]
    );

    phone.ledger.keep_songs_of(at, true).expect("kept");

    let follow = Follow {
        playlists: true,
        ..Follow::default()
    };

    assert_eq!(
        phone
            .ledger
            .wanted(follow, &phone.shelf)
            .expect("wanted")
            .len(),
        1
    );

    phone.shelf.add_song(song("B"), b"B".to_vec());
    phone.ledger.settle(&phone.shelf).expect("settled");

    assert_eq!(titles(&phone, "Summer"), ["A", "B"]);
    assert!(
        phone
            .ledger
            .wanted(follow, &phone.shelf)
            .expect("wanted")
            .is_empty()
    );

    let echoed = phone.send();

    assert!(
        !echoed
            .iter()
            .any(|change| matches!(change, Change::Playlist(_)))
    );
}

#[test]
fn a_song_deleted_by_hand_is_wanted_again_while_its_playlist_and_favourite_stay() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);
    desk.shelf.favour_here("B", true);
    sync(&mut desk, &mut phone);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");
    let at = LocalPlaylistId {
        id: on_phone,
        created: on_phone * 1000,
    };

    phone.ledger.keep_songs_of(at, true).expect("kept");

    let playlists = Follow {
        playlists: true,
        ..Follow::default()
    };
    let favourites = Follow {
        favourites: true,
        ..Follow::default()
    };

    assert!(
        phone
            .ledger
            .wanted(playlists, &phone.shelf)
            .expect("wanted")
            .is_empty()
    );
    assert!(
        phone
            .ledger
            .wanted(favourites, &phone.shelf)
            .expect("wanted")
            .is_empty()
    );

    phone.shelf.inside.lock().deleted_by_hand = vec!["A".into(), "B".into()];
    phone.shelf.moved();
    phone.send();

    let wanted_titles = |follow| {
        let mut titles = phone
            .ledger
            .wanted(follow, &phone.shelf)
            .expect("wanted")
            .into_iter()
            .map(|recording| recording.title)
            .collect::<Vec<_>>();

        titles.sort();
        titles
    };

    assert_eq!(wanted_titles(playlists), ["A", "B"]);
    assert_eq!(wanted_titles(favourites), ["B"]);
    assert_eq!(titles(&phone, "Summer"), ["A", "B"]);
    assert_eq!(phone.shelf.favourite_titles(), ["B"]);
}

#[test]
fn a_song_deleted_from_the_app_stays_off_the_device_and_one_lost_behind_its_back_is_wanted() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);
    sync(&mut desk, &mut phone);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");

    phone
        .ledger
        .keep_songs_of(
            LocalPlaylistId {
                id: on_phone,
                created: on_phone * 1000,
            },
            true,
        )
        .expect("kept");

    let playlists = Follow {
        playlists: true,
        ..Follow::default()
    };

    phone.shelf.inside.lock().deleted_by_hand = vec!["A".into(), "B".into()];
    phone.shelf.inside.lock().deleted_here = vec!["A".into()];
    phone.shelf.moved();
    phone.send();

    let wanted = phone
        .ledger
        .wanted(playlists, &phone.shelf)
        .expect("wanted")
        .into_iter()
        .map(|recording| recording.title)
        .collect::<Vec<_>>();

    assert_eq!(wanted, ["B"]);
    assert_eq!(phone.shelf.inside.lock().deleted_here, ["A"]);
    assert_eq!(titles(&phone, "Summer"), ["A", "B"]);
}

#[test]
fn a_song_deleted_from_the_app_and_held_again_is_forgiven_and_wanted_when_lost_once_more() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.make_playlist("Summer", &["A"]);
    sync(&mut desk, &mut phone);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");

    phone
        .ledger
        .keep_songs_of(
            LocalPlaylistId {
                id: on_phone,
                created: on_phone * 1000,
            },
            true,
        )
        .expect("kept");

    let playlists = Follow {
        playlists: true,
        ..Follow::default()
    };

    phone.shelf.inside.lock().deleted_here = vec!["A".into()];

    assert!(
        phone
            .ledger
            .wanted(playlists, &phone.shelf)
            .expect("wanted")
            .is_empty()
    );
    assert!(phone.shelf.inside.lock().deleted_here.is_empty());

    phone.shelf.inside.lock().deleted_by_hand = vec!["A".into()];

    assert_eq!(
        phone
            .ledger
            .wanted(playlists, &phone.shelf)
            .expect("wanted")
            .len(),
        1
    );
}

#[test]
fn a_song_known_by_its_mbid_in_a_playlist_and_by_its_tags_as_a_favourite_is_wanted_once() {
    let mut desk = Device::new("Desk", &[]);
    let mut phone = Device::new("Phone", &[]);

    desk.shelf.add_song(identified("A"), b"A".repeat(1000));

    let id = desk.shelf.make_playlist("Summer", &[]);

    desk.shelf
        .inside
        .lock()
        .playlists
        .get_mut(&id)
        .expect("a playlist")
        .2 = vec![identified("A")];
    desk.shelf.inside.lock().favourites.push(song("A"));
    desk.shelf.moved();
    sync(&mut desk, &mut phone);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");

    phone
        .ledger
        .keep_songs_of(
            LocalPlaylistId {
                id: on_phone,
                created: on_phone * 1000,
            },
            true,
        )
        .expect("kept");

    let wanted = phone
        .ledger
        .wanted(
            Follow {
                favourites: true,
                playlists: true,
                everything: false,
            },
            &phone.shelf,
        )
        .expect("wanted");

    assert_eq!(wanted, [identified("A")]);
}

#[test]
fn a_playlist_removed_on_one_device_is_removed_on_the_other() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);
    let summer = desk.shelf.make_playlist("Summer", &["A"]);

    sync(&mut desk, &mut phone);

    desk.shelf.drop_playlist(summer);
    sync(&mut desk, &mut phone);

    assert!(phone.shelf.playlist_named("Summer").is_none());
}

#[test]
fn playlists_of_one_name_meeting_for_the_first_time_become_one() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &["A", "B", "C"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);
    phone.shelf.make_playlist("summer", &["B", "C"]);

    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    assert_eq!(phone.shelf.playlist_names(), ["Summer"]);
    assert_eq!(titles(&phone, "Summer"), ["A", "B", "C"]);
    assert_eq!(titles(&desk, "Summer"), ["A", "B", "C"]);
}

#[test]
fn an_arriving_name_another_playlist_holds_is_aliased_and_never_renamed_back() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);
    let summer = desk.shelf.make_playlist("Summer", &["A"]);

    desk.shelf.make_playlist("Winter", &["A"]);
    sync(&mut desk, &mut phone);

    let (winter_on_phone, _) = phone.shelf.playlist_named("Winter").expect("arrived");
    let (summer_on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");

    phone.shelf.rename_playlist(winter_on_phone, "Mix");
    desk.shelf.rename_playlist(summer, "Mix");
    sync(&mut desk, &mut phone);

    assert_eq!(
        phone.shelf.playlist_named("Mix (2)").expect("aliased").0,
        summer_on_phone
    );

    let echoed = phone.send();
    let playlists = echoed
        .iter()
        .filter_map(|change| match change {
            Change::Playlist(note) => note.body.as_ref().map(|body| body.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(playlists, ["Mix"]);
}

#[test]
fn a_saved_query_made_on_one_device_arrives_as_text_and_is_not_sent_back() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.make_query("Recent", "added:<30d");

    sync(&mut desk, &mut phone);

    assert_eq!(
        phone.shelf.query_named("Recent").map(|(_, text)| text),
        Some(Some("added:<30d".to_owned()))
    );
    assert!(phone.shelf.playlist_names().is_empty());

    let echoed = phone.send();

    assert!(
        echoed
            .iter()
            .all(|change| !matches!(change, Change::Query(_))),
        "{echoed:?}"
    );
}

#[test]
fn a_saved_query_revised_renamed_and_dropped_follows_on_the_other_device() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);
    let recent = desk.shelf.make_query("Recent", "added:<30d");

    sync(&mut desk, &mut phone);

    desk.shelf.revise_query(recent, "Recent", "added:<7d");
    sync(&mut desk, &mut phone);

    assert_eq!(
        phone.shelf.query_named("Recent").map(|(_, text)| text),
        Some(Some("added:<7d".to_owned()))
    );

    desk.shelf.revise_query(recent, "Lately", "added:<7d");
    sync(&mut desk, &mut phone);

    assert_eq!(phone.shelf.query_names(), ["Lately"]);

    desk.shelf.drop_query(recent);
    sync(&mut desk, &mut phone);

    assert!(phone.shelf.query_names().is_empty());
}

#[test]
fn saved_queries_of_one_name_meeting_for_the_first_time_become_one_the_synced_text_standing() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.make_query("Recent", "added:<30d");
    phone.shelf.make_query("recent", "plays:0");

    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    assert_eq!(phone.shelf.query_names().len(), 1);
    assert_eq!(desk.shelf.query_names(), ["Recent"]);
    assert_eq!(
        phone.shelf.query_named("Recent").map(|(_, text)| text),
        Some(Some("added:<30d".to_owned()))
    );
}

#[test]
fn a_query_and_a_list_never_stand_for_one_another() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.make_query("Mix", "added:<30d");
    phone.shelf.make_playlist("Mix", &["A"]);

    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    assert_eq!(phone.shelf.playlist_names(), ["Mix"]);
    assert_eq!(phone.shelf.query_names(), ["Mix"]);
    assert_eq!(desk.shelf.playlist_names(), ["Mix"]);
    assert_eq!(desk.shelf.query_names(), ["Mix"]);
}

#[test]
fn a_device_keeping_no_queries_passes_them_on_untouched() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut pocket = Device::new("Pocket", &["A"]);
    let mut tablet = Device::new("Tablet", &["A"]);

    pocket.shelf.inside.lock().without_queries = true;
    desk.shelf.make_query("Recent", "added:<30d");

    sync(&mut desk, &mut pocket);

    let snapshot = pocket.ledger.snapshot().expect("snapshot");

    tablet.receive(&snapshot.into_iter().flatten().collect::<Vec<_>>());

    assert!(pocket.shelf.query_names().is_empty());
    assert_eq!(tablet.shelf.query_names(), ["Recent"]);
}

#[test]
fn a_device_that_begins_keeping_queries_takes_up_those_it_passed_on_and_adopts_its_own_namesake() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut pocket = Device::new("Pocket", &["A"]);

    pocket.shelf.inside.lock().without_queries = true;
    desk.shelf.make_query("Recent", "added:<30d");
    desk.shelf.make_query("Unheard", "plays:0");
    sync(&mut desk, &mut pocket);

    pocket.shelf.inside.lock().without_queries = false;
    pocket.shelf.make_query("unheard", "plays:<2");

    let settled = pocket.ledger.settle(&pocket.shelf).expect("settled");

    assert_eq!(settled, 2);
    assert_eq!(pocket.shelf.query_names(), ["Unheard", "Recent"]);
    assert_eq!(
        pocket.shelf.query_named("Unheard").map(|(_, text)| text),
        Some(Some("plays:0".to_owned()))
    );

    let echoed = pocket.send();

    assert!(
        echoed
            .iter()
            .all(|change| !matches!(change, Change::Query(_))),
        "{echoed:?}"
    );
    assert_eq!(pocket.ledger.settle(&pocket.shelf).expect("settled"), 0);
}

#[test]
fn a_snapshot_brings_a_new_device_its_saved_queries_and_not_the_dropped_ones() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.make_query("Recent", "added:<30d");

    let gone = desk.shelf.make_query("Gone", "plays:0");

    sync(&mut desk, &mut phone);
    desk.shelf.drop_query(gone);
    sync(&mut desk, &mut phone);

    let mut tablet = Device::new("Tablet", &["A"]);
    let snapshot = phone.ledger.snapshot().expect("snapshot");

    tablet.receive(&snapshot.into_iter().flatten().collect::<Vec<_>>());

    assert_eq!(tablet.shelf.query_names(), ["Recent"]);
}

#[test]
fn the_later_edit_wins_where_two_devices_edit_one_playlist() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &["A", "B", "C"]);
    let summer = desk.shelf.make_playlist("Summer", &["A"]);

    sync(&mut desk, &mut phone);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");

    desk.shelf.edit_playlist(summer, &["A", "B"]);

    let from_desk = desk.send();

    phone.shelf.edit_playlist(on_phone, &["A", "C"]);

    let from_phone = phone.send();

    desk.receive(&from_phone);
    phone.receive(&from_desk);

    assert_eq!(titles(&desk, "Summer"), ["A", "C"]);
    assert_eq!(titles(&phone, "Summer"), ["A", "C"]);
}

#[test]
fn a_change_stamped_far_past_the_wall_clock_is_refused_and_a_later_honest_edit_still_lands() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &["A", "B", "C"]);
    let summer = desk.shelf.make_playlist("Summer", &["A"]);

    sync(&mut desk, &mut phone);

    let hostile = desk
        .send_edit(summer, &["C"])
        .into_iter()
        .map(|change| match change {
            Change::Playlist(mut note) => {
                note.stamp.nanos = i64::MAX;
                Change::Playlist(note)
            }
            other => other,
        })
        .collect::<Vec<_>>();

    phone.receive(&hostile);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");
    let from_phone = phone.send_edit(on_phone, &["B"]);

    desk.shelf.edit_playlist(summer, &["A", "B"]);
    sync(&mut desk, &mut phone);

    let newest_stamp = from_phone
        .iter()
        .filter_map(Change::stamp)
        .map(|stamp| stamp.nanos)
        .max()
        .expect("a stamped change");

    assert!(newest_stamp < i64::MAX);
    assert_eq!(titles(&phone, "Summer"), ["A", "B"]);
}

#[test]
fn a_playlist_an_older_build_stamped_near_the_end_of_time_takes_a_later_honest_edit_once_the_ledger_opens()
 {
    let folder = scratch_folder("restamped");
    let path = folder.join("sync.sqlite");
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = device_over_a_file("Phone", &["A", "B", "C"], &path);
    let summer = desk.shelf.make_playlist("Summer", &["A"]);

    sync(&mut desk, &mut phone);
    phone.send();
    reopened_as_an_older_build_left_it(&mut phone, &path);

    desk.shelf.edit_playlist(summer, &["A", "B"]);
    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    let on_the_phone = titles(&phone, "Summer");

    drop(phone);
    fs::remove_dir_all(&folder).expect("cleared");

    assert_eq!(on_the_phone, ["A", "B"]);
    assert_eq!(titles(&desk, "Summer"), ["A", "B"]);
}

#[test]
fn two_devices_holding_a_playlist_stamped_near_the_end_of_time_restamp_apart_and_converge_on_the_next_honest_edit()
 {
    let folder = scratch_folder("restamped-apart");
    let desk_path = folder.join("desk.sqlite");
    let phone_path = folder.join("phone.sqlite");
    let mut desk = device_over_a_file("Desk", &["A", "B", "C"], &desk_path);
    let mut phone = device_over_a_file("Phone", &["A", "B", "C"], &phone_path);
    let summer = desk.shelf.make_playlist("Summer", &["A"]);

    sync(&mut desk, &mut phone);
    phone.send();

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");

    reopened_as_an_older_build_left_it(&mut desk, &desk_path);
    thread::sleep(Duration::from_millis(5));
    reopened_as_an_older_build_left_it(&mut phone, &phone_path);

    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    let after_the_restamps = (titles(&desk, "Summer"), titles(&phone, "Summer"));

    desk.shelf.edit_playlist(summer, &["A", "B"]);
    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    let after_the_desk_edits = (titles(&desk, "Summer"), titles(&phone, "Summer"));

    phone.shelf.edit_playlist(on_phone, &["A", "C"]);
    sync(&mut phone, &mut desk);
    sync(&mut desk, &mut phone);

    let after_the_phone_edits = (titles(&desk, "Summer"), titles(&phone, "Summer"));

    drop((desk, phone));
    fs::remove_dir_all(&folder).expect("cleared");

    assert_eq!(after_the_restamps, (vec!["A".into()], vec!["A".into()]));
    assert_eq!(
        after_the_desk_edits,
        (vec!["A".into(), "B".into()], vec!["A".into(), "B".into()])
    );
    assert_eq!(
        after_the_phone_edits,
        (vec!["A".into(), "C".into()], vec!["A".into(), "C".into()])
    );
}

#[test]
fn a_favourite_stamped_days_ahead_travels_once_the_clock_is_set_right_and_a_later_edit_elsewhere_wins()
 {
    let folder = scratch_folder("ran-ahead");
    let path = folder.join("sync.sqlite");
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = device_over_a_file("Phone", &["A", "B"], &path);

    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);
    phone.shelf.favour_here("A", true);

    let refused = stamped_ahead(phone.send(), 3 * A_DAY_IN_NANOS);

    desk.receive(&refused);

    let while_it_ran_ahead = desk.shelf.favourite_titles();

    reopened_after_its_clock_ran_ahead(&mut phone, &path, 3 * A_DAY_IN_NANOS);
    sync(&mut phone, &mut desk);

    let once_set_right = desk.shelf.favourite_titles();

    desk.shelf.favour_here("A", false);
    sync(&mut desk, &mut phone);

    let after_the_desk_edits = phone.shelf.favourite_titles();

    drop(phone);
    fs::remove_dir_all(&folder).expect("cleared");

    assert!(while_it_ran_ahead.is_empty());
    assert_eq!(once_set_right, ["A"]);
    assert!(after_the_desk_edits.is_empty());
    assert!(desk.shelf.favourite_titles().is_empty());
}

#[test]
fn a_favourite_stamped_within_a_day_ahead_lands_keeps_its_stamp_and_a_later_edit_here_still_wins() {
    let folder = scratch_folder("within-skew");
    let path = folder.join("sync.sqlite");
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = device_over_a_file("Phone", &["A", "B"], &path);

    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);
    desk.shelf.favour_here("A", true);

    let ahead = stamped_ahead(desk.send(), A_DAY_IN_NANOS / 2);

    phone.receive(&ahead);

    let landed = phone.shelf.favourite_titles();

    phone.ledger = Ledger::open(&path).expect("the ledger opened again");

    let owed_again = phone
        .send()
        .iter()
        .any(|change| matches!(change, Change::Favourite(_)));

    phone.shelf.favour_here("A", false);
    sync(&mut phone, &mut desk);

    drop(phone);
    fs::remove_dir_all(&folder).expect("cleared");

    assert_eq!(landed, ["A"]);
    assert!(!owed_again);
    assert!(desk.shelf.favourite_titles().is_empty());
}

#[test]
fn a_playlist_edited_before_its_first_look_outranks_an_older_note_that_arrives() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &["A", "B", "C"]);
    let summer = desk.shelf.make_playlist("Summer", &["A"]);

    sync(&mut desk, &mut phone);

    let (on_phone, _) = phone.shelf.playlist_named("Summer").expect("arrived");

    desk.shelf.edit_playlist(summer, &["A", "B"]);

    let from_desk = desk.send();

    phone.shelf.edit_playlist(on_phone, &["A", "C"]);
    phone.receive(&from_desk);
    sync(&mut phone, &mut desk);

    assert_eq!(titles(&phone, "Summer"), ["A", "C"]);
    assert_eq!(titles(&desk, "Summer"), ["A", "C"]);
}

#[test]
fn a_favourite_removed_before_its_first_look_outranks_an_older_note_that_arrives() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    desk.shelf.favour_here("A", true);
    sync(&mut desk, &mut phone);
    desk.shelf.favour_here("A", false);
    desk.send();
    desk.shelf.favour_here("A", true);

    let from_desk = desk.send();

    phone.shelf.favour_here("A", false);
    phone.receive(&from_desk);
    sync(&mut phone, &mut desk);

    assert!(phone.shelf.favourite_titles().is_empty());
    assert!(desk.shelf.favourite_titles().is_empty());
}

#[test]
fn favourites_travel_both_ways_and_a_tombstone_never_takes_an_unmet_favourite() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    desk.shelf.favour_here("A", true);
    sync(&mut desk, &mut phone);

    assert_eq!(phone.shelf.favourite_titles(), ["A"]);

    phone.shelf.favour_here("A", false);
    sync(&mut phone, &mut desk);

    assert!(desk.shelf.favourite_titles().is_empty());

    let mut tablet = Device::new("Tablet", &["A"]);

    tablet.shelf.favour_here("A", true);
    desk.send();

    let everything = desk.ledger.snapshot().expect("snapshot");

    tablet.receive(&everything.into_iter().flatten().collect::<Vec<_>>());

    assert_eq!(tablet.shelf.favourite_titles(), ["A"]);
}

#[test]
fn listens_of_another_device_are_credited_once_their_song_is_here() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.listen_here("A", days_ago(2));
    desk.shelf.listen_here("B", days_ago(1));
    sync(&mut desk, &mut phone);

    assert_eq!(phone.shelf.inside.lock().credited.len(), 1);

    sync(&mut desk, &mut phone);

    assert_eq!(phone.shelf.inside.lock().credited.len(), 1);

    phone.shelf.add_song(song("B"), b"B".to_vec());
    phone.ledger.settle(&phone.shelf).expect("settled");

    assert_eq!(phone.shelf.inside.lock().credited.len(), 2);

    let echoed = phone.send();

    assert!(
        !echoed
            .iter()
            .any(|change| matches!(change, Change::Listen(_)))
    );
}

#[test]
fn a_listen_credited_before_the_ledger_could_say_so_is_credited_once() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.listen_here("A", days_ago(2));
    phone.shelf.inside.lock().dies_after_crediting = true;

    let changes = desk.send();
    let interrupted = phone.ledger.arrived(&phone.shelf, &changes);

    assert!(interrupted.is_err());
    assert_eq!(phone.shelf.inside.lock().credited.len(), 1);

    phone.shelf.inside.lock().dies_after_crediting = false;
    phone.ledger.settle(&phone.shelf).expect("settled");
    phone.ledger.settle(&phone.shelf).expect("settled again");

    assert_eq!(phone.shelf.inside.lock().credited.len(), 1);
}

#[test]
fn listens_waiting_on_songs_absent_here_are_asked_after_again_only_once_the_catalog_moves() {
    let titles = (0..300).map(|at| format!("Song {at}")).collect::<Vec<_>>();
    let named = titles.iter().map(String::as_str).collect::<Vec<_>>();
    let mut desk = Device::new("Desk", &named);
    let mut phone = Device::new("Phone", &[]);

    for title in &titles {
        desk.shelf.listen_here(title, days_ago(2));
        desk.shelf.listen_here(title, days_ago(1));
    }
    sync(&mut desk, &mut phone);

    let asked_on_arrival = phone.shelf.inside.lock().credits_asked;

    phone.ledger.settle(&phone.shelf).expect("settled");

    let asked = phone.shelf.inside.lock().credits_asked;

    for _ in 0..3 {
        phone.ledger.settle(&phone.shelf).expect("settled again");
        phone.look();
    }

    assert_eq!(asked_on_arrival, 2 * titles.len());
    assert_eq!(asked - asked_on_arrival, titles.len());
    assert_eq!(phone.shelf.inside.lock().credits_asked, asked);
    assert!(phone.shelf.inside.lock().credited.is_empty());

    phone.shelf.add_song(song("Song 7"), b"7".to_vec());
    phone
        .ledger
        .settle(&phone.shelf)
        .expect("settled once a song arrived");
    phone
        .ledger
        .settle(&phone.shelf)
        .expect("settled once more");

    let credited = phone
        .shelf
        .inside
        .lock()
        .credited
        .iter()
        .map(|listen| listen.recording.title.clone())
        .collect::<Vec<_>>();

    assert_eq!(credited, ["Song 7", "Song 7"]);
    assert_eq!(
        phone.shelf.inside.lock().credits_asked - asked,
        titles.len() + 1
    );
}

#[test]
fn a_listen_a_failing_catalog_refused_is_credited_by_a_later_settle_though_the_catalog_never_moved()
{
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.listen_here("A", days_ago(3));
    sync(&mut desk, &mut phone);
    phone.ledger.settle(&phone.shelf).expect("settled");

    assert_eq!(phone.shelf.inside.lock().credited.len(), 1);

    desk.shelf.listen_here("A", days_ago(2));
    phone.shelf.inside.lock().refuses_crediting = true;

    let changes = desk.send();

    assert!(phone.ledger.arrived(&phone.shelf, &changes).is_err());
    assert!(phone.ledger.settle(&phone.shelf).is_err());

    phone.shelf.inside.lock().refuses_crediting = false;
    phone
        .ledger
        .settle(&phone.shelf)
        .expect("settled once the catalog writes again");
    phone.ledger.settle(&phone.shelf).expect("settled again");

    assert_eq!(phone.shelf.inside.lock().credited.len(), 2);
}

#[test]
fn a_snapshot_brings_a_new_device_to_the_same_state() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);
    desk.shelf.favour_here("B", true);

    let gone = desk.shelf.make_playlist("Gone", &["A"]);

    sync(&mut desk, &mut phone);
    desk.shelf.drop_playlist(gone);
    sync(&mut desk, &mut phone);

    let mut tablet = Device::new("Tablet", &["A", "B"]);
    let snapshot = phone.ledger.snapshot().expect("snapshot");

    tablet.receive(&snapshot.into_iter().flatten().collect::<Vec<_>>());

    assert_eq!(tablet.shelf.playlist_names(), ["Summer"]);
    assert_eq!(titles(&tablet, "Summer"), ["A", "B"]);
    assert_eq!(tablet.shelf.favourite_titles(), ["B"]);
    assert_eq!(
        tablet
            .ledger
            .devices()
            .expect("devices")
            .iter()
            .map(|device| device.name.as_str())
            .collect::<Vec<_>>(),
        ["Desk"]
    );
}

#[test]
fn entering_another_space_forgets_the_last_one() {
    let mut desk = Device::new("Desk", &["A"]);

    desk.shelf.make_playlist("Summer", &["A"]);
    desk.send();

    assert!(desk.ledger.enter_space(SpaceId([6; 16])).expect("entered"));

    let resent = desk.send();

    assert!(
        resent
            .iter()
            .any(|change| matches!(change, Change::Playlist(_)))
    );
    assert!(!desk.ledger.enter_space(SpaceId([6; 16])).expect("entered"));
}

fn device_names(device: &Device) -> Vec<String> {
    device
        .ledger
        .devices()
        .expect("devices")
        .into_iter()
        .map(|note| note.name)
        .collect()
}

fn layouts_known(device: &Device) -> Vec<String> {
    device
        .ledger
        .layouts()
        .expect("layouts")
        .into_iter()
        .map(|note| note.layout)
        .collect()
}

fn files_as(device: &Device, layout: &str) {
    device.shelf.inside.lock().layout = Some(layout.to_owned());
    device.shelf.moved();
}

#[test]
fn an_organise_layout_travels_as_its_devices_newest_word_and_is_not_sent_back() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    files_as(&desk, "{artist}/{year} {album}/{track} {title}");

    sync(&mut desk, &mut phone);

    let first = phone.ledger.layouts().expect("layouts");
    let echoed = phone.send();
    let unchanged = desk.send();

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].device, desk.ledger.device());
    assert_eq!(first[0].layout, "{artist}/{year} {album}/{track} {title}");
    assert!(
        !echoed
            .iter()
            .any(|change| matches!(change, Change::Layout(_)))
    );
    assert!(
        !unchanged
            .iter()
            .any(|change| matches!(change, Change::Layout(_)))
    );

    files_as(&desk, "{albumartist}/{album}/{title}");
    sync(&mut desk, &mut phone);
    phone.receive(&[Change::Layout(first[0].clone())]);

    assert_eq!(layouts_known(&phone), ["{albumartist}/{album}/{title}"]);

    let mut tablet = Device::new("Tablet", &["A"]);
    let snapshot = phone.ledger.snapshot().expect("snapshot");

    tablet.receive(&snapshot.into_iter().flatten().collect::<Vec<_>>());

    assert_eq!(layouts_known(&tablet), ["{albumartist}/{album}/{title}"]);
}

#[test]
fn a_device_names_no_layout_until_its_catalog_tells_one_and_forgetting_the_device_forgets_its_layout()
 {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    sync(&mut phone, &mut desk);

    assert!(layouts_known(&desk).is_empty());

    files_as(&phone, "{album}/{title}");

    let told = phone.send();

    desk.receive(&told);

    assert_eq!(layouts_known(&desk), ["{album}/{title}"]);

    desk.ledger
        .forget_device(phone.ledger.device())
        .expect("forgotten");
    desk.receive(&told);

    assert!(layouts_known(&desk).is_empty());
}

#[test]
fn a_forgotten_device_leaves_every_list_and_its_old_note_does_not_bring_it_back() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);
    let mut old = Device::new("Old phone", &["A"]);

    let old_note = old.send();

    desk.receive(&old_note);
    phone.receive(&old_note);

    let old_id = old.ledger.device();
    let desk_id = desk.ledger.device();

    let forgotten = desk.ledger.forget_device(old_id).expect("forgotten");

    assert!(forgotten);
    assert!(!desk.ledger.forget_device(old_id).expect("forgotten again"));
    assert!(!desk.ledger.forget_device(desk_id).expect("this device"));
    assert!(!device_names(&desk).contains(&"Old phone".to_owned()));

    sync(&mut desk, &mut phone);
    phone.receive(&old_note);

    assert!(!device_names(&phone).contains(&"Old phone".to_owned()));

    let mut tablet = Device::new("Tablet", &["A"]);
    let snapshot = phone.ledger.snapshot().expect("snapshot");

    tablet.receive(&old_note);
    tablet.receive(&snapshot.into_iter().flatten().collect::<Vec<_>>());

    assert!(!device_names(&tablet).contains(&"Old phone".to_owned()));
    assert!(device_names(&tablet).contains(&"Desk".to_owned()));
}

#[test]
fn a_device_forgotten_while_it_still_syncs_speaks_up_and_is_listed_again() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    sync(&mut phone, &mut desk);

    let phone_id = phone.ledger.device();

    desk.ledger.forget_device(phone_id).expect("forgotten");
    sync(&mut desk, &mut phone);

    assert!(!device_names(&desk).contains(&"Phone".to_owned()));

    let spoken = phone.send();

    desk.receive(&spoken);

    assert!(
        spoken
            .iter()
            .any(|change| matches!(change, Change::Device(note) if note.device == phone_id))
    );
    assert!(device_names(&desk).contains(&"Phone".to_owned()));
}

#[test]
fn only_a_device_the_ledger_lists_is_known_and_a_forgotten_one_is_not() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);
    let stranger = Device::new("Stranger", &["A"]).ledger.device();
    let phone_id = phone.ledger.device();

    assert!(!desk.ledger.knows_device(phone_id).expect("asked"));

    sync(&mut phone, &mut desk);

    assert!(desk.ledger.knows_device(phone_id).expect("asked"));
    assert!(!desk.ledger.knows_device(stranger).expect("asked"));

    desk.ledger.forget_device(phone_id).expect("forgotten");

    assert!(!desk.ledger.knows_device(phone_id).expect("asked"));
}

#[test]
fn a_listen_past_what_the_ledger_keeps_is_never_credited_again() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.listen_here("A", days_ago(500));
    desk.shelf.listen_here("A", days_ago(3));

    sync(&mut desk, &mut phone);

    assert_eq!(phone.shelf.inside.lock().credited.len(), 1);
}

#[test]
fn a_favourite_on_a_copy_known_by_its_mbid_and_one_known_by_its_tags_stays_favoured() {
    let mut desk = Device::new("Desk", &[]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.add_song(identified("A"), b"A".repeat(1000));
    desk.shelf.inside.lock().favourites.push(identified("A"));
    desk.shelf.moved();

    for _ in 0..3 {
        sync(&mut desk, &mut phone);
        favourites_as_held(&phone);
        sync(&mut phone, &mut desk);
        favourites_as_held(&desk);
    }

    let from_desk = desk.send();
    let from_phone = phone.send();

    assert_eq!(desk.shelf.favourite_titles(), ["A"]);
    assert_eq!(phone.shelf.favourite_titles(), ["A"]);
    assert!(
        !from_desk
            .iter()
            .chain(&from_phone)
            .any(|change| matches!(change, Change::Favourite(_))),
        "{from_desk:?} {from_phone:?}"
    );
}

fn remastered(title: &str) -> Recording {
    Recording {
        title: format!("{title} (2011 Remaster)"),
        album: "Album (Remastered)".into(),
        ..song(title)
    }
}

fn told(changes: &[Change], what: fn(&Change) -> bool) -> usize {
    changes.iter().filter(|change| what(change)).count()
}

#[test]
fn a_song_a_playlist_names_and_the_catalog_holds_under_other_names_is_written_once_and_settled_no_more()
 {
    let mut desk = Device::new("Desk", &["Echoes", "Time"]);
    let mut phone = Device::new("Phone", &["Time"]);

    phone
        .shelf
        .receive_named_otherwise(song("Echoes"), remastered("Echoes"));
    desk.shelf.make_playlist("Mix", &["Echoes", "Time"]);

    sync(&mut desk, &mut phone);

    let written_at = phone.shelf.revision();
    let settled = phone.ledger.settle(&phone.shelf).expect("settled");
    let echoed = phone.send();
    let settled_again = phone.ledger.settle(&phone.shelf).expect("settled");

    assert_eq!(titles(&phone, "Mix"), ["Echoes (2011 Remaster)", "Time"]);
    assert_eq!((settled, settled_again), (0, 0));
    assert_eq!(phone.shelf.revision(), written_at);
    assert_eq!(
        told(&echoed, |change| matches!(change, Change::Playlist(_))),
        0
    );
}

#[test]
fn an_edit_to_a_playlist_holding_a_song_under_other_names_keeps_it_once_under_the_name_it_travels_by()
 {
    let mut desk = Device::new("Desk", &["Echoes", "Time", "Money"]);
    let mut phone = Device::new("Phone", &["Time", "Money"]);

    phone
        .shelf
        .receive_named_otherwise(song("Echoes"), remastered("Echoes"));
    desk.shelf.make_playlist("Mix", &["Echoes", "Time"]);
    sync(&mut desk, &mut phone);
    phone.ledger.settle(&phone.shelf).expect("settled");

    let (on_phone, _) = phone.shelf.playlist_named("Mix").expect("arrived");

    phone
        .shelf
        .inside
        .lock()
        .playlists
        .get_mut(&on_phone)
        .expect("a playlist")
        .2 = vec![remastered("Echoes"), song("Time"), song("Money")];
    phone.shelf.moved();
    phone.ledger.settle(&phone.shelf).expect("settled");
    sync(&mut phone, &mut desk);

    assert_eq!(titles(&desk, "Mix"), ["Echoes", "Time", "Money"]);
    assert_eq!(
        titles(&phone, "Mix"),
        ["Echoes (2011 Remaster)", "Time", "Money"]
    );
}

#[test]
fn a_listeners_edit_made_before_a_song_lands_is_kept_by_the_settle_that_follows() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &["A", "C"]);

    desk.shelf.make_playlist("Mix", &["A", "B"]);
    sync(&mut desk, &mut phone);

    let (on_phone, _) = phone.shelf.playlist_named("Mix").expect("arrived");

    phone.shelf.edit_playlist(on_phone, &["A", "C"]);
    phone.shelf.add_song(song("B"), b"B".repeat(1000));
    phone.ledger.settle(&phone.shelf).expect("settled");
    phone.look();
    phone.ledger.settle(&phone.shelf).expect("settled");
    sync(&mut phone, &mut desk);

    assert_eq!(titles(&phone, "Mix"), ["A", "B", "C"]);
    assert_eq!(titles(&desk, "Mix"), ["A", "B", "C"]);
}

#[test]
fn a_favourite_on_a_song_held_under_other_names_stays_one_favourite_under_the_name_it_travels_by() {
    let mut desk = Device::new("Desk", &["Echoes"]);
    let mut phone = Device::new("Phone", &[]);

    phone
        .shelf
        .receive_named_otherwise(song("Echoes"), remastered("Echoes"));
    desk.shelf.favour_here("Echoes", true);

    sync(&mut desk, &mut phone);

    let echoed = phone.send();

    desk.receive(&echoed);

    assert_eq!(phone.shelf.favourite_titles(), ["Echoes (2011 Remaster)"]);
    assert_eq!(
        told(&echoed, |change| matches!(change, Change::Favourite(_))),
        0
    );
    assert_eq!(desk.shelf.favourite_titles(), ["Echoes"]);

    phone.shelf.favour_here("Echoes (2011 Remaster)", false);
    sync(&mut phone, &mut desk);

    assert!(desk.shelf.favourite_titles().is_empty());
}

#[test]
fn an_older_tombstone_under_one_key_never_unfavours_a_song_a_newer_favourite_under_another_keeps() {
    let mut desk = Device::new("Desk", &[]);
    let mut laptop = Device::new("Laptop", &["Echoes"]);
    let mut phone = Device::new("Phone", &["Echoes"]);

    desk.shelf.add_song(identified("Echoes"), b"E".repeat(1000));
    desk.shelf
        .inside
        .lock()
        .favourites
        .push(identified("Echoes"));
    desk.shelf.moved();

    let favoured = desk.send();

    phone.receive(&favoured);
    desk.shelf.inside.lock().favourites.clear();
    desk.shelf.moved();

    let unfavoured = desk.send();

    laptop.shelf.favour_here("Echoes", true);

    let favoured_again = laptop.send();

    phone.receive(&favoured_again);
    phone.receive(&unfavoured);
    phone.ledger.settle(&phone.shelf).expect("settled");

    assert_eq!(phone.shelf.favourite_titles(), ["Echoes"]);
}

#[test]
fn a_ledger_kept_without_a_server_owes_everything_again_once_it_has_one() {
    let mut desk = Device::new("Desk", &["A"]);

    desk.shelf.make_playlist("Summer", &["A"]);

    let while_serverless = desk.ledger.through_a_server(false).expect("serverless");

    desk.send();

    let owed_before = desk.ledger.owed_count().expect("owed");
    let owed_again = desk.ledger.through_a_server(true).expect("a server");
    let owed_after = desk.ledger.owed_count().expect("owed");

    desk.send();

    let once_more = desk.ledger.through_a_server(true).expect("a server still");

    assert!(!while_serverless);
    assert_eq!(owed_before, 0);
    assert!(owed_again);
    assert!(owed_after >= 2, "{owed_after}");
    assert!(!once_more);
    assert_eq!(desk.ledger.owed_count().expect("owed"), 0);
}

#[test]
fn a_device_unnamed_is_called_after_its_host_and_platform_and_a_chosen_name_wins() {
    let hosted = Me::called("", "vincent\n", Platform::Linux, "Linux");
    let chosen = Me::called("  Desk ", "vincent", Platform::Linux, "Linux");
    let alone = Me::called(" ", "\n", Platform::Android, "Android");

    assert_eq!(hosted.name, "vincent (Linux)");
    assert_eq!(chosen.name, "Desk");
    assert_eq!(alone.name, "Android");
}

fn sung(text: &str, synced: bool) -> Answer {
    Answer::Lyrics(SharedLyrics {
        text: text.into(),
        synced,
    })
}

fn studied(packets: u64) -> Answer {
    Answer::Study(SharedStudy {
        file: WholeFile {
            packets,
            frames: 7_938_000,
            rate: 44_100,
        },
        judged_under: 1,
        verdict: "genuine".into(),
        cutoff_hz: None,
        cutoff_drop: None,
        lossy_guess: None,
        upsampled_from: None,
        bits_in_use: Some(16),
        declared_bits: Some(16),
        peak: Figure::of(-0.5),
        rms: Figure::of(-17.0),
        true_peak: Figure::of(-0.2),
        loudness: Some(Figure::of(-13.9)),
        loudness_range: Some(Figure::of(6.5)),
        dynamic_range: Some(9),
        clipped: 0,
        mono_as_stereo: false,
        band_levels: vec![3; 29],
    })
}

fn peaked(packets: u64) -> Answer {
    Answer::BandPeaks(SharedBandPeaks {
        file: WholeFile {
            packets,
            frames: 7_938_000,
            rate: 44_100,
        },
        judged_under: 1,
        peaks: vec![9; 472],
    })
}

fn answers_in(changes: &[Change]) -> usize {
    changes
        .iter()
        .filter(|change| matches!(change, Change::Answer(_)))
        .count()
}

#[test]
fn an_answer_learnt_on_one_device_lands_on_another_holding_the_song_and_is_not_sent_back() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    desk.shelf.answer_here("A", sung("[00:01.00] Words", true));
    desk.shelf.answer_here("A", studied(42));
    desk.shelf.answer_here("B", studied(7));

    let told = desk.send();

    phone.receive(&told);

    assert_eq!(answers_in(&told), 3);
    assert_eq!(
        phone.shelf.answers_of("A"),
        [sung("[00:01.00] Words", true), studied(42)]
    );
    assert_eq!(phone.shelf.answers_of("B"), [studied(7)]);
    assert_eq!(answers_in(&phone.send()), 0);
    assert_eq!(answers_in(&desk.send()), 0);
}

#[test]
fn an_answer_for_a_song_not_held_yet_lands_once_the_song_arrives() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &[]);

    desk.shelf.answer_here("A", sung("Words", false));
    sync(&mut desk, &mut phone);

    assert!(phone.shelf.answers_of("A").is_empty());

    phone.shelf.add_song(song("A"), b"A".repeat(1000));
    phone.ledger.settle(&phone.shelf).expect("settled");

    assert_eq!(phone.shelf.answers_of("A"), [sung("Words", false)]);
    assert_eq!(answers_in(&phone.send()), 0);
}

#[test]
fn an_answer_for_a_song_not_held_is_offered_again_only_once_its_song_arrives() {
    let mut desk = Device::new("Desk", &["A", "B", "C"]);
    let mut phone = Device::new("Phone", &[]);

    for title in ["A", "B", "C"] {
        desk.shelf.answer_here(title, sung(title, false));
    }
    sync(&mut desk, &mut phone);

    let offered_on_arrival = phone.shelf.inside.lock().answers_offered;

    phone.shelf.listen_here("Elsewhere", days_ago(0));
    phone.ledger.settle(&phone.shelf).expect("settled");

    let offered_after_a_play = phone.shelf.inside.lock().answers_offered;

    phone.shelf.add_song(song("B"), b"B".repeat(1000));
    phone.ledger.settle(&phone.shelf).expect("settled");

    let offered_after_b_arrived = phone.shelf.inside.lock().answers_offered;

    assert_eq!(offered_on_arrival, 3);
    assert_eq!(offered_after_a_play, offered_on_arrival);
    assert_eq!(offered_after_b_arrived, offered_on_arrival + 1);
    assert_eq!(phone.shelf.answers_of("B"), [sung("B", false)]);
    assert!(phone.shelf.answers_of("A").is_empty());
}

#[test]
fn an_answer_for_a_song_not_held_is_not_offered_again_by_a_ledger_opened_anew() {
    let folder = std::env::temp_dir().join(format!(
        "resonate-ledger-unheld-{}-{}",
        std::process::id(),
        days_ago(0)
    ));

    std::fs::create_dir_all(&folder).expect("a scratch folder");

    let path = folder.join("sync.sqlite");
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &[]);

    phone.ledger = Ledger::open(&path).expect("a ledger");
    phone.ledger.enter_space(SPACE).expect("a space");
    for title in ["A", "B"] {
        desk.shelf.answer_here(title, sung(title, false));
    }
    sync(&mut desk, &mut phone);
    phone.ledger.settle(&phone.shelf).expect("settled");

    let offered_before = phone.shelf.inside.lock().answers_offered;

    phone.ledger = Ledger::open(&path).expect("the ledger again");
    phone.shelf.listen_here("Elsewhere", days_ago(0));
    phone.ledger.settle(&phone.shelf).expect("settled");

    let offered = phone.shelf.inside.lock().answers_offered - offered_before;

    drop(phone);
    std::fs::remove_dir_all(&folder).expect("cleared");

    assert_eq!(offered, 0);
}

#[test]
fn a_song_received_under_the_name_an_answer_was_told_by_takes_the_answer() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &[]);

    desk.shelf.answer_here("A", sung("Words", false));
    sync(&mut desk, &mut phone);

    let own = Recording {
        mbid: Some(Mbid::new(ECHOES).expect("an mbid")),
        album: "Another Album".into(),
        ..song("A")
    };

    phone.shelf.receive_named_otherwise(song("A"), own);

    let offered_before = phone.shelf.inside.lock().answers_offered;

    phone.ledger.settle(&phone.shelf).expect("settled");

    let offered = phone.shelf.inside.lock().answers_offered - offered_before;

    assert_eq!(offered, 1);
}

#[test]
fn a_catalog_that_cannot_say_what_it_came_to_hold_is_offered_every_unheld_answer_in_pages() {
    let titles = (0..1_200)
        .map(|at| format!("Song {at}"))
        .collect::<Vec<_>>();
    let mut desk = Device::new("Desk", &[]);
    let mut phone = Device::new("Phone", &[]);

    phone.shelf.inside.lock().arrivals_untracked = true;
    for title in &titles {
        desk.shelf.answer_here(title, sung(title, false));
    }
    sync(&mut desk, &mut phone);

    let offered_on_arrival = phone.shelf.inside.lock().answers_offered;

    phone.shelf.add_song(song("Song 1100"), b"S".repeat(1000));
    phone.ledger.settle(&phone.shelf).expect("settled");

    let offered = phone.shelf.inside.lock().answers_offered - offered_on_arrival;

    assert_eq!(offered_on_arrival, titles.len());
    assert_eq!(offered, titles.len());
    assert_eq!(
        phone.shelf.answers_of("Song 1100"),
        [sung("Song 1100", false)]
    );
}

#[test]
fn songs_held_in_one_instant_past_a_whole_page_each_take_their_answer_across_a_restart() {
    let folder = scratch_folder("held-in-one-instant");
    let path = folder.join("sync.sqlite");
    let songs = (0..650)
        .map(|at| song(&format!("Tied {at}")))
        .collect::<Vec<_>>();
    let mut desk = Device::new("Desk", &[]);
    let mut phone = Device::new("Phone", &[]);

    phone.ledger = Ledger::open(&path).expect("a ledger");
    phone.ledger.enter_space(SPACE).expect("a space");
    for held in &songs {
        desk.shelf
            .answer_here(&held.title, sung(&held.title, false));
    }
    sync(&mut desk, &mut phone);
    phone.shelf.inside.lock().held_listings_before_refusing = Some(1);
    phone.shelf.add_songs_in_one_instant(&songs, days_ago(0));

    let interrupted = phone.ledger.settle(&phone.shelf);
    let answered_before_the_restart = songs
        .iter()
        .filter(|held| !phone.shelf.answers_of(&held.title).is_empty())
        .count();

    phone.ledger = Ledger::open(&path).expect("the ledger again");
    phone.shelf.inside.lock().held_listings_before_refusing = None;
    phone.ledger.settle(&phone.shelf).expect("settled");

    let unanswered = songs
        .iter()
        .filter(|held| phone.shelf.answers_of(&held.title) != [sung(&held.title, false)])
        .count();

    drop(phone);
    fs::remove_dir_all(&folder).expect("cleared");

    assert!(interrupted.is_err());
    assert_eq!(answered_before_the_restart, 500);
    assert_eq!(unanswered, 0);
}

#[test]
fn answers_kept_in_one_instant_past_a_whole_page_are_each_told_across_a_reopen() {
    let folder = scratch_folder("answered-in-one-instant");
    let path = folder.join("sync.sqlite");
    let answers = (0..650)
        .map(|nth| {
            let title = format!("Tied {nth}");

            (song(&title), sung(&title, false))
        })
        .collect::<Vec<_>>();
    let mut desk = device_over_a_file("Desk", &[], &path);

    desk.shelf.answer_here_in_one_instant(&answers, days_ago(0));
    desk.shelf.inside.lock().answer_listings_before_refusing = Some(1);

    let interrupted = desk.ledger.look(&desk.shelf, &desk.me);

    desk.ledger = Ledger::open(&path).expect("the ledger again");
    desk.shelf.inside.lock().answer_listings_before_refusing = None;

    let told = desk
        .send()
        .into_iter()
        .filter_map(|change| match change {
            Change::Answer(note) => Some(note.recording.title),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let told_again = answers_in(&desk.send());

    drop(desk);
    fs::remove_dir_all(&folder).expect("cleared");

    assert!(interrupted.is_err());
    assert_eq!(told.len(), answers.len());
    assert_eq!(told_again, 0);
}

#[test]
fn a_song_arriving_just_after_the_last_page_was_read_takes_its_answer_on_the_next_settle() {
    let mut desk = Device::new("Desk", &[]);
    let mut phone = Device::new("Phone", &[]);

    for title in ["A", "B"] {
        desk.shelf.answer_here(title, sung(title, false));
    }
    sync(&mut desk, &mut phone);
    phone.shelf.inside.lock().arriving_after_the_last_page = Some(song("B"));
    phone.shelf.add_song(song("A"), b"A".repeat(1000));
    phone.ledger.settle(&phone.shelf).expect("settled");

    let b_while_it_arrived = phone.shelf.answers_of("B");

    phone.ledger.settle(&phone.shelf).expect("settled");

    assert_eq!(phone.shelf.answers_of("A"), [sung("A", false)]);
    assert!(b_while_it_arrived.is_empty());
    assert_eq!(phone.shelf.answers_of("B"), [sung("B", false)]);
}

#[test]
fn plainer_words_told_later_never_undo_timed_ones_and_the_timed_ones_spread() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.answer_here("A", sung("[00:01.00] Words", true));

    let timed = desk.send();

    phone.shelf.answer_here("A", sung("Words", false));

    let plain = phone.send();

    desk.receive(&plain);
    phone.receive(&timed);
    sync(&mut phone, &mut desk);
    sync(&mut desk, &mut phone);

    assert_eq!(desk.shelf.answers_of("A"), [sung("[00:01.00] Words", true)]);
    assert_eq!(
        phone.shelf.answers_of("A"),
        [sung("[00:01.00] Words", true)]
    );
}

#[test]
fn a_catalog_that_takes_no_answers_passes_them_on_untouched() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);
    let mut tablet = Device::new("Tablet", &["A"]);

    phone.shelf.inside.lock().takes_no_answers = true;
    desk.shelf.answer_here("A", sung("Words", false));

    sync(&mut desk, &mut phone);

    let snapshot = phone.ledger.snapshot().expect("a snapshot");

    tablet.receive(&snapshot.into_iter().flatten().collect::<Vec<_>>());

    assert!(phone.shelf.answers_of("A").is_empty());
    assert_eq!(tablet.shelf.answers_of("A"), [sung("Words", false)]);
}

fn released(title: &str) -> Answer {
    Answer::Release(Box::new(SharedRelease {
        id: Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("an mbid"),
        group: None,
        title: title.into(),
        credit: vec![SharedCredit {
            name: "Pink Floyd".into(),
            joined_by: String::new(),
            mbid: None,
        }],
        date: Some("1971".into()),
        country: None,
        label: None,
        catalog_number: None,
        barcode: None,
        kind: None,
        disambiguation: None,
        has_front_cover: false,
        links: vec![Link::new(
            "discogs",
            "https://www.discogs.com/release/1".into(),
        )],
        media: vec![SharedMedium {
            position: 1,
            format: None,
            title: None,
            tracks: vec![SharedTrack {
                position: 1,
                number: "1".into(),
                title: "A".into(),
                artist: None,
                recording: None,
                track: None,
                length_ms: Some(1_000),
                isrcs: Vec::new(),
                links: Vec::new(),
            }],
        }],
    }))
}

#[test]
fn a_release_travels_beside_the_words_of_its_song_and_is_not_sent_back() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.answer_here("A", sung("Words", false));
    desk.shelf.answer_here("A", released("Meddle"));

    let told = desk.send();

    phone.receive(&told);

    assert_eq!(answers_in(&told), 2);
    assert_eq!(
        phone.shelf.answers_of("A"),
        [sung("Words", false), released("Meddle")]
    );
    assert_eq!(answers_in(&phone.send()), 0);
    assert_eq!(answers_in(&desk.send()), 0);
}

#[test]
fn the_peaks_of_a_study_travel_as_an_answer_of_their_own_beside_it_and_are_not_sent_back() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.answer_here("A", studied(42));
    desk.shelf.answer_here("A", peaked(42));

    let told = desk.send();

    phone.receive(&told);

    assert_eq!(answers_in(&told), 2);
    assert_eq!(phone.shelf.answers_of("A"), [studied(42), peaked(42)]);
    assert_eq!(answers_in(&phone.send()), 0);
    assert_eq!(answers_in(&desk.send()), 0);
}

#[test]
fn the_peaks_of_two_encodes_of_a_song_are_two_answers() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf.answer_here("A", peaked(42));
    desk.shelf.answer_here("A", peaked(43));

    let told = desk.send();

    phone.receive(&told);

    assert_eq!(answers_in(&told), 2);
    assert_eq!(phone.shelf.answers_of("A"), [peaked(42), peaked(43)]);
}

#[test]
fn a_release_heavier_than_the_server_keeps_stays_home() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    desk.shelf
        .answer_here("A", released(&"Meddle ".repeat(10_000)));

    let told = desk.send();

    phone.receive(&told);

    assert_eq!(answers_in(&told), 0);
    assert!(phone.shelf.answers_of("A").is_empty());
}

fn sorted_names(device: &Device) -> Vec<String> {
    let mut names = device.shelf.playlist_names();

    names.sort();
    names
}

fn playlist_bodies_told(changes: &[Change]) -> usize {
    told(
        changes,
        |change| matches!(change, Change::Playlist(note) if note.body.is_some()),
    )
}

#[test]
fn playlists_of_one_name_made_apart_after_both_devices_looked_become_one_whichever_arrives_first() {
    for desk_first in [true, false] {
        let mut desk = Device::new("Desk", &["A", "B", "C"]);
        let mut phone = Device::new("Phone", &["A", "B", "C"]);

        sync(&mut desk, &mut phone);
        sync(&mut phone, &mut desk);

        desk.shelf.make_playlist("Mix", &["A", "B"]);
        desk.look();
        phone.shelf.make_playlist("mix", &["B", "C"]);
        phone.look();
        if desk_first {
            sync(&mut desk, &mut phone);
            sync(&mut phone, &mut desk);
        } else {
            sync(&mut phone, &mut desk);
            sync(&mut desk, &mut phone);
            sync(&mut phone, &mut desk);
        }
        sync(&mut desk, &mut phone);

        assert_eq!(sorted_names(&desk), ["Mix"], "desk first: {desk_first}");
        assert_eq!(sorted_names(&phone), ["Mix"], "desk first: {desk_first}");
        assert_eq!(titles(&desk, "Mix"), ["A", "B", "C"]);
        assert_eq!(titles(&phone, "Mix"), ["A", "B", "C"]);
        assert_eq!(playlist_bodies_told(&desk.send()), 0);
        assert_eq!(playlist_bodies_told(&phone.send()), 0);
    }
}

struct Log {
    records: Vec<(usize, Vec<Change>)>,
    read: Vec<usize>,
}

impl Log {
    fn pushed(&mut self, devices: &mut [Device], from: usize) {
        let changes = devices[from].send();
        if !changes.is_empty() {
            self.records.push((from, changes));
        }
    }

    fn pulled(&mut self, devices: &mut [Device], to: usize) {
        for (from, changes) in &self.records[self.read[to]..] {
            if *from != to {
                devices[to].receive(changes);
            }
        }
        self.read[to] = self.records.len();
    }
}

#[test]
fn three_devices_each_making_one_name_apart_converge_on_one_playlist_whatever_the_order() {
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for pushing in orders {
        for pulling in orders {
            let mut devices =
                ["Desk", "Phone", "Tablet"].map(|name| Device::new(name, &["A", "B", "C", "D"]));

            let mut log = Log {
                records: Vec::new(),
                read: vec![0; devices.len()],
            };

            for at in 0..devices.len() {
                log.pushed(&mut devices, at);
            }
            for at in 0..devices.len() {
                log.pulled(&mut devices, at);
            }

            devices[0].shelf.make_playlist("Mix", &["A"]);
            devices[0].look();
            devices[1].shelf.make_playlist("Mix", &["A", "B", "C"]);
            devices[1].look();
            devices[2].shelf.make_playlist("MIX", &["D", "A"]);
            devices[2].look();
            for _ in 0..4 {
                for at in pushing {
                    log.pushed(&mut devices, at);
                }
                for at in pulling {
                    log.pulled(&mut devices, at);
                }
            }

            let first = titles(&devices[0], "Mix");
            let mut held = first.clone();

            held.sort();

            assert_eq!(held, ["A", "B", "C", "D"], "{pushing:?} {pulling:?}");
            for device in &devices {
                assert_eq!(sorted_names(device), ["Mix"], "{pushing:?} {pulling:?}");
                assert_eq!(titles(device, "Mix"), first, "{pushing:?} {pulling:?}");
            }
        }
    }
}

#[test]
fn a_playlist_made_beside_a_namesake_already_known_stays_apart_as_an_alias() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    desk.shelf.make_playlist("Mix", &["A"]);
    sync(&mut desk, &mut phone);

    phone.shelf.make_playlist("Mix", &["B"]);
    sync(&mut phone, &mut desk);
    sync(&mut desk, &mut phone);
    desk.shelf.make_playlist("Mix", &["A", "B"]);
    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    assert_eq!(sorted_names(&desk), ["Mix", "Mix", "Mix (2)"]);
    assert_eq!(sorted_names(&phone), ["Mix", "Mix", "Mix (2)"]);
}

#[test]
fn an_aliased_arrival_cut_off_between_the_catalog_and_the_ledger_is_taken_up_and_not_minted_again()
{
    let mut desk = Device::new("Desk", &["A", "Z"]);
    let mut phone = Device::new("Phone", &["A"]);

    phone.shelf.receive_named_otherwise(song("Z"), song("Zed"));
    desk.shelf.make_playlist("Mix", &["A"]);
    sync(&mut desk, &mut phone);
    desk.shelf.make_playlist("Mix", &["A", "Z"]);

    let told = desk.send();

    phone.shelf.inside.lock().refuses_resolving = true;

    assert!(phone.ledger.arrived(&phone.shelf, &told).is_err());

    phone.shelf.inside.lock().refuses_resolving = false;
    phone.receive(&told);

    let echoed = phone.send();

    assert_eq!(sorted_names(&phone), ["Mix", "Mix (2)"]);
    assert_eq!(titles(&phone, "Mix (2)"), ["A", "Zed"]);
    assert_eq!(playlist_bodies_told(&echoed), 0);
}

#[test]
fn a_saved_query_once_unwritable_here_folds_the_namesake_made_meanwhile_into_it() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);
    let top = desk.shelf.make_query("Top", "plays:>5");

    desk.shelf
        .inside
        .lock()
        .queries
        .get_mut(&top)
        .expect("a query")
        .1
        .sort = 9;
    desk.look();
    phone.shelf.inside.lock().unwritable_sorts_from = Some(9);
    phone.shelf.make_query("Top", "plays:>1");
    phone.look();
    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    assert_eq!(phone.shelf.query_names(), ["Top"]);

    phone.shelf.inside.lock().unwritable_sorts_from = None;
    phone.ledger.settle(&phone.shelf).expect("settled");
    sync(&mut phone, &mut desk);
    sync(&mut desk, &mut phone);

    assert_eq!(phone.shelf.query_names(), ["Top"]);
    assert_eq!(desk.shelf.query_names(), ["Top"]);
    assert_eq!(
        phone.shelf.query_named("Top").map(|(_, text)| text),
        Some(Some("plays:>5".to_owned()))
    );
}

#[test]
fn saved_queries_of_one_name_made_apart_after_both_devices_looked_become_one() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    desk.shelf.make_query("Recent", "added:<30d");
    desk.look();
    phone.shelf.make_query("Recent", "plays:0");
    phone.look();
    sync(&mut phone, &mut desk);
    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    assert_eq!(desk.shelf.query_names(), ["Recent"]);
    assert_eq!(phone.shelf.query_names(), ["Recent"]);
    assert_eq!(
        phone.shelf.query_named("Recent").map(|(_, text)| text),
        Some(Some("added:<30d".to_owned()))
    );
}

#[test]
fn a_playlist_deleted_and_put_back_after_its_look_keeps_its_key_and_its_songs_kept_here() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);
    let mix = desk.shelf.make_playlist("Mix", &["A"]);

    sync(&mut desk, &mut phone);

    let at = LocalPlaylistId {
        id: mix,
        created: mix * 1000,
    };

    assert!(desk.ledger.keep_songs_of(at, true).expect("kept"));

    desk.shelf.drop_playlist(mix);
    desk.look();
    desk.shelf
        .inside
        .lock()
        .playlists
        .insert(mix, (mix * 1000, "Mix".into(), vec![song("A")]));
    desk.look();

    let (on_phone, _) = phone.shelf.playlist_named("Mix").expect("arrived");

    phone.shelf.edit_playlist(on_phone, &["A", "B"]);
    sync(&mut phone, &mut desk);
    sync(&mut desk, &mut phone);

    assert_eq!(desk.shelf.playlist_names(), ["Mix"]);
    assert_eq!(phone.shelf.playlist_names(), ["Mix"]);
    assert_eq!(titles(&desk, "Mix"), ["A", "B"]);
    assert!(desk.ledger.kept_songs_of(at).expect("read"));
}

fn a_share(at: Option<LocalPlaylistId>) -> resonate_sync::HeldShare {
    resonate_sync::HeldShare {
        code: "E-CODE".to_owned(),
        server: None,
        member: [1; 16],
        personal: [2; 32],
        at,
    }
}

fn hidden_as_shared(device: &Device, name: &str) -> LocalPlaylistId {
    let mut inside = device.shelf.inside.lock();

    let (id, (created, ..)) = inside
        .playlists
        .iter()
        .find(|(_, (_, held, _))| held == name)
        .expect("the playlist stands");

    let at = LocalPlaylistId {
        id: *id,
        created: *created,
    };

    inside.hidden_as_shared.push(at);

    at
}

#[test]
fn sharing_a_playlist_tells_no_other_device_to_delete_it_and_a_later_note_writes_nothing_here() {
    let mut desk = Device::new("Desk", &["One"]);
    let mut phone = Device::new("Phone", &["One"]);

    desk.shelf.make_playlist("Mix", &["One"]);
    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    assert!(phone.shelf.playlist_named("Mix").is_some());

    hidden_as_shared(&desk, "Mix");

    let told = desk.send();

    assert!(
        !told
            .iter()
            .any(|change| matches!(change, Change::Playlist(_))),
        "sharing a playlist told the other devices something of it: {told:?}"
    );

    phone.receive(&told);

    assert!(phone.shelf.playlist_named("Mix").is_some());

    let renamed = {
        let inside = phone.shelf.inside.lock();
        *inside.playlists.keys().next().expect("a playlist")
    };

    phone
        .shelf
        .inside
        .lock()
        .playlists
        .get_mut(&renamed)
        .expect("a playlist")
        .1 = "Mix edited on the phone".to_owned();

    let lists_before = desk.shelf.inside.lock().playlists.len();

    sync(&mut phone, &mut desk);

    assert_eq!(
        desk.shelf.inside.lock().playlists.len(),
        lists_before,
        "a note of the list given up here wrote a second copy"
    );
}

#[test]
fn a_share_reaches_this_listeners_other_device_which_takes_its_own_copy_over() {
    let mut desk = Device::new("Desk", &["One"]);
    let mut phone = Device::new("Phone", &["One"]);

    desk.shelf.make_playlist("Mix", &["One"]);
    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);

    let at = hidden_as_shared(&desk, "Mix");

    desk.shelf.inside.lock().shares = Some(vec![a_share(Some(at))]);
    sync(&mut desk, &mut phone);

    let taken = phone.shelf.inside.lock().shares_taken.clone();

    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].code, "E-CODE");
    assert_eq!(
        taken[0].personal, [2; 32],
        "this listener's other device is the same person"
    );

    let own = phone
        .shelf
        .playlist_named("Mix")
        .expect("the phone's copy stands");

    assert_eq!(taken[0].at.map(|at| at.id), Some(own.0));
}

#[test]
fn a_share_left_on_one_device_is_left_on_the_others_and_nothing_known_says_nothing() {
    let mut desk = Device::new("Desk", &[]);
    let mut phone = Device::new("Phone", &[]);

    desk.shelf.inside.lock().shares = Some(vec![a_share(None)]);
    sync(&mut desk, &mut phone);

    assert_eq!(phone.shelf.inside.lock().shares_taken.len(), 1);

    desk.shelf.inside.lock().shares = None;
    sync(&mut desk, &mut phone);

    assert!(
        phone.shelf.inside.lock().shares_dropped.is_empty(),
        "a device whose shared playlists are not running took them all as left"
    );

    desk.shelf.inside.lock().shares = Some(Vec::new());
    sync(&mut desk, &mut phone);

    assert_eq!(
        phone.shelf.inside.lock().shares_dropped.as_slice(),
        ["E-CODE".to_owned()]
    );
}

#[test]
fn a_compaction_through_less_than_was_read_still_counts_the_records_read_past_it() {
    let mut caught_up = Ledger::in_memory().expect("a ledger");
    let mut fresh = Ledger::in_memory().expect("a ledger");

    for seq in 1..=10 {
        caught_up.passed(seq, 100).expect("passed");
    }
    caught_up.compacted(6).expect("compacted");

    let past_through = caught_up.since_snapshot().expect("counted");

    caught_up.compacted(10).expect("compacted");

    let at_the_cursor = caught_up.since_snapshot().expect("counted");

    fresh.compacted(40).expect("compacted");

    let from_a_snapshot = fresh.since_snapshot().expect("counted");

    assert_eq!(past_through, (4, 400));
    assert_eq!(at_the_cursor, (0, 0));
    assert_eq!(caught_up.cursor().expect("the cursor"), 10);
    assert_eq!(from_a_snapshot, (0, 0));
    assert_eq!(fresh.cursor().expect("the cursor"), 40);
}

#[test]
fn a_share_is_told_once_and_a_compaction_elsewhere_owes_it_again() {
    let mut desk = Device::new("Desk", &[]);

    desk.shelf.inside.lock().shares = Some(vec![a_share(None)]);

    assert_eq!(
        desk.send()
            .iter()
            .filter(|change| matches!(change, Change::Share(_)))
            .count(),
        1
    );
    assert!(
        desk.send().is_empty()
            || !desk
                .send()
                .iter()
                .any(|change| matches!(change, Change::Share(_)))
    );

    desk.ledger.owe_queries().expect("owed");

    assert!(
        desk.send()
            .iter()
            .any(|change| matches!(change, Change::Share(_))),
        "a build without shares compacted them away and they were not told again"
    );
}

fn shares_told(changes: &[Change]) -> usize {
    changes
        .iter()
        .filter(|change| matches!(change, Change::Share(_)))
        .count()
}

#[test]
fn a_share_is_told_again_in_another_space_and_reaches_a_device_met_there() {
    let mut desk = Device::new("Desk", &[]);

    desk.shelf.inside.lock().shares = Some(vec![a_share(None)]);
    desk.send();

    let mut phone = Device::new("Phone", &[]);
    let elsewhere = SpaceId([6; 16]);

    phone.ledger.enter_space(elsewhere).expect("entered");

    desk.ledger.enter_space(elsewhere).expect("entered");

    let told = desk.send();

    phone.receive(&told);

    assert_eq!(shares_told(&told), 1);
    assert_eq!(
        phone
            .shelf
            .inside
            .lock()
            .shares_taken
            .iter()
            .map(|share| share.code.as_str())
            .collect::<Vec<_>>(),
        ["E-CODE"]
    );
}

#[test]
fn a_share_is_owed_again_when_the_space_is_made_anew_and_reaches_a_device_joining_it() {
    let mut desk = Device::new("Desk", &[]);

    desk.shelf.inside.lock().shares = Some(vec![a_share(None)]);
    desk.send();

    desk.ledger.space_made_anew().expect("made anew");

    let told = desk.send();
    let mut phone = Device::new("Phone", &[]);

    phone.receive(&told);

    assert_eq!(shares_told(&told), 1);
    assert_eq!(phone.shelf.inside.lock().shares_taken.len(), 1);
}

#[test]
fn a_share_told_without_a_server_is_owed_again_once_there_is_one() {
    let mut desk = Device::new("Desk", &[]);

    desk.shelf.inside.lock().shares = Some(vec![a_share(None)]);
    desk.ledger.through_a_server(false).expect("serverless");
    desk.send();

    desk.ledger.through_a_server(true).expect("a server");

    let told = desk.send();

    assert_eq!(shares_told(&told), 1);
}

#[test]
fn an_owed_share_is_counted_among_what_is_owed() {
    let mut desk = Device::new("Desk", &[]);

    desk.shelf.inside.lock().shares = Some(vec![a_share(None)]);

    desk.look();

    let counted = desk.ledger.owed_count().expect("owed");
    let owed = desk.ledger.owed().expect("owed").concat();

    assert_eq!(shares_told(&owed), 1);
    assert_eq!(counted, owed.len());
}

fn following_playlists(device: &mut Device, name: &str) -> Follow {
    let (id, _) = device.shelf.playlist_named(name).expect("a playlist");

    device
        .ledger
        .keep_songs_of(
            LocalPlaylistId {
                id,
                created: id * 1000,
            },
            true,
        )
        .expect("kept");
    Follow {
        playlists: true,
        ..Follow::default()
    }
}

fn wanted_titles(device: &Device, follow: Follow) -> Vec<String> {
    let mut titles = device
        .ledger
        .wanted(follow, &device.shelf)
        .expect("wanted")
        .into_iter()
        .map(|recording| recording.title)
        .collect::<Vec<_>>();

    titles.sort();
    titles
}

fn deletions(changes: &[Change]) -> Vec<(String, bool)> {
    changes
        .iter()
        .filter_map(|change| match change {
            Change::Deletion(note) => Some((note.recording.title.clone(), note.deleted)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_song_deleted_from_every_device_leaves_the_other_and_is_not_followed_back() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);
    sync(&mut desk, &mut phone);

    let follow = following_playlists(&mut phone, "Summer");

    desk.shelf.delete_everywhere("A");

    let told = desk.send();

    phone.receive(&told);

    assert_eq!(deletions(&told), [("A".to_owned(), true)]);
    assert!(desk.shelf.inside.lock().to_tell.is_empty());
    assert_eq!(phone.shelf.song_titles(), ["B"]);
    assert!(wanted_titles(&phone, follow).is_empty());
    assert!(phone.ledger.deleted_everywhere(&song("A")).expect("asked"));
    assert!(deletions(&phone.send()).is_empty());
    assert!(deletions(&desk.send()).is_empty());
}

#[test]
fn a_song_deleted_only_here_tells_no_other_device() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    sync(&mut desk, &mut phone);

    desk.shelf.inside.lock().deleted_by_hand = vec!["A".into()];
    desk.shelf.inside.lock().deleted_here = vec!["A".into()];
    desk.shelf.moved();

    let told = desk.send();

    phone.receive(&told);

    assert!(deletions(&told).is_empty());
    assert_eq!(phone.shelf.song_titles(), ["A", "B"]);
    assert!(!phone.ledger.deleted_everywhere(&song("A")).expect("asked"));
}

#[test]
fn a_device_refusing_deletions_from_others_keeps_the_song_and_no_device_follows_it_back() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);
    let mut laptop = Device::new("Laptop", &["B"]);

    phone.shelf.inside.lock().refuses_deletions = true;
    desk.shelf.make_playlist("Summer", &["A", "B"]);
    sync(&mut desk, &mut phone);
    laptop.receive(&desk.ledger.snapshot().expect("a snapshot").concat());

    let desk_follows = following_playlists(&mut desk, "Summer");
    let laptop_follows = following_playlists(&mut laptop, "Summer");

    assert_eq!(wanted_titles(&laptop, laptop_follows), ["A"]);

    desk.shelf.delete_everywhere("A");

    let told = desk.send();

    phone.receive(&told);
    laptop.receive(&told);

    assert_eq!(phone.shelf.song_titles(), ["A", "B"]);
    assert!(
        phone
            .ledger
            .deleted_everywhere(&song("A"))
            .expect("recorded")
    );
    assert!(deletions(&phone.send()).is_empty());
    assert!(wanted_titles(&desk, desk_follows).is_empty());
    assert!(wanted_titles(&laptop, laptop_follows).is_empty());
}

#[test]
fn a_song_added_again_after_its_deletion_is_never_deleted_again_and_comes_back() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);
    let mut laptop = Device::new("Laptop", &["B"]);

    desk.shelf.make_playlist("Summer", &["A", "B"]);
    sync(&mut desk, &mut phone);
    laptop.receive(&desk.ledger.snapshot().expect("a snapshot").concat());

    let follow = following_playlists(&mut phone, "Summer");

    desk.shelf.delete_everywhere("A");

    let told = desk.send();

    phone.receive(&told);
    laptop.shelf.add_song(song("A"), b"A".repeat(1000));
    laptop.receive(&told);

    assert_eq!(phone.shelf.song_titles(), ["B"]);
    assert_eq!(laptop.shelf.song_titles(), ["A", "B"]);
    assert!(deletions(&laptop.send()).is_empty());

    desk.shelf.add_song(song("A"), b"A".repeat(1000));

    let answered = desk.send();

    phone.receive(&answered);
    phone.receive(&told);
    desk.receive(&told);

    assert_eq!(deletions(&answered), [("A".to_owned(), false)]);
    assert_eq!(desk.shelf.song_titles(), ["A", "B"]);
    assert!(!phone.ledger.deleted_everywhere(&song("A")).expect("asked"));
    assert!(phone.shelf.inside.lock().deleted_here.is_empty());
    assert_eq!(wanted_titles(&phone, follow), ["A"]);
}

#[test]
fn a_song_added_by_hand_after_a_deletion_stamped_ahead_arrived_stays_and_answers_it_everywhere() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    sync(&mut desk, &mut phone);
    sync(&mut phone, &mut desk);
    desk.shelf.delete_everywhere("A");

    let told = stamped_ahead(desk.send(), A_DAY_IN_NANOS / 2);

    phone.shelf.inside.lock().deletions_wait = true;
    phone.receive(&told);
    thread::sleep(Duration::from_millis(5));
    phone.shelf.add_song(song("A"), b"A".repeat(1000));
    phone.shelf.inside.lock().deletions_wait = false;
    phone.ledger.settle(&phone.shelf).expect("settled");

    let kept = phone.shelf.song_titles();
    let answered = phone.send();

    desk.receive(&answered);

    assert_eq!(kept, ["A", "B"]);
    assert_eq!(phone.shelf.inside.lock().arrivals.len(), 2);
    assert_eq!(deletions(&answered), [("A".to_owned(), false)]);
    assert!(!phone.ledger.deleted_everywhere(&song("A")).expect("asked"));
    assert!(!desk.ledger.deleted_everywhere(&song("A")).expect("asked"));
}

#[test]
fn a_device_paired_after_a_deletion_keeps_the_songs_it_held_and_tells_none_back() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    sync(&mut desk, &mut phone);
    desk.shelf.delete_everywhere("A");
    sync(&mut desk, &mut phone);

    let mut laptop = Device::new("Laptop", &["A", "B"]);

    laptop.receive(&desk.ledger.snapshot().expect("a snapshot").concat());

    assert_eq!(phone.shelf.song_titles(), ["B"]);
    assert_eq!(laptop.shelf.song_titles(), ["A", "B"]);
    assert!(
        laptop
            .ledger
            .deleted_everywhere(&song("A"))
            .expect("recorded")
    );
    assert!(deletions(&laptop.send()).is_empty());
}

#[test]
fn a_deletion_the_trash_refuses_keeps_the_song_and_is_not_tried_again() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = Device::new("Phone", &["A", "B"]);

    sync(&mut desk, &mut phone);
    phone.shelf.inside.lock().trash_refuses = true;

    desk.shelf.delete_everywhere("A");
    sync(&mut desk, &mut phone);
    phone.shelf.inside.lock().trash_refuses = false;
    phone.ledger.settle(&phone.shelf).expect("settled");

    assert_eq!(phone.shelf.song_titles(), ["A", "B"]);
    assert!(deletions(&phone.send()).is_empty());
}

fn waiting_removal(device: &Device) -> Vec<String> {
    device
        .shelf
        .inside
        .lock()
        .waiting_removal
        .iter()
        .map(|waiting| waiting.title.clone())
        .collect()
}

fn deletions_taken(device: &Device) -> usize {
    device.shelf.inside.lock().deletions_taken
}

fn told_a_deletion_the_platform_removes(desk: &mut Device, device: &mut Device) {
    device.shelf.inside.lock().platform_removes = true;
    sync(desk, device);
    sync(device, desk);
    desk.shelf.delete_everywhere("A");

    let told = desk.send();

    device.receive(&told);
}

#[test]
fn a_deletion_waiting_for_the_platform_stands_over_a_reopen_and_settles_once_the_file_is_gone() {
    let folder = scratch_folder("waiting-removal");
    let path = folder.join("sync.sqlite");
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut phone = device_over_a_file("Phone", &["A", "B"], &path);

    told_a_deletion_the_platform_removes(&mut desk, &mut phone);

    let queued = waiting_removal(&phone);

    phone.ledger.settle(&phone.shelf).expect("settled");
    phone.ledger = Ledger::open(&path).expect("the ledger opened again");
    phone.ledger.settle(&phone.shelf).expect("settled");

    let still_queued = waiting_removal(&phone);
    let weighed_while_waiting = deletions_taken(&phone);

    phone.shelf.remove_waiting();
    phone.ledger.settle(&phone.shelf).expect("settled");

    let weighed_once_removed = deletions_taken(&phone);

    phone.ledger.settle(&phone.shelf).expect("settled");
    phone.ledger = Ledger::open(&path).expect("the ledger opened again");
    phone.ledger.settle(&phone.shelf).expect("settled");

    let weighed_after = deletions_taken(&phone);
    let told_back = deletions(&phone.send());
    let titles = phone.shelf.song_titles();

    drop(phone);
    fs::remove_dir_all(&folder).expect("cleared");

    assert_eq!(queued, ["A"]);
    assert_eq!(still_queued, ["A"]);
    assert_eq!(weighed_while_waiting, 3);
    assert_eq!(weighed_once_removed, 4);
    assert_eq!(weighed_after, 4);
    assert_eq!(titles, ["B"]);
    assert!(told_back.is_empty());
}

#[test]
fn a_removal_waiting_for_the_platform_is_withdrawn_by_a_newer_answer_from_anywhere() {
    let mut desk = Device::new("Desk", &["A", "B"]);
    let mut answered_there = Device::new("Phone", &["A", "B"]);
    let mut added_here = Device::new("Tablet", &["A", "B"]);
    let mut asked_here = Device::new("Laptop", &["A", "B"]);

    told_a_deletion_the_platform_removes(&mut desk, &mut answered_there);
    told_a_deletion_the_platform_removes(&mut desk, &mut added_here);
    told_a_deletion_the_platform_removes(&mut desk, &mut asked_here);

    let queued = [
        waiting_removal(&answered_there),
        waiting_removal(&added_here),
        waiting_removal(&asked_here),
    ];

    desk.shelf.add_song(song("A"), b"A".repeat(1000));

    let answered = desk.send();

    answered_there.receive(&answered);
    thread::sleep(Duration::from_millis(5));
    added_here.shelf.add_song(song("A"), b"A".repeat(1000));

    let told_by_the_listener = deletions(&added_here.send());
    let asked = asked_here
        .ledger
        .asked_back(&asked_here.shelf, &song("A"))
        .expect("answered");

    for device in [&mut answered_there, &mut added_here, &mut asked_here] {
        device.ledger.settle(&device.shelf).expect("settled");
    }

    assert_eq!(queued, [["A"], ["A"], ["A"]]);
    assert_eq!(deletions(&answered), [("A".to_owned(), false)]);
    assert_eq!(told_by_the_listener, [("A".to_owned(), false)]);
    assert!(asked);
    for device in [&answered_there, &added_here, &asked_here] {
        assert!(waiting_removal(device).is_empty());
        assert!(device.shelf.song_titles().contains(&"A".to_owned()));
        assert!(!device.ledger.deleted_everywhere(&song("A")).expect("asked"));
    }
}

#[test]
fn a_song_asked_for_after_its_deletion_answers_the_deletion_for_every_device() {
    let mut desk = Device::new("Desk", &["A"]);
    let mut phone = Device::new("Phone", &["A"]);

    sync(&mut desk, &mut phone);
    desk.shelf.delete_everywhere("A");
    sync(&mut desk, &mut phone);

    assert!(
        phone
            .ledger
            .asked_back(&phone.shelf, &song("A"))
            .expect("answered")
    );

    let answered = phone.send();

    desk.receive(&answered);

    assert_eq!(deletions(&answered), [("A".to_owned(), false)]);
    assert!(!desk.ledger.deleted_everywhere(&song("A")).expect("asked"));
    assert!(desk.shelf.inside.lock().deleted_here.is_empty());
    assert!(
        !phone
            .ledger
            .asked_back(&phone.shelf, &song("A"))
            .expect("nothing standing")
    );
}

const KEPT: Incarnation = Incarnation(11);
const ANOTHER: Incarnation = Incarnation(12);
const CURSOR: u64 = 5;

fn welcomed_once_sent_through(cursor: u64) -> Device {
    let mut desk = Device::new("Desk", &["A"]);

    desk.shelf.make_playlist("Before", &["A"]);
    desk.send();
    desk.ledger.passed(cursor, 0).expect("passed");

    let first = desk.ledger.welcomed(cursor, Some(KEPT)).expect("welcomed");

    assert_eq!(first, Welcomed::Continued);
    desk
}

#[test]
fn a_space_of_another_incarnation_is_made_anew_though_its_head_is_past_the_cursor() {
    let mut desk = welcomed_once_sent_through(CURSOR);

    let owed_before = desk.ledger.owed_count().expect("owed");
    let told_another = desk
        .ledger
        .welcomed(CURSOR + 4, Some(ANOTHER))
        .expect("welcomed");
    let cursor = desk.ledger.cursor().expect("the cursor");
    let owed = desk.ledger.owed_count().expect("owed");
    let learnt = desk.ledger.incarnation().expect("read");
    let told_again = desk
        .ledger
        .welcomed(CURSOR + 4, Some(ANOTHER))
        .expect("welcomed");
    let told = desk.send();

    assert_eq!(owed_before, 0);
    assert_eq!(told_another, Welcomed::MadeAnew);
    assert_eq!(cursor, 0);
    assert!(owed > 0);
    assert_eq!(learnt, Some(ANOTHER));
    assert_eq!(told_again, Welcomed::Continued);
    assert!(told.iter().any(|change| matches!(
        change,
        Change::Playlist(note) if note.body.as_ref().is_some_and(|body| body.name == "Before")
    )));
}

#[test]
fn a_server_that_tells_no_incarnation_is_judged_by_its_head_alone_and_the_one_kept_stays() {
    let mut desk = welcomed_once_sent_through(CURSOR);

    let at_the_cursor = desk.ledger.welcomed(CURSOR, None).expect("welcomed");
    let kept = desk.ledger.incarnation().expect("read");
    let below_the_cursor = desk.ledger.welcomed(CURSOR - 1, None).expect("welcomed");
    let cursor = desk.ledger.cursor().expect("the cursor");

    assert_eq!(at_the_cursor, Welcomed::Continued);
    assert_eq!(kept, Some(KEPT));
    assert_eq!(below_the_cursor, Welcomed::MadeAnew);
    assert_eq!(cursor, 0);
}

#[test]
fn a_space_of_the_same_incarnation_whose_head_fell_below_the_cursor_is_made_anew() {
    let mut desk = welcomed_once_sent_through(CURSOR);

    let fallen = desk
        .ledger
        .welcomed(CURSOR - 1, Some(KEPT))
        .expect("welcomed");

    assert_eq!(fallen, Welcomed::MadeAnew);
    assert!(desk.ledger.owed_count().expect("owed") > 0);
}

#[test]
fn entering_another_space_forgets_the_incarnation_of_the_last() {
    let mut desk = welcomed_once_sent_through(CURSOR);

    desk.ledger
        .enter_space(SpaceId([6; 16]))
        .expect("another space");

    let forgotten = desk.ledger.incarnation().expect("read");
    let first = desk.ledger.welcomed(0, Some(ANOTHER)).expect("welcomed");

    assert_eq!(forgotten, None);
    assert_eq!(first, Welcomed::Continued);
}
