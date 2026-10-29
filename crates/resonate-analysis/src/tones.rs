use resonate_core::SampleRate;

use crate::{
    peaks::{Heights, NODES, TREE, WHOLE},
    split::passed_below,
};

const PASSED_AT_LEAST: f64 = 1e-9;

const LOWEST_EDGE_HZ: f64 = 1.0;

const SPANS_AN_OCTAVE: f64 = 24.0;

const PIVOTS_AT_MOST: usize = 400;

const GAINED_AT_LEAST: f64 = 1e-12;

const MOVED_AT_LEAST: f64 = 1e-12;

const SETTLED_WITHIN: f64 = 1e-3;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tones {
    spans: Vec<(f64, f64)>,
    starts: Vec<usize>,
    passed: Vec<(usize, f64)>,
}

impl Tones {
    pub(crate) fn across(rate: SampleRate) -> Self {
        let nyquist = f64::from(rate.hz()) / 2.0;
        let octaves = (nyquist / LOWEST_EDGE_HZ).log2();
        let steps = (octaves * SPANS_AN_OCTAVE).ceil().max(1.0) as usize;
        let edges: Vec<f64> = std::iter::once(0.0)
            .chain((0..=steps).map(|step| {
                (LOWEST_EDGE_HZ * 2_f64.powf(step as f64 / SPANS_AN_OCTAVE)).min(nyquist)
            }))
            .collect();
        let at_edges: Vec<Heights> = edges
            .iter()
            .map(|hertz| {
                std::array::from_fn(|index| {
                    let node = TREE[index];
                    passed_below(node.to, *hertz, rate) - passed_below(node.from, *hertz, rate)
                })
            })
            .collect();
        let mut spans = Vec::with_capacity(edges.len());
        let mut starts = Vec::with_capacity(edges.len());
        let mut passed = Vec::new();
        starts.push(0);
        for (bounds, sides) in edges.windows(2).zip(at_edges.windows(2)) {
            spans.push((bounds[0], bounds[1]));
            for (index, (lower, upper)) in sides[0].iter().zip(&sides[1]).enumerate() {
                let share = lower.min(*upper);
                if index != WHOLE && share >= PASSED_AT_LEAST {
                    passed.push((index, share));
                }
            }
            starts.push(passed.len());
        }
        Self {
            spans,
            starts,
            passed,
        }
    }

    pub(crate) fn spans(&self) -> &[(f64, f64)] {
        &self.spans
    }

    fn points(&self) -> usize {
        self.starts.len() - 1
    }

    fn passed_at(&self, point: usize) -> &[(usize, f64)] {
        &self.passed[self.starts[point]..self.starts[point + 1]]
    }
}

pub(crate) struct Solving<'a> {
    tones: &'a Tones,
    heights: Heights,
    covering: [bool; NODES],
    basis: [usize; NODES],
    inverse: Box<[[f64; NODES]; NODES]>,
    values: Heights,
    room: Vec<f64>,
}

