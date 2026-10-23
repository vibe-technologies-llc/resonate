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

use base64::{Engine, engine::general_purpose::STANDARD};
use resonate_server::{Limits, Server, Store, listening, serving};
use resonate_sync::{Follow, Me, Platform, Settings, Standing, Sync, SyncKey};
use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::shelf::{Shelf, song};

const SETTLES_WITHIN: Duration = Duration::from_secs(30);

trait HeldHere {
    fn holds_title(&self, title: &str) -> bool;
    fn bytes_of(&self, title: &str) -> Vec<u8>;
}

impl HeldHere for Shelf {
    fn holds_title(&self, title: &str) -> bool {
        self.inside
            .lock()
            .songs
            .iter()
            .any(|(recording, _)| recording.title == title)
    }

    fn bytes_of(&self, title: &str) -> Vec<u8> {
        self.inside
            .lock()
            .songs
            .iter()
            .find(|(recording, _)| recording.title == title)
            .map(|(_, bytes)| bytes.clone())
            .unwrap_or_default()
    }
}

struct Hosted {
    address: SocketAddr,
    trusted: PathBuf,
    _runtime: tokio::runtime::Runtime,
}

fn scratch(name: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!(
        "resonate-sync-{name}-{}-{:?}",
        std::process::id(),
        Instant::now()
    ));
    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn hosted(folder: &Path) -> Hosted {
    let signing = rcgen::KeyPair::generate().expect("a key");
    let certificate = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
        .and_then(|params| params.self_signed(&signing))
        .expect("a certificate");
    let pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        STANDARD.encode(certificate.der())
    );
    let trusted = folder.join("server.pem");
    fs::write(&trusted, pem).expect("written");

    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing.serialize_der()));
    let config = serving(vec![certificate.der().clone()], key).expect("a configuration");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let endpoint = {
        let _entered = runtime.enter();
        listening(config, SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("listening")
    };
    let address = endpoint.local_addr().expect("an address");
    let server = Server::new(Store::in_memory().expect("a store"), Limits::default());
    runtime.spawn(server.serve(endpoint));
    Hosted {
        address,
        trusted,
        _runtime: runtime,
    }
}

fn device(
    hosted: &Hosted,
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    follow: Follow,
) -> Sync {
    let settings = Settings {
        server: resonate_sync::Server::At(format!("localhost:{}", hosted.address.port())),
        trusted: Some(hosted.trusted.clone()),
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
fn two_devices_of_one_key_share_playlists_and_songs_through_the_server_and_each_other() {
    let folder = scratch("pair");
    let hosted = hosted(&folder);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));
    desk_shelf.make_playlist("Summer", &["A", "B"]);
    desk_shelf.favour_here("B", true);

    let desk = device(
        &hosted,
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Follow::default(),
    );
    let phone = device(
        &hosted,
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Follow::default(),
    );

    eventually("the playlist arriving", || {
        phone_shelf.playlist_named("Summer").is_some()
    });
    assert_eq!(
        phone_shelf.playlist_named("Summer").expect("arrived").1,
        ["A"]
    );
    eventually("both devices seeing each other online", || {
        phone
            .shown()
            .devices
            .iter()
            .any(|seen| seen.name == "Desk" && seen.online)
    });
    assert_eq!(phone.shown().standing, Standing::Connected);

    phone.fetch(vec![song("B")], desk.device());
    eventually("the song arriving from the desk", || {
        phone_shelf.holds_title("B")
    });
    assert_eq!(phone_shelf.bytes_of("B"), desk_shelf.bytes_of("B"));
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
fn a_saved_query_travels_through_the_server_as_text_and_follows_its_revision_and_removal() {
    let folder = scratch("queries");
    let hosted = hosted(&folder);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));
    let recent = desk_shelf.make_query("Recent", "added:<30d");

    let desk = device(
        &hosted,
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Follow::default(),
    );
    let phone = device(
        &hosted,
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Follow::default(),
    );

    eventually("the query arriving", || {
        phone_shelf.query_named("Recent").is_some()
    });
    assert_eq!(
        phone_shelf.query_named("Recent").expect("arrived").1,
        Some("added:<30d".to_owned())
    );

    desk_shelf.revise_query(recent, "Recent", "added:<7d");
    eventually("the revision reaching the phone", || {
        phone_shelf.query_named("Recent").map(|(_, text)| text) == Some(Some("added:<7d".into()))
    });

    desk_shelf.drop_query(recent);
    eventually("the removal reaching the phone", || {
        phone_shelf.query_names().is_empty()
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_device_following_favourites_fetches_them_when_the_other_is_online() {
    let folder = scratch("follow");
    let hosted = hosted(&folder);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.favour_here("A", true);
    let follow = Follow {
        favourites: true,
        ..Follow::default()
    };

    let desk = device(
        &hosted,
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Follow::default(),
    );
    eventually("the desk connecting", || {
        desk.shown().standing == Standing::Connected
    });
    thread::sleep(Duration::from_secs(5));
    let phone = device(
        &hosted,
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        follow,
    );

    eventually("the favourite song arriving", || {
        phone_shelf.holds_title("A")
    });
    assert!(!phone_shelf.holds_title("B"));

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_forced_sync_reconnects_to_the_server_and_carries_on() {
    let folder = scratch("forced");
    let hosted = hosted(&folder);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A"]));

    let desk = device(
        &hosted,
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Follow::default(),
    );
    let phone = device(
        &hosted,
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Follow::default(),
    );
    eventually("both connecting", || {
        desk.shown().standing == Standing::Connected
            && phone.shown().standing == Standing::Connected
    });

    desk.force();
    desk_shelf.make_playlist("After", &["A"]);

    eventually("the desk connected again", || {
        desk.shown().standing == Standing::Connected
    });
    eventually("the playlist made after the force arriving", || {
        phone_shelf.playlist_named("After").is_some()
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_device_of_another_key_sees_nothing_of_the_first() {
    let folder = scratch("strangers");
    let hosted = hosted(&folder);
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let stranger_shelf = Arc::new(Shelf::holding(&["A"]));
    desk_shelf.make_playlist("Private", &["A"]);

    let desk = device(
        &hosted,
        &folder,
        &SyncKey::minted().expect("a key"),
        "Desk",
        Arc::clone(&desk_shelf),
        Follow::default(),
    );
    let stranger = device(
        &hosted,
        &folder,
        &SyncKey::minted().expect("a key"),
        "Stranger",
        Arc::clone(&stranger_shelf),
        Follow::default(),
    );
    eventually("both connecting", || {
        desk.shown().standing == Standing::Connected
            && stranger.shown().standing == Standing::Connected
    });
    thread::sleep(Duration::from_secs(6));

    assert!(stranger_shelf.playlist_named("Private").is_none());
    assert!(
        stranger
            .shown()
            .devices
            .iter()
            .all(|seen| seen.name == "Stranger")
    );

    drop(stranger);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}
