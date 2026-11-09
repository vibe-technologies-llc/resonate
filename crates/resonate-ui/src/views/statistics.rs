use std::{
    rc::Rc,
    time::{Duration, SystemTime},
};

use gpui::{
    AnyElement, App, Background, Context, Div, FontWeight, Role, SharedString, Stateful, div,
    linear_color_stop, linear_gradient, prelude::*, px, relative, rgb,
};
use resonate_core::{
    Accent, AlbumId, ArtistId, Calendar, CivilDate,
    wrapped::{DAYS_IN_A_WEEK, FormatShares, Ranked, Rhythm},
};
use resonate_library::{Day, Habits, Listened, MostListened, Statistics, Window};

use crate::{
    Selection, format, icons,
    icons::Icon,
    models::Portrayed,
    theme,
    views::{
        browser::portrait_frame,
        hint::Names,
        kit::{self, EndsInAnEllipsis as _},
        listing::Pictured,
        reorder::{self, Listed, Shift},
        root::{RootView, empty, framed_cover, tall_row},
        scrollbar::Scrollbars,
        sorting,
        spoken::{Placed, Spoken as _},
    },
    words,
};

const HOURS_ON_THE_AXIS: [&str; 5] = ["00:00", "06:00", "12:00", "18:00", "24:00"];

type Said = fn() -> &'static str;

const WEEKDAYS: [(Said, Said); DAYS_IN_A_WEEK] = [
    (words::statistics::mon, words::statistics::monday),
    (words::statistics::tue, words::statistics::tuesday),
    (words::statistics::wed, words::statistics::wednesday),
    (words::statistics::thu, words::statistics::thursday),
    (words::statistics::fri, words::statistics::friday),
    (words::statistics::sat, words::statistics::saturday),
    (words::statistics::sun, words::statistics::sunday),
];

const WHOLE_SHARE: f32 = 1_000.0;

const WASH_OF_A_ROW: u8 = 0x1c;

const WASH_OF_A_RAISED_BAR: u8 = 0x1f;

const WHOLE_STRENGTH: u8 = 0xff;

const AVATAR_LETTER_TO_SIDE: f32 = 0.42;

const A_WASH_STANDS_OFF_ITS_ROW_BY: f32 = 2.0;

const WASH_OF_A_MARK: u8 = 0x24;

const WASH_OF_THE_HEADLINE_FROM: u8 = 0x38;

const WASH_OF_THE_HEADLINE_TO: u8 = 0x08;

const BODY_OF_A_BAR: u8 = 0x8c;

const GUIDE_LINE: u8 = 0x30;

const AVERAGE_LINE: u8 = 0xa0;

pub(crate) const BARS_AT_MOST: usize = 120;

#[derive(Clone, Copy)]
struct Kind {
    named: Said,
    id: &'static str,
}

const MOST_LISTENED_TRACKS: Kind = Kind {
    named: words::statistics::most_listened_tracks,
    id: "most-listened-track",
};

const MOST_LISTENED_ALBUMS: Kind = Kind {
    named: words::statistics::most_listened_albums,
    id: "most-listened-album",
};

const MOST_LISTENED_ARTISTS: Kind = Kind {
    named: words::statistics::most_listened_artists,
    id: "most-listened-artist",
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
    average: Duration,
    average_share: f32,
    days_a_bar: usize,
    this_year: i64,
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
        let days_a_bar = folded(days.len(), at_most);

        for run in days.chunks(days_a_bar) {
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
        let average = u32::try_from(bars.len())
            .ok()
            .filter(|drawn| *drawn > 0)
            .map_or(Duration::ZERO, |drawn| {
                bars.iter().fold(Duration::ZERO, |held, bar| {
                    held.saturating_add(bar.listened)
                }) / drawn
            });
        let average_share = match ceiling > 0.0 {
            true => (average.as_secs_f32() / ceiling).clamp(0.0, 1.0),
            false => 0.0,
        };

        Self {
            bars,
            tallest,
            average,
            average_share,
            days_a_bar,
            this_year,
        }
    }

    fn says(&self) -> String {
        let readings = words::statistics::chart_says(
            format::heard_for(self.average),
            format::heard_for(self.tallest),
        );

        match self.days_a_bar {
            0 | 1 => readings,
            days => words::statistics::chart_says_folded(days, readings),
        }
    }

    fn is_the_peak(&self, bar: Bar) -> bool {
        bar.plays > 0 && bar.listened == self.tallest
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
            0 => words::statistics::today().to_owned(),
            1 => words::statistics::yesterday().to_owned(),
            _ => dated(self.date, self.this_year),
        }
    }
}

fn spanned(bar: Bar) -> String {
    if bar.first.ago == bar.last.ago {
        return bar.first.named();
    }

    words::statistics::span(bar.first.named(), bar.last.named())
}

fn said(bar: Bar) -> String {
    if bar.plays == 0 {
        return words::statistics::bar_unplayed(spanned(bar));
    }

    words::statistics::bar_played(bar.plays, format::heard_for(bar.listened), spanned(bar))
}

