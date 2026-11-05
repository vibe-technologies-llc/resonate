use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use resonate_core::{
    ChannelLayout, Frames, Gain, MediaLocation, Recording, RemoteTrust, SampleFormat, SampleRate,
    TrackId, ZoneId,
};
use resonate_driven::{ByTheirTags, Driven, RowNames, unnamed};
use resonate_engine::{
    AudioSource, Backend, Command, EngineConfig, NodeName, Opener, Opening, Opens, Player,
    QueueItem, Result as EngineResult, SinkChange, SinkFormats, SinkId, SinkInfo, SinkResult,
    SinkStream, StreamClock, StreamCommand, StreamRequest, Surveyor, Words, ZoneName, Zones,
};
use resonate_sync::{
    Broadcast, Catalog, Controlled, Controlling, DeviceId, Follow, Held, LocalListen,
    LocalPlaylist, LocalPlaylistId, Me, Nearby, Opened, Permille, Platform, Refused, RemoteCommand,
    RemoteListen, RemotePlayback, RemoteRow, RemoteZone, Resumed, Server, Settings, SongTags, Sync,
    SyncKey, Uncontrolled, Watched,
};

const RATE: u32 = 44_100;
const SECONDS_A_SONG: u32 = 30;
const CHANNELS: u16 = 2;
const BYTES_A_FRAME: u32 = 4;
const PULLED_AT_ONCE: usize = 4_096;
const PULLED_EVERY: Duration = Duration::from_millis(5);
const SETTLES_WITHIN: Duration = Duration::from_secs(30);
const MEETS_WITHIN: Duration = Duration::from_secs(40);
const LOOKED_AGAIN_AFTER: Duration = Duration::from_millis(10);
const NO_SERVER_HERE: &str = "127.0.0.1:9";

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let folder = env::temp_dir().join(format!(
            "resonate-driven-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&folder).expect("a test folder");
        Self(folder)
    }

    fn song(&self, name: &str, tags: &[(&[u8; 4], &str)], id: u64) -> QueueItem {
        let path = self.0.join(format!("{name}.wav"));
        fs::write(&path, silence(tags)).expect("a song");
        QueueItem::whole(
            TrackId::new(id).expect("an id"),
            MediaLocation::local(&path),
        )
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&u32::try_from(payload.len()).expect("a chunk").to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn silence(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let bytes = RATE * SECONDS_A_SONG * BYTES_A_FRAME;
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1_u16.to_le_bytes());
    fmt.extend_from_slice(&CHANNELS.to_le_bytes());
    fmt.extend_from_slice(&RATE.to_le_bytes());
    fmt.extend_from_slice(&(RATE * BYTES_A_FRAME).to_le_bytes());
    fmt.extend_from_slice(&4_u16.to_le_bytes());
    fmt.extend_from_slice(&16_u16.to_le_bytes());

    let mut info = b"INFO".to_vec();
    for (id, value) in tags {
        let mut text = value.as_bytes().to_vec();
        text.push(0);
        chunk(&mut info, id, &text);
    }

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &fmt);
    if !tags.is_empty() {
        chunk(&mut body, b"LIST", &info);
    }
    chunk(&mut body, b"data", &vec![0; bytes as usize]);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&u32::try_from(body.len()).expect("a file").to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn speakers() -> SinkInfo {
    let rate = SampleRate::new(RATE).expect("a rate");
    SinkInfo {
        id: SinkId::new(1),
        name: NodeName::new("test.speakers"),
        aliases: Vec::new(),
        description: "Test speakers".to_owned(),
        is_default: true,
        is_hardware: true,
        port: None,
        profile: None,
        formats: vec![SinkFormats {
            format: SampleFormat::S16,
            words: Words::offered_for(SampleFormat::S16),
            rates: vec![rate],
            channels: vec![ChannelLayout::Stereo],
        }],
        allowed_rates: vec![rate],
        current_rate: Some(rate),
    }
}

struct Pulled;

impl Surveyor for Pulled {
    fn enumerate_sinks(&self, _: Duration) -> SinkResult<Vec<SinkInfo>> {
        Ok(vec![speakers()])
    }
}

impl Opener for Pulled {
    fn open(&self, _: &StreamRequest, mut source: Box<dyn AudioSource>) -> SinkResult<SinkStream> {
        let (announces, incoming) = unbounded();
        let active = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let (pulling, ending) = (Arc::clone(&active), Arc::clone(&closed));

        thread::spawn(move || {
            let _announces = announces;
            let mut block = vec![0_u8; PULLED_AT_ONCE];
            while !ending.load(Ordering::Acquire) {
                if pulling.load(Ordering::Acquire) {
                    source.fill(&mut block);
                }
                thread::sleep(PULLED_EVERY);
            }
        });

        Ok(SinkStream::new(
            incoming,
            Arc::new(StreamClock::behind_by(Frames(0))),
            Box::new(move |command| {
                match command {
                    StreamCommand::SetActive(on) => active.store(on, Ordering::Release),
                    StreamCommand::Close => closed.store(true, Ordering::Release),
                    StreamCommand::Drain => {}
                }
                Ok(())
            }),
        ))
    }
}

struct Sink {
    _announces: Sender<SinkChange>,
    changes: Receiver<SinkChange>,
}

impl Backend for Sink {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Pulled)
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Pulled)
    }

    fn set_device_volume(&self, _: SinkId, _: Gain) -> SinkResult<()> {
        Ok(())
    }

    fn set_device_mute(&self, _: SinkId, _: bool) -> SinkResult<()> {
        Ok(())
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        Ok(())
    }
}