impl<'a> Solving<'a> {
    pub(crate) fn over(tones: &'a Tones, heights: &Heights) -> Self {
        let points = tones.points();
        let mut inverse = Box::new([[0.0; NODES]; NODES]);
        for (row, held) in inverse.iter_mut().enumerate() {
            held[row] = 1.0;
        }
        let heights = heights.map(|height| height.max(0.0));
        let covering = heights.map(|height| height > 0.0);
        let room = (0..points)
            .map(|point| {
                tones
                    .passed_at(point)
                    .iter()
                    .filter(|(node, _)| covering[*node])
                    .map(|(node, share)| heights[*node] / share)
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        Self {
            tones,
            heights,
            covering,
            room,
            basis: std::array::from_fn(|node| points + node),
            inverse,
            values: heights,
        }
    }

    pub(crate) fn heard_alone(&self, apart: &[f64]) -> f64 {
        apart
            .iter()
            .zip(&self.room)
            .filter(|(wanted, _)| **wanted > 0.0)
            .map(|(wanted, room)| wanted * room)
            .fold(0.0, f64::max)
    }

    pub(crate) fn bound(&mut self, apart: &[f64], beaten_at: f64) -> Option<f64> {
        let mut paid_at_most = f64::INFINITY;
        for _ in 0..PIVOTS_AT_MOST {
            let weights = self.weights(apart);
            let (entering, paid) = self.priced(apart, &weights);
            paid_at_most = paid_at_most.min(paid);
            let paid_at_least = self.reached(apart);
            if paid_at_least >= beaten_at {
                return None;
            }
            if paid_at_most <= paid_at_least * (1.0 + SETTLED_WITHIN) {
                break;
            }
            let Some(entering) = entering else {
                break;
            };
            if !self.pivot(entering) {
                break;
            }
        }
        paid_at_most.is_finite().then_some(paid_at_most)
    }

    fn reached(&self, apart: &[f64]) -> f64 {
        let points = self.tones.points();
        self.basis
            .iter()
            .zip(&self.values)
            .filter(|(variable, _)| **variable < points)
            .map(|(variable, value)| apart[*variable] * value)
            .sum()
    }

    fn priced(&self, apart: &[f64], weights: &Heights) -> (Option<usize>, f64) {
        let points = self.tones.points();
        let largest = apart.iter().copied().fold(0.0_f64, f64::max);
        let kept = weights.map(|weight| weight.max(0.0));
        let mut best = (GAINED_AT_LEAST * largest.max(1.0), None);
        let mut scale: f64 = 1.0;
        for (point, wanted) in apart.iter().enumerate().take(points) {
            let mut covered = 0.0;
            let mut covered_kept = 0.0;
            for (node, share) in self.covered_at(point) {
                covered += weights[*node] * share;
                covered_kept += kept[*node] * share;
            }
            let gained = wanted - covered;
            if gained > best.0 {
                best = (gained, Some(point));
            }
            if *wanted > 0.0 {
                scale = if covered_kept > 0.0 {
                    scale.max(wanted / covered_kept)
                } else {
                    f64::INFINITY
                };
            }
        }
        for (node, weight) in weights.iter().enumerate() {
            if -weight > best.0 {
                best = (-weight, Some(points + node));
            }
        }
        let paid: f64 = kept
            .iter()
            .zip(&self.heights)
            .map(|(weight, height)| weight * height)
            .sum();
        (
            best.1.filter(|variable| !self.basis.contains(variable)),
            scale * paid,
        )
    }

    fn weights(&self, apart: &[f64]) -> Heights {
        let points = self.tones.points();
        let mut weights = [0.0; NODES];
        for (row, variable) in self.basis.iter().enumerate() {
            if *variable >= points {
                continue;
            }
            let gain = apart[*variable];
            for (weight, inverse) in weights.iter_mut().zip(&self.inverse[row]) {
                *weight += gain * inverse;
            }
        }
        weights
    }

    fn column(&self, variable: usize) -> Heights {
        let points = self.tones.points();
        let mut column = [0.0; NODES];
        if variable >= points {
            let node = variable - points;
            for (row, held) in column.iter_mut().zip(self.inverse.iter()) {
                *row = held[node];
            }
            return column;
        }
        for (row, held) in column.iter_mut().zip(self.inverse.iter()) {
            *row = self
                .covered_at(variable)
                .map(|(node, share)| held[*node] * share)
                .sum();
        }
        column
    }

    fn covered_at(&self, point: usize) -> impl Iterator<Item = &(usize, f64)> {
        self.tones
            .passed_at(point)
            .iter()
            .filter(|(node, _)| self.covering[*node])
    }

    fn pivot(&mut self, entering: usize) -> bool {
        let column = self.column(entering);
        let Some(leaving) = (0..NODES)
            .filter(|row| column[*row] > MOVED_AT_LEAST)
            .min_by(|left, right| {
                (self.values[*left] / column[*left])
                    .total_cmp(&(self.values[*right] / column[*right]))
            })
        else {
            return false;
        };
        let pivot = column[leaving];
        let pivoted = self.inverse[leaving].map(|entry| entry / pivot);
        let value = self.values[leaving] / pivot;
        for (row, factor) in column.iter().copied().enumerate() {
            if row == leaving || factor == 0.0 {
                continue;
            }
            for (entry, from) in self.inverse[row].iter_mut().zip(&pivoted) {
                *entry -= factor * from;
            }
            self.values[row] = (self.values[row] - factor * value).max(0.0);
        }
        self.inverse[leaving] = pivoted;
        self.values[leaving] = value;
        self.basis[leaving] = entering;
        true
    }
}