fn dated(date: CivilDate, this_year: i64) -> String {
    match date.year == this_year {
        true => words::statistics::date_this_year(date.day, date.month_named()),
        false => words::statistics::date(date.day, date.month_named(), date.year),
    }
}

fn the_pace_of(listened: Duration, days_heard: u32, days_spanned: usize) -> String {
    let Some(a_day) = u32::try_from(days_spanned)
        .ok()
        .filter(|days| *days > 0)
        .map(|days| listened / days)
    else {
        return String::new();
    };

    words::statistics::pace(format::heard_for(a_day), days_heard, days_spanned)
}

fn shares_of(spans: &[Duration]) -> Vec<f32> {
    let ceiling = spans
        .iter()
        .max()
        .copied()
        .unwrap_or(Duration::ZERO)
        .as_secs_f32();

    spans
        .iter()
        .map(|span| match ceiling > 0.0 {
            true => (span.as_secs_f32() / ceiling).clamp(0.0, 1.0),
            false => 0.0,
        })
        .collect()
}

fn hour_named(hour: usize) -> String {
    format!("{hour:02}:00")
}

fn hour_said(hour: usize, listened: Duration) -> String {
    let hours = hour_named(hour);

    match listened.is_zero() {
        true => words::statistics::nothing_heard(hours),
        false => words::statistics::heard(format::heard_for(listened), hours),
    }
}

fn weekday_said(named: &str, listened: Duration) -> String {
    match listened.is_zero() {
        true => words::statistics::nothing_heard(named),
        false => words::statistics::heard(format::heard_for(listened), named),
    }
}