fn player() -> Player {
    let (announces, changes) = unbounded();
    let backend = Sink {
        _announces: announces,
        changes,
    };
    Player::with_backend(EngineConfig::default(), move |_| Ok(Box::new(backend)))
        .expect("a player over the fake backend")
}

fn eventually(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + SETTLES_WITHIN;
    while !done() {
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(LOOKED_AGAIN_AFTER);
    }
}

fn the_zone(driven: &Driven, zone: ZoneId) -> RemoteZone {
    driven
        .zones()
        .into_iter()
        .find(|held| held.zone == zone)
        .expect("the zone mirrored")
}

fn titles(zone: &RemoteZone) -> Vec<&str> {
    zone.rows.iter().map(|row| row.title.as_str()).collect()
}

fn load_three(player: &Player, folder: &Folder, autoplay: bool) {
    let items = vec![
        folder.song("echoes", &[(b"INAM", "Echoes"), (b"IART", "Pink Floyd")], 1),
        folder.song("time", &[], 2),
        folder.song("money", &[], 3),
    ];
    player
        .settle(Command::Load {
            items,
            start_at: 0,
            autoplay,
        })
        .expect("the queue loaded");
}

#[test]
fn a_real_engine_is_mirrored_and_takes_every_command_a_device_may_send() {
    let folder = Folder::new();
    let player = Arc::new(player());
    let driven = Driven::player(&player);
    let first = ZoneId::FIRST;

    assert_eq!(the_zone(&driven, first).playback, RemotePlayback::Idle);
    assert_eq!(the_zone(&driven, first).queue_length, 0);
    assert_eq!(
        driven.control(first, RemoteCommand::Seek(Duration::from_secs(1))),
        Err(Uncontrolled::NoSuchRow)
    );
    assert_eq!(
        driven.control(first, RemoteCommand::Jump(0)),
        Err(Uncontrolled::NoSuchRow)
    );

    load_three(&player, &folder, false);

    eventually("the rows named by what the files say", || {
        let zone = the_zone(&driven, first);
        zone.name == "Zone 1"
            && zone.queue_length == 3
            && zone.first_row == 0
            && zone.playing == Some(0)
            && titles(&zone) == ["Echoes", "time", "money"]
            && zone.rows[0].artist == "Pink Floyd"
            && zone.rows[1].length == Some(Duration::from_secs(u64::from(SECONDS_A_SONG)))
    });

    driven.control(first, RemoteCommand::Play).expect("play");
    eventually("the engine playing", || {
        let zone = the_zone(&driven, first);
        zone.playback == RemotePlayback::Playing
            && zone.length == Some(Duration::from_secs(u64::from(SECONDS_A_SONG)))
    });

    driven.control(first, RemoteCommand::Pause).expect("pause");
    eventually("the engine paused", || {
        the_zone(&driven, first).playback == RemotePlayback::Paused
    });

    driven
        .control(first, RemoteCommand::Toggle)
        .expect("toggle");
    eventually("the engine playing again", || {
        the_zone(&driven, first).playback == RemotePlayback::Playing
    });

    let sought = Duration::from_secs(12);
    driven
        .control(first, RemoteCommand::Seek(sought))
        .expect("seek");
    eventually("the place moved", || {
        the_zone(&driven, first).position >= sought
    });

    driven.control(first, RemoteCommand::Next).expect("next");
    eventually("the next row", || {
        the_zone(&driven, first).playing == Some(1)
    });

    driven.control(first, RemoteCommand::Jump(2)).expect("jump");
    eventually("the row jumped to", || {
        the_zone(&driven, first).playing == Some(2)
    });

    driven
        .control(first, RemoteCommand::Volume(Permille::new(250)))
        .expect("volume");
    eventually("the volume taken", || {
        the_zone(&driven, first).volume == Permille::new(250)
    });

    driven
        .control(first, RemoteCommand::Move { from: 2, to: 0 })
        .expect("move");
    eventually("the row moved with the song still heard", || {
        let zone = the_zone(&driven, first);
        titles(&zone) == ["money", "Echoes", "time"] && zone.playing == Some(0)
    });

    driven
        .control(first, RemoteCommand::Move { from: 1, to: 2 })
        .expect("move down");
    eventually("the row moved down", || {
        titles(&the_zone(&driven, first)) == ["money", "time", "Echoes"]
    });

    driven
        .control(first, RemoteCommand::Remove(1))
        .expect("remove");
    eventually("the row gone", || {
        let zone = the_zone(&driven, first);
        titles(&zone) == ["money", "Echoes"] && zone.queue_length == 2
    });

    driven
        .control(first, RemoteCommand::Previous)
        .expect("previous");

    assert_eq!(
        driven.control(first, RemoteCommand::Remove(2)),
        Err(Uncontrolled::NoSuchRow)
    );
    assert_eq!(
        driven.control(first, RemoteCommand::Jump(9)),
        Err(Uncontrolled::NoSuchRow)
    );
    assert_eq!(
        driven.control(first, RemoteCommand::Move { from: 0, to: 2 }),
        Err(Uncontrolled::NoSuchRow)
    );
    assert_eq!(
        driven.control(first, RemoteCommand::Move { from: 5, to: 0 }),
        Err(Uncontrolled::NoSuchRow)
    );
    assert_eq!(
        driven.control(ZoneId::new(2).expect("a zone"), RemoteCommand::Play),
        Err(Uncontrolled::NoSuchZone)
    );

    drop(driven);
    player.send(Command::Stop).expect("stopped");
}

