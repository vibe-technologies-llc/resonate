use std::{fmt, sync::Arc, thread};

use ahash::AHashMap;
use crossbeam_channel::{Receiver, bounded};

use crate::{QueueItem, seed};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShuffleStyle {
    Random,
    #[default]
    Smart,
    Albums,
}

impl ShuffleStyle {
    pub const ALL: [Self; 3] = [Self::Random, Self::Smart, Self::Albums];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Smart => "smart",
            Self::Albums => "albums",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.as_str() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SmartShuffle {
    pub style: ShuffleStyle,
    pub spreads_artists: bool,
    pub spreads_albums: bool,
    pub fresh_first: bool,
}

impl SmartShuffle {
    pub const RANDOM: Self = Self {
        style: ShuffleStyle::Random,
        spreads_artists: false,
        spreads_albums: false,
        fresh_first: false,
    };

    pub const fn reads_kin(self) -> bool {
        match self.style {
            ShuffleStyle::Random => false,
            ShuffleStyle::Albums => true,
            ShuffleStyle::Smart => self.spreads_artists || self.spreads_albums || self.fresh_first,
        }
    }
}

impl Default for SmartShuffle {
    fn default() -> Self {
        Self {
            style: ShuffleStyle::Smart,
            spreads_artists: true,
            spreads_albums: true,
            fresh_first: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Kin {
    pub artist: Option<u64>,
    pub album: Option<u64>,
    pub heard_lately: bool,
}

pub trait Kinship: Send + Sync {
    fn kin_of(&self, rows: &[Arc<QueueItem>]) -> Vec<Option<Kin>>;
}

#[derive(Clone, Default)]
pub struct KinSource(Option<Arc<dyn Kinship>>);

impl KinSource {
    pub fn by(kinship: Arc<dyn Kinship>) -> Self {
        Self(Some(kinship))
    }

    pub(crate) fn kinship(&self) -> Option<&Arc<dyn Kinship>> {
        self.0.as_ref()
    }
}

impl fmt::Debug for KinSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(_) => f.write_str("KinSource(given)"),
            None => f.write_str("KinSource(none)"),
        }
    }
}

impl PartialEq for KinSource {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(ours), Some(theirs)) => Arc::ptr_eq(ours, theirs),
            (None, None) => true,
            _ => false,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Shuffler(u64);

const UNIT_STEPS: u64 = 1 << 53;

impl Shuffler {
    pub(crate) fn new() -> Self {
        Self(seed::from_clock())
    }

