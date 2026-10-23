#[path = "../../resonate-sync/tests/shelf/mod.rs"]
mod shelf;

use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use resonate_server::{Allowance, Limits, Server, Store};
use resonate_sync::{Follow, Me, Platform, Settings, Standing, Sync, SyncKey, wire::Refusal};
use tokio::net::TcpListener;

use crate::shelf::Shelf;

const SETTLES_WITHIN: Duration = Duration::from_secs(30);

struct Hosted {
    address: SocketAddr,
    _runtime: tokio::runtime::Runtime,
}

fn scratch(name: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!(
        "resonate-web-{name}-{}-{:?}",
        std::process::id(),
        Instant::now()
    ));
    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn hosted(limits: Limits, forwarded_by: Option<&str>) -> Hosted {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let listener = runtime
        .block_on(TcpListener::bind(SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            0,
        ))))
        .expect("listening");
    let address = listener.local_addr().expect("an address");
    let server = Server::new(Store::in_memory().expect("a store"), limits);
    runtime.spawn(server.serve_web(listener, forwarded_by.map(str::to_owned)));
    Hosted {
        address,
        _runtime: runtime,
    }
}

fn device(hosted: &Hosted, folder: &Path, key: &SyncKey, name: &str, shelf: Arc<Shelf>) -> Sync {
    let settings = Settings {
        server: resonate_sync::Server::At(format!("ws://{}/", hosted.address)),
        trusted: None,
        stun: None,
        key: key.clone(),
        me: Me {
            name: name.into(),
            platform: Platform::Linux,
        },
        follow: Follow::default(),
        ledger: folder.join(format!("{name}.sqlite")),
        staging: folder.join(format!("{name}-staging")),
        transfers_allowed: true,
        nearby: None,
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

#[test]
fn two_devices_share_a_playlist_through_the_server_over_a_web_socket() {
    let folder = scratch("pair");
    let hosted = hosted(Limits::default(), None);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));
    desk_shelf.make_playlist("Summer", &["A", "B"]);
    desk_shelf.favour_here("A", true);

    let desk = device(&hosted, &folder, &key, "Desk", Arc::clone(&desk_shelf));
    let phone = device(&hosted, &folder, &key, "Phone", Arc::clone(&phone_shelf));

    eventually("the playlist arriving", || {
        phone_shelf.playlist_named("Summer").is_some()
    });
    eventually("both devices seeing each other online", || {
        phone
            .shown()
            .devices
            .iter()
            .any(|seen| seen.name == "Desk" && seen.online)
    });
    assert_eq!(phone.shown().standing, Standing::Connected);
    eventually("the favourite arriving", || {
        phone_shelf.favourite_titles() == ["A"]
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
fn a_listener_behind_a_proxy_turns_away_a_connection_the_proxy_did_not_forward() {
    let folder = scratch("unforwarded");
    let hosted = hosted(Limits::default(), Some("CF-Connecting-IP"));
    let key = SyncKey::minted().expect("a key");
    let shelf = Arc::new(Shelf::holding(&["A"]));

    let desk = device(&hosted, &folder, &key, "Desk", shelf);

    eventually("the connection being turned away", || {
        desk.shown().standing == Standing::Unreachable
    });

    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn an_address_that_makes_more_spaces_than_it_is_allowed_in_a_day_is_turned_away() {
    let folder = scratch("spaces");
    let limits = Limits {
        allowance: Allowance {
            spaces_per_address_per_day: 1,
            ..Allowance::default()
        },
        ..Limits::default()
    };
    let hosted = hosted(limits, None);
    let first = SyncKey::minted().expect("a key");
    let second = SyncKey::minted().expect("a key");
    let shelf = Arc::new(Shelf::holding(&["A"]));

    let allowed = device(&hosted, &folder, &first, "First", Arc::clone(&shelf));
    eventually("the first space being made", || {
        allowed.shown().standing == Standing::Connected
    });
    let refused = device(&hosted, &folder, &second, "Second", shelf);

    eventually("the second space being refused", || {
        refused.shown().standing == Standing::Refused(Refusal::TooFast)
    });

    drop(refused);
    drop(allowed);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_known_space_is_not_counted_against_the_spaces_an_address_may_make() {
    let folder = scratch("known");
    let limits = Limits {
        allowance: Allowance {
            spaces_per_address_per_day: 1,
            ..Allowance::default()
        },
        ..Limits::default()
    };
    let hosted = hosted(limits, None);
    let key = SyncKey::minted().expect("a key");
    let shelf = Arc::new(Shelf::holding(&["A"]));

    let desk = device(&hosted, &folder, &key, "Desk", Arc::clone(&shelf));
    let phone = device(&hosted, &folder, &key, "Phone", shelf);

    eventually("both devices connecting", || {
        desk.shown().standing == Standing::Connected
            && phone.shown().standing == Standing::Connected
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}
