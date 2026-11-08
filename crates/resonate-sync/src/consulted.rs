use std::{sync::mpsc as std_mpsc, time::Duration};

use crate::DeviceId;

pub const CONSULT_ANSWERED_WITHIN: Duration = Duration::from_secs(5);
pub const KEPT_PAGES_A_MEETING: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptAnswer {
    pub seq: u64,
    pub at: u64,
    pub key: Vec<u8>,
    pub packed: Vec<u8>,
}

pub trait Consultable: Send + Sync + 'static {
    fn consult(&self, asked: &[u8]) -> Option<Vec<u8>>;

    fn kept_after(&self, after: u64, most: usize, asker: DeviceId) -> Vec<KeptAnswer>;

    fn kept_mark(&self, device: DeviceId) -> u64;

    fn take_kept(&self, device: DeviceId, answers: Vec<KeptAnswer>, mark: u64);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Consulting {
    Answered(Vec<u8>),
    Unconsulted,
    Unanswered,
}

pub struct AskedToConsult(pub(crate) std_mpsc::Receiver<Consulting>);

impl AskedToConsult {
    pub fn answer(self, within: Duration) -> Consulting {
        self.0
            .recv_timeout(within)
            .unwrap_or(Consulting::Unanswered)
    }
}
