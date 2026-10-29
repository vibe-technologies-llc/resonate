use resonate_audio::{SinkFormats, Words};
use resonate_core::{ChannelCount, ChannelLayout, SampleFormat, SampleRate, StreamSpec};

const FLAC_KINDS: [&str; 2] = ["audio/flac", "audio/x-flac"];

const WAVE_KINDS: [&str; 4] = ["audio/wav", "audio/x-wav", "audio/wave", "audio/vnd.wave"];

const L16_KIND: &str = "audio/l16";

const RATES_OFFERED: [SampleRate; 6] = [
    SampleRate::HZ_44100,
    SampleRate::HZ_48000,
    SampleRate::HZ_88200,
    SampleRate::HZ_96000,
    SampleRate::HZ_176400,
    SampleRate::HZ_192000,
];

const FLAC_RATES_AT_MOST_HZ: u32 = 96_000;

const L16_RATES_UNSAID: [SampleRate; 2] = [SampleRate::HZ_44100, SampleRate::HZ_48000];

const CHANNELS_AT_MOST: u8 = 8;

const CHANNELS_UNSAID: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Wrapping {
    Flac,
    Wave,
    L16,
}

impl Wrapping {
    const fn kind(self) -> &'static str {
        match self {
            Self::Flac => "audio/flac",
            Self::Wave => "audio/wav",
            Self::L16 => "audio/L16",
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Wave => "wav",
            Self::L16 => "pcm",
        }
    }

    const fn formats(self) -> &'static [(SampleFormat, Words)] {
        match self {
            Self::Flac | Self::Wave => &[
                (SampleFormat::S16, Words::Whole),
                (SampleFormat::S24, Words::Packed),
            ],
            Self::L16 => &[(SampleFormat::S16, Words::Whole)],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carriage {
    wrapping: Wrapping,
    rates: Vec<SampleRate>,
    layouts: Vec<ChannelLayout>,
}

impl Carriage {
    pub const fn wrapping(&self) -> Wrapping {
        self.wrapping
    }

    pub fn formats(&self) -> Vec<SinkFormats> {
        self.wrapping
            .formats()
            .iter()
            .map(|(format, words)| SinkFormats {
                format: *format,
                words: *words,
                rates: self.rates.clone(),
                channels: self.layouts.clone(),
            })
            .collect()
    }

    pub fn rates(&self) -> &[SampleRate] {
        &self.rates
    }

    pub fn carries(&self, spec: StreamSpec) -> bool {
        self.layouts.contains(&spec.channels)
            && self.rates.contains(&spec.rate)
            && self
                .wrapping
                .formats()
                .iter()
                .any(|(format, _)| *format == spec.format)
    }

    pub fn kind_of(&self, spec: StreamSpec) -> String {
        match self.wrapping {
            Wrapping::L16 => format!(
                "{};rate={};channels={}",
                self.wrapping.kind(),
                spec.rate.hz(),
                spec.channel_count().get()
            ),
            Wrapping::Flac | Wrapping::Wave => self.wrapping.kind().to_owned(),
        }
    }

    pub const fn extension(&self) -> &'static str {
        self.wrapping.extension()
    }

    pub(crate) fn accepted_by(sink: &str) -> Vec<Self> {
        let mut flac = Seen::default();
        let mut wave = Seen::default();
        let mut l16 = Seen::default();
        for entry in sink.split(',') {
            let Some(kind) = entry.split(':').nth(2) else {
                continue;
            };
            let kind = kind.trim().to_ascii_lowercase();
            let mut parts = kind.split(';');
            let named = parts.next().unwrap_or_default().trim();
            let seen = if FLAC_KINDS.contains(&named) {
                &mut flac
            } else if WAVE_KINDS.contains(&named) {
                &mut wave
            } else if named == L16_KIND {
                &mut l16
            } else {
                continue;
            };
            seen.read(parts);
        }

        let mut carriages = Vec::new();
        if flac.entries > 0 {
            let carriage = flac.carriage(Wrapping::Flac, &RATES_OFFERED);
            if !carriage.rates.is_empty() {
                carriages.push(carriage);
            }
        }
        if wave.entries > 0 {
            wave.rates.clear();
            carriages.push(wave.carriage(Wrapping::Wave, &RATES_OFFERED));
        }
        if l16.entries > 0 {
            carriages.push(l16.carriage(Wrapping::L16, &L16_RATES_UNSAID));
        }
        carriages
    }
}

#[derive(Default)]
struct Seen {
    entries: usize,
    rates: Vec<SampleRate>,
    counts: Vec<u8>,
}

impl Seen {
    fn read<'a>(&mut self, parameters: impl Iterator<Item = &'a str>) {
        self.entries += 1;
        for parameter in parameters {
            let parameter = parameter.trim();
            if let Some(rate) = parameter.strip_prefix("rate=")
                && let Ok(hz) = rate.parse()
                && let Ok(rate) = SampleRate::new(hz)
                && !self.rates.contains(&rate)
            {
                self.rates.push(rate);
            } else if let Some(count) = parameter.strip_prefix("channels=")
                && let Ok(count) = count.parse::<u8>()
                && (1..=CHANNELS_AT_MOST).contains(&count)
                && !self.counts.contains(&count)
            {
                self.counts.push(count);
            }
        }
    }

    fn carriage(self, wrapping: Wrapping, unsaid: &[SampleRate]) -> Carriage {
        let mut rates = if self.rates.is_empty() {
            unsaid.to_vec()
        } else {
            self.rates
        };
        if wrapping == Wrapping::Flac {
            rates.retain(|rate| rate.hz() <= FLAC_RATES_AT_MOST_HZ);
        }
        let counts = if self.counts.is_empty() {
            vec![CHANNELS_UNSAID]
        } else {
            self.counts
        };
        let mut layouts = Vec::new();
        for count in counts {
            for layout in layouts_of(count) {
                if !layouts.contains(&layout) {
                    layouts.push(layout);
                }
            }
        }
        Carriage {
            wrapping,
            rates,
            layouts,
        }
    }
}

