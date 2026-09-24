use std::{iter, num::NonZeroUsize, sync::Arc, time::Duration};

use resonate_core::{
    SampleRate, StreamSpec,
    eq::{Biquad, MAX_BANDS, Profile},
};

use crate::{Error, ProcessCount, Processor, Result, fused::multiply_add};

const DENORMAL_FLOOR: f64 = 1e-30;
const EASED_OVER: Duration = Duration::from_millis(40);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Easing {
    Entering,
    Returning,
    Leaving,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Blend {
    wet_for: usize,
    over: NonZeroUsize,
    toward_wet: bool,
}

impl Blend {
    const fn settled_wet() -> Self {
        Self {
            wet_for: 1,
            over: NonZeroUsize::MIN,
            toward_wet: true,
        }
    }

    fn over(&mut self, over: NonZeroUsize) {
        let was_wet = self.wet_for == self.over.get();
        self.over = over;
        self.wet_for = if was_wet {
            over.get()
        } else {
            self.wet_for.min(over.get())
        };
    }

    fn ease(&mut self, easing: Easing) {
        match easing {
            Easing::Entering => {
                self.wet_for = 0;
                self.toward_wet = true;
            }
            Easing::Returning => self.toward_wet = true,
            Easing::Leaving => self.toward_wet = false,
        }
    }

    const fn is_settled_wet(&self) -> bool {
        self.toward_wet && self.wet_for == self.over.get()
    }

    const fn is_moving(&self) -> bool {
        if self.toward_wet {
            self.wet_for < self.over.get()
        } else {
            self.wet_for > 0
        }
    }

    fn settle(&mut self) {
        self.wet_for = if self.toward_wet { self.over.get() } else { 0 };
    }

    fn mixed(&mut self, dry: &[f64], wet: &mut [f64], channels: usize) {
        let over = self.over.get();
        for (dry, wet) in dry
            .chunks_exact(channels)
            .zip(wet.chunks_exact_mut(channels))
        {
            match self.wet_for {
                0 => wet.copy_from_slice(dry),
                all if all == over => {}
                some => {
                    let weight = some as f64 / over as f64;
                    for (dry, wet) in dry.iter().zip(wet.iter_mut()) {
                        *wet = dry + (*wet - dry) * weight;
                    }
                }
            }
            self.wet_for = if self.toward_wet {
                (self.wet_for + 1).min(over)
            } else {
                self.wet_for.saturating_sub(1)
            };
        }
    }
}

fn usable(value: f64) -> f64 {
    let magnitude = value.abs();
    if magnitude > DENORMAL_FLOOR && magnitude < f64::INFINITY {
        value
    } else {
        0.0
    }
}

const SECTIONS_STAGGERED: usize = 4;
const CACHE_LINE_BYTES: usize = 64;
const SAMPLES_A_LINE: usize = CACHE_LINE_BYTES / size_of::<f64>();

#[derive(Clone, Copy)]
enum Group {
    One,
    Two,
    Four,
    Six,
    Eight,
}

impl Group {
    const WIDEST: usize = 8;
    const FOR_WHAT_IS_LEFT: [&[Self]; Self::WIDEST] = [
        &[],
        &[Self::One],
        &[Self::Two],
        &[Self::Two, Self::One],
        &[Self::Four],
        &[Self::Four, Self::One],
        &[Self::Six],
        &[Self::Six, Self::One],
    ];

    const fn width(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Six => 6,
            Self::Eight => Self::WIDEST,
        }
    }

    fn splitting(channels: usize) -> impl Iterator<Item = Self> {
        let left = Self::FOR_WHAT_IS_LEFT
            .get(channels % Self::WIDEST)
            .copied()
            .unwrap_or_default();
        iter::repeat_n(Self::Eight, channels / Self::WIDEST).chain(left.iter().copied())
    }
}

