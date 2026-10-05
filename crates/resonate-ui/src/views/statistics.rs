use std::{
    rc::Rc,
    time::{Duration, SystemTime},
};

use gpui::{
    AnyElement, App, Context, Div, SharedString, Stateful, div, prelude::*, px, relative, rgb,
};
use resonate_core::{AlbumId, ArtistId, Calendar, CivilDate};
use resonate_library::{Day, Listened, Lit, MostListened, Statistics, Window};

use crate::{
    Selection, format,
    icons::Icon,
    theme,
    views::{
        hint::Names,
        kit, listing,
        reorder::{self, Listed, Shift},
        root::{RootView, empty, row},
        scrollbar::Scrollbars,
        sorting,
    },
};

const NOTHING_PLAYED: &str = "Nothing has been played yet.";

const PLAY_SOMETHING: &str = "Play a track and this fills itself in.";

const NOTHING_IN_THIS_WINDOW: &str = "Nothing was played in this window.";

const A_WIDER_WINDOW: &str = "Choose a wider window to reach further back.";

const WINDOW_HINT: &str = "How far back every reading on this pane reaches";

const OPEN_ALBUM_HINT: &str = "Show this album";

const OPEN_ARTIST_HINT: &str = "Show this artist";

pub(crate) const BARS_AT_MOST: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Portrayed {
    ByInitial,
    No,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Kind {
    named: &'static str,
    id: &'static str,
    portrayed: Portrayed,
}

const MOST_LISTENED_TRACKS: Kind = Kind {
    named: "TRACKS MOST LISTENED TO",
    id: "most-listened-track",
    portrayed: Portrayed::No,
};

const MOST_LISTENED_ALBUMS: Kind = Kind {
    named: "ALBUMS MOST LISTENED TO",
    id: "most-listened-album",
    portrayed: Portrayed::No,
};

const MOST_LISTENED_ARTISTS: Kind = Kind {
    named: "ARTISTS MOST LISTENED TO",
    id: "most-listened-artist",
    portrayed: Portrayed::ByInitial,
};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bar {
    first: Dated,
    last: Dated,
    plays: u64,
    listened: Duration,
    share: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Chart {
    bars: Vec<Bar>,
    tallest: Duration,
}

impl Chart {
    pub(crate) fn of(days: &[Day], now: SystemTime, at_most: usize) -> Self {
        Self::in_calendar(days, now, &Calendar::local(), at_most)
    }

    fn in_calendar(days: &[Day], now: SystemTime, calendar: &Calendar, at_most: usize) -> Self {
        let today = calendar.day_of(now);
        let this_year = CivilDate::of_day(today).year;
        let dated = |at: SystemTime| {
            let day = calendar.day_of(at);
            Dated {
                ago: u64::try_from(today.saturating_sub(day)).unwrap_or(0),
                date: CivilDate::of_day(day),
                this_year,
            }
        };
        let mut bars: Vec<Bar> = Vec::new();

        for run in days.chunks(folded(days.len(), at_most)) {
            let (Some(first), Some(last)) = (run.first(), run.last()) else {
                continue;
            };
            bars.push(Bar {
                first: dated(first.at),
                last: dated(last.at),
                plays: run
                    .iter()
                    .fold(0, |held, day| held.saturating_add(day.plays)),
                listened: run.iter().fold(Duration::ZERO, |held, day| {
                    held.saturating_add(day.listened)
                }),
                share: 0.0,
            });
        }

        let tallest = bars
            .iter()
            .map(|bar| bar.listened)
            .max()
            .unwrap_or(Duration::ZERO);
        let ceiling = tallest.as_secs_f32();
        if ceiling > 0.0 {
            for bar in &mut bars {
                bar.share = (bar.listened.as_secs_f32() / ceiling).clamp(0.0, 1.0);
            }
        }

        Self { bars, tallest }
    }

    fn axis(&self) -> Option<(String, String)> {
        Some((spanned(*self.bars.first()?), spanned(*self.bars.last()?)))
    }
}

const fn folded(days: usize, at_most: usize) -> usize {
    if at_most == 0 || days <= at_most {
        return 1;
    }

    days.div_ceil(at_most)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Dated {
    ago: u64,
    date: CivilDate,
    this_year: i64,
}

impl Dated {
    fn named(self) -> String {
        match self.ago {
            0 => "today".to_owned(),
            1 => "yesterday".to_owned(),
            _ if self.date.year == self.this_year => {
                format!("{} {}", self.date.day, self.date.month_named())
            }
            _ => format!(
                "{} {} {}",
                self.date.day,
                self.date.month_named(),
                self.date.year
            ),
        }
    }
}

fn spanned(bar: Bar) -> String {
    if bar.first.ago == bar.last.ago {
        return bar.first.named();
    }

    format!("{} to {}", bar.first.named(), bar.last.named())
}

fn said(bar: Bar) -> String {
    if bar.plays == 0 {
        return format!("{} · nothing played", spanned(bar));
    }

    format!(
        "{} · {} · {} listened",
        spanned(bar),
        format::counted(bar.plays as usize, "play", "plays"),
        format::heard_for(bar.listened)
    )
}

const fn spelled(window: Window) -> &'static str {
    match window {
        Window::Week => "Last week",
        Window::Month => "Last month",
        Window::Year => "Last year",
        Window::Everything => "All time",
    }
}

const fn across(window: Window) -> &'static str {
    match window {
        Window::Week => "over the last week",
        Window::Month => "over the last month",
        Window::Year => "over the last year",
        Window::Everything => "all told",
    }
}

impl RootView {
    pub(crate) fn statistics_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let window = library.window();
        let counts = library.statistics();
        let listened = library.most_listened();
        let chart = library.charted();
        let nothing = counts.plays == 0;

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(self.statistics_heading(window, counts, cx))
            .when(nothing, |pane| {
                pane.child(match window {
                    Window::Everything => {
                        empty(Icon::Statistics, NOTHING_PLAYED, Some(PLAY_SOMETHING))
                    }
                    Window::Week | Window::Month | Window::Year => empty(
                        Icon::Statistics,
                        NOTHING_IN_THIS_WINDOW,
                        Some(A_WIDER_WINDOW),
                    ),
                })
            })
            .when(!nothing, |pane| {
                pane.child(Scrollbars::of(cx).around(
                    "statistics-scrollbar",
                    self.statistics_scroll.clone(),
                    self.what_was_heard(counts, &listened, &chart, cx),
                ))
            })
            .into_any_element()
    }

    fn statistics_heading(
        &self,
        window: Window,
        counts: Statistics,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut chosen = sorting::shape("Window");
        for (index, offered) in Window::ALL.into_iter().enumerate() {
            chosen = chosen.child(
                kit::chip(
                    ("statistics-window", index),
                    SharedString::new_static(spelled(offered)),
                    offered == window,
                )
                .names(WINDOW_HINT)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.let_go_of_the_reach_in(Shift::Listing(Listed::Heard));
                    this.library
                        .update(cx, |library, cx| library.read_over(offered, cx));
                })),
            );
        }

        kit::heading()
            .child(
                kit::heading_row().child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(theme::heading_name()))
                        .gap_1()
                        .child(kit::eyebrow("LIBRARY"))
                        .child(kit::title("Statistics"))
                        .child(kit::subtitle(format!(
                            "{} {}",
                            format::counted(counts.plays as usize, "play", "plays"),
                            across(window)
                        ))),
                ),
            )
            .child(chosen)
    }

    fn what_was_heard(
        &self,
        counts: Statistics,
        listened: &MostListened,
        chart: &Chart,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("statistics")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_5()
            .px_6()
            .py_5()
            .overflow_y_scroll()
            .track_scroll(&self.statistics_scroll)
            .child(tiles(counts))
            .child(charted(chart))
            .child(self.most_listened_to(MOST_LISTENED_TRACKS, &listened.tracks, 0, |_| None, cx))
            .child(self.most_listened_to(
                MOST_LISTENED_ALBUMS,
                &listened.albums,
                listened.tracks.len(),
                |album: AlbumId| Some((Selection::Album(album), OPEN_ALBUM_HINT)),
                cx,
            ))
            .child(self.most_listened_to(
                MOST_LISTENED_ARTISTS,
                &listened.artists,
                listened.tracks.len() + listened.albums.len(),
                |artist: ArtistId| Some((Selection::Artist(artist), OPEN_ARTIST_HINT)),
                cx,
            ))
    }

    fn most_listened_to<Id: Copy>(
        &self,
        kind: Kind,
        rows: &[Listened<Id>],
        reached_from: usize,
        opens: impl Fn(Id) -> Option<(Selection, &'static str)>,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut listed = div().flex().flex_col().py_1();
        for (rank, held) in rows.iter().enumerate() {
            let reached = self.reaches(Shift::Listing(Listed::Heard), reached_from + rank);
            let drawn = self.listened_row(kind, rank, held, opens(held.id), cx);
            listed = listed.child(reorder::marked(drawn, reached).when(reached, |row| {
                row.child(kit::brought_into_view(
                    self.statistics_scroll.clone(),
                    Rc::clone(&self.reached_unseen),
                ))
            }));
        }

        kit::section()
            .child(kit::section_header().child(kit::section_name(kind.named)))
            .child(listed)
    }

    pub(crate) fn heard_rows(&self, cx: &App) -> usize {
        let library = self.library.read(cx);
        if library.statistics().plays == 0 {
            return 0;
        }
        let listened = library.most_listened();

        listened.tracks.len() + listened.albums.len() + listened.artists.len()
    }

    pub(crate) fn heard_a_page(&self) -> usize {
        let seen = f32::from(self.statistics_scroll.bounds().size.height);

        ((seen / theme::row_height()) as usize).max(1)
    }

    pub(crate) fn open_what_was_heard_at(&mut self, row: usize, cx: &mut Context<Self>) {
        let listened = self.library.read(cx).most_listened();
        let tracks = listened.tracks.len();
        let albums = tracks + listened.albums.len();
        if let Some(heard) = listened.tracks.get(row) {
            let ids: Vec<_> = listened.tracks.iter().map(|held| held.id).collect();
            let found = self.library.read(cx).held_tracks(&ids);
            let Some(start) = found.iter().position(|track| track.id == heard.id) else {
                return;
            };
            self.play(&found, start, cx);
            return;
        }
        let opened = match row.checked_sub(albums) {
            Some(at) => listened
                .artists
                .get(at)
                .map(|held| Selection::Artist(held.id)),
            None => listened
                .albums
                .get(row - tracks)
                .map(|held| Selection::Album(held.id)),
        };
        if let Some(opened) = opened {
            self.opened(opened, cx);
        }
    }

    fn listened_row<Id>(
        &self,
        kind: Kind,
        rank: usize,
        held: &Listened<Id>,
        opens: Option<(Selection, &'static str)>,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = SharedString::from(held.name.clone());
        let named = match opens {
            Some((selection, saying)) => div()
                .flex()
                .flex_1()
                .min_w(px(0.0))
                .child(self.opens((kind.id, rank), name, saying, Some(selection), cx))
                .into_any_element(),
            None => listing::title_cell(name, Lit::new(), false).into_any_element(),
        };

        row(false)
            .child(listing::number_cell(SharedString::from(
                (rank + 1).to_string(),
            )))
            .when(kind.portrayed == Portrayed::ByInitial, |row| {
                row.child(
                    kit::avatar(&held.name, false)
                        .size(px(theme::row_cover()))
                        .text_size(px(theme::text_xs())),
                )
            })
            .child(named)
            .child(listing::heard(
                u32::try_from(held.plays).unwrap_or(u32::MAX),
                None,
                self.drawn_at(),
            ))
            .child(
                kit::figure(format::heard_for(held.listened))
                    .w(px(theme::row_plays()))
                    .flex_none()
                    .text_right(),
            )
    }
}

fn tiles(counts: Statistics) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_3()
        .child(tile("PLAYS", counts.plays.to_string()))
        .child(tile("TIME LISTENED", format::heard_for(counts.listened)))
        .child(tile("TRACKS HEARD", counts.tracks.to_string()))
        .child(tile("ALBUMS HEARD", counts.albums.to_string()))
        .child(tile("ARTISTS HEARD", counts.artists.to_string()))
}

