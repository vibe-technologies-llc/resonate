use std::{
    env,
    fs::{self, File},
    io::ErrorKind,
    os::fd::IntoRawFd,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Backend, Error as SinkError, NodeName, Opener, SinkChange, SinkFormats, SinkId,
    SinkInfo, SinkResult, SinkStream, StreamClock, StreamRequest, Surveyor, Words,
};
use resonate_codec::Sources;
use resonate_core::{
    ChannelLayout, Frames, Gain, Resumable, Resumption, SampleFormat, SampleRate, TrackId, ZoneId,
};
use resonate_engine::{Command, EngineConfig, Player};
use resonate_upnp::{Network, Searching};

use crate::{
    Documents, MobilePlayer, PlaybackPreferences, Snapshot, Track,
    catalog::Catalog,
    documents::{DOCUMENT_NOT_FOUND, DOCUMENT_REFUSED, GrantedFiles, location},
    downloads::Downloads,
    features::Services,
    output::{self, Routing},
    rendering::Rendering,
    sync::Syncing,
    zoning::{Engines, Playing},
};

pub(crate) const RATE: u32 = 44_100;
pub(crate) const SECONDS_A_SONG: u32 = 2;
pub(crate) const SETTLED_WITHIN: Duration = Duration::from_secs(10);
pub(crate) const LOOKED_AGAIN_AFTER: Duration = Duration::from_millis(10);
pub(crate) const FOLDER: &str = "content://test/";
pub(crate) const SONGS: [&str; 4] = ["a", "b", "c", "d"];

pub(crate) struct Folder(PathBuf);

impl Documents for Folder {
    fn open_document(&self, uri: String) -> i32 {
        let Some(name) = uri.strip_prefix(FOLDER) else {
            return DOCUMENT_REFUSED;
        };
        match File::open(self.0.join(name)) {
            Ok(file) => file.into_raw_fd(),
            Err(error) if error.kind() == ErrorKind::NotFound => DOCUMENT_NOT_FOUND,
            Err(_) => DOCUMENT_REFUSED,
        }
    }
}

struct Silent {
    _announces: Sender<SinkChange>,
    changes: Receiver<SinkChange>,
}

struct Nowhere;

impl Surveyor for Nowhere {
    fn enumerate_sinks(&self, _: Duration) -> SinkResult<Vec<SinkInfo>> {
        Ok(Vec::new())
    }
}

impl Opener for Nowhere {
    fn open(&self, _: &StreamRequest, _: Box<dyn AudioSource>) -> SinkResult<SinkStream> {
        Err(SinkError::NoSink)
    }
}

impl Backend for Silent {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Nowhere)
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Nowhere)
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

struct Sounding {
    _announces: Sender<SinkChange>,
    changes: Receiver<SinkChange>,
    sink: SinkId,
}

struct Heard(SinkId);