fn by_line(by: Option<&str>, plays: u64) -> String {
    match by {
        Some(by) => words::statistics::by_line(by, plays),
        None => words::statistics::plays(plays),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SoundShares {
    high_resolution: f32,
    cd_quality: f32,
    lossy: f32,
}

impl SoundShares {
    fn of(shares: FormatShares) -> Self {
        let high = f32::from(shares.high_resolution_permille);
        let lossless = f32::from(shares.lossless_permille);

        Self {
            high_resolution: high / WHOLE_SHARE,
            cd_quality: (lossless - high).max(0.0) / WHOLE_SHARE,
            lossy: (WHOLE_SHARE - lossless).max(0.0) / WHOLE_SHARE,
        }
    }

    fn parts(self) -> [(&'static str, f32, u32); 3] {
        [
            (
                words::statistics::high_resolution(),
                self.high_resolution,
                theme::accent(),
            ),
            (
                words::statistics::cd_quality(),
                self.cd_quality,
                theme::lossless(),
            ),
            (words::statistics::lossy(), self.lossy, theme::lossy()),
        ]
    }
}

fn percent(share: f32) -> String {
    format!("{:.0}%", share * 100.0)
}

fn spelled(window: Window) -> &'static str {
    match window {
        Window::Week => words::statistics::last_week(),
        Window::Month => words::statistics::last_month(),
        Window::Year => words::statistics::last_year(),
        Window::Everything => words::statistics::all_time(),
    }
}

fn across(window: Window) -> &'static str {
    match window {
        Window::Week => words::statistics::over_the_last_week(),
        Window::Month => words::statistics::over_the_last_month(),
        Window::Year => words::statistics::over_the_last_year(),
        Window::Everything => words::statistics::all_told(),
    }
}

impl RootView {
    pub(crate) fn statistics_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let window = library.window();
        let counts = library.statistics();
        let listened = library.most_listened();
        let chart = library.charted();
        let habits = library.habits();
        let days_spanned = library.listening_by_day().len();
        let nothing = counts.plays == 0;
        let heard = Heard {
            counts,
            listened: &listened,
            chart: &chart,
            habits: &habits,
            days_spanned,
        };

        kit::pane()
            .child(self.statistics_heading(window, counts, cx))
            .when(nothing, |pane| {
                pane.child(match window {
                    Window::Everything => empty(
                        Icon::Statistics,
                        words::statistics::nothing_played(),
                        Some(words::statistics::play_something()),
                    ),
                    Window::Week | Window::Month | Window::Year => empty(
                        Icon::Statistics,
                        words::statistics::nothing_in_this_window(),
                        Some(words::statistics::a_wider_window()),
                    ),
                })
            })
            .when(!nothing, |pane| {
                pane.child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .child(kit::measures_the_grid(self.grid_width.clone()))
                        .child(Scrollbars::of(cx).around(
                            "statistics-scrollbar",
                            self.statistics_scroll.clone(),
                            self.what_was_heard(&heard, cx),
                        )),
                )
            })
            .into_any_element()
    }

    fn statistics_heading(
        &self,
        window: Window,
        counts: Statistics,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut windows = kit::segmented();
        for (index, offered) in Window::ALL.into_iter().enumerate() {
            windows = windows.child(
                self.in_the_pane_ring(
                    kit::segment(
                        ("statistics-window", index),
                        SharedString::new_static(spelled(offered)),
                        offered == window,
                    )
                    .names(words::statistics::window_hint()),
                    move |this, _, cx| {
                        this.let_go_of_the_reach_in(Shift::Listing(Listed::Heard));
                        this.library
                            .update(cx, |library, cx| library.read_over(offered, cx));
                    },
                    cx,
                ),
            );
        }
        let chosen = sorting::shape(words::statistics::window()).child(windows);

        kit::heading()
            .child(
                kit::heading_row()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(theme::heading_name()))
                            .gap_1()
                            .child(kit::title(words::statistics::statistics()))
                            .child(kit::subtitle(words::statistics::plays_across(
                                across(window),
                                counts.plays,
                            ))),
                    )
                    .child(kit::actions().child(self.in_the_pane_ring(
                        kit::button(
                            "open-wrapped",
                            Some(Icon::Statistics),
                            words::statistics::open_wrapped(),
                            words::statistics::open_wrapped_hint(),
                            kit::Tone::Outlined,
                        ),
                        |this, _, cx| this.open_wrapped(cx),
                        cx,
                    ))),
            )
            .child(chosen)
    }

    fn what_was_heard(&mut self, heard: &Heard<'_>, cx: &mut Context<Self>) -> Stateful<Div> {
        let wide = f32::from(self.grid_width.get()) >= theme::statistics_two_columns();
        let Heard {
            counts,
            listened,
            chart,
            habits,
            days_spanned,
        } = *heard;
        let tracks = listened.tracks.len();
        let albums = listened.albums.len();

        div()
            .id("statistics")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_4()
            .p_6()
            .overflow_y_scroll()
            .track_scroll(&self.statistics_scroll)
            .child(side_by_side(
                wide,
                the_headline(counts, habits, days_spanned),
                counted_tiles(counts),
            ))
            .children(self.number_ones(wide, listened, cx))
            .child(charted(chart))
            .children(moments(&habits.rhythm, chart.this_year))
            .child(side_by_side(
                wide,
                hours_heard(&habits.rhythm),
                weekdays_heard(&habits.rhythm),
            ))
            .children(the_sound_heard(wide, habits))
            .child(self.most_listened_to(
                MOST_LISTENED_TRACKS,
                &listened.tracks,
                0,
                |_| None,
                |this, held, cx| this.pictured_cover(held.album, cx),
                cx,
            ))
            .child(side_by_side(
                wide,
                self.most_listened_to(
                    MOST_LISTENED_ALBUMS,
                    &listened.albums,
                    tracks,
                    |album: AlbumId| {
                        Some((
                            Selection::Album(album),
                            words::statistics::open_album_hint(),
                        ))
                    },
                    |this, held, cx| this.pictured_cover(held.album, cx),
                    cx,
                )
                .flex_1(),
                self.most_listened_to(
                    MOST_LISTENED_ARTISTS,
                    &listened.artists,
                    tracks + albums,
                    |artist: ArtistId| {
                        Some((
                            Selection::Artist(artist),
                            words::statistics::open_artist_hint(),
                        ))
                    },
                    |this, held, cx| {
                        this.pictured_face(held, Portrayed::InARow, theme::row_cover(), cx)
                    },
                    cx,
                )
                .flex_1(),
            ))
    }

    fn pictured_cover(&self, album: Option<AlbumId>, cx: &mut Context<Self>) -> AnyElement {
        match album {
            Some(album) => self.cover(Pictured::Album(album), cx).into_any_element(),
            None => framed_cover(None, theme::row_cover()).into_any_element(),
        }
    }

    fn pictured_face(
        &self,
        held: &Listened<ArtistId>,
        portrayed: Portrayed,
        side: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let portrait = held
            .has_portrait
            .then(|| {
                self.library
                    .update(cx, |library, cx| library.portrait(held.id, portrayed, cx))
            })
            .flatten();

        match portrait {
            Some(art) => portrait_frame(art, side).into_any_element(),
            None => kit::avatar(&held.name, false)
                .size(px(side))
                .text_size(px(side * AVATAR_LETTER_TO_SIDE))
                .into_any_element(),
        }
    }

    fn number_ones(
        &self,
        wide: bool,
        listened: &MostListened,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let side = theme::number_one_art();
        let cover = |this: &Self, album: Option<AlbumId>, cx: &mut Context<Self>| match album {
            Some(album) => this
                .cover_sized(Pictured::Album(album), crate::Drawn::InAGrid, side, cx)
                .into_any_element(),
            None => framed_cover(None, side).into_any_element(),
        };
        let mut drawn: Vec<Stateful<Div>> = Vec::new();
        if let Some(track) = listened.tracks.first() {
            drawn.push(number_one(
                "top-track",
                words::statistics::top_track(),
                cover(self, track.album, cx),
                &track.name,
                track.by.as_deref(),
                track.plays,
                track.listened,
            ));
        }
        if let Some(album) = listened.albums.first() {
            drawn.push(number_one(
                "top-album",
                words::statistics::top_album(),
                cover(self, album.album, cx),
                &album.name,
                album.by.as_deref(),
                album.plays,
                album.listened,
            ));
        }
        if let Some(artist) = listened.artists.first() {
            drawn.push(number_one(
                "top-artist",
                words::statistics::top_artist(),
                self.pictured_face(artist, Portrayed::InAGrid, side, cx),
                &artist.name,
                None,
                artist.plays,
                artist.listened,
            ));
        }
        if drawn.is_empty() {
            return None;
        }

        Some(
            div()
                .flex()
                .w_full()
                .gap_3()
                .when(!wide, |row| row.flex_col())
                .children(
                    drawn
                        .into_iter()
                        .map(|one| one.when(wide, |one| one.flex_1())),
                ),
        )
    }

    fn most_listened_to<Id: Copy>(
        &self,
        kind: Kind,
        rows: &[Listened<Id>],
        reached_from: usize,
        opens: impl Fn(Id) -> Option<(Selection, &'static str)>,
        pictured: impl Fn(&Self, &Listened<Id>, &mut Context<Self>) -> AnyElement,
        cx: &mut Context<Self>,
    ) -> Div {
        let shares = shares_of(&rows.iter().map(|held| held.listened).collect::<Vec<_>>());
        let mut listed = div().flex().flex_col().p_1();
        for (rank, held) in rows.iter().enumerate() {
            let reached = self.reaches(Shift::Listing(Listed::Heard), reached_from + rank);
            let art = pictured(self, held, cx);
            let placed = Placing {
                rank,
                share: shares.get(rank).copied().unwrap_or(0.0),
                art,
            };
            let drawn = self.listened_row(kind, placed, held, opens(held.id), cx);
            listed = listed.child(reorder::marked(drawn, reached).when(reached, |row| {
                row.child(kit::brought_into_view(
                    self.statistics_scroll.clone(),
                    Rc::clone(&self.reached_unseen),
                ))
            }));
        }

        kit::section()
            .child(kit::section_header().child(kit::section_name((kind.named)())))
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

        ((seen / theme::tall_row_height()) as usize).max(1)
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
        placed: Placing,
        held: &Listened<Id>,
        opens: Option<(Selection, &'static str)>,
        cx: &mut Context<Self>,
    ) -> Div {
        let Placing { rank, share, art } = placed;
        let name = SharedString::from(held.name.clone());
        let named = match opens {
            Some((selection, saying)) => div()
                .flex()
                .min_w(px(0.0))
                .text_color(rgb(theme::text()))
                .child(self.opens((kind.id, rank), name, saying, Some(selection), cx))
                .into_any_element(),
            None => a_name(name).into_any_element(),
        };

        tall_row(false)
            .relative()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(A_WASH_STANDS_OFF_ITS_ROW_BY))
                    .bottom(px(A_WASH_STANDS_OFF_ITS_ROW_BY))
                    .w(relative(share))
                    .rounded_md()
                    .bg(theme::tinted(theme::accent(), WASH_OF_A_ROW)),
            )
            .child(
                kit::figure(SharedString::from((rank + 1).to_string()))
                    .w(px(theme::row_number()))
                    .flex_none()
                    .text_color(rgb(theme::faint())),
            )
            .child(art)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(named)
                    .child(a_faint_line(by_line(held.by.as_deref(), held.plays))),
            )
            .child(
                kit::figure(format::heard_for(held.listened))
                    .flex_none()
                    .text_right(),
            )
    }
}

