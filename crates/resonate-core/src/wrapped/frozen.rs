use std::time::Duration;

use thiserror::Error;

use crate::{
    CivilDate,
    wrapped::{
        DAYS_IN_A_WEEK, DayHeard, FirstSong, FormatShares, HOURS_IN_A_DAY, MONTHS_IN_A_YEAR,
        Ranked, Rhythm, Streak, YearReview,
    },
};

pub const FROZEN_LAYOUT: u8 = 1;

const NANOS_IN_A_SECOND: u32 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum Unfrozen {
    #[error("a frozen review holds nothing")]
    Empty,

    #[error("a frozen review is laid out as {0}, which this build does not read")]
    UnknownLayout(u8),

    #[error("a frozen review ends part way through a field")]
    Truncated,

    #[error("a frozen review holds text that is not UTF-8")]
    NotText,

    #[error("a frozen review holds {0} nanoseconds past a whole second")]
    NanosPastASecond(u32),

    #[error("a frozen review holds {0} where it means yes or no")]
    NotAChoice(u8),

    #[error("a frozen review holds {0} bytes past its end")]
    Trailing(usize),
}

impl Unfrozen {
    pub const fn is_from_a_later_build(self) -> bool {
        matches!(self, Self::UnknownLayout(_))
    }
}

struct Writing(Vec<u8>);

impl Writing {
    fn byte(&mut self, byte: u8) {
        self.0.push(byte);
    }

    fn small(&mut self, number: u16) {
        self.0.extend_from_slice(&number.to_le_bytes());
    }

    fn medium(&mut self, number: u32) {
        self.0.extend_from_slice(&number.to_le_bytes());
    }

    fn large(&mut self, number: u64) {
        self.0.extend_from_slice(&number.to_le_bytes());
    }

    fn signed(&mut self, number: i64) {
        self.0.extend_from_slice(&number.to_le_bytes());
    }

    fn lasting(&mut self, span: Duration) {
        self.large(span.as_secs());
        self.medium(span.subsec_nanos());
    }

    fn text(&mut self, text: &str) {
        self.medium(u32::try_from(text.len()).unwrap_or(u32::MAX));
        self.0.extend_from_slice(text.as_bytes());
    }

    fn date(&mut self, date: CivilDate) {
        self.signed(date.year);
        self.signed(date.month);
        self.signed(date.day);
    }

    fn maybe<T>(&mut self, held: Option<&T>, write: impl FnOnce(&mut Self, &T)) {
        match held {
            Some(held) => {
                self.byte(1);
                write(self, held);
            }
            None => self.byte(0),
        }
    }

    fn ranked(&mut self, ranked: &[Ranked]) {
        self.medium(u32::try_from(ranked.len()).unwrap_or(u32::MAX));
        for one in ranked {
            self.text(&one.name);
            self.maybe(one.by.as_ref(), |this, by| this.text(by));
            self.large(one.plays);
            self.lasting(one.listened);
        }
    }
}

struct Reading<'a>(&'a [u8]);

impl<'a> Reading<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], Unfrozen> {
        if self.0.len() < length {
            return Err(Unfrozen::Truncated);
        }
        let (taken, rest) = self.0.split_at(length);
        self.0 = rest;

        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Unfrozen> {
        self.take(N)?.try_into().map_err(|_| Unfrozen::Truncated)
    }

    fn byte(&mut self) -> Result<u8, Unfrozen> {
        Ok(self.array::<1>()?[0])
    }

    fn small(&mut self) -> Result<u16, Unfrozen> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn medium(&mut self) -> Result<u32, Unfrozen> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn large(&mut self) -> Result<u64, Unfrozen> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn signed(&mut self) -> Result<i64, Unfrozen> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    fn lasting(&mut self) -> Result<Duration, Unfrozen> {
        let seconds = self.large()?;
        let nanos = self.medium()?;
        if nanos >= NANOS_IN_A_SECOND {
            return Err(Unfrozen::NanosPastASecond(nanos));
        }

        Ok(Duration::new(seconds, nanos))
    }

    fn text(&mut self) -> Result<String, Unfrozen> {
        let length = usize::try_from(self.medium()?).map_err(|_| Unfrozen::Truncated)?;

        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| Unfrozen::NotText)
    }

    fn date(&mut self) -> Result<CivilDate, Unfrozen> {
        Ok(CivilDate {
            year: self.signed()?,
            month: self.signed()?,
            day: self.signed()?,
        })
    }

    fn maybe<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, Unfrozen>,
    ) -> Result<Option<T>, Unfrozen> {
        match self.byte()? {
            0 => Ok(None),
            1 => read(self).map(Some),
            other => Err(Unfrozen::NotAChoice(other)),
        }
    }

    fn lastings<const N: usize>(&mut self) -> Result<[Duration; N], Unfrozen> {
        let mut spans = [Duration::ZERO; N];
        for span in &mut spans {
            *span = self.lasting()?;
        }

        Ok(spans)
    }

    fn ranked(&mut self) -> Result<Vec<Ranked>, Unfrozen> {
        let count = self.medium()?;
        let mut ranked = Vec::new();
        for _ in 0..count {
            ranked.push(Ranked {
                name: self.text()?,
                by: self.maybe(Self::text)?,
                plays: self.large()?,
                listened: self.lasting()?,
            });
        }

        Ok(ranked)
    }
}

