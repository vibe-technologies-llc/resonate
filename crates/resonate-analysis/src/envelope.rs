use resonate_core::Frames;

use crate::reduce::{Absorbs, Doubling};

pub const ENVELOPE_COLUMNS: usize = 2048;

pub const ENVELOPE_LANES: usize = 8;

const FRAMES_PER_UNIT: u64 = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reach {
    pub low: f32,
    pub high: f32,
    pub rms: f32,
}

#[derive(Clone, Copy, Debug)]
struct Gathered {
    low: f32,
    high: f32,
    squares: f64,
    samples: u64,
}

impl Gathered {
    const EMPTY: Self = Self {
        low: 0.0,
        high: 0.0,
        squares: 0.0,
        samples: 0,
    };

    fn note(&mut self, sample: f32) {
        if self.samples == 0 {
            self.low = sample;
            self.high = sample;
        } else {
            self.low = self.low.min(sample);
            self.high = self.high.max(sample);
        }
        self.squares += f64::from(sample) * f64::from(sample);
        self.samples += 1;
    }

    fn absorb(&mut self, later: &Self) {
        if later.samples == 0 {
            return;
        }
        if self.samples == 0 {
            *self = *later;
            return;
        }
        self.low = self.low.min(later.low);
        self.high = self.high.max(later.high);
        self.squares += later.squares;
        self.samples += later.samples;
    }

    fn reach(&self) -> Reach {
        if self.samples == 0 {
            return Reach::default();
        }
        Reach {
            low: self.low,
            high: self.high,
            rms: (self.squares / self.samples as f64).sqrt() as f32,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Column([Gathered; ENVELOPE_LANES]);

impl Column {
    const EMPTY: Self = Self([Gathered::EMPTY; ENVELOPE_LANES]);
}

impl Absorbs for Column {
    fn absorb(&mut self, later: &Self) {
        for (held, arrived) in self.0.iter_mut().zip(&later.0) {
            held.absorb(arrived);
        }
    }
}

pub(crate) struct Enveloping {
    channels: usize,
    lanes: usize,
    columns: Doubling<Column>,
    unit: Column,
    in_unit: u64,
    frames: u64,
}

impl Enveloping {
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            channels: channels.max(1),
            lanes: channels.clamp(1, ENVELOPE_LANES),
            columns: Doubling::new(ENVELOPE_COLUMNS),
            unit: Column::EMPTY,
            in_unit: 0,
            frames: 0,
        }
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        for frame in interleaved.chunks_exact(self.channels) {
            for (lane, sample) in self.unit.0.iter_mut().zip(frame) {
                lane.note(*sample);
            }
            self.in_unit += 1;
            self.frames += 1;
            if self.in_unit == FRAMES_PER_UNIT {
                self.columns.push(self.unit);
                self.unit = Column::EMPTY;
                self.in_unit = 0;
            }
        }
    }

    pub(crate) fn finished(mut self) -> Envelope {
        if self.in_unit > 0 {
            self.columns.push(self.unit);
        }
        let frames_per_column = self.columns.per_column() * FRAMES_PER_UNIT;
        let columns = self
            .columns
            .finished()
            .into_iter()
            .map(|column| column.0.map(|lane| lane.reach()))
            .collect();

        Envelope {
            lanes: self.lanes,
            frames_per_column,
            frames: self.frames,
            columns,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    lanes: usize,
    frames_per_column: u64,
    frames: u64,
    columns: Vec<[Reach; ENVELOPE_LANES]>,
}

impl Envelope {
    pub const fn lanes(&self) -> usize {
        self.lanes
    }

    pub const fn frames(&self) -> Frames {
        Frames(self.frames)
    }

    pub fn condensed(&self, lane: usize, wanted: usize) -> Vec<Reach> {
        if self.frames == 0 || wanted == 0 || lane >= self.lanes || self.columns.is_empty() {
            return Vec::new();
        }

        let last = self.columns.len() - 1;
        let column_of = |frame: u64| ((frame / self.frames_per_column) as usize).min(last);
        (0..wanted as u64)
            .map(|nth| {
                let from = nth * self.frames / wanted as u64;
                let to = ((nth + 1) * self.frames / wanted as u64).max(from + 1);
                let first = column_of(from);
                let past = column_of(to - 1) + 1;
                merged(self.columns[first..past].iter().map(|column| column[lane]))
            })
            .collect()
    }
}

fn merged(reaches: impl Iterator<Item = Reach>) -> Reach {
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    let mut squares = 0.0_f64;
    let mut count = 0_u32;
    for reach in reaches {
        low = low.min(reach.low);
        high = high.max(reach.high);
        squares += f64::from(reach.rms) * f64::from(reach.rms);
        count += 1;
    }
    if count == 0 {
        return Reach::default();
    }

    Reach {
        low,
        high,
        rms: (squares / f64::from(count)).sqrt() as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enveloped(channels: usize, interleaved: &[f32]) -> Envelope {
        let mut enveloping = Enveloping::new(channels);
        for block in interleaved.chunks(1_000 * channels) {
            enveloping.note(block);
        }
        enveloping.finished()
    }

    #[test]
    fn each_channel_is_a_lane_of_its_own_reach() {
        let interleaved: Vec<f32> = (0..48_000)
            .flat_map(|frame| {
                let rising = frame as f32 / 48_000.0;
                [rising, -0.5 * rising]
            })
            .collect();
        let envelope = enveloped(2, &interleaved);

        assert_eq!(envelope.lanes(), 2);
        assert_eq!(envelope.frames(), Frames(48_000));

        let left = envelope.condensed(0, 4);
        let right = envelope.condensed(1, 4);
        assert_eq!(left.len(), 4);
        assert!(left.windows(2).all(|pair| pair[1].high > pair[0].high));
        assert!(left[3].high > 0.99 && left[0].low < 0.01);
        assert!(right[3].low < -0.49 && right[3].high <= 0.0);
        assert!(envelope.condensed(2, 4).is_empty());
    }

    #[test]
    fn a_long_stream_stays_within_the_columns_and_condenses_to_any_width() {
        let interleaved: Vec<f32> = (0..3_000_000)
            .map(|frame| if frame % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let envelope = enveloped(1, &interleaved);

        assert!(envelope.columns.len() <= ENVELOPE_COLUMNS);
        for wanted in [1, 7, 640, 5_000] {
            let condensed = envelope.condensed(0, wanted);
            assert_eq!(condensed.len(), wanted);
            assert!(condensed.iter().all(|reach| {
                reach.low == -0.5 && reach.high == 0.5 && (reach.rms - 0.5).abs() < 1e-4
            }));
        }
    }

    #[test]
    fn nothing_heard_condenses_to_nothing() {
        let envelope = enveloped(2, &[]);
        assert!(envelope.condensed(0, 100).is_empty());
    }
}