#[test]
fn rows_are_counted_in_the_order_they_play_under_shuffle_and_a_jump_lands_on_the_row_named() {
    let folder = Folder::new();
    let player = Arc::new(player());
    let driven = Driven::player(&player);
    let first = ZoneId::FIRST;

    load_three(&player, &folder, true);
    player.settle(Command::SetShuffle(true)).expect("shuffled");

    eventually("the queue published shuffled", || {
        player.state().shuffle && the_zone(&driven, first).queue_length == 3
    });

    let zone = the_zone(&driven, first);
    let last = zone.rows.last().expect("a last row").title.clone();

    driven.control(first, RemoteCommand::Jump(2)).expect("jump");
    eventually("the row named playing", || {
        let zone = the_zone(&driven, first);
        zone.playing == Some(2) && zone.playing_row().is_some_and(|row| row.title == last)
    });
}

struct NoOpening;

impl Opens for NoOpening {
    fn open(&self, _: &Opening, _: &Player) -> EngineResult<Arc<Player>> {
        Ok(Arc::new(player()))
    }
}

struct NamedByTheCatalog;

impl RowNames for NamedByTheCatalog {
    fn rows(&self, _: &Player, items: &[Arc<QueueItem>]) -> Vec<RemoteRow> {
        items
            .iter()
            .map(|item| RemoteRow {
                title: format!("catalog says {}", unnamed(item)),
                artist: String::new(),
                length: None,
            })
            .collect()
    }
}