fn biquad(filter: Biquad, sample: f64, history: [f64; 4]) -> f64 {
    let [behind_in, further_behind_in, behind_out, further_behind_out] = history;
    let fed_forward = multiply_add(
        filter.b2,
        further_behind_in,
        multiply_add(filter.b1, behind_in, filter.b0 * sample),
    );
    let settled = multiply_add(-filter.a2, further_behind_out, fed_forward);
    usable(multiply_add(-filter.a1, behind_out, settled))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Section {
    behind_in: f64,
    further_behind_in: f64,
    behind_out: f64,
    further_behind_out: f64,
}

#[derive(Clone, Copy)]
struct Lanes<const WIDTH: usize> {
    behind_in: [f64; WIDTH],
    further_behind_in: [f64; WIDTH],
    behind_out: [f64; WIDTH],
    further_behind_out: [f64; WIDTH],
}

impl<const WIDTH: usize> Lanes<WIDTH> {
    fn gathered(sections: [Section; WIDTH]) -> Self {
        Self {
            behind_in: sections.map(|section| section.behind_in),
            further_behind_in: sections.map(|section| section.further_behind_in),
            behind_out: sections.map(|section| section.behind_out),
            further_behind_out: sections.map(|section| section.further_behind_out),
        }
    }

    fn scattered(self) -> [Section; WIDTH] {
        let mut sections = [Section::default(); WIDTH];
        for (section, history) in sections.iter_mut().zip(self.histories()) {
            let [behind_in, further_behind_in, behind_out, further_behind_out] = history;
            *section = Section {
                behind_in,
                further_behind_in,
                behind_out,
                further_behind_out,
            };
        }
        sections
    }

    fn histories(&self) -> impl Iterator<Item = [f64; 4]> {
        self.behind_in
            .into_iter()
            .zip(self.further_behind_in)
            .zip(self.behind_out.into_iter().zip(self.further_behind_out))
            .map(
                |((behind_in, further_behind_in), (behind_out, further_behind_out))| {
                    [behind_in, further_behind_in, behind_out, further_behind_out]
                },
            )
    }

    fn run(&mut self, filters: &[Biquad; WIDTH], entering: [f64; WIDTH]) -> [f64; WIDTH] {
        let mut leaving = entering;
        for ((sample, history), filter) in leaving.iter_mut().zip(self.histories()).zip(filters) {
            *sample = biquad(*filter, *sample, history);
        }
        self.further_behind_in = self.behind_in;
        self.behind_in = entering;
        self.further_behind_out = self.behind_out;
        self.behind_out = leaving;
        leaving
    }
}

fn run_staggered<const WIDTH: usize, const SECTIONS: usize>(
    filters: &[[Biquad; WIDTH]; SECTIONS],
    kept: &mut [[Section; WIDTH]; SECTIONS],
    frames: &mut [[f64; WIDTH]],
) {
    let mut held = kept.map(Lanes::gathered);
    for leading in 0..frames.len() + SECTIONS - 1 {
        for (behind, (filter, lanes)) in filters.iter().zip(held.iter_mut()).enumerate() {
            if let Some(frame) = frames.get_mut(leading.wrapping_sub(behind)) {
                *frame = lanes.run(filter, *frame);
            }
        }
    }
    *kept = held.map(Lanes::scattered);
}

fn take_staggered<'a, 'b, const WIDTH: usize, const SECTIONS: usize>(
    filters: &'a [[Biquad; WIDTH]],
    kept: &'b mut [[Section; WIDTH]],
    frames: &mut [[f64; WIDTH]],
) -> (&'a [[Biquad; WIDTH]], &'b mut [[Section; WIDTH]]) {
    let Some((taken, filters)) = filters.split_first_chunk::<SECTIONS>() else {
        return (filters, kept);
    };
    let Some((held, kept)) = kept.split_first_chunk_mut::<SECTIONS>() else {
        return (&[], &mut []);
    };
    run_staggered(taken, held, frames);
    (filters, kept)
}

fn cascade<const WIDTH: usize>(
    coefficients: &[Biquad],
    state: &mut [Section],
    frames: &mut [[f64; WIDTH]],
) {
    let mut filters = coefficients.as_chunks::<WIDTH>().0;
    let mut kept = state.as_chunks_mut::<WIDTH>().0;
    while filters.len() >= SECTIONS_STAGGERED {
        (filters, kept) = take_staggered::<WIDTH, SECTIONS_STAGGERED>(filters, kept, frames);
    }
    let (filters, kept) = take_staggered::<WIDTH, 2>(filters, kept, frames);
    take_staggered::<WIDTH, 1>(filters, kept, frames);
}

fn widen(taken: &[f64], block: &mut [f64], preamp: f64) {
    for (slot, sample) in block.iter_mut().zip(taken) {
        *slot = usable(sample * preamp);
    }
}

fn narrow(block: &[f64], made: &mut [f64]) {
    for (slot, carried) in made.iter_mut().zip(block) {
        *slot = *carried;
    }
}

fn line_padded(samples: usize) -> usize {
    samples
        .checked_next_multiple_of(SAMPLES_A_LINE)
        .unwrap_or(samples)
}

struct Widened {
    samples: Vec<f64>,
    origin: usize,
}

impl Widened {
    fn holding(len: usize) -> Self {
        let samples = vec![0.0; len.saturating_add(SAMPLES_A_LINE - 1)];
        let offset = samples.as_ptr().align_offset(CACHE_LINE_BYTES);
        Self {
            origin: if offset < SAMPLES_A_LINE { offset } else { 0 },
            samples,
        }
    }

    fn aligned(&mut self) -> &mut [f64] {
        self.samples.get_mut(self.origin..).unwrap_or_default()
    }
}

struct Piece<'a> {
    taken: &'a [f64],
    made: &'a mut [f64],
    channels: usize,
    first_channel: usize,
    preamp: f64,
}

impl Piece<'_> {
    fn equalise<const WIDTH: usize>(
        &mut self,
        coefficients: &[Biquad],
        state: &mut [Section],
        widened: &mut [f64],
    ) {
        let frames = widened.as_chunks_mut::<WIDTH>().0;
        if self.channels == WIDTH {
            widen(self.taken, frames.as_flattened_mut(), self.preamp);
        } else {
            for (taken, slot) in self
                .taken
                .chunks_exact(self.channels)
                .zip(frames.iter_mut())
            {
                widen(
                    taken.get(self.first_channel..).unwrap_or_default(),
                    slot,
                    self.preamp,
                );
            }
        }
        cascade(coefficients, state, frames);
        if self.channels == WIDTH {
            narrow(frames.as_flattened(), self.made);
        } else {
            for (made, slot) in self.made.chunks_exact_mut(self.channels).zip(frames.iter()) {
                narrow(slot, made.get_mut(self.first_channel..).unwrap_or_default());
            }
        }
        self.first_channel += WIDTH;
    }
}