impl YearReview {
    pub fn frozen(&self) -> Vec<u8> {
        let mut writing = Writing(vec![FROZEN_LAYOUT]);

        writing.signed(self.year);
        writing.lasting(self.listened);
        writing.large(self.plays);
        writing.large(self.songs);
        writing.large(self.artists);
        writing.large(self.albums);
        writing.maybe(self.songs_new.as_ref(), |this, new| this.large(*new));
        writing.large(self.songs_added);

        writing.ranked(&self.top_songs);
        writing.ranked(&self.top_artists);
        writing.ranked(&self.top_albums);
        writing.ranked(&self.top_genres);

        for span in self
            .rhythm
            .months
            .iter()
            .chain(&self.rhythm.hours)
            .chain(&self.rhythm.weekdays)
        {
            writing.lasting(*span);
        }
        writing.medium(self.rhythm.days_heard);
        writing.maybe(self.rhythm.busiest_day.as_ref(), |this, day| {
            this.date(day.date);
            this.lasting(day.listened);
        });
        writing.maybe(self.rhythm.longest_streak.as_ref(), |this, streak| {
            this.medium(streak.days);
            this.date(streak.from);
        });

        writing.maybe(self.first_song.as_ref(), |this, song| {
            this.text(&song.title);
            this.maybe(song.by.as_ref(), |this, by| this.text(by));
            this.date(song.on);
        });
        writing.maybe(self.formats.as_ref(), |this, formats| {
            this.small(formats.lossless_permille);
            this.small(formats.high_resolution_permille);
        });

        writing.0
    }