struct Placing {
    rank: usize,
    share: f32,
    art: AnyElement,
}

#[derive(Clone, Copy)]
struct Heard<'a> {
    counts: Statistics,
    listened: &'a MostListened,
    chart: &'a Chart,
    habits: &'a Habits,
    days_spanned: usize,
}

fn a_name(name: SharedString) -> Div {
    div()
        .min_w(px(0.0))
        .text_color(rgb(theme::text()))
        .truncate()
        .ends_in_an_ellipsis()
        .child(name)
}

fn a_faint_line(text: String) -> Div {
    kit::figure(text)
        .text_color(rgb(theme::faint()))
        .truncate()
        .ends_in_an_ellipsis()
}

fn side_by_side(wide: bool, one: impl IntoElement, other: impl IntoElement) -> Div {
    let holder = |element: AnyElement| {
        div()
            .flex()
            .flex_col()
            .min_w(px(0.0))
            .when(wide, |holder| holder.flex_1())
            .when(!wide, |holder| holder.w_full())
            .child(element)
    };

    div()
        .flex()
        .w_full()
        .gap_3()
        .when(!wide, |row| row.flex_col())
        .child(holder(one.into_any_element()))
        .child(holder(other.into_any_element()))
}

fn the_headline_ground() -> Background {
    linear_gradient(
        135.0,
        linear_color_stop(
            theme::tinted(theme::accent(), WASH_OF_THE_HEADLINE_FROM),
            0.0,
        ),
        linear_color_stop(theme::tinted(theme::accent(), WASH_OF_THE_HEADLINE_TO), 1.0),
    )
}

