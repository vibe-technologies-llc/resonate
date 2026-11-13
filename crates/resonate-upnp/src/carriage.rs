use resonate_audio::{Plays, SinkFormats, Trial, Words};
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

const FLAC_RATES_UNSAID_AT_MOST_HZ: u32 = 96_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FlacUnsaid {
    #[default]
    UpTo96kHz,
    AnyRate,
}

const L16_RATES_UNSAID: [SampleRate; 2] = [SampleRate::HZ_44100, SampleRate::HZ_48000];

const CHANNELS_AT_MOST: u8 = 8;

const SURROUND_TRIED_FIRST_AT: SampleRate = SampleRate::HZ_48000;

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
struct Shape {
    rates: Vec<SampleRate>,
    layouts: Vec<ChannelLayout>,
}

impl Shape {
    fn holds(&self, spec: StreamSpec) -> bool {
        self.layouts.contains(&spec.channels) && self.rates.contains(&spec.rate)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carriage {
    wrapping: Wrapping,
    said: Shape,
    played: Vec<Shape>,
}

impl Carriage {
    pub const fn new(
        wrapping: Wrapping,
        rates: Vec<SampleRate>,
        layouts: Vec<ChannelLayout>,
    ) -> Self {
        Self {
            wrapping,
            said: Shape { rates, layouts },
            played: Vec::new(),
        }
    }

    pub const fn wrapping(&self) -> Wrapping {
        self.wrapping
    }

    fn shapes(&self) -> impl Iterator<Item = &Shape> {
        std::iter::once(&self.said).chain(&self.played)
    }

    pub fn formats(&self) -> Vec<SinkFormats> {
        self.shapes()
            .flat_map(|shape| {
                self.wrapping
                    .formats()
                    .iter()
                    .map(|(format, words)| SinkFormats {
                        format: *format,
                        words: *words,
                        rates: shape.rates.clone(),
                        channels: shape.layouts.clone(),
                    })
            })
            .collect()
    }

    pub fn rates(&self) -> Vec<SampleRate> {
        let mut rates = Vec::new();
        for rate in self.shapes().flat_map(|shape| &shape.rates) {
            if !rates.contains(rate) {
                rates.push(*rate);
            }
        }
        rates
    }

    pub fn carries(&self, spec: StreamSpec) -> bool {
        self.shapes().any(|shape| shape.holds(spec))
            && self
                .wrapping
                .formats()
                .iter()
                .any(|(format, _)| *format == spec.format)
    }

    pub fn with_the_rates_played(mut self, plays: &Plays) -> Self {
        let rates: Vec<SampleRate> = plays
            .rates()
            .filter(|rate| !self.carries_stereo_at(*rate))
            .collect();
        if !rates.is_empty() {
            self.played.push(Shape {
                rates,
                layouts: vec![ChannelLayout::Stereo],
            });
        }
        self
    }

    pub fn with_the_surround_played(mut self, plays: &Plays) -> Self {
        for (layout, highest) in plays.surround() {
            let layouts: Vec<ChannelLayout> = layouts_of(layout.count().get())
                .into_iter()
                .filter(|layout| !self.said.layouts.contains(layout))
                .collect();
            if layouts.is_empty() {
                continue;
            }
            let mut rates: Vec<SampleRate> = self
                .said
                .rates
                .iter()
                .copied()
                .filter(|rate| *rate <= highest)
                .collect();
            if !rates.contains(&highest) {
                rates.push(highest);
            }
            self.played.push(Shape { rates, layouts });
        }
        self
    }

    fn carries_stereo_at(&self, rate: SampleRate) -> bool {
        self.shapes().any(|shape| {
            shape.rates.contains(&rate) && shape.layouts.contains(&ChannelLayout::Stereo)
        })
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

    pub(crate) fn accepted_by(sink: &str, flac_unsaid: FlacUnsaid) -> Vec<Self> {
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
            let carriage = flac.carriage(Wrapping::Flac, &RATES_OFFERED, flac_unsaid);
            if !carriage.said.rates.is_empty() {
                carriages.push(carriage);
            }
        }
        if wave.entries > 0 {
            wave.rates.clear();
            carriages.push(wave.carriage(Wrapping::Wave, &RATES_OFFERED, flac_unsaid));
        }
        if l16.entries > 0 {
            carriages.push(l16.carriage(Wrapping::L16, &L16_RATES_UNSAID, flac_unsaid));
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

    fn carriage(
        self,
        wrapping: Wrapping,
        unsaid: &[SampleRate],
        flac_unsaid: FlacUnsaid,
    ) -> Carriage {
        let stated = !self.rates.is_empty();
        let mut rates = if stated { self.rates } else { unsaid.to_vec() };
        if wrapping == Wrapping::Flac && !stated && flac_unsaid == FlacUnsaid::UpTo96kHz {
            rates.retain(|rate| rate.hz() <= FLAC_RATES_UNSAID_AT_MOST_HZ);
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
        Carriage::new(wrapping, rates, layouts)
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

pub(crate) fn taught(said: &[Carriage], plays: Option<&Plays>) -> Vec<Carriage> {
    let mut carriages = said.to_vec();
    let Some(plays) = plays else {
        return carriages;
    };
    if let Some(at) = carriages
        .iter()
        .position(|carriage| carriage.wrapping == Wrapping::Flac)
    {
        carriages[at] = carriages[at].clone().with_the_rates_played(plays);
    }
    if let Some(at) = surround_tried_in(said) {
        carriages[at] = carriages[at].clone().with_the_surround_played(plays);
    }
    carriages
}

pub(crate) fn tried_in(said: &[Carriage], trial: Trial) -> Option<(Carriage, StreamSpec)> {
    let wrapping = if trial.is_a_rate() {
        said.iter()
            .find(|carriage| carriage.wrapping == Wrapping::Flac)?
            .wrapping
    } else {
        let carriage = said.get(surround_tried_in(said)?)?;
        if !carriage.said.rates.contains(&trial.rate()) {
            return None;
        }
        carriage.wrapping
    };
    let format = if wrapping
        .formats()
        .iter()
        .any(|(format, _)| *format == SampleFormat::S24)
    {
        SampleFormat::S24
    } else {
        SampleFormat::S16
    };
    let spec = trial.spec(format);
    let carriage = Carriage::new(wrapping, vec![spec.rate], vec![spec.channels]);
    Some((carriage, spec))
}

fn surround_tried_in(said: &[Carriage]) -> Option<usize> {
    said.iter().position(|carriage| {
        carriage.said.rates.contains(&SURROUND_TRIED_FIRST_AT)
            && carriage.said.layouts.contains(&ChannelLayout::Stereo)
    })
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
            if !rates.contains(&rate) {
                rates.push(rate);
            }
        }
    }
    rates.sort();
    (formats, rates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted(sink: &str) -> Vec<Carriage> {
        Carriage::accepted_by(sink, FlacUnsaid::default())
    }

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
            accepted(sink)
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
            accepted(l16)[0].rates(),
            [SampleRate::HZ_48000, SampleRate::HZ_44100]
        );
        assert_eq!(accepted(bare)[0].rates(), L16_RATES_UNSAID);
    }

    #[test]
    fn a_carriage_takes_sixteen_and_twenty_four_bit_stereo_and_only_what_the_renderer_said_beyond()
    {
        let wave = &accepted("http-get:*:audio/wav:*")[0];

        assert!(wave.carries(stereo(SampleRate::HZ_96000, SampleFormat::S24)));
        assert!(!wave.carries(stereo(SampleRate::HZ_96000, SampleFormat::F32)));
        assert!(!wave.carries(StreamSpec::new(
            SampleRate::HZ_44100,
            ChannelLayout::Surround51,
            SampleFormat::S16
        )));

        let l16 = &accepted("http-get:*:audio/L16;rate=44100;channels=2:*")[0];
        assert!(!l16.carries(stereo(SampleRate::HZ_44100, SampleFormat::S24)));
        assert!(!l16.carries(stereo(SampleRate::HZ_48000, SampleFormat::S16)));
    }

    #[test]
    fn flac_is_offered_up_to_ninety_six_kilohertz_unless_the_renderer_states_a_higher_rate() {
        let flac = &accepted("http-get:*:audio/flac:*")[0];

        assert!(flac.carries(stereo(SampleRate::HZ_96000, SampleFormat::S24)));
        assert!(!flac.carries(stereo(SampleRate::HZ_192000, SampleFormat::S16)));

        let stated =
            &accepted("http-get:*:audio/flac;rate=96000:*,http-get:*:audio/flac;rate=192000:*")[0];
        assert!(stated.carries(stereo(SampleRate::HZ_192000, SampleFormat::S24)));
        assert!(stated.carries(stereo(SampleRate::HZ_96000, SampleFormat::S24)));
        assert!(!stated.carries(stereo(SampleRate::HZ_44100, SampleFormat::S24)));
    }

    #[test]
    fn a_renderer_vouched_for_is_offered_flac_at_every_rate_but_one_that_states_rates_keeps_them() {
        let said = "http-get:*:audio/flac:*,http-get:*:audio/wav:*";
        let vouched = &Carriage::accepted_by(said, FlacUnsaid::AnyRate)[0];

        assert_eq!(vouched.wrapping(), Wrapping::Flac);
        assert_eq!(vouched.rates(), RATES_OFFERED);
        assert!(vouched.carries(stereo(SampleRate::HZ_192000, SampleFormat::S24)));
        assert!(!vouched.carries(StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S24
        )));

        let stated =
            &Carriage::accepted_by("http-get:*:audio/flac;rate=48000:*", FlacUnsaid::AnyRate)[0];
        assert_eq!(stated.rates(), [SampleRate::HZ_48000]);

        let wave = &Carriage::accepted_by("http-get:*:audio/wav:*", FlacUnsaid::AnyRate)[0];
        assert_eq!(wave.wrapping(), Wrapping::Wave);
    }

    #[test]
    fn channels_beyond_stereo_are_offered_only_where_the_protocol_info_states_them() {
        let said = accepted(
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
    fn what_a_renderer_played_joins_the_carriage_it_was_tried_in_and_nothing_else() {
        let said = accepted("http-get:*:audio/flac:*,http-get:*:audio/wav:*");
        let plays = Plays::from_iter([Trial::FlacAt192kHz, Trial::SixChannels]);

        let grown = taught(&said, Some(&plays));

        assert!(grown[0].carries(stereo(SampleRate::HZ_192000, SampleFormat::S24)));
        assert!(!grown[0].carries(stereo(SampleRate::HZ_176400, SampleFormat::S24)));
        assert!(grown[0].carries(StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S24
        )));
        assert!(!grown[0].carries(StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround71,
            SampleFormat::S24
        )));
        assert!(!grown[0].carries(StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Surround51,
            SampleFormat::S24
        )));
        assert!(!grown[0].carries(StreamSpec::new(
            SampleRate::HZ_192000,
            ChannelLayout::Surround51,
            SampleFormat::S24
        )));
        assert_eq!(grown[1], said[1]);
        assert_eq!(taught(&said, None), said);
        assert_eq!(taught(&said, Some(&Plays::default())), said);
    }

    #[test]
    fn surround_played_at_a_rate_is_offered_up_to_it_and_a_rate_played_in_stereo_alone() {
        let said = accepted(
            "http-get:*:audio/flac;rate=48000:*,http-get:*:audio/flac;rate=96000:*,http-get:*:audio/flac;rate=192000:*",
        );
        let plays = Plays::from_iter([
            Trial::FlacAt384kHz,
            Trial::SixChannels,
            Trial::SixChannelsAt96kHz,
        ]);
        let surround = |rate| StreamSpec::new(rate, ChannelLayout::Surround51, SampleFormat::S24);

        let grown = taught(&said, Some(&plays));

        assert!(grown[0].carries(surround(SampleRate::HZ_48000)));
        assert!(grown[0].carries(surround(SampleRate::HZ_96000)));
        assert!(!grown[0].carries(surround(SampleRate::HZ_192000)));
        assert!(grown[0].carries(stereo(SampleRate::HZ_384000, SampleFormat::S24)));
        assert!(!grown[0].carries(StreamSpec::new(
            SampleRate::HZ_384000,
            ChannelLayout::Surround51,
            SampleFormat::S24
        )));
        assert_eq!(
            unite(&grown).1,
            [
                SampleRate::HZ_48000,
                SampleRate::HZ_96000,
                SampleRate::HZ_192000,
                SampleRate::HZ_384000
            ]
        );
        assert!(
            tried_in(&said, Trial::SixChannelsAt96kHz).is_some(),
            "the carriage holding 48 kHz holds 96 kHz too"
        );
        assert!(
            tried_in(
                &accepted("http-get:*:audio/L16;rate=48000:*"),
                Trial::SixChannelsAt96kHz
            )
            .is_none(),
            "surround is tried at 96 kHz only where the carriage takes the rate"
        );
    }

    #[test]
    fn a_rate_is_tried_in_flac_alone_and_surround_in_the_carriage_preferred_at_forty_eight_kilohertz()
     {
        let flac = accepted("http-get:*:audio/flac:*,http-get:*:audio/wav:*");
        let wave = accepted("http-get:*:audio/wav:*");
        let l16 = accepted("http-get:*:audio/L16;rate=48000:*");

        let (carriage, spec) = tried_in(&flac, Trial::FlacAt192kHz).expect("FLAC is taken");
        assert_eq!(carriage.wrapping(), Wrapping::Flac);
        assert_eq!(spec, stereo(SampleRate::HZ_192000, SampleFormat::S24));
        assert!(carriage.carries(spec));

        assert!(tried_in(&wave, Trial::FlacAt176kHz).is_none());
        let (carriage, spec) = tried_in(&wave, Trial::EightChannels).expect("WAVE at 48 kHz");
        assert_eq!(carriage.wrapping(), Wrapping::Wave);
        assert_eq!(spec.channels, ChannelLayout::Surround71);

        let (carriage, spec) = tried_in(&l16, Trial::SixChannels).expect("L16 at 48 kHz");
        assert_eq!(carriage.wrapping(), Wrapping::L16);
        assert_eq!(spec.format, SampleFormat::S16);
        assert!(carriage.carries(spec));
    }

    #[test]
    fn what_several_wrappings_carry_is_united_and_the_rates_sorted() {
        let carriages = accepted(
            "http-get:*:audio/flac:*,http-get:*:audio/wav:*,http-get:*:audio/L16;rate=44100;channels=2:*",
        );
        let (formats, rates) = unite(&carriages);

        assert_eq!(formats.len(), 5);
        assert_eq!(rates, RATES_OFFERED);
    }
}
