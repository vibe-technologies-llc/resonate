mod shelf;

use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use resonate_sync::{Follow, Me, Nearby, Platform, Settings, Standing, Sync, SyncKey};

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
    let told = told
        .and_then(Sync::nearby_port)
        .map(|port| SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .into_iter()
        .collect();
    let settings = Settings {
        server: Some(NO_SERVER_HERE.into()),
        trusted: None,
        key: key.clone(),
        me: Me {
            name: name.into(),
            platform: Platform::Linux,
        },
        follow: Follow::default(),
        ledger: folder.join(format!("{name}.sqlite")),
        staging: folder.join(format!("{name}-staging")),
        transfers_allowed: true,
        nearby: Some(Nearby { port: 0, told }),
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