fn layouts_of(count: u8) -> Vec<ChannelLayout> {
    let Ok(counted) = ChannelCount::new(u16::from(count)) else {
        return Vec::new();
    };
    let named = match count {
        5 => ChannelLayout::Surround50,
        7 => ChannelLayout::Surround61,
        _ => ChannelLayout::from_count(counted),
    };
    let unnamed = ChannelLayout::from_count(counted);
    if named == unnamed {
        vec![named]
    } else {
        vec![named, unnamed]
    }
}

pub(crate) fn unite(carriages: &[Carriage]) -> (Vec<SinkFormats>, Vec<SampleRate>) {
    let mut formats: Vec<SinkFormats> = Vec::new();
    let mut rates: Vec<SampleRate> = Vec::new();
    for carriage in carriages {
        for offered in carriage.formats() {
            if !formats.contains(&offered) {
                formats.push(offered);
            }
        }
        for rate in carriage.rates() {
            if !rates.contains(rate) {
                rates.push(*rate);
            }
        }
    }
    rates.sort();
    (formats, rates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(rate: SampleRate, format: SampleFormat) -> StreamSpec {
        StreamSpec::new(rate, ChannelLayout::Stereo, format)
    }

    #[test]
    fn a_renderer_is_offered_every_wrapping_it_names_flac_first_and_l16_at_its_rates() {
        let wave = "http-get:*:audio/mpeg:*,http-get:*:audio/x-wav:*,http-get:*:audio/L16;rate=44100;channels=2:*";
        let l16 = "http-get:*:audio/L16;rate=48000;channels=2:*,http-get:*:audio/L16;rate=44100;channels=2:DLNA.ORG_PN=LPCM";
        let bare = "http-get:*:audio/L16:*";
        let flac = "http-get:*:audio/wav:*,http-get:*:audio/x-flac:*,http-get:*:audio/flac:*";

        let wrappings = |sink| {
            Carriage::accepted_by(sink)
                .iter()
                .map(Carriage::wrapping)
                .collect::<Vec<_>>()
        };

        assert_eq!(wrappings(wave), [Wrapping::Wave, Wrapping::L16]);
        assert_eq!(wrappings(flac), [Wrapping::Flac, Wrapping::Wave]);
        assert_eq!(wrappings(l16), [Wrapping::L16]);
        assert_eq!(wrappings(bare), [Wrapping::L16]);
        assert!(wrappings("http-get:*:audio/mpeg:*").is_empty());

        assert_eq!(
            Carriage::accepted_by(l16)[0].rates(),
            [SampleRate::HZ_48000, SampleRate::HZ_44100]
        );
        assert_eq!(Carriage::accepted_by(bare)[0].rates(), L16_RATES_UNSAID);
    }

    #[test]
    fn a_carriage_takes_sixteen_and_twenty_four_bit_stereo_and_only_what_the_renderer_said_beyond()
    {
        let wave = &Carriage::accepted_by("http-get:*:audio/wav:*")[0];

        assert!(wave.carries(stereo(SampleRate::HZ_96000, SampleFormat::S24)));
        assert!(!wave.carries(stereo(SampleRate::HZ_96000, SampleFormat::F32)));
        assert!(!wave.carries(StreamSpec::new(
            SampleRate::HZ_44100,
            ChannelLayout::Surround51,
            SampleFormat::S16
        )));

        let l16 = &Carriage::accepted_by("http-get:*:audio/L16;rate=44100;channels=2:*")[0];
        assert!(!l16.carries(stereo(SampleRate::HZ_44100, SampleFormat::S24)));
        assert!(!l16.carries(stereo(SampleRate::HZ_48000, SampleFormat::S16)));
    }

    #[test]
    fn flac_is_offered_up_to_ninety_six_kilohertz_and_a_wrapping_without_rates_is_not_offered() {
        let flac = &Carriage::accepted_by("http-get:*:audio/flac:*")[0];

        assert!(flac.carries(stereo(SampleRate::HZ_96000, SampleFormat::S24)));
        assert!(!flac.carries(stereo(SampleRate::HZ_192000, SampleFormat::S16)));
        assert!(Carriage::accepted_by("http-get:*:audio/flac;rate=192000;channels=2:*").is_empty());
    }

    #[test]
    fn channels_beyond_stereo_are_offered_only_where_the_protocol_info_states_them() {
        let said = Carriage::accepted_by(
            "http-get:*:audio/flac;channels=2:*,http-get:*:audio/flac;channels=6:*,http-get:*:audio/flac;channels=3:*",
        );
        let flac = &said[0];

        assert!(flac.carries(StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S24
        )));
        assert!(flac.carries(StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::from_count(ChannelCount::new(3).expect("three channels")),
            SampleFormat::S16
        )));
        assert!(!flac.carries(StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround71,
            SampleFormat::S16
        )));
        assert!(flac.carries(stereo(SampleRate::HZ_48000, SampleFormat::S16)));
    }

    #[test]
    fn what_several_wrappings_carry_is_united_and_the_rates_sorted() {
        let carriages = Carriage::accepted_by(
            "http-get:*:audio/flac:*,http-get:*:audio/wav:*,http-get:*:audio/L16;rate=44100;channels=2:*",
        );
        let (formats, rates) = unite(&carriages);

        assert_eq!(formats.len(), 5);
        assert_eq!(rates, RATES_OFFERED);
    }
}