fn eased_over(rate: SampleRate) -> NonZeroUsize {
    let frames = (EASED_OVER.as_secs_f64() * f64::from(rate.hz())).round() as usize;
    NonZeroUsize::new(frames).unwrap_or(NonZeroUsize::MIN)
}

pub struct Equaliser {
    profile: Arc<Profile>,
    rate: SampleRate,
    channels: NonZeroUsize,
    preamp: f64,
    coefficients: Vec<Biquad>,
    bands: usize,
    state: Vec<Section>,
    frames_a_piece: NonZeroUsize,
    widened: Widened,
    blend: Blend,
}

impl Equaliser {
    pub fn new(profile: Arc<Profile>, rate: SampleRate) -> Self {
        let mut stage = Self {
            profile,
            rate,
            channels: NonZeroUsize::MIN,
            preamp: 1.0,
            coefficients: Vec::with_capacity(MAX_BANDS),
            bands: 0,
            state: Vec::new(),
            frames_a_piece: NonZeroUsize::MIN,
            widened: Widened::holding(0),
            blend: Blend::settled_wet(),
        };
        stage.blend.over(eased_over(rate));
        stage.take_the_profile();
        stage
    }

    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    pub fn profile(&self) -> &Arc<Profile> {
        &self.profile
    }

    pub const fn sections(&self) -> usize {
        self.bands
    }

    fn take_the_profile(&mut self) {
        self.preamp = self.profile.preamp().amplitude();
        let bands = self
            .profile
            .bands()
            .get(..MAX_BANDS)
            .unwrap_or(self.profile.bands());
        self.bands = bands.len();
        self.coefficients.clear();

        let mut first_channel = 0;
        for group in Group::splitting(self.channels.get()) {
            for band in bands {
                self.coefficients.extend(
                    (first_channel..first_channel + group.width())
                        .map(|channel| band.design_for(self.rate, channel)),
                );
            }
            first_channel += group.width();
        }
    }

    fn silence_what_is_no_longer_reached(&mut self) {
        let reached = self.bands;
        let mut state = self.state.as_mut_slice();
        for group in Group::splitting(self.channels.get()) {
            let Some((kept, rest)) = state.split_at_mut_checked(group.width() * MAX_BANDS) else {
                return;
            };
            for section in kept.iter_mut().skip(group.width() * reached) {
                *section = Section::default();
            }
            state = rest;
        }
    }
}

impl Processor for Equaliser {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        if spec.rate != self.rate {
            return Err(Error::InputRateMismatch {
                expected: self.rate,
                actual: spec.rate,
            });
        }

        let channels = usize::from(spec.channel_count().get());
        self.channels = NonZeroUsize::new(channels).unwrap_or(NonZeroUsize::MIN);
        self.state.clear();
        self.state
            .resize(self.channels.get() * MAX_BANDS, Section::default());
        self.coefficients = Vec::with_capacity(self.channels.get() * MAX_BANDS);
        self.frames_a_piece = NonZeroUsize::new(max_frames_in).unwrap_or(NonZeroUsize::MIN);
        let room = Group::splitting(channels)
            .map(|group| line_padded(group.width().saturating_mul(self.frames_a_piece.get())))
            .fold(0, usize::saturating_add);
        self.widened = Widened::holding(room);
        self.take_the_profile();

