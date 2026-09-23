use std::f64::consts::FRAC_1_SQRT_2;

use resonate_core::{ChannelLayout, ChannelPosition, StreamSpec};

use crate::{Error, ProcessCount, Processor, Result};

const UNITY: f64 = 1.0;
const MINUS_3_DB: f64 = FRAC_1_SQRT_2;
const FULL_SCALE: f64 = 1.0;

pub struct Remix {
    input: ChannelLayout,
    output: ChannelLayout,
    rows: Vec<f64>,
    transparent: bool,
}

impl Remix {
    pub fn new(input: ChannelLayout, output: ChannelLayout) -> Self {
        let rows = matrix(input, output);
        let transparent = is_identity(&rows, input, output);
        Self {
            input,
            output,
            rows,
            transparent,
        }
    }

    pub const fn input(&self) -> ChannelLayout {
        self.input
    }

    pub const fn output(&self) -> ChannelLayout {
        self.output
    }

    pub fn gain(&self, into: usize, from: usize) -> Option<f64> {
        self.rows.get(into * self.inputs() + from).copied()
    }

    const fn inputs(&self) -> usize {
        self.input.count().get() as usize
    }

    const fn outputs(&self) -> usize {
        self.output.count().get() as usize
    }
}

impl Processor for Remix {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        if spec.channel_count() != self.input.count() {
            return Err(Error::ChannelMismatch {
                expected: self.input.count(),
                actual: spec.channel_count(),
            });
        }
        Ok(max_frames_in)
    }

    fn reset(&mut self) {}

    fn output_spec(&self, input: StreamSpec) -> StreamSpec {
        StreamSpec::new(input.rate, self.output, input.format)
    }

    fn latency_frames(&self) -> f64 {
        0.0
    }

    fn is_transparent(&self) -> bool {
        self.transparent
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let (inputs, outputs) = (self.inputs(), self.outputs());
        let frames = (input.len() / inputs).min(output.len() / outputs);

        for frame in 0..frames {
            let taken = input
                .get(frame * inputs..frame * inputs + inputs)
                .unwrap_or_default();
            for channel in 0..outputs {
                let row = self
                    .rows
                    .get(channel * inputs..channel * inputs + inputs)
                    .unwrap_or_default();
                let mixed: f64 = row
                    .iter()
                    .zip(taken)
                    .map(|(gain, sample)| gain * sample)
                    .sum();
                if let Some(slot) = output.get_mut(frame * outputs + channel) {
                    *slot = mixed;
                }
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

fn matrix(input: ChannelLayout, output: ChannelLayout) -> Vec<f64> {
    let (sources, targets) = (input.positions(), output.positions());
    let inputs = input.count().get() as usize;
    let outputs = output.count().get() as usize;

    if sources.len() != inputs || targets.len() != outputs {
        return one_for_one(inputs, outputs);
    }

    let mut rows = vec![0.0; outputs * inputs];
    for (column, source) in sources.iter().enumerate() {
        for (target, gain) in routes(*source, targets) {
            if let Some(row) = targets.iter().position(|position| *position == target)
                && let Some(cell) = rows.get_mut(row * inputs + column)
            {
                *cell += gain;
            }
        }
    }
    within_full_scale(rows, inputs)
}

fn one_for_one(inputs: usize, outputs: usize) -> Vec<f64> {
    let mut rows = vec![0.0; outputs * inputs];
    for channel in 0..outputs.min(inputs) {
        if let Some(cell) = rows.get_mut(channel * inputs + channel) {
            *cell = UNITY;
        }
    }
    rows
}

fn routes(source: ChannelPosition, targets: &[ChannelPosition]) -> Vec<(ChannelPosition, f64)> {
    if targets.contains(&source) {
        return vec![(source, UNITY)];
    }
    folded_into(source, targets)
        .into_iter()
        .map(|position| (position, MINUS_3_DB))
        .collect()
}

fn folded_into(source: ChannelPosition, targets: &[ChannelPosition]) -> Vec<ChannelPosition> {
    use ChannelPosition::{
        FrontCenter, FrontLeft, FrontRight, Lfe, RearLeft, RearRight, SideLeft, SideRight,
    };

    let nearest = |preferred: &[ChannelPosition]| -> Vec<ChannelPosition> {
        preferred
            .iter()
            .copied()
            .find(|position| targets.contains(position))
            .into_iter()
            .collect()
    };
    let spread = |over: &[ChannelPosition]| -> Vec<ChannelPosition> {
        over.iter()
            .copied()
            .filter(|position| targets.contains(position))
            .collect()
    };

    match source {
        FrontLeft | FrontRight => nearest(&[FrontCenter]),
        FrontCenter => spread(&[FrontLeft, FrontRight]),
        Lfe => Vec::new(),
        RearLeft => nearest(&[SideLeft, FrontLeft, FrontCenter]),
        RearRight => nearest(&[SideRight, FrontRight, FrontCenter]),
        SideLeft => nearest(&[RearLeft, FrontLeft, FrontCenter]),
        SideRight => nearest(&[RearRight, FrontRight, FrontCenter]),
    }
}

fn within_full_scale(mut rows: Vec<f64>, inputs: usize) -> Vec<f64> {
    for row in rows.chunks_mut(inputs) {
        let carried: f64 = row.iter().map(|gain| gain.abs()).sum();
        if carried > FULL_SCALE {
            for gain in row.iter_mut() {
                *gain /= carried;
            }
        }
    }
    rows
}

fn is_identity(rows: &[f64], input: ChannelLayout, output: ChannelLayout) -> bool {
    let inputs = input.count().get() as usize;
    if input.count() != output.count() {
        return false;
    }
    rows.iter().enumerate().all(|(cell, gain)| {
        let diagonal = cell / inputs == cell % inputs;
        *gain == if diagonal { UNITY } else { 0.0 }
    })
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelCount, SampleFormat, SampleRate};

    use super::*;

    const FRAMES: usize = 4;

    fn spec(channels: ChannelLayout) -> StreamSpec {
        StreamSpec::new(SampleRate::HZ_44100, channels, SampleFormat::F32)
    }

    fn run(input: ChannelLayout, output: ChannelLayout, frame: &[f64]) -> Vec<f64> {
        let mut stage = Remix::new(input, output);
        stage
            .prepare(spec(input), FRAMES)
            .expect("the stage takes the layout it was built for");

        let fed: Vec<f64> = frame.repeat(FRAMES);
        let mut produced = vec![0.0; FRAMES * output.count().get() as usize];
        let count = stage.process(&fed, &mut produced);

        assert_eq!(count.frames_in, FRAMES);
        assert_eq!(count.frames_out, FRAMES);
        produced
            .get(..output.count().get() as usize)
            .expect("one frame came out")
            .to_vec()
    }

    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() < 1e-6
    }

    #[test]
    fn a_layout_remixed_into_itself_is_transparent() {
        for layout in [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Quad,
            ChannelLayout::Surround51,
            ChannelLayout::Surround71,
        ] {
            assert!(
                Remix::new(layout, layout).is_transparent(),
                "{layout} was rewritten on its way to itself"
            );
        }
    }

    #[test]
    fn a_relabelled_layout_of_the_same_width_is_transparent() {
        let two = ChannelCount::new(2).expect("2 is in range");
        assert!(Remix::new(ChannelLayout::Discrete(two), ChannelLayout::Stereo).is_transparent());
    }

    #[test]
    fn surround_folds_into_stereo_by_the_standard_matrix() {
        let stage = Remix::new(ChannelLayout::Surround51, ChannelLayout::Stereo);
        let carried = UNITY + MINUS_3_DB + MINUS_3_DB;

        for (channel, column, expected) in [
            (0, 0, UNITY / carried),
            (0, 2, MINUS_3_DB / carried),
            (0, 4, MINUS_3_DB / carried),
            (1, 1, UNITY / carried),
            (1, 2, MINUS_3_DB / carried),
            (1, 5, MINUS_3_DB / carried),
        ] {
            let gain = stage.gain(channel, column).expect("a cell of the matrix");
            assert!(
                close(gain, expected),
                "channel {channel} takes {gain} of input {column}, not {expected}"
            );
        }
    }

    #[test]
    fn the_low_frequency_channel_is_dropped_rather_than_folded() {
        let stage = Remix::new(ChannelLayout::Surround51, ChannelLayout::Stereo);
        const LFE: usize = 3;

        assert_eq!(stage.gain(0, LFE), Some(0.0));
        assert_eq!(stage.gain(1, LFE), Some(0.0));
    }

    #[test]
    fn no_downmix_can_clip_a_full_scale_source() {
        for input in [
            ChannelLayout::Stereo,
            ChannelLayout::Quad,
            ChannelLayout::Surround51,
            ChannelLayout::Surround71,
        ] {
            for output in [
                ChannelLayout::Mono,
                ChannelLayout::Stereo,
                ChannelLayout::Quad,
                ChannelLayout::Surround51,
            ] {
                let frame = vec![1.0; input.count().get() as usize];
                for sample in run(input, output, &frame) {
                    assert!(
                        sample <= FULL_SCALE + 1e-6,
                        "{input} into {output} reached {sample}"
                    );
                }
            }
        }
    }

    #[test]
    fn stereo_folds_into_mono_as_the_average_of_its_two_channels() {
        let mixed = run(ChannelLayout::Stereo, ChannelLayout::Mono, &[1.0, 0.0]);
        assert_eq!(mixed.len(), 1);
        assert!(
            close(mixed.first().copied().unwrap_or_default(), 0.5),
            "{mixed:?}"
        );
    }

    #[test]
    fn mono_reaches_both_speakers_at_equal_power() {
        let spread = run(ChannelLayout::Mono, ChannelLayout::Stereo, &[1.0]);
        assert_eq!(spread.len(), 2);
        for sample in spread {
            assert!(close(sample, MINUS_3_DB), "got {sample}");
        }
    }

    #[test]
    fn an_upmix_leaves_the_channels_it_has_nothing_for_silent() {
        let spread = run(
            ChannelLayout::Stereo,
            ChannelLayout::Surround51,
            &[1.0, -1.0],
        );

        assert_eq!(spread.first().copied(), Some(1.0));
        assert_eq!(spread.get(1).copied(), Some(-1.0));
        for silent in spread.get(2..).unwrap_or_default() {
            assert_eq!(*silent, 0.0);
        }
    }

    #[test]
    fn the_side_channels_of_a_seven_one_mix_fold_into_the_rears_of_a_five_one() {
        let stage = Remix::new(ChannelLayout::Surround71, ChannelLayout::Surround51);
        const REAR_LEFT: usize = 4;
        const SIDE_LEFT: usize = 6;

        let folded = stage
            .gain(REAR_LEFT, SIDE_LEFT)
            .expect("a cell of the matrix");
        assert!(folded > 0.0, "the side channel was dropped");
        assert_eq!(stage.gain(REAR_LEFT, 5), Some(0.0));
    }

    #[test]
    fn a_layout_the_stage_was_not_built_for_is_refused() {
        let mut stage = Remix::new(ChannelLayout::Surround51, ChannelLayout::Stereo);
        let error = stage
            .prepare(spec(ChannelLayout::Stereo), FRAMES)
            .expect_err("a five-channel stage took a stereo frame");

        assert_eq!(
            error,
            Error::ChannelMismatch {
                expected: ChannelCount::new(6).expect("6 is in range"),
                actual: ChannelCount::STEREO,
            }
        );
    }

    #[test]
    fn a_discrete_layout_is_truncated_one_channel_for_one() {
        let three = ChannelCount::new(3).expect("3 is in range");
        let frame = [1.0, 2.0, 3.0];
        let kept = run(
            ChannelLayout::Discrete(three),
            ChannelLayout::Stereo,
            &frame,
        );

        assert_eq!(kept, vec![1.0, 2.0]);
    }
}