    pub fn unfrozen(bytes: &[u8]) -> Result<Self, Unfrozen> {
        let (layout, rest) = bytes.split_first().ok_or(Unfrozen::Empty)?;
        if *layout != FROZEN_LAYOUT {
            return Err(Unfrozen::UnknownLayout(*layout));
        }
        let mut reading = Reading(rest);

        let year = reading.signed()?;
        let listened = reading.lasting()?;
        let plays = reading.large()?;
        let songs = reading.large()?;
        let artists = reading.large()?;
        let albums = reading.large()?;
        let songs_new = reading.maybe(Reading::large)?;
        let songs_added = reading.large()?;

        let top_songs = reading.ranked()?;
        let top_artists = reading.ranked()?;
        let top_albums = reading.ranked()?;
        let top_genres = reading.ranked()?;

        let rhythm = Rhythm {
            months: reading.lastings::<MONTHS_IN_A_YEAR>()?,
            hours: reading.lastings::<HOURS_IN_A_DAY>()?,
            weekdays: reading.lastings::<DAYS_IN_A_WEEK>()?,
            days_heard: reading.medium()?,
            busiest_day: reading.maybe(|this| {
                Ok(DayHeard {
                    date: this.date()?,
                    listened: this.lasting()?,
                })
            })?,
            longest_streak: reading.maybe(|this| {
                Ok(Streak {
                    days: this.medium()?,
                    from: this.date()?,
                })
            })?,
        };

        let first_song = reading.maybe(|this| {
            Ok(FirstSong {
                title: this.text()?,
                by: this.maybe(Reading::text)?,
                on: this.date()?,
            })
        })?;
        let formats = reading.maybe(|this| {
            Ok(FormatShares {
                lossless_permille: this.small()?,
                high_resolution_permille: this.small()?,
            })
        })?;

        if !reading.0.is_empty() {
            return Err(Unfrozen::Trailing(reading.0.len()));
        }

        Ok(Self {
            year,
            listened,
            plays,
            songs,
            artists,
            albums,
            songs_new,
            songs_added,
            top_songs,
            top_artists,
            top_albums,
            top_genres,
            rhythm,
            first_song,
            formats,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranked(name: &str, by: Option<&str>) -> Ranked {
        Ranked {
            name: name.to_owned(),
            by: by.map(str::to_owned),
            plays: 7,
            listened: Duration::new(4_000, 250),
        }
    }

    fn a_busy_year() -> YearReview {
        let mut rhythm = Rhythm {
            days_heard: 212,
            busiest_day: Some(DayHeard {
                date: CivilDate {
                    year: 2025,
                    month: 11,
                    day: 3,
                },
                listened: Duration::from_secs(41_000),
            }),
            longest_streak: Some(Streak {
                days: 19,
                from: CivilDate {
                    year: 2025,
                    month: 2,
                    day: 8,
                },
            }),
            ..Rhythm::default()
        };
        for (at, span) in rhythm.months.iter_mut().enumerate() {
            *span = Duration::from_secs(at as u64 * 3_600 + 1);
        }
        for (at, span) in rhythm.hours.iter_mut().enumerate() {
            *span = Duration::from_millis(at as u64 * 90_001);
        }
        for (at, span) in rhythm.weekdays.iter_mut().enumerate() {
            *span = Duration::from_secs(at as u64 * 777);
        }

        YearReview {
            year: 2025,
            listened: Duration::new(9_000_000, 999_999_999),
            plays: 4_321,
            songs: 900,
            artists: 120,
            albums: 80,
            songs_new: Some(310),
            songs_added: 505,
            top_songs: vec![ranked("Echoes", Some("Pink Floyd")), ranked("Łódź ♪", None)],
            top_artists: vec![ranked("Björk", None)],
            top_albums: vec![ranked("Homogenic", Some("Björk"))],
            top_genres: vec![ranked("Ambient", None)],
            rhythm,
            first_song: Some(FirstSong {
                title: "Midnight".to_owned(),
                by: Some("Ada".to_owned()),
                on: CivilDate {
                    year: 2025,
                    month: 1,
                    day: 1,
                },
            }),
            formats: Some(FormatShares {
                lossless_permille: 640,
                high_resolution_permille: 120,
            }),
        }
    }

    #[test]
    fn a_review_frozen_and_thawed_is_the_review_it_was_whatever_it_held() {
        for review in [a_busy_year(), YearReview::default()] {
            assert_eq!(YearReview::unfrozen(&review.frozen()), Ok(review));
        }
    }

    #[test]
    fn a_frozen_review_names_its_layout_first_and_refuses_one_it_does_not_know() {
        let mut frozen = a_busy_year().frozen();

        assert_eq!(frozen[0], FROZEN_LAYOUT);
        frozen[0] = FROZEN_LAYOUT + 1;
        assert_eq!(
            YearReview::unfrozen(&frozen),
            Err(Unfrozen::UnknownLayout(FROZEN_LAYOUT + 1))
        );
        assert_eq!(YearReview::unfrozen(&[]), Err(Unfrozen::Empty));
    }

    #[test]
    fn a_frozen_review_cut_short_anywhere_is_refused_and_never_panics() {
        let frozen = a_busy_year().frozen();

        for length in 1..frozen.len() {
            assert!(
                YearReview::unfrozen(&frozen[..length]).is_err(),
                "a review cut at {length} bytes was read"
            );
        }
    }

    #[test]
    fn bytes_past_the_end_text_that_is_not_text_and_a_choice_of_three_are_refused() {
        let mut frozen = a_busy_year().frozen();
        frozen.extend_from_slice(&[0, 0]);
        assert_eq!(YearReview::unfrozen(&frozen), Err(Unfrozen::Trailing(2)));

        let mut chosen = YearReview::default().frozen();
        let songs_new_at = 1 + 8 + 12 + 8 * 4;
        chosen[songs_new_at] = 3;
        assert_eq!(YearReview::unfrozen(&chosen), Err(Unfrozen::NotAChoice(3)));

        let mut named = YearReview {
            top_songs: vec![ranked("ab", None)],
            ..YearReview::default()
        }
        .frozen();
        let text_at = named
            .iter()
            .position(|byte| *byte == b'a')
            .expect("the name");
        named[text_at] = 0xff;
        assert_eq!(YearReview::unfrozen(&named), Err(Unfrozen::NotText));
    }

    #[test]
    fn a_span_of_more_than_a_second_of_nanoseconds_is_refused() {
        let mut frozen = YearReview::default().frozen();
        let nanos_at = 1 + 8 + 8;
        frozen[nanos_at..nanos_at + 4].copy_from_slice(&NANOS_IN_A_SECOND.to_le_bytes());

        assert_eq!(
            YearReview::unfrozen(&frozen),
            Err(Unfrozen::NanosPastASecond(NANOS_IN_A_SECOND))
        );
    }
}