        Ok(max_frames_in)
    }

    fn reset(&mut self) {
        self.state.fill(Section::default());
        self.blend.settle();
    }

    fn latency_frames(&self) -> f64 {
        0.0
    }

    fn is_transparent(&self) -> bool {
        self.profile.is_transparent()
    }

    fn is_ramping(&self) -> bool {
        self.blend.is_moving()
    }

    fn ease_equalisation(&mut self, easing: Easing) {
        self.blend.ease(easing);
    }

    fn set_equalisation(&mut self, profile: &Arc<Profile>) {
        self.profile = Arc::clone(profile);
        self.take_the_profile();
        self.silence_what_is_no_longer_reached();
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels.get();
        let span = self.frames_a_piece.saturating_mul(self.channels).get();
        let frames = (input.len() / channels).min(output.len() / channels);
        let designed = self.coefficients.as_slice();
        let bands = self.bands;

        for (taken, made) in input.chunks(span).zip(output.chunks_mut(span)) {
            let whole = taken.len().min(made.len()) / channels;
            let mut state = self.state.as_mut_slice();
            let mut widened = self.widened.aligned();
            let mut coefficients = designed;
            let mut piece = Piece {
                taken,
                made,
                channels,
                first_channel: 0,
                preamp: self.preamp,
            };
            for group in Group::splitting(channels) {
                let width = group.width();
                let (Some((kept, more_kept)), Some((room, more_room))) = (
                    state.split_at_mut_checked(width * MAX_BANDS),
                    widened.split_at_mut_checked(line_padded(width * whole)),
                ) else {
                    break;
                };
                let Some(block) = room.get_mut(..width * whole) else {
                    break;
                };
                let Some((ours, theirs)) = coefficients.split_at_checked(width * bands) else {
                    break;
                };
                match group {
                    Group::One => {
                        piece.equalise::<{ Group::One.width() }>(ours, kept, block);
                    }
                    Group::Two => {
                        piece.equalise::<{ Group::Two.width() }>(ours, kept, block);
                    }
                    Group::Four => {
                        piece.equalise::<{ Group::Four.width() }>(ours, kept, block);
                    }
                    Group::Six => {
                        piece.equalise::<{ Group::Six.width() }>(ours, kept, block);
                    }
                    Group::Eight => {
                        piece.equalise::<{ Group::Eight.width() }>(ours, kept, block);
                    }
                }
                state = more_kept;
                widened = more_room;
                coefficients = theirs;
            }
            if !self.blend.is_settled_wet() {
                let reached = whole * channels;
                self.blend.mixed(
                    taken.get(..reached).unwrap_or_default(),
                    piece.made.get_mut(..reached).unwrap_or_default(),
                    channels,
                );
            }
        }

        ProcessCount {
            frames_in: frames,
            frames_out: frames,
        }
    }

    fn flush(&mut self, _output: &mut [f64]) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{
        ChannelCount, ChannelLayout, SampleFormat,
        eq::{Band, BandGain, BandKind, Frequency, Preamp, Q},
    };

    use super::*;

    const BLOCK: usize = 1024;
    const SETTLING_BLOCKS: usize = 24;
    const NULL_BELOW_DB: f64 = -60.0;
    const PREPARED_FRAMES: usize = 256;
    const DECAYING_FRAMES: usize = 4_096;

    fn spec(rate: SampleRate) -> StreamSpec {
        StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::F32)
    }

    fn band(kind: BandKind, at: f64, gain: f64, q: f64) -> Band {
        Band::new(
            kind,
            Frequency::from_hertz(at).expect("in range"),
            BandGain::from_decibels(gain).expect("in range"),
            Q::from_units(q).expect("in range"),
        )
    }

    fn profile(bands: Vec<Band>) -> Arc<Profile> {
        Arc::new(Profile::new(Preamp::NONE, bands).expect("a profile in range"))
    }

    fn prepared(profile: &Arc<Profile>, rate: SampleRate) -> Equaliser {
        let mut stage = Equaliser::new(Arc::clone(profile), rate);
        stage
            .prepare(spec(rate), BLOCK)
            .expect("the stage takes the rate it was built for");
        stage
    }

    fn tone(hertz: f64, rate: SampleRate, from: usize, frames: usize) -> Vec<f64> {
        let step = std::f64::consts::TAU * hertz / f64::from(rate.hz());
        (0..frames)
            .flat_map(|frame| {
                let sample = ((from + frame) as f64 * step).sin();
                [sample, sample]
            })
            .collect()
    }

    fn measured_db(stage: &mut Equaliser, hertz: f64, rate: SampleRate) -> f64 {
        let cycles_a_block = f64::from(rate.hz()) / hertz / BLOCK as f64;
        let settling = SETTLING_BLOCKS + (cycles_a_block.ceil() as usize) * SETTLING_BLOCKS;
        let mut output = vec![0.0; BLOCK * 2];
        let mut highest = 0.0_f64;
        for block in 0..=settling {
            let input = tone(hertz, rate, block * BLOCK, BLOCK);
            stage.process(&input, &mut output);
            if block >= settling.saturating_sub(4) {
                highest = output
                    .iter()
                    .fold(highest, |peak, sample| peak.max(sample.abs()));
            }
        }
        20.0 * highest.log10()
    }

    fn loud_and_attenuated() -> Arc<Profile> {
        Arc::new(
            Profile::new(
                Preamp::from_decibels(-6.0).expect("in range"),
                vec![band(BandKind::Peaking, 1_000.0, 6.0, 1.0)],
            )
            .expect("a profile in range"),
        )
    }

    fn played(stage: &mut Equaliser, rate: SampleRate, blocks: usize) -> (Vec<f64>, Vec<f64>) {
        played_from(stage, rate, 0, blocks)
    }

    fn played_from(
        stage: &mut Equaliser,
        rate: SampleRate,
        from: usize,
        blocks: usize,
    ) -> (Vec<f64>, Vec<f64>) {
        let mut dry = Vec::new();
        let mut wet = Vec::new();
        let mut output = vec![0.0; BLOCK * 2];
        for block in 0..blocks {
            let input = tone(997.0, rate, from + block * BLOCK, BLOCK);
            stage.process(&input, &mut output);
            dry.extend_from_slice(&input);
            wet.extend_from_slice(&output);
        }
        (dry, wet)
    }

    fn largest_step(samples: &[f64]) -> f64 {
        let frames = samples.as_chunks::<2>().0;
        frames
            .windows(2)
            .map(|pair| (pair[1][0] - pair[0][0]).abs())
            .fold(0.0_f64, f64::max)
    }

    #[test]
    fn a_stage_entering_a_running_stream_starts_dry_and_is_wholly_wet_once_eased_in() {
        let rate = SampleRate::HZ_48000;
        let wanted = loud_and_attenuated();
        let over = eased_over(rate).get();
        let blocks = over / BLOCK + 2;
        let at_the_crest = 12;
        let before = tone(997.0, rate, at_the_crest - 1, 1);
        let mut plain = prepared(&wanted, rate);
        let mut entering = prepared(&wanted, rate);
        entering.ease_equalisation(Easing::Entering);

        assert!(entering.is_ramping());
        let (dry, eased) = played_from(&mut entering, rate, at_the_crest, blocks);
        let (_, wet) = played_from(&mut plain, rate, at_the_crest, blocks);

        assert!(!entering.is_ramping(), "the easing outlived its length");
        assert_eq!(eased[..2], dry[..2], "the first frame was not the dry one");
        assert_eq!(eased[over * 2..], wet[over * 2..]);
        let heard = largest_step(&[before.as_slice(), &dry].concat());
        let eased_in = largest_step(&[before.as_slice(), &eased].concat());
        let switched = largest_step(&[before.as_slice(), &wet].concat());
        assert!(
            eased_in <= heard * 1.05,
            "an eased entrance stepped {eased_in} where the tone steps {heard}"
        );
        assert!(
            switched > heard * 2.0,
            "a stage switched in at once stepped only {switched} where the tone steps {heard}"
        );
    }

    #[test]
    fn a_stage_leaving_eases_out_to_the_dry_signal_and_stays_there() {
        let rate = SampleRate::HZ_44100;
        let over = eased_over(rate).get();
        let mut stage = prepared(&loud_and_attenuated(), rate);
        played(&mut stage, rate, 4);

        stage.ease_equalisation(Easing::Leaving);
        assert!(stage.is_ramping());
        let (dry, eased) = played(&mut stage, rate, over / BLOCK + 2);

        assert!(!stage.is_ramping(), "the easing outlived its length");
        assert_ne!(eased[..2], dry[..2]);
        assert_eq!(eased[over * 2..], dry[over * 2..]);
    }

    #[test]
    fn a_stage_asked_back_while_leaving_returns_to_the_wet_signal() {
        let rate = SampleRate::HZ_44100;
        let over = eased_over(rate).get();
        let wanted = loud_and_attenuated();
        let mut plain = prepared(&wanted, rate);
        let mut stage = prepared(&wanted, rate);
        played(&mut plain, rate, 1);
        played(&mut stage, rate, 1);

        stage.ease_equalisation(Easing::Leaving);
        played(&mut plain, rate, 1);
        played(&mut stage, rate, 1);
        stage.ease_equalisation(Easing::Returning);
        let blocks = over / BLOCK + 2;
        let (_, wet) = played(&mut plain, rate, blocks);
        let (_, returned) = played(&mut stage, rate, blocks);

        assert!(!stage.is_ramping());
        assert_eq!(returned[over * 2..], wet[over * 2..]);
    }

    #[test]
    fn a_settled_stage_asked_to_return_is_left_alone() {
        let rate = SampleRate::HZ_44100;
        let wanted = loud_and_attenuated();
        let mut plain = prepared(&wanted, rate);
        let mut stage = prepared(&wanted, rate);
        stage.ease_equalisation(Easing::Returning);

        assert!(!stage.is_ramping());
        assert_eq!(played(&mut stage, rate, 2), played(&mut plain, rate, 2));
    }

    #[test]
    fn the_gain_a_band_advertises_is_the_gain_the_stage_applies() {
        for rate in [SampleRate::HZ_44100, SampleRate::HZ_192000] {
            for kind in BandKind::ALL {
                for at in [120.0, 1_000.0, 9_000.0] {
                    let wanted = profile(vec![band(kind, at, 6.0, 1.2)]);
                    let modelled = wanted.magnitude_db(at, rate);
                    let mut stage = prepared(&wanted, rate);
                    let measured = measured_db(&mut stage, at, rate);
                    if modelled < NULL_BELOW_DB {
                        assert!(
                            measured < NULL_BELOW_DB,
                            "{kind} at {at} Hz, {rate}: {measured}"
                        );
                        continue;
                    }
                    assert!(
                        (measured - modelled).abs() < 0.05,
                        "{kind} at {at} Hz, {rate}: modelled {modelled} dB, measured {measured} dB"
                    );
                }
            }
        }
    }

    #[test]
    fn a_low_frequency_high_q_shelf_at_the_highest_rate_holds_its_gain() {
        let rate = SampleRate::HZ_192000;
        let wanted = profile(vec![band(BandKind::LowShelf, 20.0, 12.0, 4.0)]);
        let mut stage = prepared(&wanted, rate);

        let modelled = wanted.magnitude_db(20.0, rate);
        let measured = measured_db(&mut stage, 20.0, rate);
        assert!(
            (measured - modelled).abs() < 0.05,
            "modelled {modelled} dB, measured {measured} dB"
        );
    }

    #[test]
    fn a_profile_with_no_bands_leaves_the_stage_transparent_so_the_chain_drops_it() {
        let rate = SampleRate::HZ_48000;
        let empty = Arc::new(Profile::flat());
        assert!(Equaliser::new(empty, rate).is_transparent());

        let flat = profile(vec![band(BandKind::Peaking, 1_000.0, 0.0, 1.0)]);
        assert!(
            !Equaliser::new(flat, rate).is_transparent(),
            "a profile holding a band keeps its stage so a later change reaches it"
        );
    }

    #[test]
    fn a_profile_of_flat_bands_passes_the_samples_through_untouched() {
        let rate = SampleRate::HZ_48000;
        let flat = profile(vec![
            band(BandKind::Peaking, 100.0, 0.0, 1.0),
            band(BandKind::Peaking, 1_000.0, 0.0, 1.0),
        ]);
        let mut stage = prepared(&flat, rate);

        let input = tone(440.0, rate, 0, BLOCK);
        let mut output = vec![0.0; BLOCK * 2];
        stage.process(&input, &mut output);

        for (made, taken) in output.iter().zip(input.iter()) {
            assert!((made - taken).abs() < 1e-7, "{made} against {taken}");
        }
    }

    #[test]
    fn a_band_for_the_left_channel_shapes_the_left_and_passes_the_right_through_untouched() {
        let rate = SampleRate::HZ_48000;
        let left_only = Band {
            channels: resonate_core::eq::ChannelSet::of(&[0]).expect("the left channel"),
            ..band(BandKind::Peaking, 200.0, 12.0, 4.0)
        };
        let mut stage = prepared(&profile(vec![left_only]), rate);

        let input: Vec<f64> = (0..BLOCK * 2)
            .map(|at| if at / 2 == 0 { 1.0 } else { 0.0 })
            .collect();
        let mut output = vec![0.0; BLOCK * 2];
        stage.process(&input, &mut output);

        let frames = output.as_chunks::<2>().0;
        assert_eq!(
            frames[0][1], 1.0,
            "the right channel was shaped by a left band"
        );
        assert!(frames.iter().skip(1).all(|frame| frame[1] == 0.0));
        assert!(
            frames.iter().skip(1).any(|frame| frame[0] != 0.0),
            "the left channel was not shaped by its own band"
        );
    }

    #[test]
    fn every_channel_keeps_a_history_of_its_own() {
        let rate = SampleRate::HZ_48000;
        let wanted = profile(vec![band(BandKind::Peaking, 200.0, 12.0, 4.0)]);
        let mut stage = prepared(&wanted, rate);

        let mut input = vec![0.0_f64; BLOCK * 2];
        for frame in 0..BLOCK {
            if let Some(slot) = input.get_mut(frame * 2) {
                *slot = if frame == 0 { 1.0 } else { 0.0 };
            }
        }

        let mut output = vec![0.0; BLOCK * 2];
        stage.process(&input, &mut output);

        let right_moved = output
            .as_chunks::<2>()
            .0
            .iter()
            .any(|frame| frame[1] != 0.0);
        assert!(!right_moved, "a transient on the left reached the right");
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .any(|frame| frame[0] != 0.0)
        );
    }

    #[test]
    fn retuning_a_band_carries_the_history_rather_than_clicking() {
        let rate = SampleRate::HZ_48000;
        let before = profile(vec![band(BandKind::Peaking, 200.0, 3.0, 1.0)]);
        let mut stage = prepared(&before, rate);

        let mut output = vec![0.0; BLOCK * 2];
        let mut widest_within = 0.0_f64;
        for block in 0..8 {
            let input = tone(200.0, rate, block * BLOCK, BLOCK);
            stage.process(&input, &mut output);
            for pair in output.as_chunks::<2>().0.windows(2) {
                widest_within = widest_within.max((pair[1][0] - pair[0][0]).abs());
            }
        }
        let last = output.get(output.len() - 2).copied().unwrap_or_default();

        stage.set_equalisation(&profile(vec![band(BandKind::Peaking, 200.0, 4.0, 1.0)]));
        let input = tone(200.0, rate, 8 * BLOCK, BLOCK);
        stage.process(&input, &mut output);
        let first = output.first().copied().unwrap_or_default();

        assert!(
            (first - last).abs() < widest_within * 4.0,
            "the seam stepped {} where a block steps at most {widest_within}",
            (first - last).abs()
        );
    }

    #[test]
    fn changing_the_band_count_while_it_runs_allocates_nothing() {
        let rate = SampleRate::HZ_48000;
        let one = band(BandKind::Peaking, 100.0, 3.0, 1.0);
        let mut stage = prepared(&profile(vec![one; 3]), rate);

        let held = stage.coefficients.capacity();
        let lanes = stage.state.len();
        let room = stage.widened.samples.capacity();

        for count in [5, 2, MAX_BANDS, 1] {
            stage.set_equalisation(&profile(vec![one; count]));
            assert_eq!(stage.sections(), count);
            assert_eq!(stage.coefficients.capacity(), held);
            assert_eq!(stage.state.len(), lanes);
            assert_eq!(stage.widened.samples.capacity(), room);
        }
    }

    #[test]
    fn a_band_that_is_no_longer_reached_leaves_no_history_behind_it() {
        let rate = SampleRate::HZ_48000;
        let loud = band(BandKind::Peaking, 1_000.0, 18.0, 2.0);
        let mut stage = prepared(&profile(vec![loud; 4]), rate);
        let quiet = vec![0.0_f64; BLOCK * 2];
        let mut output = vec![0.0; BLOCK * 2];

        stage.process(&tone(1_000.0, rate, 0, BLOCK), &mut output);
        stage.set_equalisation(&profile(vec![loud]));
        for _ in 0..8 {
            stage.process(&quiet, &mut output);
        }

        stage.set_equalisation(&profile(vec![loud; 4]));
        stage.process(&quiet, &mut output);

        let carried = output
            .iter()
            .fold(0.0_f64, |peak, sample| peak.max(sample.abs()));
        assert!(carried == 0.0, "a vacated lane thumped at {carried}");
    }

    #[test]
    fn a_tail_that_has_decayed_is_flushed_rather_than_left_denormal() {
        let rate = SampleRate::HZ_48000;
        let wanted = profile(vec![band(BandKind::Peaking, 1_000.0, 12.0, 1.0)]);
        let mut stage = prepared(&wanted, rate);

        let mut output = vec![0.0; BLOCK * 2];
        let mut kick = vec![0.0_f64; BLOCK * 2];
        if let Some(slot) = kick.first_mut() {
            *slot = 1.0;
        }
        stage.process(&kick, &mut output);

        let quiet = vec![0.0_f64; BLOCK * 2];
        for _ in 0..8 {
            stage.process(&quiet, &mut output);
        }
        assert!(
            stage.state.iter().all(|section| section.behind_out == 0.0),
            "a decayed tail was left as a denormal"
        );
    }

    #[test]
    fn a_sample_that_is_not_a_number_does_not_poison_the_history() {
        let rate = SampleRate::HZ_48000;
        let wanted = profile(vec![band(BandKind::Peaking, 200.0, 6.0, 2.0)]);
        let mut stage = prepared(&wanted, rate);

        let mut output = vec![0.0; BLOCK * 2];
        stage.process(&vec![f64::NAN; BLOCK * 2], &mut output);
        stage.process(&tone(200.0, rate, 0, BLOCK), &mut output);

        assert!(
            output.iter().all(|sample| sample.is_finite()),
            "a corrupt sample outlived the block it arrived in"
        );
    }

    #[test]
    fn the_preamp_scales_ahead_of_the_bands_the_way_the_profile_text_says() {
        let rate = SampleRate::HZ_48000;
        let wanted = Arc::new(
            Profile::new(
                Preamp::from_decibels(-6.0).expect("in range"),
                vec![band(BandKind::Peaking, 1_000.0, 6.0, 1.0)],
            )
            .expect("one band"),
        );
        let mut stage = prepared(&wanted, rate);

        let at_centre = measured_db(&mut stage, 1_000.0, rate);
        assert!(at_centre.abs() < 0.05, "{at_centre} dB at the centre");

        let mut stage = prepared(&wanted, rate);
        let well_away = measured_db(&mut stage, 60.0, rate);
        assert!((well_away + 6.0).abs() < 0.1, "{well_away} dB well away");
    }

    #[test]
    fn a_stage_built_for_one_rate_refuses_another() {
        let wanted = profile(vec![band(BandKind::Peaking, 1_000.0, 3.0, 1.0)]);
        let mut stage = Equaliser::new(wanted, SampleRate::HZ_44100);

        let refused = stage.prepare(spec(SampleRate::HZ_88200), BLOCK);
        assert!(matches!(
            refused,
            Err(Error::InputRateMismatch {
                expected: SampleRate::HZ_44100,
                actual: SampleRate::HZ_88200,
            })
        ));
    }

    #[test]
    fn equalising_changes_no_frame_count_and_adds_no_latency() {
        let rate = SampleRate::HZ_48000;
        let wanted = profile(vec![band(BandKind::Peaking, 1_000.0, 3.0, 1.0)]);
        let mut stage = prepared(&wanted, rate);

        let mut output = vec![0.0; BLOCK * 2];
        let count = stage.process(&tone(440.0, rate, 0, BLOCK), &mut output);
        assert_eq!(count.frames_in, BLOCK);
        assert_eq!(count.frames_out, BLOCK);
        assert_eq!(stage.latency_frames(), 0.0);
        assert_eq!(stage.max_output_frames(BLOCK), BLOCK);
        assert_eq!(stage.flush(&mut output), 0);

        let mut short = vec![0.0; 64 * 2];
        let count = stage.process(&tone(440.0, rate, 0, BLOCK), &mut short);
        assert_eq!(count.frames_out, 64);
    }

    struct FrameMajor {
        rate: SampleRate,
        channels: usize,
        preamp: f64,
        coefficients: Vec<Biquad>,
        histories: Vec<[f64; 4]>,
    }

    impl FrameMajor {
        fn new(profile: &Profile, rate: SampleRate, channels: usize) -> Self {
            let mut cascade = Self {
                rate,
                channels,
                preamp: 1.0,
                coefficients: Vec::new(),
                histories: vec![[0.0; 4]; channels * MAX_BANDS],
            };
            cascade.retune(profile);
            cascade
        }

        fn retune(&mut self, profile: &Profile) {
            self.preamp = profile.preamp().amplitude();
            self.coefficients = profile.designed(self.rate).collect();
            let reached = self.coefficients.len();
            for lane in self.histories.as_chunks_mut::<MAX_BANDS>().0 {
                for history in lane.iter_mut().skip(reached) {
                    *history = [0.0; 4];
                }
            }
        }

        fn reset(&mut self) {
            self.histories.fill([0.0; 4]);
        }

        fn process(&mut self, input: &[f64], output: &mut [f64]) -> usize {
            let frames = (input.len() / self.channels).min(output.len() / self.channels);
            for (taken, made) in input
                .chunks_exact(self.channels)
                .zip(output.chunks_exact_mut(self.channels))
            {
                for ((sample, slot), lane) in taken
                    .iter()
                    .zip(made.iter_mut())
                    .zip(self.histories.as_chunks_mut::<MAX_BANDS>().0)
                {
                    let mut carried = usable(*sample * self.preamp);
                    for (filter, history) in self.coefficients.iter().zip(lane.iter_mut()) {
                        let [behind_in, further_behind_in, behind_out, further_behind_out] =
                            *history;
                        let out = biquad(
                            *filter,
                            carried,
                            [behind_in, further_behind_in, behind_out, further_behind_out],
                        );
                        *history = [carried, behind_in, out, behind_out];
                        carried = out;
                    }
                    *slot = carried;
                }
            }
            frames
        }
    }

    fn of_every_kind(count: usize, shift: usize, preamp: f64) -> Arc<Profile> {
        let bands = (0..count)
            .zip(BandKind::ALL.iter().cycle().skip(shift))
            .map(|(index, kind)| {
                let spread = ((index * 7 + shift * 3) % 32) as f64 / 31.0;
                band(
                    *kind,
                    40.0 * 600.0_f64.powf(spread),
                    ((index * 5 + shift) % 25) as f64 - 12.0,
                    0.4 + ((index * 3 + shift) % 9) as f64 * 0.45,
                )
            })
            .collect();
        Arc::new(
            Profile::new(Preamp::from_decibels(preamp).expect("in range"), bands)
                .expect("a profile in range"),
        )
    }

    fn music(channels: usize, from: usize, frames: usize) -> Vec<f64> {
        (0..frames * channels)
            .map(|index| {
                let at = (from + index / channels) as f64;
                let channel = (index % channels) as f64;
                let hiss = ((at * 12.9898 + channel * 78.233).sin() * 43_758.545).fract();
                0.45 * (at * (0.031 + 0.007 * channel)).sin()
                    + 0.3 * (at * (0.57 + 0.05 * channel)).sin()
                    + 0.15 * hiss
            })
            .collect()
    }

    struct Twins {
        stage: Equaliser,
        cascade: FrameMajor,
        channels: usize,
        played: usize,
        named: String,
    }

    impl Twins {
        fn feed(&mut self, input: &[f64], room: usize, step: &str) {
            let mut made = vec![f64::NAN; room];
            let mut expected = vec![f64::NAN; room];
            let count = self.stage.process(input, &mut made);
            let frames = self.cascade.process(input, &mut expected);
            assert_eq!(count.frames_in, frames, "{}, {step}", self.named);
            assert_eq!(count.frames_out, frames, "{}, {step}", self.named);
            let parted = made
                .iter()
                .zip(&expected)
                .position(|(made, expected)| made.to_bits() != expected.to_bits());
            assert_eq!(parted, None, "{}, {step}: the samples part", self.named);
        }

        fn play(&mut self, frames: usize, step: &str) {
            let input = music(self.channels, self.played, frames);
            self.played += frames;
            self.feed(&input, input.len(), step);
        }

        fn retune(&mut self, profile: &Arc<Profile>) {
            self.stage.set_equalisation(profile);
            self.cascade.retune(profile);
        }
    }

    fn walk_beside_a_frame_major_cascade(layout: ChannelLayout, bands: usize, rate: SampleRate) {
        let channels = usize::from(layout.count().get());
        let first = of_every_kind(bands, 0, -2.5);
        let mut stage = Equaliser::new(Arc::clone(&first), rate);
        stage
            .prepare(
                StreamSpec::new(rate, layout, SampleFormat::F32),
                PREPARED_FRAMES,
            )
            .expect("the stage takes the rate it was built for");
        let mut twins = Twins {
            stage,
            cascade: FrameMajor::new(&first, rate, channels),
            channels,
            played: 0,
            named: format!("{layout} at {rate}, {bands} sections"),
        };

        for frames in [PREPARED_FRAMES, 37, 1, 0, PREPARED_FRAMES * 3 + 5] {
            twins.play(frames, &format!("a block of {frames} frames"));
        }
        let longer = music(channels, twins.played, PREPARED_FRAMES);
        twins.feed(&longer, 100 * channels, "an output shorter than the input");
        let mut ragged = music(channels, twins.played, PREPARED_FRAMES);
        ragged.push(0.5);
        twins.feed(&ragged, ragged.len() + 3, "a partial frame at the end");

        let mut corrupt = music(channels, twins.played, PREPARED_FRAMES);
        let wild = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e-310, -1.7e308];
        for (slot, value) in corrupt.iter_mut().step_by(17).zip(wild.iter().cycle()) {
            *slot = *value;
        }
        twins.feed(
            &corrupt,
            corrupt.len(),
            "a burst of samples that are not music",
        );
        twins.play(PREPARED_FRAMES, "music after the burst");

        let quiet = vec![0.0; DECAYING_FRAMES * channels];
        twins.feed(&quiet, quiet.len(), "a tail decaying through silence");

        twins.retune(&of_every_kind(bands, 1, 1.5));
        twins.play(PREPARED_FRAMES, "retuned under the stream");
        twins.retune(&of_every_kind(bands / 2, 2, 0.0));
        twins.play(PREPARED_FRAMES, "fewer bands");
        twins.retune(&of_every_kind(bands, 3, -6.0));
        twins.play(PREPARED_FRAMES * 2, "the bands grown back");

        twins.stage.reset();
        twins.cascade.reset();
        twins.play(PREPARED_FRAMES, "after a reset");
    }

    #[test]
    fn the_stage_is_bit_identical_to_a_frame_major_cascade() {
        let discrete = |count| ChannelLayout::Discrete(ChannelCount::new(count).expect("in range"));
        let layouts = [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            discrete(3),
            ChannelLayout::Quad,
            discrete(5),
            ChannelLayout::Surround51,
            discrete(7),
            ChannelLayout::Surround71,
            discrete(12),
        ];
        for layout in layouts {
            for bands in [1, 2, 3, 5, 10, MAX_BANDS] {
                for rate in [SampleRate::HZ_44100, SampleRate::HZ_192000] {
                    walk_beside_a_frame_major_cascade(layout, bands, rate);
                }
            }
        }
    }
}
