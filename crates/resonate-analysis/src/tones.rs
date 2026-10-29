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

const STRAYED_AT_MOST: f64 = 1e-9;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tones {
    spans: Vec<(f64, f64)>,
    starts: Vec<usize>,
    passed: Vec<(usize, f64)>,
    reaches: Vec<Reach>,
}

#[derive(Clone, Debug, PartialEq)]
struct Reach {
    first: usize,
    shares: Vec<f64>,
}

impl Reach {
    fn of(node: usize, starts: &[usize], passed: &[(usize, f64)]) -> Self {
        let points = starts.len() - 1;
        let share_at = |point: usize| {
            passed[starts[point]..starts[point + 1]]
                .iter()
                .find(|(passing, _)| *passing == node)
                .map(|(_, share)| *share)
        };
        let reached: Vec<usize> = (0..points)
            .filter(|point| share_at(*point).is_some())
            .collect();
        let (Some(first), Some(last)) = (reached.first(), reached.last()) else {
            return Self {
                first: 0,
                shares: Vec::new(),
            };
        };
        Self {
            first: *first,
            shares: (*first..=*last)
                .map(|point| share_at(point).unwrap_or(0.0))
                .collect(),
        }
    }
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
                if share >= PASSED_AT_LEAST {
                    passed.push((index, share));
                }
            }
            starts.push(passed.len());
        }
        let reaches = (0..NODES)
            .map(|node| Reach::of(node, &starts, &passed))
            .collect();
        Self {
            spans,
            starts,
            passed,
            reaches,
        }
    }

    pub(crate) fn spans(&self) -> &[(f64, f64)] {
        &self.spans
    }

    pub(crate) fn paid_by(&self, weights: &Heights, prices: &Heights, apart: &[f64]) -> f64 {
        let mut covered = vec![0.0; self.points()];
        for ((reach, weight), price) in self.reaches.iter().zip(weights).zip(prices) {
            if *weight <= 0.0 || *price <= 0.0 {
                continue;
            }
            let span = reach.first..reach.first + reach.shares.len();
            for (covered, share) in covered[span].iter_mut().zip(&reach.shares) {
                *covered += weight * share;
            }
        }
        let scale = apart
            .iter()
            .zip(&covered)
            .filter(|(wanted, _)| **wanted > 0.0)
            .map(|(wanted, covered)| {
                if *covered > 0.0 {
                    wanted / covered
                } else {
                    f64::INFINITY
                }
            })
            .fold(1.0, f64::max);
        let paid: f64 = weights
            .iter()
            .zip(prices)
            .filter(|(_, price)| **price > 0.0)
            .map(|(weight, price)| weight.max(0.0) * price)
            .sum();
        scale * paid
    }

    fn points(&self) -> usize {
        self.starts.len() - 1
    }

    fn passed_at(&self, point: usize) -> &[(usize, f64)] {
        &self.passed[self.starts[point]..self.starts[point + 1]]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Paid {
    pub(crate) paid: f64,
    pub(crate) weights: Heights,
}

pub(crate) struct Solving<'a> {
    tones: &'a Tones,
    heights: Heights,
    whole: f64,
    covering: [bool; NODES],
    basis: [usize; NODES],
    inverse: Box<[[f64; NODES]; NODES]>,
    values: Heights,
    room: Vec<f64>,
    covered: Vec<f64>,
    covered_kept: Vec<f64>,
}

