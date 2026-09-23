use resonate_codec::{Codec, Speakers};
use resonate_core::{ChannelCount, SampleRate, StreamSpec};

pub(crate) const WIDEST_FLAC_BITS: u8 = 24;
pub(crate) const MOST_FLAC_CHANNELS: u8 = 8;
pub const FASTEST_FLAC_RATE: SampleRate = SampleRate::HZ_96000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Form {
    #[default]
    Flac,
    Wave,
    Kept,
}

impl Form {
    pub const ALL: [Self; 3] = [Self::Flac, Self::Wave, Self::Kept];

    pub const fn of(codec: Codec, spec: StreamSpec, bits: u8) -> Self {
        if !codec.is_lossless()
            || matches!(codec, Codec::Dsd)
            || spec.channel_count().get() > MOST_FLAC_CHANNELS
        {
            return Self::Kept;
        }
        if spec.format.is_float()
            || bits > WIDEST_FLAC_BITS
            || spec.rate.hz() > FASTEST_FLAC_RATE.hz()
        {
            return Self::Wave;
        }
        Self::Flac
    }

    pub fn placing(self, speakers: Speakers, channels: ChannelCount) -> Self {
        match self {
            Self::Flac
                if speakers.is_named()
                    && Speakers::in_flac_order(channels.get()) != Some(speakers) =>
            {
                Self::Wave.placing(speakers, channels)
            }
            Self::Wave if speakers.wave_mask().is_none() => Self::Kept,
            placed => placed,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Wave => "wave",
            Self::Kept => "kept",
        }
    }
}
