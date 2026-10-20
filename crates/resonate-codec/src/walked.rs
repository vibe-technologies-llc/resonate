use std::{collections::BTreeMap, time::Duration};

use resonate_core::Frames;

const WRITTEN_AS: &str = "1";
const NOTHING: &str = "-";
const TRACK_APART_FROM_ITS_DISCARD: char = ':';
const DISCARDS_APART: char = ',';

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClusterTally {
    pub(crate) counted: Option<Duration>,
    pub(crate) discarded: BTreeMap<u64, Duration>,
    pub(crate) opus_samples: Option<u64>,
    pub(crate) framed_samples: Option<u64>,
    pub(crate) counted_track: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Walked {
    pub(crate) size: u64,
    pub(crate) mpeg_frames: Option<Frames>,
    pub(crate) clusters: ClusterTally,
}

impl Walked {
    pub const fn size(&self) -> u64 {
        self.size
    }

    pub fn written(&self) -> String {
        let clusters = &self.clusters;
        let fields = [
            WRITTEN_AS.to_owned(),
            optional(self.mpeg_frames.map(Frames::get)),
            optional(clusters.counted.map(nanoseconds)),
            optional(clusters.opus_samples),
            optional(clusters.framed_samples),
            optional(clusters.counted_track),
            discards_written(&clusters.discarded),
        ];

        fields.join(" ")
    }

    pub fn read(written: &str, size: u64) -> Option<Self> {
        let mut fields = written.split(' ');
        if fields.next()? != WRITTEN_AS {
            return None;
        }
        let mpeg_frames = number(fields.next()?)?.map(Frames);
        let counted = number(fields.next()?)?.map(Duration::from_nanos);
        let opus_samples = number(fields.next()?)?;
        let framed_samples = number(fields.next()?)?;
        let counted_track = number(fields.next()?)?;
        let discarded = discards_read(fields.next()?)?;

        fields.next().is_none().then_some(Self {
            size,
            mpeg_frames,
            clusters: ClusterTally {
                counted,
                discarded,
                opus_samples,
                framed_samples,
                counted_track,
            },
        })
    }
}

fn nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn optional(value: Option<u64>) -> String {
    value.map_or_else(|| NOTHING.to_owned(), |value| value.to_string())
}

fn number(field: &str) -> Option<Option<u64>> {
    if field == NOTHING {
        return Some(None);
    }
    field.parse().ok().map(Some)
}

fn discards_written(discarded: &BTreeMap<u64, Duration>) -> String {
    if discarded.is_empty() {
        return NOTHING.to_owned();
    }
    discarded
        .iter()
        .map(|(track, held)| {
            format!(
                "{track}{TRACK_APART_FROM_ITS_DISCARD}{}",
                nanoseconds(*held)
            )
        })
        .collect::<Vec<_>>()
        .join(&DISCARDS_APART.to_string())
}

fn discards_read(field: &str) -> Option<BTreeMap<u64, Duration>> {
    if field == NOTHING {
        return Some(BTreeMap::new());
    }
    field
        .split(DISCARDS_APART)
        .map(|pair| {
            let (track, held) = pair.split_once(TRACK_APART_FROM_ITS_DISCARD)?;
            Some((
                track.parse().ok()?,
                Duration::from_nanos(held.parse().ok()?),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walked() -> Walked {
        Walked {
            size: 4_096,
            mpeg_frames: Some(Frames(1_152_000)),
            clusters: ClusterTally {
                counted: Some(Duration::from_nanos(212_345_678_901)),
                discarded: BTreeMap::from([(1, Duration::from_millis(7)), (3, Duration::ZERO)]),
                opus_samples: Some(10_192_000),
                framed_samples: None,
                counted_track: Some(1),
            },
        }
    }

    #[test]
    fn what_was_written_reads_back_the_same() {
        let held = walked();

        assert_eq!(Walked::read(&held.written(), held.size()), Some(held));
    }

    #[test]
    fn nothing_found_reads_back_as_nothing() {
        let held = Walked {
            size: 9,
            ..Walked::default()
        };

        assert_eq!(held.written(), "1 - - - - - -");
        assert_eq!(Walked::read(&held.written(), 9), Some(held));
    }

    #[test]
    fn a_text_of_another_shape_is_no_walk() {
        for text in [
            "",
            "2 - - - - - -",
            "1 - - - - -",
            "1 - - - - - - -",
            "1 x - - - - -",
            "1 - - - - - 1:x",
            "1 - - - - - 1",
        ] {
            assert_eq!(Walked::read(text, 1), None, "{text:?}");
        }
    }

    #[test]
    fn a_walk_names_the_size_it_was_read_at() {
        let held = Walked::read("1 5 - - - - -", 77).expect("a walk");

        assert_eq!(held.size(), 77);
        assert_eq!(held.mpeg_frames, Some(Frames(5)));
    }
}
