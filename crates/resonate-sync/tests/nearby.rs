mod shelf;

use std::{
    env, fs,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use resonate_core::{
    Chromaprint, Mbid, SharedHeard, SharedLyricsfile, SharedPrint, SharedRecognition, WholeFile,
};
use resonate_sync::{
    Broadcast, DeviceId, Follow, Me, Nearby, Platform, Server, Settings, Standing, Sync, SyncKey,
    TransferState,
};

use crate::shelf::{Shelf, song};

const SETTLES_WITHIN: Duration = Duration::from_secs(40);
const NO_SERVER_HERE: &str = "127.0.0.1:9";

fn scratch(name: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!(
        "resonate-nearby-{name}-{}-{:?}",
        std::process::id(),
        Instant::now()
    ));
    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn device(
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    told: Option<&Sync>,
) -> Sync {
    device_following(folder, key, name, shelf, told, Follow::default())
}

fn device_following(
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    told: Option<&Sync>,
    follow: Follow,
) -> Sync {
    device_through(
        folder,
        key,
        name,
        shelf,
        told,
        follow,
        Server::At(NO_SERVER_HERE.into()),
    )
}

fn device_through(
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    told: Option<&Sync>,
    follow: Follow,
    server: Server,
) -> Sync {
    let told = told
        .and_then(Sync::nearby_port)
        .map(|port| SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .into_iter()
        .collect();
    started_through(
        folder,
        key,
        name,
        shelf,
        Nearby {
            broadcast: Some(Broadcast { port: 0, told }),
            bonjour: false,
        },
        follow,
        server,
    )
}

fn device_told(
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    told: &[&Sync],
    follow: Follow,
) -> Sync {
    let told = told
        .iter()
        .filter_map(|sync| sync.nearby_port())
        .map(|port| SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .collect();
    started_through(
        folder,
        key,
        name,
        shelf,
        Nearby {
            broadcast: Some(Broadcast { port: 0, told }),
            bonjour: false,
        },
        follow,
        Server::At(NO_SERVER_HERE.into()),
    )
}

fn following_favourites() -> Follow {
    Follow {
        favourites: true,
        ..Follow::default()
    }
}

fn rows_from(sync: &Sync, from: DeviceId) -> Vec<(String, TransferState)> {
    sync.shown()
        .transfers
        .into_iter()
        .filter(|transfer| transfer.from == from)
        .map(|transfer| (transfer.recording.title, transfer.state))
        .collect()
}

fn device_over_ipv6(
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    told: Option<&Sync>,
) -> Sync {
    let told = told
        .and_then(Sync::nearby_port)
        .map(|port| SocketAddr::from((Ipv6Addr::LOCALHOST, port)))
        .into_iter()
        .collect();
    started(
        folder,
        key,
        name,
        shelf,
        Nearby {
            broadcast: Some(Broadcast { port: 0, told }),
            bonjour: false,
        },
    )
}

fn ipv6_loopback_here() -> bool {
    UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).is_ok()
}

fn started(folder: &Path, key: &SyncKey, name: &str, shelf: Arc<Shelf>, nearby: Nearby) -> Sync {
    started_following(folder, key, name, shelf, nearby, Follow::default())
}

fn started_following(
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    nearby: Nearby,
    follow: Follow,
) -> Sync {
    started_through(
        folder,
        key,
        name,
        shelf,
        nearby,
        follow,
        Server::At(NO_SERVER_HERE.into()),
    )
}

fn started_through(
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    nearby: Nearby,
    follow: Follow,
    server: Server,
) -> Sync {
    let settings = Settings {
        server,
        trusted: None,
        stun: None,
        key: key.clone(),
        me: Me {
            name: name.into(),
            platform: Platform::Linux,
        },
        follow,
        ledger: folder.join(format!("{name}.sqlite")),
        staging: folder.join(format!("{name}-staging")),
        transfers_allowed: true,
        relay_only: false,
        nearby: Some(nearby),
    };
    Sync::start(settings, shelf).expect("started")
}

fn eventually(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + SETTLES_WITHIN;
    while !done() {
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(100));
    }
}

fn on_a_network() -> bool {
    if_addrs::get_if_addrs().is_ok_and(|interfaces| {
        interfaces
            .iter()
            .any(|interface| !interface.is_loopback() && interface.ip().is_ipv4())
    })
}

fn nearby_of(sync: &Sync, name: &str) -> bool {
    sync.shown()
        .devices
        .iter()
        .any(|seen| seen.name == name && seen.nearby)
}

fn holds_title(shelf: &Shelf, title: &str) -> bool {
    shelf
        .inside
        .lock()
        .songs
        .iter()
        .any(|(recording, _)| recording.title == title)
}

#[test]
fn two_devices_of_one_key_on_one_network_sync_without_a_server() {
    let folder = scratch("pair");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));
    desk_shelf.make_playlist("Summer", &["A", "B"]);
    phone_shelf.make_playlist("Commute", &["A"]);
    desk_shelf.favour_here("B", true);

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
    );

    eventually("the desk's playlist reaching the phone", || {
        phone_shelf.playlist_named("Summer").is_some()
    });
    eventually("the phone's playlist reaching the desk", || {
        desk_shelf.playlist_named("Commute").is_some()
    });
    eventually("the desk seeing the phone nearby", || {
        desk.shown()
            .devices
            .iter()
            .any(|seen| seen.name == "Phone" && seen.nearby && seen.online)
    });
    assert_ne!(phone.shown().standing, Standing::Connected);

    phone.fetch(vec![song("B")], desk.device());
    eventually("the song arriving over the network", || {
        holds_title(&phone_shelf, "B")
    });
    eventually("the playlist taking the song in", || {
        phone_shelf.playlist_named("Summer").expect("held").1 == ["A", "B"]
    });
    eventually("the favourite taking the song in", || {
        phone_shelf.favourite_titles() == ["B"]
    });

    let (on_phone, _) = phone_shelf.playlist_named("Summer").expect("held");
    phone_shelf.rename_playlist(on_phone, "Summer days");
    eventually("the rename reaching the desk", || {
        desk_shelf.playlist_named("Summer days").is_some()
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_cover_one_device_holds_reaches_another_lacking_it_and_one_neither_holds_stays_missing() {
    let folder = scratch("covers");
    let key = SyncKey::minted().expect("a key");
    let held = Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("mbid");
    let lacking = Mbid::new("a5bdf1b3-0cd8-3a1e-9e3a-0a4b1a4c4f0e").expect("mbid");
    let picture = vec![0xff, 0xd8, 0xff, 0xe0, 7, 7, 7];
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));
    desk_shelf
        .inside
        .lock()
        .covers
        .insert(held.as_str().to_owned(), picture.clone());
    phone_shelf.inside.lock().uncovered = vec![held.clone(), lacking.clone()];

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
    );

    eventually("the cover reaching the phone", || {
        phone_shelf.inside.lock().covers.get(held.as_str()) == Some(&picture)
    });
    assert!(
        !phone_shelf
            .inside
            .lock()
            .covers
            .contains_key(lacking.as_str())
    );
    assert!(
        !desk_shelf
            .inside
            .lock()
            .covers
            .contains_key(lacking.as_str())
    );

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_lyricsfile_one_device_holds_reaches_another_lacking_it_and_one_neither_holds_stays_missing() {
    let folder = scratch("lyricsfiles");
    let key = SyncKey::minted().expect("a key");
    let held = song("Echoes");
    let lacking = song("Time");
    let lyricsfile = SharedLyricsfile {
        text: "[00:01.00]Overhead the albatross".into(),
        synced: true,
        document: "version: '1.0'".into(),
    };
    let desk_shelf = Arc::new(Shelf::holding(&["Echoes", "Time"]));
    let phone_shelf = Arc::new(Shelf::holding(&["Echoes", "Time"]));
    desk_shelf
        .inside
        .lock()
        .lyricsfiles
        .push((held.clone(), lyricsfile.clone()));
    phone_shelf.inside.lock().lacking_lyricsfiles = vec![held.clone(), lacking.clone()];

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
    );

    eventually("the lyricsfile reaching the phone", || {
        phone_shelf
            .inside
            .lock()
            .lyricsfiles
            .iter()
            .any(|(recording, told)| recording.title == held.title && *told == lyricsfile)
    });
    assert!(
        !phone_shelf
            .inside
            .lock()
            .lyricsfiles
            .iter()
            .any(|(recording, _)| recording.title == lacking.title)
    );
    assert_eq!(desk_shelf.inside.lock().lyricsfiles.len(), 1);

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_print_and_its_recognition_one_device_holds_reach_another_lacking_them_and_one_neither_holds_stays_missing()
 {
    let folder = scratch("prints");
    let key = SyncKey::minted().expect("a key");
    let held = WholeFile {
        packets: 0xfeed,
        frames: 8_000_000,
        rate: 44_100,
    };
    let lacking = WholeFile {
        packets: 0xbeef,
        frames: 9_000_000,
        rate: 48_000,
    };
    let print = SharedPrint {
        print: Chromaprint::new("AQADtEmSJEmSJEmS", Duration::from_secs(181)).expect("a print"),
        recognition: Some(SharedRecognition::Heard(SharedHeard {
            recording: Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("mbid"),
            score: 96,
            title: "Echoes".into(),
            artist: Some("Pink Floyd".into()),
        })),
    };
    let desk_shelf = Arc::new(Shelf::holding(&["Echoes"]));
    let phone_shelf = Arc::new(Shelf::holding(&["Echoes"]));
    desk_shelf.inside.lock().prints.push((held, print.clone()));
    phone_shelf.inside.lock().lacking_prints = vec![held, lacking];

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
    );

    eventually("the print reaching the phone", || {
        phone_shelf
            .inside
            .lock()
            .prints
            .iter()
            .any(|(file, told)| *file == held && *told == print)
    });
    assert!(
        !phone_shelf
            .inside
            .lock()
            .prints
            .iter()
            .any(|(file, _)| *file == lacking)
    );
    assert_eq!(desk_shelf.inside.lock().prints.len(), 1);

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn two_devices_told_to_use_no_server_stand_nearby_and_owe_nothing_once_they_have_told_each_other() {
    let folder = scratch("serverless");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));
    desk_shelf.make_playlist("Summer", &["A", "B"]);
    phone_shelf.make_playlist("Commute", &["A"]);

    let desk = device_through(
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        None,
        Follow::default(),
        Server::Nowhere,
    );
    let phone = device_through(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
        Follow::default(),
        Server::Nowhere,
    );

    assert_eq!(desk.shown().standing, Standing::Nearby);
    assert_eq!(phone.shown().standing, Standing::Nearby);
    eventually("the desk's playlist reaching the phone", || {
        phone_shelf.playlist_named("Summer").is_some()
    });
    eventually("the phone's playlist reaching the desk", || {
        desk_shelf.playlist_named("Commute").is_some()
    });
    eventually("both owing nothing once told", || {
        desk.shown().owed == 0 && phone.shown().owed == 0 && desk.shown().settled
    });
    assert_eq!(desk.shown().standing, Standing::Nearby);

    let (on_phone, _) = phone_shelf.playlist_named("Summer").expect("held");
    phone_shelf.rename_playlist(on_phone, "Summer days");
    eventually("the rename reaching the desk", || {
        desk_shelf.playlist_named("Summer days").is_some()
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_device_of_another_key_on_the_network_meets_nobody() {
    let folder = scratch("strangers");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let stranger_shelf = Arc::new(Shelf::holding(&["A"]));
    desk_shelf.make_playlist("Private", &["A"]);

    let desk = device(
        &folder,
        &SyncKey::minted().expect("a key"),
        "Desk",
        Arc::clone(&desk_shelf),
        None,
    );
    let stranger = device(
        &folder,
        &SyncKey::minted().expect("a key"),
        "Stranger",
        Arc::clone(&stranger_shelf),
        Some(&desk),
    );
    thread::sleep(Duration::from_secs(2));

    assert!(stranger_shelf.playlist_named("Private").is_none());
    assert!(desk.shown().devices.iter().all(|seen| !seen.nearby));
    assert!(stranger.shown().devices.iter().all(|seen| !seen.nearby));

    drop(stranger);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_device_told_another_by_its_platform_meets_it_by_the_record_alone() {
    if !on_a_network() {
        return;
    }
    let folder = scratch("platform");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.make_playlist("Evening", &["A"]);

    let desk = started(
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Nearby::told_by_the_platform(),
    );
    let phone = started(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Nearby::told_by_the_platform(),
    );
    let desk_record = desk.nearby_record().expect("a record of the desk");
    let phone_record = phone.nearby_record().expect("a record of the phone");
    phone.nearby_heard(&desk_record);
    desk.nearby_heard(&phone_record);

    eventually("the playlist reaching the phone", || {
        phone_shelf.playlist_named("Evening").is_some()
    });
    eventually("the phone seeing the desk nearby", || {
        nearby_of(&phone, "Desk")
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_device_meets_another_that_only_it_has_heard_of_whichever_dials_first() {
    if !on_a_network() {
        return;
    }
    let folder = scratch("one-way");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.make_playlist("Morning", &["A"]);

    let desk = started(
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Nearby::told_by_the_platform(),
    );
    let phone = started(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Nearby::told_by_the_platform(),
    );
    let desk_record = desk.nearby_record().expect("a record of the desk");
    phone.nearby_heard(&desk_record);

    eventually(
        "the playlist reaching the phone told of the desk alone",
        || phone_shelf.playlist_named("Morning").is_some(),
    );
    eventually("the desk seeing the phone it was never told of", || {
        nearby_of(&desk, "Phone")
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_favourite_learned_after_meeting_is_fetched_without_a_restart() {
    let folder = scratch("favourite-after");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.favour_here("A", true);

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device_following(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
        Follow {
            favourites: true,
            ..Follow::default()
        },
    );

    eventually("the favoured song reaching the phone", || {
        holds_title(&phone_shelf, "A")
    });
    desk_shelf.favour_here("B", true);
    eventually("a song favoured later reaching the phone", || {
        holds_title(&phone_shelf, "B")
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_record_of_another_code_is_passed_over() {
    let folder = scratch("stranger-record");
    let desk = started(
        &folder,
        &SyncKey::minted().expect("a key"),
        "Desk",
        Arc::new(Shelf::holding(&[])),
        Nearby::told_by_the_platform(),
    );
    let stranger = started(
        &folder,
        &SyncKey::minted().expect("a key"),
        "Stranger",
        Arc::new(Shelf::holding(&[])),
        Nearby::told_by_the_platform(),
    );
    let Some(stranger_record) = stranger.nearby_record() else {
        return;
    };

    desk.nearby_heard(&stranger_record);
    desk.nearby_heard("not a record");
    thread::sleep(Duration::from_secs(1));

    assert!(desk.shown().devices.iter().all(|seen| !seen.nearby));

    drop(stranger);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn two_devices_advertising_over_bonjour_find_each_other() {
    if !on_a_network() {
        return;
    }
    let folder = scratch("bonjour");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.make_playlist("Night", &["A"]);
    let bonjour = Nearby {
        broadcast: None,
        bonjour: true,
    };

    let desk = started(
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        bonjour.clone(),
    );
    let phone = started(&folder, &key, "Phone", Arc::clone(&phone_shelf), bonjour);

    eventually("the playlist reaching the phone over Bonjour", || {
        phone_shelf.playlist_named("Night").is_some()
    });
    eventually("the desk seeing the phone nearby", || {
        nearby_of(&desk, "Phone")
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn two_devices_told_of_each_other_over_ipv6_alone_meet_and_sync() {
    if !ipv6_loopback_here() {
        return;
    }
    let folder = scratch("ipv6");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));
    desk_shelf.make_playlist("Summer", &["A", "B"]);

    let desk = device_over_ipv6(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device_over_ipv6(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
    );

    eventually("the desk's playlist reaching the phone over IPv6", || {
        phone_shelf.playlist_named("Summer").is_some()
    });
    eventually("the desk seeing the phone nearby", || {
        nearby_of(&desk, "Phone")
    });

    phone.fetch(vec![song("B")], desk.device());
    eventually("the song arriving over IPv6", || {
        holds_title(&phone_shelf, "B")
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_device_is_followed_by_one_run_at_a_time_however_many_records_arrive_meanwhile() {
    let folder = scratch("one-follow");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.favour_here("A", true);
    desk_shelf.opening.shut();

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device_following(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
        following_favourites(),
    );
    eventually("the phone asking the desk for the favoured song", || {
        desk_shelf.inside.lock().opened > 0
    });
    for name in ["One", "Two", "Three"] {
        desk_shelf.make_playlist(name, &["A"]);
        eventually("the playlist reaching the phone", || {
            phone_shelf.playlist_named(name).is_some()
        });
    }
    thread::sleep(Duration::from_millis(1500));
    let listed_meanwhile = desk_shelf.inside.lock().listed;
    desk_shelf.opening.open();

    assert_eq!(listed_meanwhile, 1);
    eventually("the favoured song reaching the phone", || {
        holds_title(&phone_shelf, "A")
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_song_another_device_landed_meanwhile_leaves_no_row_for_the_device_that_sent_nothing() {
    let folder = scratch("landed-meanwhile");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let laptop_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.favour_here("A", true);
    desk_shelf.favour_here("B", true);
    desk_shelf.opening.shut();
    laptop_shelf.listing.shut();

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device_following(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
        following_favourites(),
    );
    eventually("the phone asking the desk for the favoured song", || {
        desk_shelf.inside.lock().opened > 0
    });
    let laptop = device_told(
        &folder,
        &key,
        "Laptop",
        Arc::clone(&laptop_shelf),
        &[&desk, &phone],
        Follow::default(),
    );
    eventually("the phone asking what the laptop holds", || {
        laptop_shelf.inside.lock().listed > 0
    });
    desk_shelf.opening.open();
    eventually("the song reaching the phone from the desk", || {
        holds_title(&phone_shelf, "A")
    });
    laptop_shelf.listing.open();
    eventually("the other song reaching the phone from the laptop", || {
        holds_title(&phone_shelf, "B")
    });

    assert_eq!(
        rows_from(&phone, laptop.device()),
        [("B".to_owned(), TransferState::Landed)]
    );
    assert_eq!(
        rows_from(&phone, desk.device()),
        [("A".to_owned(), TransferState::Landed)]
    );

    drop(laptop);
    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_song_passed_over_while_another_device_fetched_it_is_fetched_once_that_one_fails() {
    let folder = scratch("passed-over");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let laptop_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.opening.shut();

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let laptop = device(
        &folder,
        &key,
        "Laptop",
        Arc::clone(&laptop_shelf),
        Some(&desk),
    );
    let phone = device_told(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        &[&desk, &laptop],
        Follow::default(),
    );
    eventually("the phone meeting both", || {
        nearby_of(&phone, "Desk") && nearby_of(&phone, "Laptop")
    });
    phone.fetch(vec![song("A")], desk.device());
    eventually("the desk asked for the song", || {
        desk_shelf.inside.lock().opened > 0
    });
    phone.fetch(vec![song("A"), song("B")], laptop.device());
    eventually("the laptop's other song reaching the phone", || {
        holds_title(&phone_shelf, "B")
    });
    desk_shelf.inside.lock().refuses_opening = true;
    desk_shelf.opening.open();

    eventually(
        "the song passed over reaching the phone from the laptop",
        || holds_title(&phone_shelf, "A"),
    );
    assert!(rows_from(&phone, laptop.device()).contains(&("A".to_owned(), TransferState::Landed)));

    drop(phone);
    drop(laptop);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_song_deleted_from_the_app_is_not_followed_and_sync_now_clears_the_mark() {
    let folder = scratch("deleted-here");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.favour_here("A", true);
    phone_shelf.inside.lock().deleted_here = vec!["A".into()];

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device_following(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
        following_favourites(),
    );
    eventually("the phone meeting the desk", || {
        phone
            .shown()
            .devices
            .iter()
            .any(|seen| seen.name == "Desk" && seen.nearby && seen.online)
    });
    thread::sleep(Duration::from_secs(1));

    assert!(!holds_title(&phone_shelf, "A"));
    assert!(phone.shown().transfers.is_empty());

    phone.force();

    eventually("the song fetched once the mark is cleared", || {
        holds_title(&phone_shelf, "A")
    });
    assert!(phone_shelf.inside.lock().deleted_here.is_empty());

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_song_refused_at_landing_is_not_fetched_again_after_a_restart() {
    let folder = scratch("refused-kept");
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.favour_here("A", true);
    phone_shelf.inside.lock().refuses_landing = true;

    let desk = device(&folder, &key, "Desk", Arc::clone(&desk_shelf), None);
    let phone = device_following(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
        following_favourites(),
    );
    eventually("the landing refused", || {
        rows_from(&phone, desk.device()) == [("A".to_owned(), TransferState::Failed)]
    });
    drop(phone);
    let (opened, listed) = {
        let inside = desk_shelf.inside.lock();
        (inside.opened, inside.listed)
    };

    let phone = device_following(
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Some(&desk),
        following_favourites(),
    );
    eventually("the phone following the desk again", || {
        desk_shelf.inside.lock().listed > listed
    });
    thread::sleep(Duration::from_secs(1));

    assert_eq!(desk_shelf.inside.lock().opened, opened);
    assert!(phone.shown().transfers.is_empty());

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}