    #[cfg(test)]
    pub(crate) const fn seeded(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn step(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub(crate) fn next_below(&mut self, bound: usize) -> usize {
        (self.step() % bound as u64) as usize
    }

    fn unit(&mut self) -> f64 {
        (self.step() % UNIT_STEPS) as f64 / UNIT_STEPS as f64
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Kindred {
    pub place: usize,
    pub kin: Option<Kin>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Level {
    Artist,
    Album,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Belonging {
    Artist(u64),
    Album(u64),
    Alone(usize),
}

const JITTER_OF_A_STRIDE: f64 = 0.2;

pub(crate) fn arranged(
    rows: &[Kindred],
    smart: SmartShuffle,
    shuffler: &mut Shuffler,
) -> Vec<usize> {
    let all: Vec<usize> = (0..rows.len()).collect();
    match smart.style {
        ShuffleStyle::Random => shuffled(all, shuffler),
        ShuffleStyle::Albums => album_by_album(rows, all, shuffler),
        ShuffleStyle::Smart => {
            let levels: Vec<Level> = [
                smart.spreads_artists.then_some(Level::Artist),
                smart.spreads_albums.then_some(Level::Album),
            ]
            .into_iter()
            .flatten()
            .collect();
            if !smart.fresh_first {
                return spread_apart(rows, all, &levels, shuffler);
            }

            let (heard_lately, fresh): (Vec<usize>, Vec<usize>) = all
                .into_iter()
                .partition(|row| rows[*row].kin.is_some_and(|kin| kin.heard_lately));

            let mut arranged = spread_apart(rows, fresh, &levels, shuffler);
            arranged.extend(spread_apart(rows, heard_lately, &levels, shuffler));
            arranged
        }
    }
}

pub(crate) fn album_of(row: &Kindred) -> Option<u64> {
    row.kin.and_then(|kin| kin.album)
}

fn shuffled(mut rows: Vec<usize>, shuffler: &mut Shuffler) -> Vec<usize> {
    let mut remaining = rows.len();
    while remaining > 1 {
        let pick = shuffler.next_below(remaining);
        remaining -= 1;
        rows.swap(pick, remaining);
    }
    rows
}

fn belonging(row: usize, of: &Kindred, level: Level) -> Belonging {
    let kin = of.kin.unwrap_or_default();
    match level {
        Level::Artist => kin.artist.map_or(Belonging::Alone(row), Belonging::Artist),
        Level::Album => kin.album.map_or(Belonging::Alone(row), Belonging::Album),
    }
}

fn grouped(rows: &[Kindred], chosen: Vec<usize>, level: Level) -> Vec<Vec<usize>> {
    let mut found: AHashMap<Belonging, usize> = AHashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for row in chosen {
        let belongs = belonging(row, &rows[row], level);
        let at = *found.entry(belongs).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[at].push(row);
    }
    groups
}

fn spread_apart(
    rows: &[Kindred],
    chosen: Vec<usize>,
    levels: &[Level],
    shuffler: &mut Shuffler,
) -> Vec<usize> {
    let Some((level, deeper)) = levels.split_first() else {
        return shuffled(chosen, shuffler);
    };

    let count = chosen.len();
    let mut placed: Vec<(f64, usize)> = Vec::with_capacity(count);
    for group in grouped(rows, chosen, *level) {
        let inner = spread_apart(rows, group, deeper, shuffler);
        let stride = 1.0 / inner.len() as f64;
        let offset = shuffler.unit() * stride;
        for (nth, row) in inner.into_iter().enumerate() {
            let jitter = (shuffler.unit() - 0.5) * stride * JITTER_OF_A_STRIDE;
            placed.push((offset + nth as f64 * stride + jitter, row));
        }
    }

    placed.sort_by(|ours, theirs| ours.0.total_cmp(&theirs.0));
    placed.into_iter().map(|(_, row)| row).collect()
}

fn album_by_album(rows: &[Kindred], chosen: Vec<usize>, shuffler: &mut Shuffler) -> Vec<usize> {
    let mut groups = grouped(rows, chosen, Level::Album);
    for group in &mut groups {
        group.sort_unstable_by_key(|row| rows[*row].place);
    }

    let order = shuffled((0..groups.len()).collect(), shuffler);
    order
        .into_iter()
        .flat_map(|group| std::mem::take(&mut groups[group]))
        .collect()
}

pub(crate) struct KinLanded {
    pub epoch: u64,
    pub kin: Vec<(usize, Kin)>,
}

pub(crate) struct KinAsking {
    pub landed: Receiver<KinLanded>,
    pub respread_at: Option<u64>,
}

pub(crate) struct KinAsk {
    pub epoch: u64,
    pub rows: Vec<(usize, Arc<QueueItem>)>,
    pub respread_at: Option<u64>,
}

impl KinAsk {
    pub(crate) fn sent_to(self, kinship: &Arc<dyn Kinship>) -> Option<KinAsking> {
        let (answer, landed) = bounded(1);
        let kinship = Arc::clone(kinship);
        let epoch = self.epoch;
        let respread_at = self.respread_at;
        let rows = self.rows;

        thread::Builder::new()
            .name("resonate-kin".to_owned())
            .spawn(move || {
                let items: Vec<Arc<QueueItem>> =
                    rows.iter().map(|(_, item)| Arc::clone(item)).collect();
                let kin = kinship
                    .kin_of(&items)
                    .into_iter()
                    .zip(&rows)
                    .filter_map(|(kin, (place, _))| kin.map(|kin| (*place, kin)))
                    .collect();
                let _ = answer.send(KinLanded { epoch, kin });
            })
            .map_err(|error| tracing::warn!(%error, "the kin of the queue was not asked for"))
            .ok()?;

        Some(KinAsking {
            landed,
            respread_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kindred(rows: &[(Option<u64>, Option<u64>, bool)]) -> Vec<Kindred> {
        rows.iter()
            .enumerate()
            .map(|(place, (artist, album, heard_lately))| Kindred {
                place,
                kin: Some(Kin {
                    artist: *artist,
                    album: *album,
                    heard_lately: *heard_lately,
                }),
            })
            .collect()
    }

    fn artists_in(rows: &[Kindred], arranged: &[usize]) -> Vec<Option<u64>> {
        arranged
            .iter()
            .map(|row| rows[*row].kin.and_then(|kin| kin.artist))
            .collect()
    }

    fn is_a_permutation(arranged: &[usize], count: usize) -> bool {
        let mut sorted = arranged.to_vec();
        sorted.sort_unstable();
        sorted == (0..count).collect::<Vec<_>>()
    }

    #[test]
    fn every_style_arranges_each_row_once() {
        let rows = kindred(
            &(0..40)
                .map(|row| (Some(row % 3), Some(row % 7), row % 5 == 0))
                .collect::<Vec<_>>(),
        );
        let styles = [
            SmartShuffle::RANDOM,
            SmartShuffle::default(),
            SmartShuffle {
                style: ShuffleStyle::Albums,
                ..SmartShuffle::default()
            },
        ];

        for (seed, smart) in styles.into_iter().enumerate() {
            let mut shuffler = Shuffler::seeded(seed as u64 + 7);
            let arranged = arranged(&rows, smart, &mut shuffler);

            assert!(is_a_permutation(&arranged, rows.len()), "{smart:?}");
        }
    }

    fn neighbours_alike(rows: &[Kindred], smart: SmartShuffle, seeds: u64) -> usize {
        (0..seeds)
            .map(|seed| {
                let mut shuffler = Shuffler::seeded(seed);
                let arranged = arranged(rows, smart, &mut shuffler);
                artists_in(rows, &arranged)
                    .windows(2)
                    .filter(|pair| pair[0] == pair[1])
                    .count()
            })
            .sum()
    }

    #[test]
    fn artists_of_equal_weight_spread_apart_almost_never_meet() {
        let rows = kindred(
            &(0..30)
                .map(|row| (Some(row % 3), Some(row % 3), false))
                .collect::<Vec<_>>(),
        );
        let smart = SmartShuffle {
            fresh_first: false,
            ..SmartShuffle::default()
        };

        let spread = neighbours_alike(&rows, smart, 200);
        let random = neighbours_alike(&rows, SmartShuffle::RANDOM, 200);

        assert!(spread * 20 < random, "spread {spread}, random {random}");
    }

    #[test]
    fn artists_of_unequal_weight_meet_far_less_often_than_at_random() {
        let weights = [20, 10, 5, 3, 2];
        let rows = kindred(
            &weights
                .iter()
                .enumerate()
                .flat_map(|(artist, songs)| {
                    (0..*songs).map(move |song| (Some(artist as u64), Some(song % 2), false))
                })
                .collect::<Vec<_>>(),
        );
        let smart = SmartShuffle {
            fresh_first: false,
            ..SmartShuffle::default()
        };

        let spread = neighbours_alike(&rows, smart, 200);
        let random = neighbours_alike(&rows, SmartShuffle::RANDOM, 200);

        assert!(spread * 2 < random, "spread {spread}, random {random}");
    }

    #[test]
    fn songs_heard_lately_wait_behind_the_rest() {
        let rows = kindred(
            &(0..12)
                .map(|row| (Some(row), Some(row), row < 4))
                .collect::<Vec<_>>(),
        );
        let mut shuffler = Shuffler::seeded(3);

        let arranged = arranged(&rows, SmartShuffle::default(), &mut shuffler);

        assert!(arranged[..8].iter().all(|row| *row >= 4), "{arranged:?}");
        assert!(arranged[8..].iter().all(|row| *row < 4), "{arranged:?}");
    }

    #[test]
    fn album_by_album_keeps_each_album_whole_and_in_its_loaded_order() {
        let rows = kindred(
            &(0..12)
                .map(|row| (Some(row % 2), Some(row % 3), false))
                .collect::<Vec<_>>(),
        );
        let smart = SmartShuffle {
            style: ShuffleStyle::Albums,
            ..SmartShuffle::default()
        };
        let mut shuffler = Shuffler::seeded(11);

        let arranged = arranged(&rows, smart, &mut shuffler);
        let albums: Vec<u64> = arranged
            .iter()
            .filter_map(|row| album_of(&rows[*row]))
            .collect();

        let runs = albums.windows(2).filter(|pair| pair[0] != pair[1]).count();
        assert_eq!(runs, 2, "{albums:?}");
        for album in
            arranged.chunk_by(|ours, theirs| album_of(&rows[*ours]) == album_of(&rows[*theirs]))
        {
            assert!(album.is_sorted(), "{arranged:?}");
        }
    }

    #[test]
    fn rows_of_unknown_kin_are_each_their_own_and_still_all_arranged() {
        let rows: Vec<Kindred> = (0..10).map(|place| Kindred { place, kin: None }).collect();

        for smart in [
            SmartShuffle::default(),
            SmartShuffle {
                style: ShuffleStyle::Albums,
                ..SmartShuffle::default()
            },
        ] {
            let mut shuffler = Shuffler::seeded(5);
            let arranged = arranged(&rows, smart, &mut shuffler);

            assert!(is_a_permutation(&arranged, rows.len()));
        }
    }

    #[test]
    fn a_kin_source_is_equal_only_to_itself() {
        struct Nobody;
        impl Kinship for Nobody {
            fn kin_of(&self, rows: &[Arc<QueueItem>]) -> Vec<Option<Kin>> {
                vec![None; rows.len()]
            }
        }
        let given = KinSource::by(Arc::new(Nobody));

        assert_eq!(given, given.clone());
        assert_ne!(given, KinSource::by(Arc::new(Nobody)));
        assert_eq!(KinSource::default(), KinSource::default());
    }
}