#[test]
fn each_zone_is_driven_alone_named_as_the_seam_names_its_rows_and_gone_with_its_zones() {
    let folder = Folder::new();
    let zones = Zones::start(
        Arc::new(player()),
        ZoneName::first_given_to(ZoneId::FIRST),
        Arc::new(NoOpening),
    );
    let second = zones.add().expect("a second zone");
    let driven = Driven::zones(&zones, NamedByTheCatalog);

    load_three(&second.player, &folder, false);

    eventually("the second zone mirrored by its own name and rows", || {
        let held = driven.zones();
        held.len() == 2
            && held[0].queue_length == 0
            && held[1].name == "Zone 2"
            && titles(&held[1])
                == [
                    "catalog says echoes",
                    "catalog says time",
                    "catalog says money",
                ]
    });

    driven
        .control(second.id, RemoteCommand::Volume(Permille::new(300)))
        .expect("the second zone's volume");
    driven
        .control(second.id, RemoteCommand::Jump(1))
        .expect("the second zone's jump");

    eventually("the second zone moved and the first not", || {
        let held = driven.zones();
        held[1].volume == Permille::new(300)
            && held[1].playing == Some(1)
            && held[0].volume == Permille::WHOLE
            && held[0].playing.is_none()
    });

    let by_tags = Driven::zones(&zones, ByTheirTags);
    eventually("the tags read for the same rows", || {
        titles(&the_zone(&by_tags, second.id)) == ["Echoes", "time", "money"]
    });

    drop(second);
    for zone in zones.listed() {
        if !zone.id.is_first() {
            zones.remove(zone.id).expect("the zone removed");
        }
    }
    drop(zones);

    assert!(driven.zones().is_empty());
    assert_eq!(
        driven.control(ZoneId::FIRST, RemoteCommand::Play),
        Err(Uncontrolled::NoSuchZone)
    );
}

struct NothingHeld;

impl Catalog for NothingHeld {
    fn playlists(&self) -> Result<Vec<LocalPlaylist>, Refused> {
        Ok(Vec::new())
    }

    fn write_playlist(
        &self,
        _: Option<LocalPlaylistId>,
        _: &str,
        _: &[Recording],
    ) -> Result<LocalPlaylist, Refused> {
        Err(Refused::Unwritable)
    }

    fn remove_playlist(&self, _: LocalPlaylistId) -> Result<(), Refused> {
        Ok(())
    }

    fn favourites(&self) -> Result<Vec<Recording>, Refused> {
        Ok(Vec::new())
    }

    fn favour(&self, _: &Recording, _: bool) -> Result<bool, Refused> {
        Ok(false)
    }

    fn listens_after(&self, _: u64, _: usize) -> Result<Vec<LocalListen>, Refused> {
        Ok(Vec::new())
    }

    fn credit(&self, _: &RemoteListen) -> Result<bool, Refused> {
        Ok(false)
    }

    fn resumption(&self) -> Result<Option<Resumed>, Refused> {
        Ok(None)
    }

    fn holds(&self, _: &Recording) -> Result<bool, Refused> {
        Ok(false)
    }

