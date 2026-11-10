use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use resonate_audio::NodeName;

pub const CLIP_BY_DEFAULT: Duration = Duration::from_secs(12);

pub const DESKTOP_IS_HEARD: bool = cfg!(not(target_os = "macos"));

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listening {
    Desktop,
    Microphone(Option<NodeName>),
}

impl Default for Listening {
    fn default() -> Self {
        if DESKTOP_IS_HEARD {
            Self::Desktop
        } else {
            Self::Microphone(None)
        }
    }
}

const DESKTOP: &str = "desktop";
const ANY_MICROPHONE: &str = "microphone";

impl Listening {
    pub fn named(text: &str) -> Self {
        match text.trim() {
            "" => Self::default(),
            DESKTOP => Self::Desktop,
            ANY_MICROPHONE => Self::Microphone(None),
            node => Self::Microphone(Some(NodeName::new(node.to_owned()))),
        }
    }

    pub fn written(&self) -> String {
        match self {
            Self::Desktop => DESKTOP.to_owned(),
            Self::Microphone(None) => ANY_MICROPHONE.to_owned(),
            Self::Microphone(Some(node)) => node.to_string(),
        }
    }

    pub const fn is_a_microphone(&self) -> bool {
        matches!(self, Self::Microphone(_))
    }
}

#[derive(Debug, Default)]
pub struct Hearing {
    pub(crate) heard: AtomicU64,
    pub(crate) wanted: AtomicU64,
    stopped: AtomicBool,
}

impl Hearing {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    pub fn share(&self) -> f32 {
        let wanted = self.wanted.load(Ordering::Relaxed);
        if wanted == 0 {
            return 0.0;
        }
        (self.heard.load(Ordering::Relaxed) as f64 / wanted as f64).min(1.0) as f32
    }
}
