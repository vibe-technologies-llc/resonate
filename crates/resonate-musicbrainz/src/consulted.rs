use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::{Answer, Asked, CopyPart, Kept, KeptAnswer, Mirrored, Offered, Peers};

pub const CONSULTED_AGAIN_AFTER: Duration = Duration::from_secs(10 * 60);

pub struct Consultation {
    mirror: Option<Arc<Mirrored>>,
    kept: Option<Arc<Kept>>,
}

impl Consultation {
    pub fn new(mirror: Option<Arc<Mirrored>>, kept: Option<Arc<Kept>>) -> Self {
        Self { mirror, kept }
    }

    pub fn consulted(&self, asked: &[u8]) -> Option<Vec<u8>> {
        let asked = match Asked::decoded(asked) {
            Ok(asked) => asked,
            Err(error) => {
                tracing::debug!(%error, "another device asked something this build cannot read");
                return None;
            }
        };
        if let Some(mirror) = self.mirror.as_ref().and_then(|mirror| mirror.held()) {
            match mirror.answer(&asked) {
                Ok(answer) => return Some(answer.encoded()),
                Err(error) => {
                    tracing::warn!(%error, "the mirror could not answer another device");
                }
            }
        }
        let kept = self.kept.as_ref()?;
        match kept.answer(&asked) {
            Ok(Some(answer)) => Some(answer.encoded()),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(%error, "the kept answers could not answer another device");
                None
            }
        }
    }

    pub fn offered(&self, part: CopyPart) -> Option<Offered> {
        let mirror = self.mirror.as_ref()?.held()?;
        mirror.offered(part).unwrap_or_else(|error| {
            tracing::warn!(%error, "the copy of MusicBrainz could not be offered to another device");
            None
        })
    }

    pub fn kept_after(&self, after: u64, most: usize, asker: &str) -> Vec<KeptAnswer> {
        let Some(kept) = &self.kept else {
            return Vec::new();
        };
        kept.after(after, most, asker).unwrap_or_else(|error| {
            tracing::warn!(%error, "the kept answers could not be listed for another device");
            Vec::new()
        })
    }

    pub fn kept_mark(&self, device: &str) -> u64 {
        self.kept
            .as_ref()
            .and_then(|kept| kept.mark_of(device).ok())
            .unwrap_or(0)
    }

    pub fn take_kept(&self, device: &str, answers: &[KeptAnswer], mark: u64) -> usize {
        let Some(kept) = &self.kept else {
            return 0;
        };
        kept.take(device, answers, mark).unwrap_or_else(|error| {
            tracing::warn!(%error, "the answers another device kept could not be taken");
            0
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asking {
    Answered(Vec<u8>),
    Unconsulted,
    Unanswered,
}

pub trait Ask: Send + Sync {
    fn devices(&self) -> Vec<String>;

    fn ask(&self, device: &str, asked: Vec<u8>) -> Asking;
}

pub struct Consulting<A> {
    asking: A,
    passed_over: Mutex<BTreeMap<String, Instant>>,
}

impl<A: Ask> Consulting<A> {
    pub fn new(asking: A) -> Self {
        Self {
            asking,
            passed_over: Mutex::new(BTreeMap::new()),
        }
    }
}

impl<A: Ask> Peers for Consulting<A> {
    fn consult(&self, asked: &Asked) -> Option<Answer> {
        let encoded = asked.encoded();
        for device in self.asking.devices() {
            let lately = self
                .passed_over
                .lock()
                .get(&device)
                .is_some_and(|at| at.elapsed() < CONSULTED_AGAIN_AFTER);
            if lately {
                continue;
            }
            match self.asking.ask(&device, encoded.clone()) {
                Asking::Answered(bytes) => match Answer::decoded(&bytes) {
                    Ok(answer) if asked.is_answered_by(&answer) => return Some(answer),
                    Ok(_) | Err(_) => {
                        tracing::debug!(%device, "another device answered otherwise than asked");
                    }
                },
                Asking::Unconsulted | Asking::Unanswered => {
                    self.passed_over.lock().insert(device, Instant::now());
                }
            }
        }
        None
    }
}