fn the_headline(counts: Statistics, habits: &Habits, days_spanned: usize) -> Stateful<Div> {
    let reading = format::heard_for(counts.listened);
    let pace = the_pace_of(counts.listened, habits.rhythm.days_heard, days_spanned);

    kit::card()
        .id("statistics-headline")
        .read_plainly(&words::statistics::time_listened_said(&pace, &reading))
        .flex_1()
        .justify_center()
        .gap_1()
        .bg(the_headline_ground())
        .child(kit::eyebrow(words::statistics::time_listened()))
        .child(
            kit::figure(reading)
                .font(theme::ui(FontWeight::SEMIBOLD))
                .text_size(px(theme::text_lyric()))
                .text_color(rgb(theme::text())),
        )
        .child(kit::figure(pace).text_color(rgb(theme::muted())))
}

fn counted_tiles(counts: Statistics) -> Div {
    let a_pair = |one: Stateful<Div>, other: Stateful<Div>| {
        div()
            .flex()
            .flex_1()
            .gap_3()
            .child(one.flex_1().min_w(px(0.0)))
            .child(other.flex_1().min_w(px(0.0)))
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .gap_3()
        .child(a_pair(
            tile(
                "plays",
                words::statistics::plays_tile(),
                Icon::Play,
                counts.plays.to_string(),
            ),
            tile(
                "tracks-heard",
                words::statistics::tracks_heard(),
                Icon::Tracks,
                counts.tracks.to_string(),
            ),
        ))
        .child(a_pair(
            tile(
                "albums-heard",
                words::statistics::albums_heard(),
                Icon::Albums,
                counts.albums.to_string(),
            ),
            tile(
                "artists-heard",
                words::statistics::artists_heard(),
                Icon::Artists,
                counts.artists.to_string(),
            ),
        ))
}

fn tile(id: &'static str, named: &'static str, mark: Icon, reading: String) -> Stateful<Div> {
    kit::card()
        .id(id)
        .read_plainly(&format!("{named}: {reading}"))
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(theme::stat_mark()))
                .rounded_lg()
                .bg(theme::tinted(theme::accent(), WASH_OF_A_MARK))
                .child(icons::icon(mark, theme::stat_mark_icon(), theme::accent())),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.0))
                .gap_0p5()
                .child(kit::eyebrow(named))
                .child(
                    kit::figure(reading)
                        .text_size(px(theme::text_lg()))
                        .text_color(rgb(theme::text())),
                ),
        )
}

fn moments(rhythm: &Rhythm, this_year: i64) -> Option<Div> {
    let mut drawn: Vec<Stateful<Div>> = Vec::new();
    if let Some(busiest) = rhythm.busiest_day {
        drawn.push(moment(
            "busiest-day",
            words::statistics::busiest_day(),
            dated(busiest.date, this_year),
            words::statistics::listened(format::heard_for(busiest.listened)),
        ));
    }
    if let Some(streak) = rhythm.longest_streak {
        drawn.push(moment(
            "longest-run",
            words::statistics::longest_run(),
            words::statistics::days(streak.days),
            words::statistics::from(dated(streak.from, this_year)),
        ));
    }
    if let Some(hour) = rhythm.busiest_hour() {
        drawn.push(moment(
            "most-often-at",
            words::statistics::most_often_at(),
            hour_named(hour),
            words::statistics::the_hour_heard_most().to_owned(),
        ));
    }
    if let Some((_, named)) = rhythm
        .busiest_weekday()
        .and_then(|day| WEEKDAYS.get(day).copied())
    {
        drawn.push(moment(
            "most-often-on",
            words::statistics::most_often_on(),
            named().to_owned(),
            words::statistics::the_day_heard_most().to_owned(),
        ));
    }

    (!drawn.is_empty()).then(|| div().flex().flex_wrap().gap_3().children(drawn))
}

fn moment(id: &'static str, named: &'static str, reading: String, note: String) -> Stateful<Div> {
    kit::card()
        .id(id)
        .read_plainly(&format!("{named}: {reading}, {note}"))
        .flex_1()
        .gap_1()
        .min_w(px(theme::stat_tile()))
        .child(kit::eyebrow(named))
        .child(
            kit::figure(reading)
                .text_size(px(theme::text_lg()))
                .text_color(rgb(theme::text())),
        )
        .child(kit::figure(note).text_color(rgb(theme::faint())))
}

fn listed_days(chart: &Chart) -> Stateful<Div> {
    let mut bars = div()
        .id("listening-by-day")
        .named_as(Role::List, words::statistics::listening_by_day(), "")
        .flex()
        .w_full()
        .flex_none()
        .items_end()
        .h(px(theme::chart_height()))
        .gap(px(theme::CHART_BAR_GAP));
    for (index, bar) in chart.bars.iter().copied().enumerate() {
        bars = bars.child(drawn_bar(
            index,
            bar,
            chart.bars.len(),
            chart.is_the_peak(bar),
        ));
    }

    bars
}

fn a_guide(from_the_top: f32, colour: gpui::Rgba) -> Div {
    div()
        .absolute()
        .left_0()
        .right_0()
        .top(relative(from_the_top))
        .h(px(theme::FRAME_BORDER))
        .bg(colour)
}

