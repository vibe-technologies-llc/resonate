use std::{cmp::Reverse, time::Duration};

use ahash::AHashMap;
use rusqlite::{Row, Transaction, params};

use crate::{
    Error, StoreOp,
    store::{codec_named_by, folded_letters, format_named_by},
};

const THE_SAME_LENGTH_WITHIN: Duration = Duration::from_secs(2);

const FIRST_DISC: u32 = 1;

const EVERY_COPY: &str =
    "SELECT t.id, coalesce(a.release_title, a.title), coalesce(r.name, t.artist),
            t.disc_number, t.track_number, t.title, t.duration, t.sample_rate, t.sample_format,
            t.codec, t.file_size, t.span_frames, t.alternative_of, a.title
       FROM tracks t
       LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = a.artist_id";

#[derive(Clone, PartialEq, Eq, Hash)]
struct Song {
    album: String,
    artist: String,
    disc: u32,
    track: Option<u32>,
    title: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Quality {
    lossless: bool,
    bits: u8,
    rate: i64,
    bitrate: u64,
}

struct Held {
    id: i64,
    song: Song,
    tagged_album: Option<String>,
    length: Option<Duration>,
    quality: Quality,
    held_under: Option<i64>,
}

impl Song {
    fn is_named_well_enough_to_meet(&self) -> bool {
        !self.title.is_empty() && !self.artist.is_empty()
    }
}

impl Held {
    fn songs(&self) -> impl Iterator<Item = Song> {
        let tagged = self
            .tagged_album
            .clone()
            .filter(|album| *album != self.song.album)
            .map(|album| Song {
                album,
                ..self.song.clone()
            });
        [Some(self.song.clone()), tagged]
            .into_iter()
            .flatten()
            .filter(Song::is_named_well_enough_to_meet)
    }

    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        let folded = |text: Option<String>| folded_letters(text.as_deref().unwrap_or_default());
        let rate: i64 = row.get(7)?;
        let frames: Option<i64> = row.get(6)?;
        let length = frames
            .filter(|_| rate > 0)
            .map(|frames| Duration::from_secs_f64(frames.max(0) as f64 / rate as f64));
        let codec = codec_named_by(row.get(9)?);
        let bits = format_named_by(row.get(8)?).map_or(0, |format| format.valid_bits());
        let whole_file = row.get::<_, Option<i64>>(11)?.is_none();
        let size: i64 = row.get(10)?;
        let bitrate = match length {
            Some(length) if whole_file && !length.is_zero() => {
                (size.max(0) as f64 * 8.0 / length.as_secs_f64()) as u64
            }
            _ => 0,
        };

        Ok(Self {
            id: row.get(0)?,
            song: Song {
                album: folded(row.get(1)?),
                artist: folded(row.get(2)?),
                disc: row.get::<_, Option<u32>>(3)?.unwrap_or(FIRST_DISC),
                track: row.get(4)?,
                title: folded(Some(row.get(5)?)),
            },
            tagged_album: row
                .get::<_, Option<String>>(13)?
                .map(|title| folded(Some(title))),
            length,
            quality: Quality {
                lossless: codec.is_some_and(|codec| codec.is_lossless()),
                bits,
                rate,
                bitrate,
            },
            held_under: row.get(12)?,
        })
    }
}

pub(crate) fn settle(tx: &Transaction<'_>) -> crate::Result<u64> {
    let copies: Vec<Held> = tx
        .prepare(EVERY_COPY)
        .and_then(|mut statement| {
            statement
                .query_map([], Held::read)
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        })
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut moved = 0;
    for copies in one_song_apiece(copies) {
        for (copy, best) in held_under_the_best(copies) {
            if copy.held_under == best {
                continue;
            }
            moved += tx
                .execute(
                    "UPDATE tracks SET alternative_of = ?1 WHERE id = ?2",
                    params![best, copy.id],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
        }
    }
    Ok(moved as u64)
}

fn one_song_apiece(copies: Vec<Held>) -> Vec<Vec<Held>> {
    let mut gathered = Gathering::of(copies.len());
    let mut first_named: AHashMap<Song, usize> = AHashMap::new();
    for (index, copy) in copies.iter().enumerate() {
        for song in copy.songs() {
            match first_named.get(&song) {
                Some(&first) => gathered.join(first, index),
                None => {
                    first_named.insert(song, index);
                }
            }
        }
    }

    let mut songs: AHashMap<usize, Vec<Held>> = AHashMap::new();
    for (index, copy) in copies.into_iter().enumerate() {
        songs.entry(gathered.root(index)).or_default().push(copy);
    }
    songs.into_values().collect()
}

struct Gathering {
    under: Vec<usize>,
}

impl Gathering {
    fn of(count: usize) -> Self {
        Self {
            under: (0..count).collect(),
        }
    }

    fn root(&mut self, mut index: usize) -> usize {
        while let Some(&above) = self.under.get(index)
            && above != index
        {
            let grand = self.under.get(above).copied().unwrap_or(above);
            if let Some(slot) = self.under.get_mut(index) {
                *slot = grand;
            }
            index = above;
        }
        index
    }

    fn join(&mut self, one: usize, other: usize) {
        let (one, other) = (self.root(one), self.root(other));
        let (low, high) = (one.min(other), one.max(other));
        if let Some(slot) = self.under.get_mut(high) {
            *slot = low;
        }
    }
}

fn held_under_the_best(mut copies: Vec<Held>) -> Vec<(Held, Option<i64>)> {
    copies.sort_by_key(|copy| copy.length);
    let mut settled = Vec::with_capacity(copies.len());
    let mut run: Vec<Held> = Vec::new();

    for copy in copies {
        let joins = match (run.first().map(|first| first.length), copy.length) {
            (Some(Some(first)), Some(length)) => {
                length.saturating_sub(first) <= THE_SAME_LENGTH_WITHIN
            }
            (Some(None), None) => true,
            _ => false,
        };
        if !joins {
            settled.extend(crowned(std::mem::take(&mut run)));
        }
        run.push(copy);
    }
    settled.extend(crowned(run));
    settled
}

fn crowned(run: Vec<Held>) -> Vec<(Held, Option<i64>)> {
    let best = run
        .iter()
        .max_by_key(|copy| (copy.quality, Reverse(copy.id)))
        .map(|copy| copy.id);

    run.into_iter()
        .map(|copy| {
            let under = best.filter(|id| *id != copy.id);
            (copy, under)
        })
        .collect()
}
