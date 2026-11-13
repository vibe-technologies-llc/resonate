use std::{
    io::{Read, Seek},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc as std_mpsc,
    },
    time::Duration,
};

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

    fn export(&self, _part: ExportPart) -> Option<OfferedExport> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportPart {
    Copy,
    Amendments,
}

impl ExportPart {
    pub(crate) const fn coded(self) -> u8 {
        match self {
            Self::Copy => 0,
            Self::Amendments => 1,
        }
    }

    pub(crate) const fn of_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Copy),
            1 => Some(Self::Amendments),
            _ => None,
        }
    }
}

pub trait ReadsExport: Read + Seek + Send {}

impl<T: Read + Seek + Send> ReadsExport for T {}

pub struct OfferedExport {
    pub dump: String,
    pub sequence: u64,
    pub size: u64,
    pub reader: Box<dyn ReadsExport>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportHeld {
    pub dump: String,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportFetched {
    Whole(ExportHeld),
    AlreadyHeld,
    NotHeld,
    Failed,
}

#[derive(Debug, Default)]
pub struct ExportReceiving {
    received: AtomicU64,
    of: AtomicU64,
}

impl ExportReceiving {
    pub fn so_far(&self) -> (u64, u64) {
        (
            self.received.load(Ordering::Relaxed),
            self.of.load(Ordering::Relaxed),
        )
    }

    pub(crate) fn heard(&self, received: u64, of: u64) {
        self.received.store(received, Ordering::Relaxed);
        self.of.store(of, Ordering::Relaxed);
    }
}

pub struct AskedForExport(pub(crate) std_mpsc::Receiver<ExportFetched>);

impl AskedForExport {
    pub fn answer(&self) -> ExportFetched {
        self.0.recv().unwrap_or(ExportFetched::Failed)
    }

    pub fn answer_within(&self, within: Duration) -> Option<ExportFetched> {
        match self.0.recv_timeout(within) {
            Ok(fetched) => Some(fetched),
            Err(std_mpsc::RecvTimeoutError::Timeout) => None,
            Err(std_mpsc::RecvTimeoutError::Disconnected) => Some(ExportFetched::Failed),
        }
    }
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