fn charted(chart: &Chart) -> Div {
    const GUIDES: [f32; 3] = [0.0, 0.5, 1.0];

    let tallest = format::heard_for(chart.tallest);
    let half = format::heard_for(chart.tallest / 2);
    let guide_colour = theme::tinted(theme::text(), GUIDE_LINE);
    let plot = div()
        .relative()
        .flex_1()
        .min_w(px(0.0))
        .children(GUIDES.map(|at| a_guide(at, guide_colour)))
        .when(chart.average_share > 0.0, |plot| {
            plot.child(a_guide(
                1.0 - chart.average_share,
                theme::tinted(theme::accent(), AVERAGE_LINE),
            ))
        })
        .child(listed_days(chart));
    let scale = div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(theme::chart_axis()))
        .h(px(theme::chart_height()))
        .justify_between()
        .child(kit::figure(tallest))
        .child(kit::figure(half))
        .child(kit::figure("0"));
    let axis = chart.axis().map(|(from, to)| {
        div()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(theme::chart_axis()))
            .child(kit::figure(from))
            .child(kit::figure(to))
    });

    kit::section()
        .child(
            kit::section_header()
                .child(kit::section_name(words::statistics::listening_by_day()))
                .child(kit::figure(chart.says())),
        )
        .child(
            kit::section_body()
                .gap_2()
                .child(div().flex().w_full().child(scale).child(plot))
                .children(axis),
        )
}

fn a_fill(heard: bool, lit: bool) -> gpui::Rgba {
    match (heard, lit) {
        (false, _) => theme::tinted(theme::soft_edge(), WHOLE_STRENGTH),
        (true, true) => theme::tinted(theme::accent(), WHOLE_STRENGTH),
        (true, false) => theme::tinted(theme::accent(), BODY_OF_A_BAR),
    }
}

struct Standing {
    said: SharedString,
    placed: Placed,
    share: f32,
    fill: gpui::Rgba,
    widest: f32,
}

fn a_standing_bar(id: (&'static str, usize), standing: Standing) -> Stateful<Div> {
    let Standing {
        said,
        placed,
        share,
        fill,
        widest,
    } = standing;
    let raised = said.clone();

    div()
        .id(id)
        .listed_as(|| said)
        .placed(placed)
        .flex()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_end()
        .h_full()
        .min_w(px(theme::CHART_BAR))
        .rounded_sm()
        .hover(|column| column.bg(theme::tinted(theme::accent(), WASH_OF_A_RAISED_BAR)))
        .names_when_raised(move || raised.clone())
        .child(
            div()
                .w_full()
                .max_w(px(widest))
                .h(relative(share))
                .min_h(px(theme::CHART_BASELINE))
                .rounded_sm()
                .bg(fill),
        )
}

fn drawn_bar(index: usize, bar: Bar, of: usize, peak: bool) -> Stateful<Div> {
    a_standing_bar(
        ("listening-day", index),
        Standing {
            said: SharedString::from(said(bar)),
            placed: Placed { at: index, of },
            share: bar.share,
            fill: a_fill(bar.plays > 0, peak),
            widest: theme::chart_bar_widest(),
        },
    )
}

#[derive(Clone, Debug, PartialEq)]
struct Column {
    said: String,
    share: f32,
    lit: bool,
}

fn columns_of(spans: &[Duration], said: impl Fn(usize, Duration) -> String) -> Vec<Column> {
    let shares = shares_of(spans);

    spans
        .iter()
        .zip(shares)
        .enumerate()
        .map(|(at, (span, share))| Column {
            said: said(at, *span),
            share,
            lit: share >= 1.0,
        })
        .collect()
}

fn listed_columns(
    id: &'static str,
    named: &'static str,
    columns: Vec<Column>,
    bar_id: &'static str,
) -> Stateful<Div> {
    let of = columns.len();
    let mut bars = div()
        .id(id)
        .named_as(Role::List, named, "")
        .flex()
        .w_full()
        .flex_none()
        .items_end()
        .h(px(theme::rhythm_height()))
        .gap(px(theme::CHART_BAR_GAP));
    for (at, column) in columns.into_iter().enumerate() {
        bars = bars.child(a_standing_bar(
            (bar_id, at),
            Standing {
                said: SharedString::from(column.said),
                placed: Placed { at, of },
                share: column.share,
                fill: a_fill(column.share > 0.0, column.lit),
                widest: theme::rhythm_bar_widest(),
            },
        ));
    }

    bars
}

fn hours_heard(rhythm: &Rhythm) -> Div {
    let columns = columns_of(&rhythm.hours, hour_said);
    let axis = div()
        .flex()
        .items_center()
        .justify_between()
        .children(HOURS_ON_THE_AXIS.map(kit::figure));

    kit::section()
        .flex_1()
        .child(kit::section_header().child(kit::section_name(words::statistics::when_in_the_day())))
        .child(
            kit::section_body()
                .gap_2()
                .child(listed_columns(
                    "listening-by-hour",
                    words::statistics::listening_by_hour(),
                    columns,
                    "listening-hour",
                ))
                .child(axis),
        )
}

fn weekdays_heard(rhythm: &Rhythm) -> Div {
    let columns = columns_of(&rhythm.weekdays, |at, listened| {
        weekday_said(WEEKDAYS.get(at).map_or("", |(_, named)| named()), listened)
    });
    let axis = div()
        .flex()
        .gap(px(theme::CHART_BAR_GAP))
        .children(WEEKDAYS.map(|(short, _)| {
            div()
                .flex()
                .flex_1()
                .justify_center()
                .child(kit::figure(short()))
        }));

    kit::section()
        .flex_1()
        .child(
            kit::section_header().child(kit::section_name(words::statistics::when_in_the_week())),
        )
        .child(
            kit::section_body()
                .gap_2()
                .child(listed_columns(
                    "listening-by-weekday",
                    words::statistics::listening_by_weekday(),
                    columns,
                    "listening-weekday",
                ))
                .child(axis),
        )
}

fn the_sound_heard(wide: bool, habits: &Habits) -> Option<Div> {
    let genres = (!habits.genres.is_empty()).then(|| genres_heard(&habits.genres));
    let sound = habits.formats.map(sound_heard);

    match (genres, sound) {
        (Some(genres), Some(sound)) => Some(side_by_side(wide, genres, sound)),
        (Some(alone), None) | (None, Some(alone)) => Some(div().flex().child(alone)),
        (None, None) => None,
    }
}

fn a_meter(share: f32, colour: u32) -> Div {
    div()
        .w_full()
        .h(px(theme::meter_height()))
        .rounded_full()
        .overflow_hidden()
        .bg(theme::tinted(theme::text(), GUIDE_LINE))
        .child(
            div()
                .h_full()
                .w(relative(share))
                .rounded_full()
                .bg(rgb(colour)),
        )
}

fn genres_heard(genres: &[Ranked]) -> Div {
    let shares = shares_of(
        &genres
            .iter()
            .map(|genre| genre.listened)
            .collect::<Vec<_>>(),
    );
    let of = genres.len();
    let mut listed = kit::section_body()
        .id("genres-most-heard")
        .named_as(Role::List, words::statistics::genres_most_heard(), "")
        .gap_3();
    for (at, (genre, share)) in genres.iter().zip(shares).enumerate() {
        let heard = format::heard_for(genre.listened);
        let (accent, _) = Accent::named_pair(&genre.name);

        listed = listed.child(
            div()
                .id(("genre-heard", at))
                .listed_as(|| {
                    SharedString::from(words::statistics::genre_heard(&genre.name, &heard))
                })
                .placed(Placed { at, of })
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(theme::text_sm()))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(a_name(SharedString::from(genre.name.clone()))),
                        )
                        .child(kit::figure(heard).flex_none()),
                )
                .child(a_meter(share, theme::hue(accent))),
        );
    }

    kit::section()
        .flex_1()
        .child(
            kit::section_header().child(kit::section_name(words::statistics::genres_most_heard())),
        )
        .child(listed)
}

