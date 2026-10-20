use std::{
    env,
    fs::{self, File},
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
    AudioSource, Backend, Error as SinkError, Opener, SinkChange, SinkId, SinkInfo, SinkResult,
    SinkStream, StreamRequest, Surveyor,
};
use resonate_codec::Sources;
use resonate_core::{Frames, Gain, Resumable, Resumption, SampleRate, TrackId};
use resonate_engine::{Command, EngineConfig, Player};

use crate::{
    Documents, MobilePlayer, PlaybackPreferences, Snapshot, Track,
    catalog::Catalog,
    documents::{GrantedFiles, location},
    downloads::Downloads,
    features::Services,
    output::Routing,
    sync::Syncing,
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
        uri.strip_prefix(FOLDER)
            .and_then(|name| File::open(self.0.join(name)).ok())
            .map_or(-1, IntoRawFd::into_raw_fd)
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

pub(crate) struct Harness {
    pub(crate) player: MobilePlayer,
    pub(crate) songs: Vec<Track>,
    pub(crate) folder: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(player) = self.player.player.lock().take() {
            let _ = player.shutdown();
        }
        let _ = fs::remove_dir_all(&self.folder);
    }
}

impl Harness {
    pub(crate) fn new() -> Self {
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
        let sources = Arc::new(Sources::local().and(Arc::new(
            GrantedFiles::new(documents, "content").expect("a granted provider"),
        )));
        let engine =
            Player::with_sources_and_backend(EngineConfig::default(), Arc::clone(&sources), |_| {
                let (announces, changes) = unbounded();
                Ok(Box::new(Silent {
                    _announces: announces,
                    changes,
                }))
            })
            .expect("an engine with no device");
        let player = MobilePlayer {
            player: Mutex::new(Some(engine)),
            catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources,
            failure: Mutex::new(None),
            waiting: Mutex::new(None),
            saved: Mutex::new(None),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: Downloads::default(),
            syncing: Syncing::under(folder.clone()),
            routing: Arc::new(Routing::new()),
            enriching: crate::enrichment::Enriching::default(),
            room: crate::room::Room::under(std::env::temp_dir()),
            tag_undo: crate::tagging::TagUndo::default(),
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
            .player
            .lock()
            .as_ref()
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