    fn holdings(&self) -> Result<Vec<Held>, Refused> {
        Ok(Vec::new())
    }

    fn open(&self, _: &Recording) -> Result<Option<Opened>, Refused> {
        Ok(None)
    }

    fn land(&self, _: &Recording, _: &SongTags, _: &str, _: &Path) -> Result<bool, Refused> {
        Ok(false)
    }

    fn revision(&self) -> u64 {
        0
    }
}

fn device(folder: &Path, key: &SyncKey, name: &str, told: Option<&Sync>) -> Sync {
    let told = told
        .and_then(Sync::nearby_port)
        .map(|port| SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .into_iter()
        .collect();
    let settings = Settings {
        server: Server::At(NO_SERVER_HERE.into()),
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
        relay_only: false,
        nearby: Some(Nearby {
            broadcast: Some(Broadcast { port: 0, told }),
            bonjour: false,
        }),
    };
    Sync::start(settings, Arc::new(NothingHeld)).expect("started")
}

fn mirrored_on(driver: &Sync, device: DeviceId, seen: impl Fn(&RemoteZone) -> bool) -> bool {
    driver.watch(device);
    matches!(
        driver.watched(device),
        Watched::Mirrored(mirror) if mirror.zones.first().is_some_and(&seen)
    )
}

#[test]
fn a_device_drives_a_real_engine_across_the_wire_as_far_as_it_is_trusted() {
    let folder = Folder::new();
    let key = SyncKey::minted().expect("a key");
    let player = Arc::new(player());
    let first = ZoneId::FIRST;

    load_three(&player, &folder, false);

    let desk = device(&folder.0, &key, "Desk", None);
    desk.control_with(Arc::new(Driven::player(&player)), RemoteTrust::Everything);
    let phone = device(&folder.0, &key, "Phone", Some(&desk));
    let deadline = Instant::now() + MEETS_WITHIN;
    while !phone
        .shown()
        .devices
        .iter()
        .any(|known| known.name == "Desk" && known.nearby)
    {
        assert!(Instant::now() < deadline, "the phone never met the desk");
        thread::sleep(LOOKED_AGAIN_AFTER);
    }
    let driven = desk.device();

    eventually("the desk's queue mirrored on the phone", || {
        mirrored_on(&phone, driven, |zone| {
            titles(zone) == ["Echoes", "time", "money"]
        })
    });

    assert_eq!(
        phone.control(driven, first, RemoteCommand::Play),
        Controlling::Done
    );
    eventually("the engine playing as the mirror says", || {
        player.state().playback == resonate_engine::PlaybackState::Playing
            && mirrored_on(&phone, driven, |zone| {
                zone.playback == RemotePlayback::Playing
            })
    });

    assert_eq!(
        phone.control(driven, first, RemoteCommand::Move { from: 2, to: 1 }),
        Controlling::Done
    );
    eventually("the move mirrored back", || {
        mirrored_on(&phone, driven, |zone| {
            titles(zone) == ["Echoes", "money", "time"]
        })
    });

    assert_eq!(
        phone.control(driven, first, RemoteCommand::Remove(9)),
        Controlling::NoSuchRow
    );

    desk.trust(RemoteTrust::Transport);
    assert_eq!(
        phone.control(driven, first, RemoteCommand::Remove(1)),
        Controlling::Untrusted
    );
    assert_eq!(player.queue().len(), 3);
    assert_eq!(
        phone.control(driven, first, RemoteCommand::Jump(1)),
        Controlling::Done
    );
    eventually("the jump heard", || {
        player.state().queue_position == Some(1)
    });

    desk.trust(RemoteTrust::Nothing);
    assert_eq!(
        phone.control(driven, first, RemoteCommand::Pause),
        Controlling::Untrusted
    );
    assert_ne!(
        player.state().playback,
        resonate_engine::PlaybackState::Paused
    );

    drop(phone);
    drop(desk);
}
