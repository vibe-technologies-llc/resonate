use std::path::Path;

use resonate_core::ChannelLayout;

use crate::{
    error::Result,
    lab::{
        metric::{Metric, Reading},
        pair::Pair,
        readings::{Curve, Measured},
        runner::Timed,
        subject::Subject,
    },
    report::markdown,
};

pub struct Outcome {
    pub readings: Vec<Reading>,
    pub curves: Vec<Curve>,
}

impl Outcome {
    pub const fn of(readings: Vec<Reading>) -> Self {
        Self {
            readings,
            curves: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn value(&self, metric: &Metric) -> f64 {
        self.readings
            .iter()
            .find(|reading| reading.metric.key == metric.key)
            .map_or(f64::NAN, |reading| reading.value)
    }
}

#[cfg(test)]
pub fn measured_as(suite: &dyn Suite, subject: &str, pair: Pair) -> Outcome {
    let subjects = suite.subjects().unwrap_or_default();
    let Some(found) = subjects.iter().find(|held| held.name().0 == subject) else {
        panic!("{} holds no subject called {subject}", suite.name());
    };

    match suite.measure(found.as_ref(), pair) {
        Ok(outcome) => outcome,
        Err(error) => panic!("{subject} at {pair}: {error}"),
    }
}

pub trait Suite: Sync {
    fn name(&self) -> &'static str;

    fn title(&self) -> &'static str;

    fn about(&self) -> &'static str;

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>>;

    fn pairs(&self) -> Vec<Pair>;

    fn metrics(&self) -> &'static [&'static Metric];

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome>;

    fn report(&self, subjects: &[Box<dyn Subject>], measured: &[Measured]) -> String {
        markdown::by_pair(
            self.title(),
            self.metrics(),
            subjects,
            &self.pairs(),
            measured,
        )
    }

    fn plotted(&self, _directory: &Path, _measured: &[Measured]) -> Result<()> {
        Ok(())
    }

    fn timed_layout(&self) -> ChannelLayout {
        ChannelLayout::Stereo
    }

    fn timed_pairs(&self) -> Vec<Pair> {
        self.pairs()
    }

    fn built_pairs(&self) -> Vec<Pair> {
        self.timed_pairs().into_iter().take(2).collect()
    }

    fn costs(&self, subjects: &[Box<dyn Subject>], costs: &[Timed]) -> String {
        markdown::costs(
            "percent of one core per second of audio",
            subjects,
            &self.timed_pairs(),
            &self.built_pairs(),
            costs,
        )
    }
}
