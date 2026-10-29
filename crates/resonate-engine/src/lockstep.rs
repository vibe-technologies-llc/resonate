use std::{fmt, sync::Arc};

use parking_lot::RwLock;
use resonate_audio::Moment;
use resonate_core::{Frames, QueueStamp, SampleRate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeardAt {
    pub row: QueueStamp,
    pub frame: Frames,
    pub at: Moment,
    pub rate: SampleRate,
}

impl HeardAt {
    pub fn frame_at(&self, moment: Moment) -> f64 {
        self.frame.get() as f64 + moment.frames_since(self.at, self.rate)
    }
}

#[derive(Clone, Default)]
pub struct HeardClock(Arc<RwLock<Option<HeardAt>>>);

impl HeardClock {
    pub fn read(&self) -> Option<HeardAt> {
        *self.0.read()
    }

    pub(crate) fn tell(&self, heard: Option<HeardAt>) {
        *self.0.write() = heard;
    }
}

impl PartialEq for HeardClock {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for HeardClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("HeardClock").field(&self.read()).finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stepping {
    #[default]
    Alone,
    Waiting,
    Joined(Moment),
}
