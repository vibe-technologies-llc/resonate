mod shelf;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::{Mbid, Recording};
use resonate_sync::{Change, Follow, Ledger, LocalListen, LocalPlaylistId, Me, Platform, SpaceId};

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
