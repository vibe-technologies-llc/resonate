use crate::bands::BANDS;

pub const NODES: usize = 2 * BANDS - 1;

pub const MOMENTS_KEPT: usize = 8;

const MOMENT_BYTES: usize = 2 + NODES;

pub const BAND_PEAKS_BYTES: usize = MOMENTS_KEPT * MOMENT_BYTES;

pub(crate) const WHOLE: usize = 0;

const PEAK_STEPS_A_DECIBEL: f64 = 16.0;

const PEAK_CODE_ZERO_DB: f64 = 200.0;

const SILENT_PEAK: u16 = 0;

const STEPS_BELOW_A_DECIBEL: f64 = 4.0;

const SILENT_STEP: u8 = u8::MAX;

const DEEPEST_STEP: u8 = u8::MAX - 1;

pub(crate) const TILES_REACHED_EACH_WAY: usize = 2;

const GROUPED_FROM_AT_MOST: usize = 1_024;

const QUIETEST_WEIGHED: f64 = 1e-3;

const WIDEST_TOLERANCE: f64 = 1e4;

const TOLERANCE_HALVINGS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) halves: Option<(usize, usize)>,
}

pub(crate) const TREE: [Node; NODES] = tree();

const fn tree() -> [Node; NODES] {
    let mut nodes = [Node {
        from: 0,
        to: 0,
        halves: None,
    }; NODES];
    let mut pending = [(0, 0, 0); NODES];
    pending[0] = (WHOLE, 0, BANDS);
    let mut waiting = 1;
    while waiting > 0 {
        waiting -= 1;
        let (index, from, to) = pending[waiting];
        if to - from > 1 {
            let middle = (from + to) / 2;
            let lower = index + 1;
            let upper = index + 2 * (middle - from);
            nodes[index] = Node {
                from,
                to,
                halves: Some((lower, upper)),
            };
            pending[waiting] = (upper, middle, to);
            pending[waiting + 1] = (lower, from, middle);
            waiting += 2;
        } else {
            nodes[index] = Node {
                from,
                to,
                halves: None,
            };
        }
    }
    nodes
}

pub(crate) type Heights = [f64; NODES];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Moment {
    loudest: u16,
    steps: [u8; NODES],
}

impl Moment {
    const SILENT: Self = Self {
        loudest: SILENT_PEAK,
        steps: [SILENT_STEP; NODES],
    };

    fn of(heights: &Heights) -> Self {
        let loudest = heights
            .iter()
            .copied()
            .filter(|height| height.is_finite())
            .fold(0.0, f64::max);
        let code = code_above(loudest);
        if code == SILENT_PEAK {
            return Self::SILENT;
        }
        let loudest_db = decibels_of(code);
        Self {
            loudest: code,
            steps: heights.map(|height| step_below(loudest_db, height)),
        }
    }

    fn heights(&self) -> Heights {
        if self.loudest == SILENT_PEAK {
            return [0.0; NODES];
        }
        let loudest_db = decibels_of(self.loudest);
        self.steps.map(|step| {
            if step == SILENT_STEP {
                0.0
            } else {
                10_f64.powf((loudest_db - f64::from(step) / STEPS_BELOW_A_DECIBEL) / 20.0)
            }
        })
    }

    fn is_silent(&self) -> bool {
        self.loudest == SILENT_PEAK
    }
}

fn step_below(loudest_db: f64, height: f64) -> u8 {
    if !height.is_finite() || height <= 0.0 {
        return SILENT_STEP;
    }
    let steps = ((loudest_db - 20.0 * height.log10()) * STEPS_BELOW_A_DECIBEL).floor();
    steps.clamp(0.0, f64::from(DEEPEST_STEP)) as u8
}

fn code_above(peak: f64) -> u16 {
    if !peak.is_finite() || peak <= 0.0 {
        return SILENT_PEAK;
    }
    let steps = ((20.0 * peak.log10() + PEAK_CODE_ZERO_DB) * PEAK_STEPS_A_DECIBEL).ceil();
    steps.clamp(1.0, f64::from(u16::MAX)) as u16
}

