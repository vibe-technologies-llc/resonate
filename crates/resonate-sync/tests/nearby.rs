mod shelf;

use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use resonate_sync::{
    Broadcast, Follow, Me, Nearby, Platform, Server, Settings, Standing, Sync, SyncKey,
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
        key: key.clone(),
        me: Me {
            name: name.into(),
            platform: Platform::Linux,
        },
        follow,
        ledger: folder.join(format!("{name}.sqlite")),
        staging: folder.join(format!("{name}-staging")),
        transfers_allowed: true,
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
    thread::sleep(Duration::from_secs(6));

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
    thread::sleep(Duration::from_secs(3));

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
