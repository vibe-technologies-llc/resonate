use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime},
};

use resonate_words::Brevity;

use crate::{Calendar, CivilDate, folded_letters, words};

mod frozen;

pub use frozen::{FROZEN_LAYOUT, Unfrozen};

pub const REVIEW_NAME: &str = "Resonate Wrapped";

pub const RANKED_AT_MOST: usize = 5;

pub const MONTHS_IN_A_YEAR: usize = 12;

pub const HOURS_IN_A_DAY: usize = 24;

pub const DAYS_IN_A_WEEK: usize = 7;

const THE_EPOCH_FELL_DAYS_AFTER_A_MONDAY: i64 = 3;

const MINUTES_IN_AN_HOUR: u32 = 60;

const SECONDS_IN_A_MINUTE: u64 = 60;

const SECONDS_IN_AN_HOUR: u64 = 3_600;

const WHOLE_SHARE: u64 = 1_000;

const DECEMBER: i64 = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    pub name: String,
    pub by: Option<String>,
    pub plays: u64,
    pub listened: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayHeard {
    pub date: CivilDate,
    pub listened: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Streak {
    pub days: u32,
    pub from: CivilDate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstSong {
    pub title: String,
    pub by: Option<String>,
    pub on: CivilDate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatHeard {
    pub lossless: bool,
    pub above_cd: bool,
    pub listened: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatShares {
    pub lossless_permille: u16,
    pub high_resolution_permille: u16,
}

impl FormatShares {
    pub fn of(heard: impl IntoIterator<Item = FormatHeard>) -> Option<Self> {
        let mut whole = 0_u128;
        let mut lossless = 0_u128;
        let mut high = 0_u128;

        for format in heard {
            let listened = format.listened.as_millis();
            whole += listened;
            if format.lossless {
                lossless += listened;
                if format.above_cd {
                    high += listened;
                }
            }
        }

        (whole > 0).then(|| Self {
            lossless_permille: permille(lossless, whole),
            high_resolution_permille: permille(high, whole),
        })
    }
}

fn permille(part: u128, whole: u128) -> u16 {
    let rounded = (part * u128::from(WHOLE_SHARE) + whole / 2) / whole;

    u16::try_from(rounded).unwrap_or(u16::MAX)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuarterHeard {
    pub at: SystemTime,
    pub listened: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rhythm {
    pub months: [Duration; MONTHS_IN_A_YEAR],
    pub hours: [Duration; HOURS_IN_A_DAY],
    pub weekdays: [Duration; DAYS_IN_A_WEEK],
    pub days_heard: u32,
    pub busiest_day: Option<DayHeard>,
    pub longest_streak: Option<Streak>,
}

impl Rhythm {
    pub fn of(
        quarters: impl IntoIterator<Item = QuarterHeard>,
        calendar: &Calendar,
        year: i64,
    ) -> Self {
        Self::of_those(quarters, calendar, |date| date.year == year)
    }

    pub fn across(quarters: impl IntoIterator<Item = QuarterHeard>, calendar: &Calendar) -> Self {
        Self::of_those(quarters, calendar, |_| true)
    }

    fn of_those(
        quarters: impl IntoIterator<Item = QuarterHeard>,
        calendar: &Calendar,
        counted: impl Fn(CivilDate) -> bool,
    ) -> Self {
        let mut rhythm = Self::default();
        let mut days: BTreeMap<i64, Duration> = BTreeMap::new();

        for quarter in quarters {
            let day = calendar.day_of(quarter.at);
            let date = CivilDate::of_day(day);
            if !counted(date) || quarter.listened.is_zero() {
                continue;
            }

            let month = usize::try_from(date.month - 1).unwrap_or(0);
            let hour = usize::try_from(calendar.minute_of_the_day(quarter.at) / MINUTES_IN_AN_HOUR)
                .unwrap_or(0);
            if let Some(held) = rhythm.months.get_mut(month) {
                *held += quarter.listened;
            }
            if let Some(held) = rhythm.hours.get_mut(hour) {
                *held += quarter.listened;
            }
            if let Some(held) = rhythm.weekdays.get_mut(weekday_of(day)) {
                *held += quarter.listened;
            }
            *days.entry(day).or_default() += quarter.listened;
        }

        rhythm.days_heard = u32::try_from(days.len()).unwrap_or(u32::MAX);
        rhythm.busiest_day = days
            .iter()
            .max_by_key(|(day, listened)| (**listened, std::cmp::Reverse(**day)))
            .map(|(day, listened)| DayHeard {
                date: CivilDate::of_day(*day),
                listened: *listened,
            });
        rhythm.longest_streak = longest_run(days.keys().copied());

        rhythm
    }

    fn busiest_month(&self) -> Option<usize> {
        busiest(&self.months)
    }

    pub fn busiest_hour(&self) -> Option<usize> {
        busiest(&self.hours)
    }

    pub fn busiest_weekday(&self) -> Option<usize> {
        busiest(&self.weekdays)
    }
}

const fn weekday_of(day: i64) -> usize {
    day.saturating_add(THE_EPOCH_FELL_DAYS_AFTER_A_MONDAY)
        .rem_euclid(DAYS_IN_A_WEEK as i64) as usize
}

fn busiest(spans: &[Duration]) -> Option<usize> {
    spans
        .iter()
        .enumerate()
        .filter(|(_, listened)| !listened.is_zero())
        .max_by_key(|(at, listened)| (**listened, std::cmp::Reverse(*at)))
        .map(|(at, _)| at)
}

fn longest_run(days: impl IntoIterator<Item = i64>) -> Option<Streak> {
    let mut longest: Option<(i64, u32)> = None;
    let mut running: Option<(i64, i64)> = None;

    for day in days {
        let (from, _) = match running {
            Some((from, last)) if last + 1 == day => (from, day),
            _ => (day, day),
        };
        running = Some((from, day));

        let length = u32::try_from(day - from + 1).unwrap_or(u32::MAX);
        if longest.is_none_or(|(_, held)| length > held) {
            longest = Some((from, length));
        }
    }

    longest.map(|(from, days)| Streak {
        days,
        from: CivilDate::of_day(from),
    })
}

pub fn genres_tallied(
    heard: impl IntoIterator<Item = (String, u64, Duration)>,
    listed_apart_by: &str,
    most: usize,
) -> Vec<Ranked> {
    let mut tally: BTreeMap<String, Ranked> = BTreeMap::new();

    for (genres, plays, listened) in heard {
        for genre in genres
            .split(listed_apart_by)
            .map(str::trim)
            .filter(|genre| !genre.is_empty())
        {
            let held = tally
                .entry(folded_letters(genre))
                .or_insert_with(|| Ranked {
                    name: genre.to_owned(),
                    by: None,
                    plays: 0,
                    listened: Duration::ZERO,
                });
            held.plays += plays;
            held.listened += listened;
        }
    }

    let mut ranked: Vec<Ranked> = tally.into_values().collect();
    ranked.sort_by(|one, other| {
        other
            .listened
            .cmp(&one.listened)
            .then(other.plays.cmp(&one.plays))
            .then_with(|| one.name.cmp(&other.name))
    });
    ranked.truncate(most);

    ranked
}

pub fn year_worth_showing(years_heard: &[i64], today: CivilDate) -> Option<i64> {
    let latest = years_heard.iter().copied().max()?;
    let earlier = years_heard
        .iter()
        .copied()
        .filter(|year| *year < today.year)
        .max();

    Some(match (latest == today.year, earlier) {
        (true, Some(earlier)) if today.month < DECEMBER => earlier,
        _ => latest,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct YearReview {
    pub year: i64,
    pub listened: Duration,
    pub plays: u64,
    pub songs: u64,
    pub artists: u64,
    pub albums: u64,
    pub songs_new: Option<u64>,
    pub songs_added: u64,
    pub top_songs: Vec<Ranked>,
    pub top_artists: Vec<Ranked>,
    pub top_albums: Vec<Ranked>,
    pub top_genres: Vec<Ranked>,
    pub rhythm: Rhythm,
    pub first_song: Option<FirstSong>,
    pub formats: Option<FormatShares>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideKind {
    Whole,
    Songs,
    Artists,
    Albums,
    Genres,
    Months,
    Hours,
    Days,
    Discoveries,
    Sound,
    FirstSong,
    Farewell,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlideRow {
    pub name: String,
    pub by: Option<String>,
    pub figure: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlideBar {
    pub label: String,
    pub said: String,
    pub share_permille: u16,
    pub peak: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slide {
    pub kind: SlideKind,
    pub eyebrow: String,
    pub headline: String,
    pub detail: Option<String>,
    pub rows: Vec<SlideRow>,
    pub bars: Vec<SlideBar>,
}

impl Slide {
    fn saying(kind: SlideKind, eyebrow: &str, headline: String) -> Self {
        Self {
            kind,
            eyebrow: eyebrow.to_owned(),
            headline,
            detail: None,
            rows: Vec::new(),
            bars: Vec::new(),
        }
    }

    fn detailed(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    fn ranked(mut self, ranked: &[Ranked]) -> Self {
        self.rows = ranked
            .iter()
            .map(|row| SlideRow {
                name: row.name.clone(),
                by: row.by.clone(),
                figure: spelled_listening(row.listened),
            })
            .collect();
        self
    }

    fn charted(mut self, spans: &[Duration], label_of: impl Fn(usize) -> String) -> Self {
        let tallest = spans.iter().copied().max().unwrap_or(Duration::ZERO);
        let peak = busiest(spans);

        self.bars = spans
            .iter()
            .enumerate()
            .map(|(at, listened)| SlideBar {
                label: label_of(at),
                said: format!("{}: {}", label_of(at), spelled_listening(*listened)),
                share_permille: permille(listened.as_millis(), tallest.as_millis().max(1)),
                peak: peak == Some(at),
            })
            .collect();
        self
    }
}

impl YearReview {
    pub fn holds_anything(&self) -> bool {
        !self.listened.is_zero() || self.plays > 0
    }

    pub fn slides(&self) -> Vec<Slide> {
        if !self.holds_anything() {
            return vec![
                Slide::saying(
                    SlideKind::Whole,
                    &self.eyebrow(),
                    words::wrapped::nothing_played(self.year),
                )
                .detailed(words::wrapped::play_something()),
            ];
        }

        let mut slides = vec![self.whole()];
        slides.extend(self.top_of(
            SlideKind::Songs,
            &self.top_songs,
            words::wrapped::song_of_the_year(),
            None,
        ));
        slides.extend(self.top_of(
            SlideKind::Artists,
            &self.top_artists,
            words::wrapped::artist_of_the_year(),
            Some(words::wrapped::what_the_rankings_count()),
        ));
        slides.extend(self.top_of(
            SlideKind::Albums,
            &self.top_albums,
            words::wrapped::album_of_the_year(),
            None,
        ));
        slides.extend(self.top_of(
            SlideKind::Genres,
            &self.top_genres,
            words::wrapped::genre_of_the_year(),
            None,
        ));
        slides.extend(self.months());
        slides.extend(self.hours());
        slides.extend(self.days());
        slides.extend(self.discoveries());
        slides.extend(self.sound());
        slides.extend(self.first());
        slides.push(self.farewell());

        slides
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![
            words::wrapped::my_year(self.year),
            words::wrapped::of_music(spelled_listening(self.listened)),
            format!(
                "{} · {} · {}",
                words::wrapped::songs(self.songs),
                words::wrapped::artists(self.artists),
                words::wrapped::albums(self.albums),
            ),
        ];
        if let Some(song) = self.top_songs.first() {
            lines.push(words::wrapped::top_song(named_with_its_artist(song)));
        }
        if let Some(artist) = self.top_artists.first() {
            lines.push(words::wrapped::top_artist(artist.name.as_str()));
        }
        if let Some(album) = self.top_albums.first() {
            lines.push(words::wrapped::top_album(named_with_its_artist(album)));
        }
        if let Some(genre) = self.top_genres.first() {
            lines.push(words::wrapped::top_genre(genre.name.as_str()));
        }
        if let Some(streak) = self.rhythm.longest_streak.filter(|streak| streak.days > 1) {
            lines.push(words::wrapped::longest_streak(streak.days));
        }
        if let Some(formats) = self.formats {
            lines.push(words::wrapped::lossless(spelled_share(
                formats.lossless_permille,
            )));
        }

        lines.join("\n")
    }

    fn eyebrow(&self) -> String {
        format!("{REVIEW_NAME} {}", self.year)
    }

    fn whole(&self) -> Slide {
        let across = [
            words::wrapped::songs(self.songs),
            words::wrapped::artists(self.artists),
            words::wrapped::albums(self.albums),
        ]
        .join(" · ");

        Slide::saying(
            SlideKind::Whole,
            &self.eyebrow(),
            words::wrapped::of_music(spelled_listening(self.listened)),
        )
        .detailed(words::wrapped::across_the_days(
            across,
            words::wrapped::days(self.rhythm.days_heard),
        ))
    }

    fn top_of(
        &self,
        kind: SlideKind,
        ranked: &[Ranked],
        eyebrow: &str,
        note: Option<&str>,
    ) -> Option<Slide> {
        let leader = ranked.first()?;
        let slide = Slide::saying(kind, eyebrow, leader.name.clone()).ranked(ranked);

        Some(match (&leader.by, note) {
            (Some(by), _) => slide.detailed(by.clone()),
            (None, Some(note)) => slide.detailed(note),
            (None, None) => slide,
        })
    }

    fn months(&self) -> Option<Slide> {
        let month = self.rhythm.busiest_month()?;

        Some(
            Slide::saying(
                SlideKind::Months,
                words::wrapped::your_year(),
                words::wrapped::biggest_month(month_in_full(month)),
            )
            .charted(&self.rhythm.months, month_briefly),
        )
    }

    fn hours(&self) -> Option<Slide> {
        let hour = self.rhythm.busiest_hour()?;

        Some(
            Slide::saying(
                SlideKind::Hours,
                words::wrapped::your_day(),
                words::wrapped::listened_most_at(format!("{hour:02}")),
            )
            .detailed(time_of_day(hour))
            .charted(&self.rhythm.hours, |at| format!("{at:02}")),
        )
    }

    fn days(&self) -> Option<Slide> {
        let busiest = self.rhythm.busiest_day?;
        let on = words::wrapped::biggest_day_was(
            spelled_date(busiest.date),
            spelled_listening(busiest.listened),
        );

        Some(match self.rhythm.longest_streak {
            Some(streak) if streak.days > 1 => Slide::saying(
                SlideKind::Days,
                words::wrapped::your_streak(),
                words::wrapped::in_a_row(words::wrapped::days(streak.days)),
            )
            .detailed(words::wrapped::streak_from(on, spelled_date(streak.from))),
            _ => Slide::saying(
                SlideKind::Days,
                words::wrapped::your_biggest_day(),
                spelled_date(busiest.date),
            )
            .detailed(spelled_listening(busiest.listened)),
        })
    }

    fn discoveries(&self) -> Option<Slide> {
        let added = (self.songs_added > 0)
            .then(|| words::wrapped::joined_your_library(words::wrapped::songs(self.songs_added)));

        match (self.songs_new.filter(|new| *new > 0), added) {
            (Some(new), added) => {
                let slide = Slide::saying(
                    SlideKind::Discoveries,
                    words::wrapped::new_to_you(),
                    words::wrapped::heard_for_the_first_time(words::wrapped::songs(new)),
                );
                Some(match added {
                    Some(added) => slide.detailed(added),
                    None => slide,
                })
            }
            (None, Some(added)) => Some(Slide::saying(
                SlideKind::Discoveries,
                words::wrapped::new_to_your_library(),
                added,
            )),
            (None, None) => None,
        }
    }

    fn sound(&self) -> Option<Slide> {
        let formats = self.formats?;

        Some(
            Slide::saying(
                SlideKind::Sound,
                words::wrapped::your_sound(),
                words::wrapped::lossless(spelled_share(formats.lossless_permille)),
            )
            .detailed(words::wrapped::above_cd_quality(spelled_share(
                formats.high_resolution_permille,
            ))),
        )
    }

    fn first(&self) -> Option<Slide> {
        let first = self.first_song.as_ref()?;
        let on = spelled_date(first.on);

        Some(
            Slide::saying(
                SlideKind::FirstSong,
                words::wrapped::where_it_began(),
                first.title.clone(),
            )
            .detailed(match &first.by {
                Some(by) => words::wrapped::by_on(by.as_str(), on),
                None => words::wrapped::only_on(on),
            }),
        )
    }

    fn farewell(&self) -> Slide {
        Slide::saying(
            SlideKind::Farewell,
            &self.eyebrow(),
            words::wrapped::that_was_your_year(self.year),
        )
        .detailed(words::wrapped::of_music(spelled_listening(self.listened)))
    }
}

fn named_with_its_artist(ranked: &Ranked) -> String {
    match &ranked.by {
        Some(by) => words::wrapped::named_by(by.as_str(), ranked.name.as_str()),
        None => ranked.name.clone(),
    }
}

fn month_in_full(index: usize) -> String {
    resonate_words::month_name(Brevity::Full, month_of(index))
}

fn month_briefly(index: usize) -> String {
    resonate_words::month_name(Brevity::Brief, month_of(index))
}

fn month_of(index: usize) -> i64 {
    i64::try_from(index).map_or(0, |index| index + 1)
}

fn time_of_day(hour: usize) -> &'static str {
    match hour {
        0..=4 => words::wrapped::small_hours(),
        5..=8 => words::wrapped::start_the_day(),
        9..=11 => words::wrapped::mornings(),
        12..=16 => words::wrapped::afternoons(),
        17..=20 => words::wrapped::evenings(),
        _ => words::wrapped::late(),
    }
}

pub fn spelled_listening(span: Duration) -> String {
    let seconds = span.as_secs();

    match seconds / SECONDS_IN_AN_HOUR {
        0 => words::wrapped::minutes(seconds / SECONDS_IN_A_MINUTE),
        hours => words::wrapped::hours(hours),
    }
}

pub fn spelled_share(permille: u16) -> String {
    format!("{}%", (u64::from(permille) + 5) / 10)
}

fn spelled_date(date: CivilDate) -> String {
    date.day_and_month(Brevity::Full)
}

impl Calendar {
    pub fn year_bounds(&self, year: i64) -> (SystemTime, SystemTime) {
        let day = |year: i64| {
            CivilDate {
                year,
                month: 1,
                day: 1,
            }
            .day_since_the_epoch()
        };

        (
            self.midnight_of(day(year)),
            self.midnight_of(day(year.saturating_add(1))),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    const A_MINUTE: Duration = Duration::from_secs(60);
    const AN_HOUR: Duration = Duration::from_secs(3_600);
    const TWO_HOURS_EAST: i32 = 7_200;
    const THE_FIRST_OF_JANUARY_2024: u64 = 19_723;

    fn at(day: u64, seconds_into_it: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(day * 86_400 + seconds_into_it)
    }

    fn date(year: i64, month: i64, day: i64) -> CivilDate {
        CivilDate { year, month, day }
    }

    fn a_song(name: &str, by: &str, listened: Duration) -> Ranked {
        Ranked {
            name: name.to_owned(),
            by: Some(by.to_owned()),
            plays: 1,
            listened,
        }
    }

    #[test]
    fn a_year_begins_at_the_listeners_own_midnight_and_ends_at_the_next() {
        let east = Calendar::fixed(TWO_HOURS_EAST).expect("a fixed offset is a zone");
        let (from, to) = east.year_bounds(2024);

        assert_eq!(east.date_at(from), date(2024, 1, 1));
        assert_eq!(
            east.date_at(from - Duration::from_secs(1)),
            date(2023, 12, 31)
        );
        assert_eq!(east.date_at(to), date(2025, 1, 1));
        assert_eq!(
            to.duration_since(from)
                .expect("a year ends after it begins"),
            Duration::from_secs(366 * 86_400)
        );
    }

    #[test]
    fn a_listen_late_on_new_years_eve_in_greenwich_falls_in_the_listeners_new_year() {
        let east = Calendar::fixed(TWO_HOURS_EAST).expect("a fixed offset is a zone");
        let late = QuarterHeard {
            at: at(THE_FIRST_OF_JANUARY_2024 - 1, 23 * 3_600),
            listened: AN_HOUR,
        };

        let in_utc = Rhythm::of([late], &Calendar::utc(), 2024);
        let in_the_east = Rhythm::of([late], &east, 2024);

        assert_eq!(in_utc.days_heard, 0);
        assert_eq!(in_the_east.days_heard, 1);
        assert_eq!(in_the_east.months[0], AN_HOUR);
        assert_eq!(in_the_east.hours[1], AN_HOUR);
    }

    #[test]
    fn a_streak_is_the_longest_run_of_days_with_no_day_missed() {
        let heard = [0, 1, 2, 5, 6, 7, 8, 12].map(|day| QuarterHeard {
            at: at(THE_FIRST_OF_JANUARY_2024 + day, 3_600),
            listened: A_MINUTE,
        });

        let rhythm = Rhythm::of(heard, &Calendar::utc(), 2024);

        assert_eq!(rhythm.days_heard, 8);
        assert_eq!(
            rhythm.longest_streak,
            Some(Streak {
                days: 4,
                from: date(2024, 1, 6)
            })
        );
    }

    #[test]
    fn the_busiest_day_is_the_one_heard_longest_and_the_earlier_of_two_alike() {
        let heard =
            [(0, A_MINUTE), (3, AN_HOUR), (9, AN_HOUR)].map(|(day, listened)| QuarterHeard {
                at: at(THE_FIRST_OF_JANUARY_2024 + day, 0),
                listened,
            });

        let rhythm = Rhythm::of(heard, &Calendar::utc(), 2024);

        assert_eq!(
            rhythm.busiest_day,
            Some(DayHeard {
                date: date(2024, 1, 4),
                listened: AN_HOUR
            })
        );
    }

    #[test]
    fn genres_listed_on_one_song_each_count_it_and_spellings_folded_alike_are_one() {
        let tallied = genres_tallied(
            [
                ("Jazz; Fusion".to_owned(), 2, AN_HOUR),
                ("jazz".to_owned(), 1, A_MINUTE),
                ("Fusion".to_owned(), 1, A_MINUTE),
                ("  ".to_owned(), 9, AN_HOUR * 9),
            ],
            "; ",
            5,
        );

        assert_eq!(
            tallied
                .iter()
                .map(|genre| (genre.name.as_str(), genre.plays))
                .collect::<Vec<_>>(),
            [("Fusion", 3), ("Jazz", 3)]
        );
        assert_eq!(tallied[0].listened, AN_HOUR + A_MINUTE);
    }

    #[test]
    fn a_reading_across_every_year_counts_them_all_and_files_each_day_under_its_weekday() {
        const THE_FIRST_OF_JANUARY_1970_WAS_A_THURSDAY: usize = 3;
        const THE_FIRST_OF_JANUARY_2024_WAS_A_MONDAY: u64 = 19_723;

        let heard = [
            QuarterHeard {
                at: at(0, 0),
                listened: AN_HOUR,
            },
            QuarterHeard {
                at: at(THE_FIRST_OF_JANUARY_2024_WAS_A_MONDAY, 0),
                listened: A_MINUTE,
            },
        ];

        let rhythm = Rhythm::across(heard, &Calendar::utc());

        assert_eq!(rhythm.days_heard, 2);
        assert_eq!(
            rhythm.weekdays[THE_FIRST_OF_JANUARY_1970_WAS_A_THURSDAY],
            AN_HOUR
        );
        assert_eq!(rhythm.weekdays[0], A_MINUTE);
        assert_eq!(
            rhythm.busiest_weekday(),
            Some(THE_FIRST_OF_JANUARY_1970_WAS_A_THURSDAY)
        );
        assert_eq!(Rhythm::of(heard, &Calendar::utc(), 2024).days_heard, 1);
    }

    #[test]
    fn shares_of_the_sound_are_of_the_time_named_and_nothing_heard_has_none() {
        let flac = FormatHeard {
            lossless: true,
            above_cd: false,
            listened: AN_HOUR * 2,
        };
        let hires = FormatHeard {
            lossless: true,
            above_cd: true,
            listened: AN_HOUR,
        };
        let mp3 = FormatHeard {
            lossless: false,
            above_cd: true,
            listened: AN_HOUR,
        };

        let shares = FormatShares::of([flac, hires, mp3]).expect("time was heard");

        assert_eq!(shares.lossless_permille, 750);
        assert_eq!(
            shares.high_resolution_permille, 250,
            "a lossy file above CD's rate is no high resolution"
        );
        assert_eq!(FormatShares::of([]), None);
    }

    #[test]
    fn the_year_shown_is_last_years_until_december_unless_this_year_is_all_there_is() {
        let in_june = date(2026, 6, 1);
        let in_december = date(2026, 12, 1);

        assert_eq!(year_worth_showing(&[2025, 2026], in_june), Some(2025));
        assert_eq!(year_worth_showing(&[2025, 2026], in_december), Some(2026));
        assert_eq!(year_worth_showing(&[2026], in_june), Some(2026));
        assert_eq!(year_worth_showing(&[2024], in_june), Some(2024));
        assert_eq!(year_worth_showing(&[], in_june), None);
    }

    #[test]
    fn a_year_with_nothing_in_it_is_one_slide_saying_so() {
        let slides = YearReview {
            year: 2026,
            ..YearReview::default()
        }
        .slides();

        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].headline, "Nothing was played in 2026");
    }

    #[test]
    fn a_slide_is_left_out_where_the_year_has_nothing_to_say_on_it() {
        let review = YearReview {
            year: 2026,
            listened: AN_HOUR * 3,
            plays: 40,
            songs: 12,
            artists: 3,
            albums: 2,
            top_songs: vec![a_song("Echoes", "Pink Floyd", AN_HOUR)],
            ..YearReview::default()
        };

        let kinds: Vec<SlideKind> = review.slides().iter().map(|slide| slide.kind).collect();

        assert_eq!(
            kinds,
            [SlideKind::Whole, SlideKind::Songs, SlideKind::Farewell]
        );
    }

    #[test]
    fn songs_new_is_said_only_where_it_could_be_told_and_what_joined_the_library_stands_alone() {
        let told = YearReview {
            songs_new: Some(4),
            songs_added: 2,
            ..YearReview::default()
        };
        let untold = YearReview {
            songs_new: None,
            songs_added: 2,
            ..YearReview::default()
        };

        assert_eq!(
            told.discoveries().map(|slide| slide.headline),
            Some("4 songs heard for the first time".to_owned())
        );
        assert_eq!(
            untold.discoveries().map(|slide| slide.headline),
            Some("2 songs joined your library.".to_owned())
        );
    }

    #[test]
    fn a_chart_is_as_tall_a_share_as_each_month_is_of_the_busiest_and_marks_it() {
        let mut review = YearReview {
            year: 2026,
            listened: AN_HOUR * 3,
            ..YearReview::default()
        };
        review.rhythm.months[2] = AN_HOUR * 2;
        review.rhythm.months[3] = AN_HOUR;

        let slide = review.months().expect("a month was heard");

        assert_eq!(slide.headline, "March was your biggest month");
        assert_eq!(slide.bars.len(), MONTHS_IN_A_YEAR);
        assert_eq!(slide.bars[2].share_permille, 1_000);
        assert_eq!(slide.bars[3].share_permille, 500);
        assert!(slide.bars[2].peak);
        assert!(!slide.bars[3].peak);
        assert_eq!(slide.bars[2].said, "Mar: 2 hours");
    }

    #[test]
    fn the_summary_says_the_year_in_lines_a_message_can_carry() {
        let review = YearReview {
            year: 2026,
            listened: AN_HOUR * 312,
            plays: 900,
            songs: 411,
            artists: 90,
            albums: 120,
            top_songs: vec![a_song("Echoes", "Pink Floyd", AN_HOUR)],
            formats: Some(FormatShares {
                lossless_permille: 874,
                high_resolution_permille: 120,
            }),
            ..YearReview::default()
        };

        assert_eq!(
            review.summary(),
            "My 2026 on Resonate\n312 hours of music\n411 songs · 90 artists · 120 albums\nTop \
             song: Echoes by Pink Floyd\n87% lossless"
        );
    }
}