fn sound_heard(shares: FormatShares) -> Div {
    let parts = SoundShares::of(shares).parts();
    let lead = div()
        .flex()
        .items_baseline()
        .gap_2()
        .child(
            kit::figure(percent(f32::from(shares.lossless_permille) / WHOLE_SHARE))
                .font(theme::ui(FontWeight::SEMIBOLD))
                .text_size(px(theme::text_title()))
                .text_color(rgb(theme::text())),
        )
        .child(kit::figure(words::statistics::of_it_lossless()).text_color(rgb(theme::muted())));
    let stacked = div()
        .flex()
        .w_full()
        .h(px(theme::meter_height()))
        .rounded_full()
        .overflow_hidden()
        .children(
            parts
                .iter()
                .filter(|(_, share, _)| *share > 0.0)
                .map(|(_, share, colour)| div().h_full().w(relative(*share)).bg(rgb(*colour))),
        );
    let legend = div().flex().flex_col().gap_2().children(
        parts
            .iter()
            .filter(|(_, share, _)| *share > 0.0)
            .enumerate()
            .map(|(at, (named, share, colour))| {
                let reading = percent(*share);

                div()
                    .id(("sound-heard", at))
                    .read_plainly(&format!("{named}: {reading}"))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::text()))
                    .child(
                        div()
                            .flex_none()
                            .size(px(theme::legend_mark()))
                            .rounded_full()
                            .bg(rgb(*colour)),
                    )
                    .child(div().flex_1().min_w(px(0.0)).child(*named))
                    .child(kit::figure(reading).flex_none())
            }),
    );

    kit::section()
        .flex_1()
        .child(
            kit::section_header().child(kit::section_name(words::statistics::the_sound_you_hear())),
        )
        .child(
            kit::section_body()
                .gap_3()
                .child(lead)
                .child(stacked)
                .child(legend),
        )
}