fn tile(named: &'static str, reading: String) -> Div {
    kit::card()
        .flex_1()
        .gap_1()
        .min_w(px(theme::stat_tile()))
        .child(kit::eyebrow(named))
        .child(
            kit::figure(reading)
                .text_size(px(theme::text_title()))
                .text_color(rgb(theme::text())),
        )
}

fn charted(chart: &Chart) -> Div {
    let mut bars = div()
        .flex()
        .w_full()
        .flex_none()
        .items_end()
        .h(px(theme::chart_height()))
        .gap(px(theme::CHART_BAR_GAP));
    for (index, bar) in chart.bars.iter().copied().enumerate() {
        bars = bars.child(drawn_bar(index, bar));
    }

    let axis = chart.axis().map(|(from, to)| {
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(kit::figure(from))
            .child(kit::figure(to))
    });

    kit::section()
        .child(
            kit::section_header()
                .child(kit::section_name("LISTENING BY DAY"))
                .child(kit::figure(format!(
                    "{} at the peak",
                    format::heard_for(chart.tallest)
                ))),
        )
        .child(kit::section_body().gap_2().child(bars).children(axis))
}

fn drawn_bar(index: usize, bar: Bar) -> Stateful<Div> {
    let colour = if bar.plays == 0 {
        theme::outline()
    } else {
        theme::accent()
    };

    div()
        .id(("listening-day", index))
        .flex()
        .flex_1()
        .flex_col()
        .justify_end()
        .h_full()
        .min_w(px(theme::CHART_BAR))
        .rounded_sm()
        .hover(|column| column.bg(theme::tinted(theme::accent(), 0x1f)))
        .names_when_raised(move || SharedString::from(said(bar)))
        .child(
            div()
                .w_full()
                .h(relative(bar.share))
                .min_h(px(theme::CHART_BASELINE))
                .rounded_sm()
                .bg(rgb(colour)),
        )
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    const SECONDS_A_DAY: u64 = 24 * 60 * 60;
    const TODAY: u64 = 20_000;

    fn a_day(days_ago: u64, plays: u64, listened: Duration) -> Day {
        Day {
            at: UNIX_EPOCH + Duration::from_secs((TODAY - days_ago) * SECONDS_A_DAY),
            plays,
            listened,
        }
    }

    fn dated_ago(ago: u64) -> Dated {
        Dated {
            ago,
            date: CivilDate::of_day((TODAY - ago) as i64),
            this_year: 2024,
        }
    }

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(TODAY * SECONDS_A_DAY + 3_600)
    }

    #[test]
    fn a_bar_is_as_tall_a_share_of_the_chart_as_its_day_is_of_the_tallest() {
        let days = [
            a_day(2, 4, Duration::from_secs(600)),
            a_day(1, 20, Duration::from_secs(2_400)),
            a_day(0, 10, Duration::from_secs(1_200)),
        ];
        let chart = Chart::in_calendar(&days, now(), &Calendar::utc(), BARS_AT_MOST);

        assert_eq!(chart.tallest, Duration::from_secs(2_400));
        assert_eq!(
            chart
                .bars
                .iter()
                .map(|drawn| drawn.share)
                .collect::<Vec<f32>>(),
            vec![0.25, 1.0, 0.5]
        );
    }

    #[test]
    fn a_day_is_dated_in_the_listeners_calendar_rather_than_greenwichs() {
        const TWO_HOURS_EAST: i32 = 2 * 60 * 60;

        let east = Calendar::fixed(TWO_HOURS_EAST).expect("a fixed offset is a zone");
        let today = east.day_of(now());
        let days = [
            Day {
                at: east.midnight_of(today - 1),
                plays: 1,
                listened: Duration::from_secs(60),
            },
            Day {
                at: east.midnight_of(today),
                plays: 1,
                listened: Duration::from_secs(60),
            },
        ];
        let chart = Chart::in_calendar(&days, now(), &east, BARS_AT_MOST);

        assert_eq!(
            chart
                .bars
                .iter()
                .map(|drawn| drawn.first.ago)
                .collect::<Vec<u64>>(),
            vec![1, 0]
        );
    }

    #[test]
    fn a_window_in_which_nothing_was_played_draws_every_bar_at_its_baseline() {
        let days = [a_day(1, 0, Duration::ZERO), a_day(0, 0, Duration::ZERO)];
        let chart = Chart::in_calendar(&days, now(), &Calendar::utc(), BARS_AT_MOST);

        assert_eq!(chart.tallest, Duration::ZERO);
        assert!(chart.bars.iter().all(|drawn| drawn.share == 0.0));
    }

    #[test]
    fn a_chart_of_no_days_at_all_has_no_bars_and_no_axis() {
        let chart = Chart::in_calendar(&[], now(), &Calendar::utc(), BARS_AT_MOST);

        assert!(chart.bars.is_empty());
        assert_eq!(chart.tallest, Duration::ZERO);
        assert_eq!(chart.axis(), None);
    }

    #[test]
    fn one_day_is_one_bar_reading_today_at_both_ends_of_the_axis() {
        let days = [a_day(0, 3, Duration::from_secs(900))];
        let chart = Chart::in_calendar(&days, now(), &Calendar::utc(), BARS_AT_MOST);

        assert_eq!(chart.bars.len(), 1);
        assert_eq!(chart.bars[0].share, 1.0);
        assert_eq!(chart.axis(), Some(("today".to_owned(), "today".to_owned())));
    }

    #[test]
    fn a_window_longer_than_the_chart_folds_whole_days_into_each_bar() {
        let days: Vec<Day> = (0..250)
            .map(|index| a_day(249 - index, 1, Duration::from_secs(60)))
            .collect();
        let chart = Chart::in_calendar(&days, now(), &Calendar::utc(), 120);

        assert_eq!(folded(250, 120), 3);
        assert_eq!(chart.bars.len(), 84);
        assert_eq!(chart.bars[0].plays, 3);
        assert_eq!(chart.bars[0].listened, Duration::from_secs(180));
        assert_eq!(chart.bars[0].first.ago, 249);
        assert_eq!(chart.bars[0].last.ago, 247);
        assert_eq!(spanned(chart.bars[0]), "29 Jan to 31 Jan".to_owned());
        assert_eq!(
            Dated {
                ago: 300,
                date: CivilDate::of_day(TODAY as i64 - 300),
                this_year: 2024,
            }
            .named(),
            "9 Dec 2023"
        );
    }

    #[test]
    fn a_bar_says_how_long_ago_it_was_and_what_was_played_in_it() {
        assert_eq!(
            said(Bar {
                first: dated_ago(1),
                last: dated_ago(1),
                plays: 1,
                listened: Duration::from_secs(3_600),
                share: 1.0,
            }),
            "yesterday · 1 play · 1h 0m listened"
        );
        assert_eq!(
            said(Bar {
                first: dated_ago(0),
                last: dated_ago(0),
                plays: 0,
                listened: Duration::ZERO,
                share: 0.0,
            }),
            "today · nothing played"
        );
    }

    #[test]
    fn no_two_windows_are_named_or_read_alike() {
        let mut named: Vec<&str> = Window::ALL.into_iter().map(spelled).collect();
        let held = named.len();
        named.sort_unstable();
        named.dedup();

        assert_eq!(held, named.len(), "two windows answer to one name");

        let mut read: Vec<&str> = Window::ALL.into_iter().map(across).collect();
        read.sort_unstable();
        read.dedup();

        assert_eq!(held, read.len(), "two windows read the same way");
    }
}
