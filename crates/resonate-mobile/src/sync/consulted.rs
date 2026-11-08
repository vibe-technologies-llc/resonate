use std::{path::Path, sync::Arc, time::Duration};

use parking_lot::Mutex;
use resonate_musicbrainz::{Ask, Asking, Consultation, KEPT_FILE, Kept, KeptAnswer, Peers};
use resonate_sync::{Consultable, Consulting, DEVICE_BYTES, DeviceId};

use super::Running;

const FOLDER: &str = "musicbrainz";
const ASKED_OF_A_DEVICE_WITHIN: Duration = Duration::from_secs(15);

pub(crate) struct Layers {
    pub(crate) kept: Option<Arc<Kept>>,
    pub(crate) peers: Arc<dyn Peers>,
}

pub(super) fn kept_under(folder: &Path) -> Option<Arc<Kept>> {
    match Kept::open(&folder.join(FOLDER).join(KEPT_FILE)) {
        Ok(kept) => Some(Arc::new(kept)),
        Err(error) => {
            tracing::warn!(%error, "the kept lookup answers could not be opened");
            None
        }
    }
}

pub(super) struct ConsultedHere(Consultation);

impl ConsultedHere {
    pub(super) fn of(kept: Option<Arc<Kept>>) -> Arc<dyn Consultable> {
        Arc::new(Self(Consultation::new(None, kept)))
    }
}

impl Consultable for ConsultedHere {
    fn consult(&self, asked: &[u8]) -> Option<Vec<u8>> {
        self.0.consulted(asked)
    }

    fn kept_after(
        &self,
        after: u64,
        most: usize,
        asker: DeviceId,
    ) -> Vec<resonate_sync::KeptAnswer> {
        self.0
            .kept_after(after, most, &asker.to_string())
            .into_iter()
            .map(|kept| resonate_sync::KeptAnswer {
                seq: kept.seq,
                at: kept.at,
                key: kept.key,
                packed: kept.packed,
            })
            .collect()
    }

    fn kept_mark(&self, device: DeviceId) -> u64 {
        self.0.kept_mark(&device.to_string())
    }

    fn take_kept(&self, device: DeviceId, answers: Vec<resonate_sync::KeptAnswer>, mark: u64) {
        let answers: Vec<KeptAnswer> = answers
            .into_iter()
            .map(|kept| KeptAnswer {
                seq: kept.seq,
                at: kept.at,
                key: kept.key,
                packed: kept.packed,
            })
            .collect();
        self.0.take_kept(&device.to_string(), &answers, mark);
    }
}

struct AskedOfDevices(Arc<Mutex<Option<Running>>>);

pub(super) fn peers_through(running: Arc<Mutex<Option<Running>>>) -> Arc<dyn Peers> {
    Arc::new(resonate_musicbrainz::Consulting::new(AskedOfDevices(
        running,
    )))
}

fn device_named(hex: &str) -> Option<DeviceId> {
    let mut id = DeviceId([0; DEVICE_BYTES]);
    if hex.len() != DEVICE_BYTES * 2 {
        return None;
    }
    for (at, byte) in id.0.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(at * 2..at * 2 + 2)?, 16).ok()?;
    }
    Some(id)
}

impl Ask for AskedOfDevices {
    fn devices(&self) -> Vec<String> {
        let Some(mut online) = self.0.lock().as_ref().map(|running| {
            running
                .sync
                .shown()
                .devices
                .into_iter()
                .filter(|device| device.online && !device.this)
                .collect::<Vec<_>>()
        }) else {
            return Vec::new();
        };
        online.sort_by_key(|device| !device.nearby);
        online.iter().map(|device| device.id.to_string()).collect()
    }

    fn ask(&self, device: &str, asked: Vec<u8>) -> Asking {
        let Some(id) = device_named(device) else {
            return Asking::Unanswered;
        };
        let asking = self
            .0
            .lock()
            .as_ref()
            .map(|running| running.sync.ask_to_consult(id, asked));
        match asking.map(|asking| asking.answer(ASKED_OF_A_DEVICE_WITHIN)) {
            Some(Consulting::Answered(answer)) => Asking::Answered(answer),
            Some(Consulting::Unconsulted) => Asking::Unconsulted,
            Some(Consulting::Unanswered) | None => Asking::Unanswered,
        }
    }
}