impl<'a> Solving<'a> {
    pub(crate) fn over(tones: &'a Tones, heights: &Heights) -> Self {
        let points = tones.points();
        let heights = heights.map(|height| height.max(0.0));
        let covering = heights.map(|height| height > 0.0);
        let room = (0..points)
            .map(|point| {
                tones
                    .passed_at(point)
                    .iter()
                    .filter(|(node, _)| *node != WHOLE && covering[*node])
                    .map(|(node, share)| heights[*node] / share)
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        let mut solving = Self {
            tones,
            heights,
            whole: heights[WHOLE],
            covering,
            room,
            basis: [0; NODES],
            inverse: Box::new([[0.0; NODES]; NODES]),
            values: heights,
            covered: vec![0.0; points],
            covered_kept: vec![0.0; points],
        };
        solving.start_over();
        solving
    }

    fn start_over(&mut self) {
        let points = self.tones.points();
        for (row, held) in self.inverse.iter_mut().enumerate() {
            *held = [0.0; NODES];
            held[row] = 1.0;
        }
        self.basis = std::array::from_fn(|node| points + node);
        self.values = self.heights;
    }

    fn priced_whole_at(&mut self, transient: f64) {
        let price = self.whole * transient;
        if price == self.heights[WHOLE] {
            return;
        }
        self.heights[WHOLE] = price;
        let largest = self.heights.iter().copied().fold(0.0, f64::max);
        let values: Heights = std::array::from_fn(|row| {
            self.inverse[row]
                .iter()
                .zip(&self.heights)
                .map(|(entry, height)| entry * height)
                .sum()
        });
        if values
            .iter()
            .any(|value| *value < -STRAYED_AT_MOST * largest)
        {
            self.start_over();
            return;
        }
        self.values = values.map(|value| value.max(0.0));
    }

    fn whole_share_at(&self, point: usize) -> f64 {
        let whole = &self.tones.reaches[WHOLE];
        point
            .checked_sub(whole.first)
            .and_then(|into| whole.shares.get(into))
            .copied()
            .unwrap_or(0.0)
    }

    pub(crate) fn heard_alone(&self, apart: &[f64], transient: f64) -> f64 {
        let price = self.whole * transient;
        apart
            .iter()
            .zip(&self.room)
            .enumerate()
            .filter(|(_, (wanted, _))| **wanted > 0.0)
            .map(|(point, (wanted, room))| {
                let share = self.whole_share_at(point);
                let room = if self.covering[WHOLE] && share > 0.0 {
                    room.min(price / share)
                } else {
                    *room
                };
                wanted * room
            })
            .fold(0.0, f64::max)
    }

    pub(crate) fn bound(
        &mut self,
        apart: &[f64],
        transient: f64,
        beaten_at: f64,
        enough_at: f64,
    ) -> Option<Paid> {
        self.priced_whole_at(transient);
        let mut paid_at_most = Paid {
            paid: f64::INFINITY,
            weights: [0.0; NODES],
        };
        for _ in 0..PIVOTS_AT_MOST {
            let weights = self.weights(apart);
            let (entering, paid) = self.priced(apart, &weights);
            if paid.paid < paid_at_most.paid {
                paid_at_most = paid;
            }
            if paid_at_most.paid <= enough_at {
                break;
            }
            let paid_at_least = self.reached(apart);
            if paid_at_least >= beaten_at {
                return None;
            }
            if paid_at_most.paid <= paid_at_least * (1.0 + SETTLED_WITHIN) {
                break;
            }
            let Some(entering) = entering else {
                break;
            };
            if !self.pivot(entering) {
                break;
            }
        }
        paid_at_most.paid.is_finite().then_some(paid_at_most)
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

    fn priced(&mut self, apart: &[f64], weights: &Heights) -> (Option<usize>, Paid) {
        let points = self.tones.points();
        let largest = apart.iter().copied().fold(0.0_f64, f64::max);
        let kept = weights.map(|weight| weight.max(0.0));
        self.cover(weights, &kept);
        let mut best = (GAINED_AT_LEAST * largest.max(1.0), None);
        for (point, (wanted, covered)) in apart.iter().zip(&self.covered).enumerate().take(points) {
            let gained = wanted - covered;
            if gained > best.0 {
                best = (gained, Some(point));
            }
        }
        let scale = apart
            .iter()
            .zip(&self.covered_kept)
            .take(points)
            .filter(|(wanted, _)| **wanted > 0.0)
            .map(|(wanted, covered_kept)| {
                if *covered_kept > 0.0 {
                    wanted / covered_kept
                } else {
                    f64::INFINITY
                }
            })
            .fold(1.0, f64::max);
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
            Paid {
                paid: scale * paid,
                weights: kept.map(|weight| weight * scale),
            },
        )
    }

    fn cover(&mut self, weights: &Heights, kept: &Heights) {
        self.covered.fill(0.0);
        self.covered_kept.fill(0.0);
        for (node, reach) in self.tones.reaches.iter().enumerate() {
            let weight = weights[node];
            if !self.covering[node] || weight == 0.0 {
                continue;
            }
            let span = reach.first..reach.first + reach.shares.len();
            for (covered, share) in self.covered[span.clone()].iter_mut().zip(&reach.shares) {
                *covered += weight * share;
            }
            let kept = kept[node];
            if kept == 0.0 {
                continue;
            }
            for (covered_kept, share) in self.covered_kept[span].iter_mut().zip(&reach.shares) {
                *covered_kept += kept * share;
            }
        }
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
        if let Some(node) = variable.checked_sub(points) {
            return std::array::from_fn(|row| self.inverse[row][node]);
        }
        let mut column = [-0.0; NODES];
        for (node, share) in self.covered_at(variable) {
            for (row, held) in column.iter_mut().zip(self.inverse.iter()) {
                *row += held[*node] * share;
            }
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