fn decibels_of(code: u16) -> f64 {
    f64::from(code) / PEAK_STEPS_A_DECIBEL - PEAK_CODE_ZERO_DB
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandPeaks {
    moments: [Moment; MOMENTS_KEPT],
}

impl BandPeaks {
    pub const SILENT: Self = Self {
        moments: [Moment::SILENT; MOMENTS_KEPT],
    };

    pub fn at_once(bands: impl IntoIterator<Item = f64>) -> Self {
        let mut leaves = [0.0; BANDS];
        for (leaf, peak) in leaves.iter_mut().zip(bands) {
            *leaf = peak.max(0.0);
        }
        let mut heights = [0.0; NODES];
        for (index, node) in TREE.iter().enumerate() {
            heights[index] = leaves[node.from..node.to].iter().sum();
        }
        Self::of_moments(&[heights])
    }

    pub(crate) fn of_moments(moments: &[Heights]) -> Self {
        let mut kept: Vec<Moment> = moments.iter().map(Moment::of).collect();
        kept.sort_by_key(|moment| std::cmp::Reverse(moment.loudest));
        let mut peaks = Self::SILENT;
        for (slot, moment) in peaks.moments.iter_mut().zip(kept) {
            *slot = moment;
        }
        peaks
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (moments, rest) = bytes.as_chunks::<MOMENT_BYTES>();
        if !rest.is_empty() || moments.len() != MOMENTS_KEPT {
            return None;
        }
        let mut peaks = Self::SILENT;
        for (slot, bytes) in peaks.moments.iter_mut().zip(moments) {
            let (loudest, steps) = bytes.split_first_chunk::<2>()?;
            *slot = Moment {
                loudest: u16::from_le_bytes(*loudest),
                steps: steps.try_into().ok()?,
            };
            if slot.is_silent() {
                *slot = Moment::SILENT;
            }
        }
        Some(peaks)
    }

    pub fn to_bytes(&self) -> [u8; BAND_PEAKS_BYTES] {
        let mut bytes = [0; BAND_PEAKS_BYTES];
        for (slot, moment) in bytes
            .as_chunks_mut::<MOMENT_BYTES>()
            .0
            .iter_mut()
            .zip(&self.moments)
        {
            let (loudest, steps) = slot.split_at_mut(2);
            loudest.copy_from_slice(&moment.loudest.to_le_bytes());
            steps.copy_from_slice(&moment.steps);
        }
        bytes
    }

    pub fn is_silent(&self) -> bool {
        self.moments.iter().all(Moment::is_silent)
    }

    pub(crate) fn moments(&self) -> impl Iterator<Item = Heights> + '_ {
        self.moments
            .iter()
            .filter(|moment| !moment.is_silent())
            .map(Moment::heights)
    }
}

pub(crate) fn grouped(tiles: &[Heights]) -> BandPeaks {
    let mut reached = reaching_their_neighbours(tiles);
    while reached.len() > GROUPED_FROM_AT_MOST {
        reached = reached
            .chunks(2)
            .map(|pair| louder_of_each(pair[0], *pair.last().unwrap_or(&pair[0])))
            .collect();
    }
    let loudest = reached
        .iter()
        .map(|heights| heights[WHOLE])
        .fold(0.0, f64::max);
    if loudest <= 0.0 {
        return BandPeaks::of_moments(&reached.iter().copied().take(1).collect::<Vec<_>>());
    }

    let mut order: Vec<usize> = (0..reached.len()).collect();
    order.sort_by(|one, other| reached[*other][WHOLE].total_cmp(&reached[*one][WHOLE]));
    let floor = loudest * QUIETEST_WEIGHED;

    let mut narrowest = 0.0;
    let mut widest = WIDEST_TOLERANCE;
    let mut best = gathered(&reached, &order, widest, loudest, floor).unwrap_or_default();
    for _ in 0..TOLERANCE_HALVINGS {
        let tolerance = (narrowest + widest) / 2.0;
        match gathered(&reached, &order, tolerance, loudest, floor) {
            Some(moments) => {
                widest = tolerance;
                best = moments;
            }
            None => narrowest = tolerance,
        }
    }
    BandPeaks::of_moments(&best)
}

fn reaching_their_neighbours(tiles: &[Heights]) -> Vec<Heights> {
    (0..tiles.len())
        .map(|tile| {
            let from = tile.saturating_sub(TILES_REACHED_EACH_WAY);
            let to = (tile + TILES_REACHED_EACH_WAY + 1).min(tiles.len());
            tiles[from..to]
                .iter()
                .fold([0.0; NODES], |louder, heights| {
                    louder_of_each(louder, *heights)
                })
        })
        .collect()
}

fn louder_of_each(one: Heights, other: Heights) -> Heights {
    std::array::from_fn(|node| one[node].max(other[node]))
}

fn gathered(
    reached: &[Heights],
    order: &[usize],
    tolerance: f64,
    loudest: f64,
    floor: f64,
) -> Option<Vec<Heights>> {
    let mut moments: Vec<Heights> = Vec::with_capacity(MOMENTS_KEPT);
    for tile in order {
        let heights = &reached[*tile];
        let nearest = moments
            .iter()
            .enumerate()
            .map(|(index, moment)| (index, raised_by(heights, moment, loudest, floor)))
            .min_by(|(_, one), (_, other)| one.total_cmp(other));
        match nearest {
            Some((index, raised)) if raised <= tolerance => {
                moments[index] = louder_of_each(moments[index], *heights);
            }
            _ if moments.len() < MOMENTS_KEPT => moments.push(*heights),
            _ => return None,
        }
    }
    Some(moments)
}

