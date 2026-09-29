use std::num::NonZeroU32;

use resonate_core::{Frames, SampleRate};
use symphonia::core::meta::{ChapterGroup, ChapterGroupItem, StandardTag, Tag};

use crate::{
    TagSet,
    cue::{CueFile, CueStart, CueTrack, CueTrackKind},
};

const NANOS_A_SECOND: NonZeroU32 = NonZeroU32::new(1_000_000_000).expect("a second is not zero");
const FEWEST_CHAPTERS_CUT: usize = 2;
pub(crate) const MOST_CHAPTERS: usize = 999;
const COMMENT_PREFIX: &str = "CHAPTER";
const NAME_SUFFIX: &str = "NAME";
const FRACTION_DIGITS: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChapterStart {
    ticks: u64,
    per_second: NonZeroU32,
}

impl ChapterStart {
    pub(crate) const fn new(ticks: u64, per_second: NonZeroU32) -> Self {
        Self { ticks, per_second }
    }

    fn at(self, rate: SampleRate) -> Frames {
        let frames =
            u128::from(self.ticks) * u128::from(rate.hz()) / u128::from(self.per_second.get());
        Frames(u64::try_from(frames).unwrap_or(u64::MAX))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Marked {
    pub(crate) start: ChapterStart,
    pub(crate) title: Option<String>,
}

pub(crate) fn of_group(group: &ChapterGroup) -> Vec<Marked> {
    let marked: Vec<Marked> = group
        .items
        .iter()
        .filter_map(|item| match item {
            ChapterGroupItem::Chapter(chapter) => {
                let nanos = u64::try_from(chapter.start_time.as_nanos()).ok()?;
                Some(Marked {
                    start: ChapterStart::new(nanos, NANOS_A_SECOND),
                    title: title_of(&chapter.tags),
                })
            }
            ChapterGroupItem::Group(_) => None,
        })
        .take(MOST_CHAPTERS)
        .collect();
    if !marked.is_empty() {
        return marked;
    }

    group
        .items
        .iter()
        .find_map(|item| match item {
            ChapterGroupItem::Group(edition) => Some(of_group(edition)),
            ChapterGroupItem::Chapter(_) => None,
        })
        .unwrap_or_default()
}

fn title_of(tags: &[Tag]) -> Option<String> {
    tags.iter().find_map(|tag| match &tag.std {
        Some(StandardTag::ChapterTitle(title) | StandardTag::TrackTitle(title)) => named(title),
        _ => None,
    })
}

pub(crate) fn of_comments(comments: &[(String, String)]) -> Vec<Marked> {
    let mut marked: Vec<(u32, Marked)> = comments
        .iter()
        .filter_map(|(key, value)| {
            let number = key.strip_prefix(COMMENT_PREFIX)?;
            if !number.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let title = comments
                .iter()
                .find(|(named, _)| {
                    named
                        .strip_prefix(COMMENT_PREFIX)
                        .and_then(|rest| rest.strip_suffix(NAME_SUFFIX))
                        == Some(number)
                })
                .and_then(|(_, title)| named(title));
            Some((
                number.parse().ok()?,
                Marked {
                    start: ChapterStart::new(clock_nanos(value)?, NANOS_A_SECOND),
                    title,
                },
            ))
        })
        .take(MOST_CHAPTERS)
        .collect();
    marked.sort_by_key(|(number, _)| *number);
    marked.into_iter().map(|(_, held)| held).collect()
}

fn clock_nanos(value: &str) -> Option<u64> {
    let (clock, fraction) = value.trim().split_once('.').unwrap_or((value.trim(), ""));
    let mut fields = clock.split(':');
    let (hours, minutes, seconds) = (fields.next()?, fields.next()?, fields.next()?);
    if fields.next().is_some() || fraction.len() > FRACTION_DIGITS {
        return None;
    }
    let whole = hours.parse::<u64>().ok()?.checked_mul(3_600)?
        + minutes.parse::<u64>().ok().filter(|held| *held < 60)? * 60
        + seconds.parse::<u64>().ok().filter(|held| *held < 60)?;
    let nanos = match fraction {
        "" => 0,
        digits => digits.parse::<u64>().ok()? * 10_u64.pow((FRACTION_DIGITS - digits.len()) as u32),
    };
    whole
        .checked_mul(u64::from(NANOS_A_SECOND.get()))?
        .checked_add(nanos)
}

pub(crate) fn named(title: &str) -> Option<String> {
    let title = title.trim_matches(char::from(0)).trim();
    (!title.is_empty()).then(|| title.to_owned())
}

pub(crate) fn cut(marked: &[Marked], rate: SampleRate, file: &TagSet) -> Option<CueFile> {
    let mut starts: Vec<(Frames, Option<&str>)> = marked
        .iter()
        .map(|held| (held.start.at(rate), held.title.as_deref()))
        .collect();
    starts.sort_by_key(|(at, _)| *at);
    starts.dedup_by_key(|(at, _)| *at);
    if starts.len() < FEWEST_CHAPTERS_CUT {
        return None;
    }

    let whole = TagSet {
        album: file.album.clone().or_else(|| file.title.clone()),
        ..file.clone()
    };
    let mut cut = CueFile {
        named: String::new(),
        tracks: starts
            .iter()
            .enumerate()
            .map(|(at, (start, _))| CueTrack {
                number: at as u32 + 1,
                kind: CueTrackKind::Audio,
                start: CueStart::Sampled(*start),
                lead_in: None,
                tags: TagSet::default(),
            })
            .collect(),
    }
    .billed_by(&whole);
    cut.heard_from_the_head();
    for (track, (_, title)) in cut.tracks.iter_mut().zip(&starts) {
        track.tags.title = title.map(str::to_owned);
    }
    Some(cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_seconds(seconds: u64, title: &str) -> Marked {
        Marked {
            start: ChapterStart::new(seconds, NonZeroU32::MIN),
            title: named(title),
        }
    }

    #[test]
    fn chapters_cut_a_file_into_rows_named_by_their_titles_and_billed_by_the_album() {
        let file = TagSet {
            title: Some("The Hobbit".to_owned()),
            artist: Some("J. R. R. Tolkien".to_owned()),
            ..TagSet::default()
        };
        let cut = cut(
            &[
                at_seconds(0, "An Unexpected Party"),
                at_seconds(1_200, "Roast Mutton"),
                at_seconds(2_000, " "),
            ],
            SampleRate::HZ_44100,
            &file,
        )
        .expect("three chapters cut the file");

        let rows: Vec<_> = cut
            .audio_tracks()
            .map(|(_, track)| {
                (
                    track.start.at(SampleRate::HZ_44100),
                    track.tags.title.clone(),
                    track.tags.album.clone(),
                    track.tags.track_number,
                )
            })
            .collect();
        let book = Some("The Hobbit".to_owned());
        assert_eq!(
            rows,
            vec![
                (
                    Frames::ZERO,
                    Some("An Unexpected Party".to_owned()),
                    book.clone(),
                    Some(1)
                ),
                (
                    Frames(1_200 * 44_100),
                    Some("Roast Mutton".to_owned()),
                    book.clone(),
                    Some(2)
                ),
                (Frames(2_000 * 44_100), None, book, Some(3)),
            ]
        );
        assert_eq!(cut.tracks[2].titled().title.as_deref(), Some("Track 3"));
    }

    #[test]
    fn chapter_comments_are_read_in_their_numbered_order_to_the_millisecond() {
        let comments = [
            ("CHAPTER002", "00:01:02.5"),
            ("CHAPTER002NAME", "Roast Mutton"),
            ("CHAPTER001", "00:00:00.000"),
            ("CHAPTER001NAME", "An Unexpected Party"),
            ("CHAPTER003", "1:00:00"),
            ("CHAPTERX", "00:00:09.000"),
            ("CHAPTER004", "00:61:00.000"),
        ]
        .map(|(key, value)| (key.to_owned(), value.to_owned()));

        let marked = of_comments(&comments);
        assert_eq!(
            marked
                .iter()
                .map(|held| (held.start.at(SampleRate::HZ_48000), held.title.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                (Frames::ZERO, Some("An Unexpected Party")),
                (Frames(62 * 48_000 + 24_000), Some("Roast Mutton")),
                (Frames(3_600 * 48_000), None),
            ]
        );
    }

    #[test]
    fn one_chapter_or_chapters_at_one_moment_cut_nothing() {
        let file = TagSet::default();

        assert_eq!(
            cut(&[at_seconds(0, "Whole")], SampleRate::HZ_44100, &file),
            None
        );
        assert_eq!(
            cut(
                &[at_seconds(5, "One"), at_seconds(5, "Again")],
                SampleRate::HZ_44100,
                &file
            ),
            None
        );
    }

    #[test]
    fn a_first_chapter_starting_late_still_starts_at_the_head_of_the_file() {
        let cut = cut(
            &[at_seconds(3, "Late"), at_seconds(9, "Later")],
            SampleRate::HZ_44100,
            &TagSet::default(),
        )
        .expect("two chapters cut the file");

        assert_eq!(cut.tracks[0].start.at(SampleRate::HZ_44100), Frames::ZERO);
    }
}