fn speakers(sink: SinkId) -> SinkInfo {
    let rate = SampleRate::new(RATE).expect("a rate");
    SinkInfo {
        id: sink,
        name: NodeName::new("test.speakers"),
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

impl Surveyor for Heard {
    fn enumerate_sinks(&self, _: Duration) -> SinkResult<Vec<SinkInfo>> {
        Ok(vec![speakers(self.0)])
    }
}

impl Opener for Heard {
    fn open(&self, _: &StreamRequest, source: Box<dyn AudioSource>) -> SinkResult<SinkStream> {
        let (events, incoming) = unbounded();
        let held = Mutex::new((events, source));
        Ok(SinkStream::new(
            incoming,
            Arc::new(StreamClock::behind_by(Frames(0))),
            Box::new(move |_| {
                let _kept = &held;
                Ok(())
            }),
        ))
    }
}

impl Backend for Sounding {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Heard(self.sink))
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Heard(self.sink))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Device {
    None,
    Speakers,
}

pub(crate) struct Harness {
    pub(crate) player: MobilePlayer,
    pub(crate) songs: Vec<Track>,
    pub(crate) folder: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(zones) = self.player.playing.take() {
            let _ = crate::zoning::shut_down(zones);
        }
        let _ = fs::remove_dir_all(&self.folder);
    }
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self::over(Device::None)
    }

    pub(crate) fn sounding() -> Self {
        Self::over(Device::Speakers)
    }

    pub(crate) fn beside_renderers(searching: Searching) -> Self {
        Self::beside(Device::Speakers, Rendering::resting(searching))
    }

    fn over(device: Device) -> Self {
        Self::beside(device, Rendering::default())
    }

    fn beside(device: Device, rendering: Rendering) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let folder = env::temp_dir().join(format!(
            "resonate-mobile-queue-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&folder).expect("a test folder");
        for name in SONGS {
            fs::write(folder.join(format!("{name}.wav")), silence()).expect("a song");
        }

        let documents: Arc<dyn Documents> = Arc::new(Folder(folder.clone()));
        let served = crate::served::Served::default();
        let sources = Arc::new(served.sources(Sources::local().and(Arc::new(
            GrantedFiles::new(documents, "content").expect("a granted provider"),
        ))));
        let network = rendering.network();
        let engines: Box<Engines> = {
            let sources = Arc::clone(&sources);
            let network = network.clone();
            Box::new(
                move |zone: ZoneId, config: EngineConfig, _: &Arc<Routing>| {
                    engine_over(device, zone, config, Arc::clone(&sources), network.clone())
                },
            )
        };
        let engine = engine_over(
            device,
            ZoneId::FIRST,
            EngineConfig::default(),
            Arc::clone(&sources),
            network,
        )
        .expect("an engine over the backend");
        let player = MobilePlayer {
            playing: Playing::start(engine, Arc::new(Routing::new()), None, engines),
            catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources,
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: Downloads::default(),
            syncing: Syncing::under(folder.clone()),
            enriching: crate::enrichment::Enriching::default(),
            room: crate::room::Room::under(std::env::temp_dir()),
            tagging: crate::tagging::Tagging::under(std::env::temp_dir()),
            served,
            rendering,
        };

        let songs = SONGS
            .iter()
            .map(|name| {
                player
                    .index_document(format!("{FOLDER}{name}.wav"), format!("{name}.wav"))
                    .expect("an indexed song")
            })
            .collect();

        Self {
            player,
            songs,
            folder,
        }
    }

    pub(crate) fn ids(&self, names: &str) -> Vec<u64> {
        names.chars().map(|name| self.song(name).id).collect()
    }

    pub(crate) fn song(&self, name: char) -> &Track {
        self.songs
            .iter()
            .find(|song| song.title.starts_with(name))
            .expect("a song of that name")
    }

    pub(crate) fn resumed(&self, names: &str, row: usize, at_ms: u64) {
        let rows = self
            .ids(names)
            .into_iter()
            .map(|id| Resumable {
                location: location(&self.player.catalog.track(id).expect("a song").uri)
                    .expect("a document"),
                span: None,
                held: Some(TrackId::new(id).expect("an id")),
            })
            .collect();
        let resumption = Resumption {
            rows,
            row,
            at: Frames::from_duration(
                Duration::from_millis(at_ms),
                SampleRate::new(RATE).expect("a rate"),
            ),
            ..Resumption::default()
        };

        self.player
            .playing
            .player()
            .expect("an open player")
            .request(Command::Resume(resumption))
            .expect("a resumption sent")
            .wait_for(SETTLED_WITHIN)
            .expect("a resumption");
        self.settled(|snapshot| snapshot.current.is_some());
    }

    pub(crate) fn queued(&self) -> String {
        self.player
            .queue()
            .expect("the queue")
            .iter()
            .map(name_of)
            .collect()
    }

    pub(crate) fn queue_ids(&self) -> Vec<u64> {
        self.player
            .queue()
            .expect("the queue")
            .iter()
            .map(|track| track.queue_id.expect("a queue row id"))
            .collect()
    }

    pub(crate) fn settled(&self, until: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let began = Instant::now();
        loop {
            let snapshot = self.player.snapshot().expect("a snapshot");
            if until(&snapshot) {
                return snapshot;
            }
            assert!(
                began.elapsed() < SETTLED_WITHIN,
                "never settled: {snapshot:?}"
            );
            thread::sleep(LOOKED_AGAIN_AFTER);
        }
    }

    pub(crate) fn playing(&self) -> (char, u32, u64) {
        let snapshot = self.player.snapshot().expect("a snapshot");
        let current = snapshot.current.expect("a current song");

        (
            name_of(&current),
            snapshot.queue_index,
            snapshot.position_ms,
        )
    }
}

fn engine_over(
    device: Device,
    zone: ZoneId,
    config: EngineConfig,
    sources: Arc<Sources>,
    network: Option<Arc<Network>>,
) -> resonate_engine::Result<Player> {
    Player::with_sources_and_backend(config, sources, move |_| {
        let (announces, changes) = unbounded();
        let backend: Box<dyn Backend> = match device {
            Device::None => Box::new(Silent {
                _announces: announces,
                changes,
            }),
            Device::Speakers => Box::new(Sounding {
                _announces: announces,
                changes,
                sink: SinkId::new(zone.get()),
            }),
        };
        Ok(output::beside(backend, network))
    })
}

pub(crate) fn name_of(track: &Track) -> char {
    track.title.chars().next().expect("a titled song")
}

pub(crate) fn silence() -> Vec<u8> {
    let channels = 2_u16;
    let bits = 16_u16;
    let block_align = channels * bits / 8;
    let data = vec![0_u8; (RATE * SECONDS_A_SONG) as usize * usize::from(block_align)];

    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&channels.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&bits.to_le_bytes());

    let mut body = b"WAVE".to_vec();
    for (id, payload) in [(b"fmt ", &format), (b"data", &data)] {
        body.extend_from_slice(id);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
    }

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}
