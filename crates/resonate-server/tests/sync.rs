use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use resonate_core::{Answer, LONGEST_SHARED_LYRICS, SharedLyrics};
use resonate_server::{Limits, Server, Store, listening, serving};
use resonate_sync::{
    Follow, Me, Platform, Settings, SongTags, Standing, Sync, SyncKey, wire::LARGEST_FRAME,
};
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
        "resonate-sync-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn hosted(folder: &Path) -> Hosted {
    hosted_over(folder, Store::in_memory().expect("a store"))
}

fn hosted_over(folder: &Path, store: Store) -> Hosted {
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
    let server = Server::new(store, Limits::default());
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
    device_reaching(hosted, folder, key, name, shelf, follow, false)
}

fn device_reaching(
    hosted: &Hosted,
    folder: &Path,
    key: &SyncKey,
    name: &str,
    shelf: Arc<Shelf>,
    follow: Follow,
    relay_only: bool,
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
        relay_only,
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
fn a_song_fetched_from_a_device_arrives_with_the_tags_that_device_declares_for_it() {
    let folder = scratch("tags");
    let hosted = hosted(&folder);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    let declared = SongTags {
        album_artist: Some("Various".into()),
        date: Some("1971".into()),
        track_number: Some(2),
        track_total: Some(9),
        disc_number: Some(1),
        disc_total: Some(2),
        compilation: true,
    };
    desk_shelf.declare("A", declared.clone());

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
    eventually("both devices seeing each other online", || {
        phone
            .shown()
            .devices
            .iter()
            .any(|seen| seen.name == "Desk" && seen.online)
    });

    phone.fetch(vec![song("A"), song("B")], desk.device());
    eventually("both songs arriving", || {
        phone_shelf.holds_title("A") && phone_shelf.holds_title("B")
    });

    assert_eq!(phone_shelf.landed_with("A"), Some(declared));
    assert_eq!(phone_shelf.landed_with("B"), Some(SongTags::default()));

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
    thread::sleep(Duration::from_secs(2));

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

#[test]
fn a_song_reaches_a_device_the_other_can_only_reach_through_the_servers_relay() {
    let folder = scratch("relayed");
    let hosted = hosted(&folder);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.favour_here("A", true);
    let follow = Follow {
        favourites: true,
        ..Follow::default()
    };

    let desk = device_reaching(
        &hosted,
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Follow::default(),
        true,
    );
    eventually("the desk connecting", || {
        desk.shown().standing == Standing::Connected
    });
    let phone = device_reaching(
        &hosted,
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        follow,
        true,
    );

    eventually("the favourite song arriving through the relay", || {
        phone_shelf.holds_title("A")
    });
    assert_eq!(phone_shelf.bytes_of("A"), desk_shelf.bytes_of("A"));
    assert!(!phone_shelf.holds_title("B"));

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_device_dialling_directly_still_reaches_one_that_dials_only_through_the_relay() {
    let folder = scratch("not-relayed");
    let hosted = hosted(&folder);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A"]));
    let phone_shelf = Arc::new(Shelf::holding(&[]));
    desk_shelf.favour_here("A", true);

    let desk = device_reaching(
        &hosted,
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Follow::default(),
        true,
    );
    eventually("the desk connecting", || {
        desk.shown().standing == Standing::Connected
    });
    let phone = device(
        &hosted,
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Follow {
            favourites: true,
            ..Follow::default()
        },
    );

    eventually("the song arriving by a direct path", || {
        phone_shelf.holds_title("A")
    });

    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_song_deleted_from_every_device_leaves_those_that_let_it_through_the_server() {
    let folder = scratch("deleted-everywhere");
    let hosted = hosted(&folder);
    let key = SyncKey::minted().expect("a key");
    let desk_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let phone_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    let laptop_shelf = Arc::new(Shelf::holding(&["A", "B"]));
    laptop_shelf.inside.lock().refuses_deletions = true;
    let joined = |name: &str, shelf: &Arc<Shelf>| {
        device(
            &hosted,
            &folder,
            &key,
            name,
            Arc::clone(shelf),
            Follow::default(),
        )
    };
    let desk = joined("Desk", &desk_shelf);
    let phone = joined("Phone", &phone_shelf);
    let laptop = joined("Laptop", &laptop_shelf);
    eventually("every device meeting the others", || {
        [&desk, &phone, &laptop]
            .iter()
            .all(|device| device.shown().devices.len() == 3)
    });

    desk_shelf.inside.lock().deleted_by_hand = vec!["B".into()];
    desk_shelf.inside.lock().deleted_here = vec!["B".into()];
    desk_shelf.delete_everywhere("A");

    eventually("the song leaving the phone", || {
        !phone_shelf.holds_title("A")
    });
    eventually("the laptop hearing of it", || {
        laptop_shelf.inside.lock().deletions_weighed > 0
    });

    assert!(laptop_shelf.holds_title("A"));
    assert!(phone_shelf.holds_title("B"));
    assert!(laptop_shelf.holds_title("B"));

    drop(laptop);
    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}

const SONGS_WHOSE_LYRICS_PASS_A_FRAMES_SHARE: usize = 760;

fn words_deflate_hardly_shortens(seed: u64) -> String {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    (0..LONGEST_SHARED_LYRICS)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let at = usize::try_from(state % LETTERS.len() as u64).expect("an index");
            char::from(LETTERS[at])
        })
        .collect()
}

fn lyrics_of(at: usize) -> Answer {
    Answer::Lyrics(SharedLyrics {
        text: words_deflate_hardly_shortens(u64::try_from(at).expect("a seed")),
        synced: false,
    })
}

#[test]
fn a_state_past_a_frames_share_is_compacted_in_parts_and_a_third_device_catches_up_from_it() {
    let folder = scratch("compacted-in-parts");
    let kept_at = folder.join("server.sqlite");
    let hosted = hosted_over(&folder, Store::open(&kept_at).expect("a store"));
    let key = SyncKey::minted().expect("a key");
    let space = key.derived().expect("derived").space;
    let titles = (0..SONGS_WHOSE_LYRICS_PASS_A_FRAMES_SHARE)
        .map(|at| format!("Song {at}"))
        .collect::<Vec<_>>();
    let held = titles.iter().map(String::as_str).collect::<Vec<_>>();
    let desk_shelf = Arc::new(Shelf::holding(&held));
    for (at, title) in titles.iter().enumerate() {
        desk_shelf.answer_here(title, lyrics_of(at));
    }
    let watching = Store::open(&kept_at).expect("the server's store, read beside it");

    let desk = device(
        &hosted,
        &folder,
        &key,
        "Desk",
        Arc::clone(&desk_shelf),
        Follow::default(),
    );
    eventually("every lyric reaching the server", || {
        let shown = desk.shown();
        shown.settled && shown.owed == 0 && watching.head(space).is_ok_and(|(head, _)| head > 0)
    });
    let phone_shelf = Arc::new(Shelf::holding(&held));
    let phone = device(
        &hosted,
        &folder,
        &key,
        "Phone",
        Arc::clone(&phone_shelf),
        Follow::default(),
    );
    eventually("the server holding a snapshot past a frame's share", || {
        watching
            .snapshot(space)
            .ok()
            .flatten()
            .is_some_and(|snapshot| {
                snapshot.parts.iter().map(Vec::len).sum::<usize>() > LARGEST_FRAME / 2
            })
    });
    let tablet_shelf = Arc::new(Shelf::holding(&held));
    let tablet = device(
        &hosted,
        &folder,
        &key,
        "Tablet",
        Arc::clone(&tablet_shelf),
        Follow::default(),
    );
    eventually("every lyric reaching the tablet", || {
        tablet_shelf.inside.lock().answers.len() == SONGS_WHOSE_LYRICS_PASS_A_FRAMES_SHARE
    });

    let (_, through) = watching.head(space).expect("the head");
    let last = SONGS_WHOSE_LYRICS_PASS_A_FRAMES_SHARE - 1;
    assert!(through > 0);
    assert_eq!(tablet_shelf.answers_of(&titles[last]), [lyrics_of(last)]);

    drop(tablet);
    drop(phone);
    drop(desk);
    let _ = fs::remove_dir_all(&folder);
}
