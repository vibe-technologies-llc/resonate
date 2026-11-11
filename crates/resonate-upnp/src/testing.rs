use std::sync::Arc;

use resonate_audio::{AudioSource, StreamClock};
use resonate_core::{ChannelLayout, Frames, SampleFormat, SampleRate, StreamSpec};

use crate::{Carriage, Feed, Wrapping};

pub(crate) struct Silence;

impl AudioSource for Silence {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        dst.len()
    }
}

pub(crate) fn silent_feed() -> Arc<Feed> {
    let spec = StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::S16,
    );
    let carriage = Carriage::new(
        Wrapping::Wave,
        vec![SampleRate::HZ_44100],
        vec![ChannelLayout::Stereo],
    );
    Arc::new(Feed::new(
        spec,
        carriage,
        Box::new(Silence),
        Arc::new(StreamClock::behind_by(Frames::ZERO)),
    ))
}