fn number_one(
    id: &'static str,
    named: &'static str,
    art: AnyElement,
    name: &str,
    by: Option<&str>,
    plays: u64,
    listened: Duration,
) -> Stateful<Div> {
    let heard = format::heard_for(listened);
    let line = by_line(by, plays);

    kit::card()
        .id(id)
        .read_plainly(&format!("{named}: {name}, {line}, {heard}"))
        .flex_row()
        .items_center()
        .gap_3()
        .min_w(px(0.0))
        .child(art)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .gap_0p5()
                .child(kit::eyebrow(named))
                .child(
                    a_name(SharedString::from(name.to_owned()))
                        .text_size(px(theme::text_base()))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(a_faint_line(line))
                .child(kit::figure(heard)),
        )
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::views::spoken::{self, node_of};

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
        assert!(chart.says().ends_with("· 3 days to a bar"));
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
    fn a_reader_hears_each_bar_as_a_place_in_the_list_of_days_saying_what_was_played() {
        spoken::listened_to(true);

        let days = [
            a_day(1, 3, Duration::from_secs(3_600)),
            a_day(0, 0, Duration::ZERO),
        ];
        let chart = Chart::in_calendar(&days, now(), &Calendar::utc(), BARS_AT_MOST);

        let list = node_of(&listed_days(&chart)).expect("a heard list is a node");
        let yesterday = node_of(&drawn_bar(0, chart.bars[0], chart.bars.len(), true))
            .expect("a heard bar is a node");
        let today = node_of(&drawn_bar(1, chart.bars[1], chart.bars.len(), false))
            .expect("a heard bar is a node");

        assert_eq!(list.role(), Role::List);
        assert_eq!(list.label(), Some("Listening by day"));
        assert_eq!(yesterday.role(), Role::ListItem);
        assert_eq!(
            yesterday.label(),
            Some("yesterday · 3 plays · 1h 0m listened")
        );
        assert_eq!(yesterday.position_in_set(), Some(1));
        assert_eq!(yesterday.size_of_set(), Some(2));
        assert_eq!(today.label(), Some("today · nothing played"));
        assert_eq!(today.position_in_set(), Some(2));
    }

    #[test]
    fn the_line_of_averages_stands_as_high_as_the_mean_of_the_bars() {
        let days = [
            a_day(1, 1, Duration::from_secs(1_000)),
            a_day(0, 1, Duration::from_secs(3_000)),
        ];
        let chart = Chart::in_calendar(&days, now(), &Calendar::utc(), BARS_AT_MOST);

        assert!((chart.average_share - 2.0 / 3.0).abs() < 1e-5);
        assert_eq!(chart.average, Duration::from_secs(2_000));
        assert_eq!(chart.says(), "33m on average · 50m at the peak");
        assert!(chart.is_the_peak(chart.bars[1]));
        assert!(!chart.is_the_peak(chart.bars[0]));
    }

    #[test]
    fn a_day_with_nothing_played_is_never_the_peak() {
        let days = [a_day(0, 0, Duration::ZERO)];
        let chart = Chart::in_calendar(&days, now(), &Calendar::utc(), BARS_AT_MOST);

        assert!(!chart.is_the_peak(chart.bars[0]));
        assert_eq!(chart.average_share, 0.0);
    }

    #[test]
    fn the_pace_is_of_every_day_the_window_spans_rather_than_those_heard() {
        assert_eq!(
            the_pace_of(Duration::from_secs(36_000), 3, 10),
            "1h 0m a day · heard on 3 of 10 days"
        );
        assert_eq!(the_pace_of(Duration::from_secs(60), 1, 0), "");
    }

    #[test]
    fn the_share_of_each_sound_adds_up_to_what_was_heard() {
        let shares = SoundShares::of(FormatShares {
            lossless_permille: 800,
            high_resolution_permille: 300,
        });

        assert!((shares.high_resolution - 0.3).abs() < 1e-5);
        assert!((shares.cd_quality - 0.5).abs() < 1e-5);
        assert!((shares.lossy - 0.2).abs() < 1e-5);
        assert_eq!(percent(shares.cd_quality), "50%");
    }

    #[test]
    fn every_column_is_a_share_of_the_tallest_and_the_tallest_is_lit() {
        let columns = columns_of(
            &[
                Duration::ZERO,
                Duration::from_secs(30),
                Duration::from_secs(60),
            ],
            hour_said,
        );

        assert_eq!(
            columns
                .iter()
                .map(|column| column.share)
                .collect::<Vec<_>>(),
            [0.0, 0.5, 1.0]
        );
        assert_eq!(
            columns.iter().map(|column| column.lit).collect::<Vec<_>>(),
            [false, false, true]
        );
        assert_eq!(columns[0].said, "00:00 · nothing heard");
        assert_eq!(columns[2].said, "02:00 · 1m listened");
        assert!(
            columns_of(&[Duration::ZERO], hour_said)
                .iter()
                .all(|column| !column.lit)
        );
    }

    #[test]
    fn a_ranked_row_names_who_made_it_where_it_knows_and_always_how_often() {
        assert_eq!(by_line(Some("Pink Floyd"), 2), "Pink Floyd · 2 plays");
        assert_eq!(by_line(None, 1), "1 play");
    }

    #[test]
    fn a_date_this_year_leaves_the_year_out() {
        let date = CivilDate {
            year: 2024,
            month: 11,
            day: 3,
        };

        assert_eq!(dated(date, 2024), "3 Nov");
        assert_eq!(dated(date, 2025), "3 Nov 2024");
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