fn raised_by(heights: &Heights, moment: &Heights, loudest: f64, floor: f64) -> f64 {
    let raised = heights
        .iter()
        .zip(moment)
        .map(|(height, held)| (height - held).max(0.0) / held.max(floor))
        .fold(0.0, f64::max);
    raised * heights[WHOLE] / loudest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heights_of(whole: f64, leaves: &[(usize, f64)]) -> Heights {
        let mut bands = [0.0; BANDS];
        for (band, peak) in leaves {
            bands[*band] = *peak;
        }
        let mut heights: Heights =
            std::array::from_fn(|node| bands[TREE[node].from..TREE[node].to].iter().sum());
        heights[WHOLE] = whole;
        heights
    }

    #[test]
    fn the_tree_halves_the_bands_down_to_each_one_alone() {
        assert_eq!(
            TREE[WHOLE],
            Node {
                from: 0,
                to: BANDS,
                halves: TREE[WHOLE].halves
            }
        );
        let mut leaves: Vec<usize> = TREE
            .iter()
            .filter(|node| node.halves.is_none())
            .map(|node| {
                assert_eq!(node.to, node.from + 1);
                node.from
            })
            .collect();
        leaves.sort_unstable();
        assert_eq!(leaves, (0..BANDS).collect::<Vec<_>>());
        for (index, node) in TREE.iter().enumerate() {
            if let Some((lower, upper)) = node.halves {
                assert!(lower > index && upper > lower);
                assert_eq!(TREE[lower].from, node.from);
                assert_eq!(TREE[lower].to, TREE[upper].from);
                assert_eq!(TREE[upper].to, node.to);
            }
        }
    }

    #[test]
    fn a_moment_is_kept_at_or_above_each_height_by_less_than_a_quarter_of_a_decibel() {
        let heights = heights_of(0.8, &[(4, 0.5), (12, 0.02), (20, 1e-5), (27, 0.9)]);

        let kept = BandPeaks::of_moments(&[heights]);
        let read = kept.moments().next().expect("a moment");

        for (height, held) in heights.iter().zip(read) {
            if *height <= 0.0 {
                assert_eq!(held, 0.0);
                continue;
            }
            assert!(held >= *height, "{held} under {height}");
            let floor = 0.9 * 10_f64.powf(-f64::from(DEEPEST_STEP) / STEPS_BELOW_A_DECIBEL / 20.0);
            assert!(
                held <= (height * 10_f64.powf(0.25 / 20.0)).max(floor * 1.01),
                "{held} over {height}"
            );
        }
    }

    #[test]
    fn peaks_read_back_from_their_bytes_and_a_wrong_length_reads_as_nothing() {
        let peaks =
            BandPeaks::of_moments(&[heights_of(0.7, &[(3, 0.6)]), heights_of(0.3, &[(22, 0.2)])]);

        assert_eq!(BandPeaks::from_bytes(&peaks.to_bytes()), Some(peaks));
        assert_eq!(BandPeaks::from_bytes(&peaks.to_bytes()[1..]), None);
        assert_eq!(BandPeaks::from_bytes(&[]), None);
        assert!(BandPeaks::SILENT.is_silent());
        assert!(!peaks.is_silent());
        assert_eq!(peaks.moments().count(), 2);
    }

    #[test]
    fn moments_far_apart_in_time_and_in_kind_are_kept_apart_and_quiet_ones_join_the_rest() {
        let kick = heights_of(0.9, &[(2, 0.85)]);
        let hat = heights_of(0.5, &[(25, 0.45)]);
        let hush = heights_of(0.01, &[(10, 0.01)]);
        let mut tiles = Vec::new();
        for beat in 0..40 {
            tiles.push(if beat % 8 == 0 { kick } else { hush });
            tiles.push(hush);
            tiles.push(hush);
            tiles.push(if beat % 8 == 4 { hat } else { hush });
            tiles.push(hush);
            tiles.push(hush);
        }

        let peaks = grouped(&tiles);

        let moments: Vec<Heights> = peaks.moments().collect();
        assert!(moments.len() <= MOMENTS_KEPT);
        let both_loud = moments
            .iter()
            .any(|moment| moment[leaf_of(2)] > 0.5 && moment[leaf_of(25)] > 0.3);
        assert!(!both_loud, "{moments:?}");
        for tile in &tiles {
            assert!(
                moments.iter().any(|moment| moment
                    .iter()
                    .zip(tile)
                    .all(|(held, height)| *held >= *height)),
                "a tile no moment holds"
            );
        }
    }

    fn leaf_of(band: usize) -> usize {
        TREE.iter()
            .position(|node| node.halves.is_none() && node.from == band)
            .expect("a leaf")
    }

    #[test]
    fn a_silent_stream_keeps_silent_peaks() {
        assert!(grouped(&[]).is_silent());
        assert!(grouped(&[[0.0; NODES]; 4]).is_silent());
    }

    #[test]
    fn bands_at_once_make_every_node_the_sum_of_the_bands_beneath_it() {
        let peaks = BandPeaks::at_once((0..BANDS).map(|band| if band == 7 { 0.5 } else { 0.0 }));

        let moment = peaks.moments().next().expect("a moment");

        for (index, node) in TREE.iter().enumerate() {
            let expected = if (node.from..node.to).contains(&7) {
                0.5
            } else {
                0.0
            };
            assert!(
                (moment[index] - expected).abs() <= expected * 0.03,
                "{index}"
            );
        }
    }
}
