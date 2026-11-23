use std::fmt;

use resonate_core::ChannelLayout;

use crate::{error::Result, lab::pair::Pair};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Ours,
    #[cfg(any(feature = "soxr", feature = "rubato"))]
    Reference,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubjectName(pub String);

impl fmt::Display for SubjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait Subject: Send + Sync {
    fn name(&self) -> SubjectName;

    fn family(&self) -> Family;

    fn supports(&self, _pair: Pair) -> bool {
        true
    }

    fn layout(&self) -> Option<ChannelLayout> {
        None
    }

    fn open(&self, pair: Pair, layout: ChannelLayout, block: usize) -> Result<Box<dyn Running>>;

    fn is_ours(&self) -> bool {
        self.family() == Family::Ours
    }

    fn layout_or(&self, otherwise: ChannelLayout) -> ChannelLayout {
        self.layout().unwrap_or(otherwise)
    }
}

pub trait Running {
    fn push(&mut self, input: &[f64], output: &mut Vec<f64>) -> Result<()>;

    fn finish(&mut self, output: &mut Vec<f64>) -> Result<()>;

    fn channels_out(&self) -> usize;

    fn starts_over(&mut self) -> bool {
        false
    }

    fn claimed_latency_frames(&self) -> Option<f64> {
        None
    }
}

pub struct Ran {
    pub output: Vec<f64>,
    pub channels: usize,
    pub claimed_latency_frames: Option<f64>,
}

impl Ran {
    pub fn channel(&self, index: usize) -> Vec<f64> {
        self.output
            .iter()
            .skip(index)
            .step_by(self.channels)
            .copied()
            .collect()
    }

    pub fn frames(&self) -> usize {
        self.output.len() / self.channels
    }
}

pub fn ran(
    subject: &dyn Subject,
    pair: Pair,
    layout: ChannelLayout,
    input: &[f64],
    block: usize,
) -> Result<Ran> {
    let mut running = subject.open(pair, layout, block)?;
    let frames_in = input.len() / usize::from(layout.count().get());
    let mut output = Vec::with_capacity(
        (frames_in * pair.to.hz() as usize / pair.from.hz() as usize + 4_096)
            * usize::from(layout.count().get()),
    );

    running.push(input, &mut output)?;
    running.finish(&mut output)?;

    Ok(Ran {
        output,
        channels: running.channels_out(),
        claimed_latency_frames: running.claimed_latency_frames(),
    })
}

pub fn ran_again_after_a_reset(
    subject: &dyn Subject,
    pair: Pair,
    layout: ChannelLayout,
    input: &[f64],
    block: usize,
) -> Result<Option<(Vec<f64>, Vec<f64>)>> {
    let mut running = subject.open(pair, layout, block)?;
    let mut first = Vec::new();
    let mut again = Vec::new();

    running.push(input, &mut first)?;
    running.finish(&mut first)?;
    if !running.starts_over() {
        return Ok(None);
    }
    running.push(input, &mut again)?;
    running.finish(&mut again)?;
    Ok(Some((first, again)))
}

pub fn resampled(
    subject: &dyn Subject,
    pair: Pair,
    input: &[f64],
    block: usize,
) -> Result<Vec<f64>> {
    ran(subject, pair, ChannelLayout::Mono, input, block).map(|ran| ran.output)
}
