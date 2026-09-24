use std::{
    f32::consts::TAU,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    Bounds, Context, Pixels, Point, ScrollHandle, SharedString, Size, Task, ease_in_out, point, px,
};
use resonate_core::{Frames, TrackId};
use resonate_engine::{PlayerState, StreamDigest};
use resonate_lyrics::{Lyricists, Lyrics, Timing, Waiting, Wanted};

use crate::theme;

pub(crate) fn near_the_words(
    pointer: Point<Pixels>,
    pane: Bounds<Pixels>,
    column: Pixels,
    read: Option<Bounds<Pixels>>,
) -> bool {
    if pane.size.height <= px(0.0) || !pane.contains(&pointer) {
        return false;
    }

    let middle = pane.left() + pane.size.width / 2.0;
    let slack = theme::width(theme::lyric_pad());
    if (pointer.x - middle).abs() > column / 2.0 + slack {
        return false;
    }

    let Some(read) = read else {
        return true;
    };
    let reach = theme::width(theme::lyric_reach());

    pointer.y >= read.top() - reach && pointer.y <= read.bottom() + reach
}

const TURN: Duration = Duration::from_millis(420);

const GLIDE_RESPONSE_SECS: f32 = 0.62;

const GLIDE_DAMPING: f32 = 0.8;

const GLIDE_SETTLES_IN: Duration = Duration::from_millis(820);

const LAG_PER_LINE: Duration = Duration::from_millis(32);

const LAGS_AT_MOST: usize = 6;

const ARRIVES_IN: Duration = Duration::from_millis(560);

const RISES_FROM: Pixels = px(22.0);

const RISE_PER_LINE: Duration = Duration::from_millis(40);

const RISES_AT_MOST: usize = 8;

const LOOKS_QUIETLY_FOR: Duration = Duration::from_millis(450);

const BREATH: Duration = Duration::from_millis(2400);

const HANDS_OFF: Duration = Duration::from_secs(6);

const BAR_LINGERS: Duration = Duration::from_millis(1_500);

const RESETTLE: Pixels = px(56.0);

const SETTLED: Pixels = px(0.5);

const LIT: f32 = 1.0;

const ADRIFT: f32 = 0.45;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reading {
    #[default]
    InPlay,
    Whole,
}

impl Reading {
    pub const ALL: [Self; 2] = [Self::InPlay, Self::Whole];

    pub const fn label(self) -> &'static str {
        match self {
            Self::InPlay => "In play",
            Self::Whole => "Whole set",
        }
    }

    pub const fn about(self) -> &'static str {
        match self {
            Self::InPlay => {
                "The line being sung and the two coming after it. Hold the pointer over the pane \
                 to open it out to the whole set."
            }
            Self::Whole => "Every line, scrolling with the track",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Falloff {
    Around,
    Across,
}

impl Falloff {
    const fn between(self, sung: usize, line: usize) -> f32 {
        if line < sung {
            return self.behind(sung - line);
        }

        self.ahead(line - sung)
    }

    const fn ahead(self, away: usize) -> f32 {
        match (self, away) {
            (_, 0) => LIT,
            (Self::Around, 1) => 0.4,
            (Self::Around, 2) => 0.18,
            (Self::Around, _) => 0.0,
            (Self::Across, 1) => 0.42,
            (Self::Across, 2) => 0.3,
            (Self::Across, 3) => 0.22,
            (Self::Across, _) => 0.16,
        }
    }

    const fn behind(self, away: usize) -> f32 {
        match self {
            Self::Around => 0.0,
            Self::Across => self.ahead(away),
        }
    }

    const fn spent(self) -> f32 {
        match self {
            Self::Around => 0.0,
            Self::Across => 0.16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    At(usize),
    Evenly,
    Spent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asked {
    track: TrackId,
    duration: Option<Frames>,
    tagged: bool,
}

impl Asked {
    pub fn of(state: &PlayerState, digest: Option<&StreamDigest>) -> Option<Self> {
        let current = state.current?;
        Some(Self {
            track: current.id,
            duration: current.duration,
            tagged: digest.is_some_and(|digest| digest.track == current.id),
        })
    }
}

#[derive(Clone)]
pub enum Look {
    Nothing,
    Searching,
    Missing,
    Found(Arc<Lyrics>),
    Refused(SharedString),
}

#[derive(Clone, Copy)]
struct Eased {
    started: Instant,
}

impl Eased {
    fn from(started: Instant) -> Self {
        Self { started }
    }

    fn through(self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();

        ease_in_out((elapsed / TURN.as_secs_f32()).clamp(0.0, 1.0))
    }

    fn settled(self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= TURN
    }
}

#[derive(Clone, Copy)]
struct Sprung {
    started: Instant,
}

impl Sprung {
    fn from(started: Instant) -> Self {
        Self { started }
    }

    fn through(self, now: Instant) -> f32 {
        spring(now.saturating_duration_since(self.started))
    }

    fn through_after(self, lag: Duration, now: Instant) -> f32 {
        spring(
            now.saturating_duration_since(self.started)
                .saturating_sub(lag),
        )
    }

    fn settled_after(self, lag: Duration, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= GLIDE_SETTLES_IN + lag
    }

    fn settled(self, now: Instant) -> bool {
        self.settled_after(Duration::ZERO, now)
    }
}

fn spring(elapsed: Duration) -> f32 {
    if elapsed >= GLIDE_SETTLES_IN {
        return 1.0;
    }
    let seconds = elapsed.as_secs_f32();
    let natural = TAU / GLIDE_RESPONSE_SECS;
    let damped = natural * GLIDE_DAMPING.mul_add(-GLIDE_DAMPING, 1.0).sqrt();
    let decay = (-GLIDE_DAMPING * natural * seconds).exp();
    let swing =
        (damped * seconds).cos() + (GLIDE_DAMPING * natural / damped) * (damped * seconds).sin();

    decay.mul_add(-swing, 1.0)
}

fn lagged(lines: usize, per_line: Duration, at_most: usize) -> Duration {
    per_line * u32::try_from(lines.min(at_most)).unwrap_or(u32::MAX)
}

#[derive(Clone, Copy)]
struct Turn<T> {
    from: T,
    to: T,
    clock: Eased,
}

impl<T: Copy + PartialEq> Turn<T> {
    fn still(to: T) -> Self {
        Self {
            from: to,
            to,
            clock: Eased::from(Instant::now()),
        }
    }

    fn onto(&mut self, to: T, now: Instant) -> bool {
        if self.to == to {
            return false;
        }

        *self = Self {
            from: self.to,
            to,
            clock: Eased::from(now),
        };
        true
    }

    fn blended(&self, now: Instant, reading: impl Fn(T) -> f32) -> f32 {
        let through = self.clock.through(now);

        reading(self.from).mul_add(1.0 - through, reading(self.to) * through)
    }
}

struct Sheet {
    text: Arc<[SharedString]>,
    moments: Arc<[Option<Duration>]>,
    written: Arc<[usize]>,
}

impl Default for Sheet {
    fn default() -> Self {
        Self {
            text: Arc::from([] as [SharedString; 0]),
            moments: Arc::from([] as [Option<Duration>; 0]),
            written: Arc::from([] as [usize; 0]),
        }
    }
}

#[derive(Clone, Copy)]
struct Glide {
    was: Pixels,
    lands: Pixels,
    clock: Sprung,
}

impl Glide {
    fn at(self, now: Instant) -> Pixels {
        self.was + (self.lands - self.was) * self.clock.through(now)
    }

    fn lag_of(self, lines_ahead: usize, now: Instant) -> Pixels {
        let lag = lagged(lines_ahead, LAG_PER_LINE, LAGS_AT_MOST);
        let behind = self.clock.through(now) - self.clock.through_after(lag, now);

        (self.lands - self.was) * -behind
    }

    fn settled(self, now: Instant) -> bool {
        self.clock
            .settled_after(lagged(LAGS_AT_MOST, LAG_PER_LINE, LAGS_AT_MOST), now)
    }
}

pub struct LyricsModel {
    lyricists: Arc<Lyricists>,
    asked_about: Option<Asked>,
    looked_for: Option<Wanted>,
    asked_at: Instant,
    look: Look,
    scroll: ScrollHandle,
    reading: Reading,
    opened_out: bool,
    sheet: Sheet,
    read_at: Option<usize>,
    laid_out: Option<Size<Pixels>>,
    placed: bool,
    arrived: Option<Sprung>,
    turn: Turn<Reads>,
    light: Turn<Option<usize>>,
    spread: Turn<Falloff>,
    glide: Option<Glide>,
    hand_at: Option<Instant>,
    _find: Task<()>,
}

impl LyricsModel {
    pub fn new(lyricists: Arc<Lyricists>) -> Self {
        Self {
            lyricists,
            asked_about: None,
            looked_for: None,
            asked_at: Instant::now(),
            look: Look::Nothing,
            scroll: ScrollHandle::new(),
            reading: Reading::default(),
            opened_out: false,
            sheet: Sheet::default(),
            read_at: None,
            laid_out: None,
            placed: false,
            arrived: None,
            turn: Turn::still(Reads::Evenly),
            light: Turn::still(None),
            spread: Turn::still(Falloff::Around),
            glide: None,
            hand_at: None,
            _find: Task::ready(()),
        }
    }

    pub const fn look(&self) -> &Look {
        &self.look
    }

    pub fn asks_again(&self, asked: Option<Asked>) -> bool {
        self.asked_about != asked
    }

    pub const fn scroll(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn text(&self) -> Arc<[SharedString]> {
        Arc::clone(&self.sheet.text)
    }

    pub fn moments(&self) -> Arc<[Option<Duration>]> {
        Arc::clone(&self.sheet.moments)
    }

    pub fn has_a_source(&self) -> bool {
        self.lyricists.has_a_source()
    }

    pub fn waiting_at(&self, position: Duration) -> Option<Waiting> {
        self.found()?.waiting_at(position)
    }

    pub fn is_synced(&self) -> bool {
        self.found()
            .is_some_and(|lyrics| lyrics.timing() == Timing::Synced)
    }

    fn found(&self) -> Option<&Lyrics> {
        match &self.look {
            Look::Found(lyrics) => Some(lyrics),
            Look::Nothing | Look::Searching | Look::Missing | Look::Refused(_) => None,
        }
    }

    pub fn looks_quietly(&self, now: Instant) -> bool {
        matches!(self.look, Look::Searching)
            && now.saturating_duration_since(self.asked_at) < LOOKS_QUIETLY_FOR
    }

    pub const fn reading(&self) -> Reading {
        self.reading
    }

    pub fn read_as(&mut self, reading: Reading) {
        self.reading = reading;
        self.spread.onto(self.falloff(), Instant::now());
    }

    pub fn open_out(&mut self, opened: bool) -> bool {
        let was = self.opened_out;
        self.opened_out = opened;
        self.spread.onto(self.falloff(), Instant::now()) || was != opened
    }

    pub const fn shows_every_line(&self) -> bool {
        self.opened_out || matches!(self.reading, Reading::Whole)
    }

    const fn falloff(&self) -> Falloff {
        if self.shows_every_line() {
            Falloff::Across
        } else {
            Falloff::Around
        }
    }

    pub const fn read_at(&self) -> Option<usize> {
        self.read_at
    }

    pub fn follow_the_track(&mut self, position: Duration, now: Instant) {
        let Some(lyrics) = self.found() else {
            self.turn.onto(Reads::Evenly, now);
            self.light.onto(None, now);
            return;
        };
        let lit = lyrics.line_in_play(position);
        let reads = if lyrics.timing() == Timing::Unsynced {
            Reads::Evenly
        } else {
            lit.map_or(Reads::Spent, Reads::At)
        };
        let read_at = lyrics
            .waiting_at(position)
            .map(|waiting| waiting.next)
            .or_else(|| lyrics.line_at(position));

        self.read_at = read_at;
        if self.placed {
            self.turn.onto(reads, now);
            self.light.onto(lit, now);
        } else {
            self.turn = Turn::still(reads);
            self.light = Turn::still(lit);
        }
    }

    pub fn standing(&self, index: usize, now: Instant) -> f32 {
        let line = self.ordinal(index);

        self.spread.blended(now, |falloff| {
            self.turn.blended(now, |reads| match reads {
                Reads::At(sung) => falloff.between(self.ordinal(sung), line),
                Reads::Evenly => ADRIFT,
                Reads::Spent => falloff.spent(),
            })
        })
    }

    pub fn lead(&self, index: usize, now: Instant) -> f32 {
        self.light
            .blended(now, |lit| f32::from(u8::from(lit == Some(index))))
    }

    pub fn lag(&self, index: usize, now: Instant) -> Pixels {
        let Some(glide) = self.glide else {
            return px(0.0);
        };
        let lines_ahead = self.read_at.map_or(0, |read| {
            self.ordinal(index).saturating_sub(self.ordinal(read))
        });

        glide.lag_of(lines_ahead, now)
    }

    pub fn rise(&self, index: usize, now: Instant) -> Pixels {
        let Some(arrived) = self.arrived else {
            return RISES_FROM;
        };
        let read = self.read_at.map_or(0, |read| self.ordinal(read));
        let away = self.ordinal(index).abs_diff(read);
        let lag = lagged(away, RISE_PER_LINE, RISES_AT_MOST);

        RISES_FROM * (1.0 - arrived.through_after(lag, now))
    }

    pub fn arrival(&self, now: Instant) -> f32 {
        let Some(arrived) = self.arrived else {
            return 0.0;
        };
        let elapsed = now.saturating_duration_since(arrived.started).as_secs_f32();

        ease_in_out((elapsed / ARRIVES_IN.as_secs_f32()).clamp(0.0, 1.0))
    }

    pub fn breath(&self, now: Instant) -> f32 {
        let since = self.arrived.map_or(Duration::ZERO, |arrived| {
            now.saturating_duration_since(arrived.started)
        });
        let phase = since.as_secs_f32() / BREATH.as_secs_f32();

        (phase * TAU).sin().mul_add(0.5, 0.5)
    }

    fn ordinal(&self, index: usize) -> usize {
        self.sheet.written.get(index).copied().unwrap_or(index)
    }

    pub fn centre_of(&self, index: usize) -> Option<Pixels> {
        let line = self.scroll.bounds_for_item(index)?;
        let pane = self.scroll.bounds();
        if pane.size.height <= px(0.0) {
            return None;
        }
        let middle = pane.top() + (pane.size.height - line.size.height) / 2.0;

        Some(middle - line.top())
    }

    pub fn pane_height(&self) -> Pixels {
        self.scroll.bounds().size.height
    }

    pub fn column_width(&self) -> Pixels {
        let room = self.scroll.bounds().size.width - theme::width(theme::lyric_gutter()) * 2.0;

        room.clamp(px(0.0), theme::width(theme::lyric_column()))
    }

    pub fn edge(&self) -> Pixels {
        self.pane_height() / 2.0
    }

    pub fn opened_by(&self, pointer: Point<Pixels>) -> bool {
        near_the_words(
            pointer,
            self.scroll.bounds(),
            self.column_width(),
            self.read_at
                .and_then(|line| self.scroll.bounds_for_item(line)),
        )
    }

    pub const fn is_placed(&self) -> bool {
        self.placed
    }

    pub fn place(&mut self, now: Instant) -> bool {
        let pane = self.scroll.bounds().size;
        let laid_out_the_same = self.laid_out == Some(pane);
        self.laid_out = Some(pane);

        if self.following(now)
            && let Some(lands) = self.landing()
        {
            if self.placed {
                self.glide_to(lands, now);
            } else if laid_out_the_same {
                self.scroll.set_offset(point(self.scroll.offset().x, lands));
                self.glide = None;
                self.placed = true;
                self.arrived = Some(Sprung::from(now));
            }
        }

        !self.placed || self.glide(now)
    }

    pub fn landing(&self) -> Option<Pixels> {
        match self.read_at {
            Some(line) => self.centre_of(line),
            None => Some(px(0.0)),
        }
    }

    pub fn glide_to(&mut self, lands: Pixels, now: Instant) {
        let Some(glide) = self.glide else {
            self.glide = Some(Glide {
                was: self.scroll.offset().y,
                lands,
                clock: Sprung::from(now),
            });
            return;
        };
        let drift = (glide.lands - lands).abs();
        if drift < SETTLED {
            return;
        }

        self.glide = if drift < RESETTLE && !glide.clock.settled(now) {
            Some(Glide { lands, ..glide })
        } else {
            Some(Glide {
                was: self.scroll.offset().y,
                lands,
                clock: Sprung::from(now),
            })
        };
    }

    pub fn glide(&mut self, now: Instant) -> bool {
        let Some(glide) = self.glide else {
            return false;
        };
        self.scroll
            .set_offset(point(self.scroll.offset().x, glide.at(now)));

        !glide.settled(now)
    }

    pub fn is_turning(&self, now: Instant) -> bool {
        !self.turn.clock.settled(now)
            || !self.light.clock.settled(now)
            || !self.spread.clock.settled(now)
            || self.is_arriving(now)
            || self.looks_quietly(now)
            || self.moved_by_hand_lately(now)
    }

    fn is_arriving(&self, now: Instant) -> bool {
        self.arrived.is_some_and(|arrived| {
            !arrived.settled_after(lagged(RISES_AT_MOST, RISE_PER_LINE, RISES_AT_MOST), now)
        })
    }

    pub fn led_by_hand(&mut self, now: Instant) {
        self.hand_at = Some(now);
        self.glide = None;
    }

    pub fn following(&self, now: Instant) -> bool {
        self.hand_at
            .is_none_or(|at| now.saturating_duration_since(at) >= HANDS_OFF)
    }

    pub fn moved_by_hand_lately(&self, now: Instant) -> bool {
        self.hand_at
            .is_some_and(|at| now.saturating_duration_since(at) < BAR_LINGERS)
    }

    pub fn follow_again(&mut self) {
        self.hand_at = None;
    }

    pub fn follow(&mut self, asked: Option<Asked>, wanted: Option<Wanted>, cx: &mut Context<Self>) {
        if self.asked_about == asked {
            return;
        }
        let moved_on = self.asked_about.map(|held| held.track) != asked.map(|asked| asked.track);
        self.asked_about = asked;

        let Some(wanted) = wanted else {
            self.looked_for = None;
            self.look = Look::Nothing;
            self.rewind();
            cx.notify();
            return;
        };

        if moved_on {
            self.look = Look::Searching;
            self.asked_at = Instant::now();
            self.rewind();
            cx.notify();
        }
        if !self.worth_looking_again(moved_on, &wanted) {
            return;
        }
        self.looked_for = Some(wanted.clone());

        let lyricists = Arc::clone(&self.lyricists);
        self._find = cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { lyricists.find(&wanted) })
                .await;

            let landed = this.update(cx, |this, cx| {
                this.rewind();
                this.look = match found {
                    Ok(Some(lyrics)) => Look::Found(Arc::new(lyrics)),
                    Ok(None) => Look::Missing,
                    Err(error) => {
                        tracing::warn!(%error, "no lyric provider answered");
                        Look::Refused(SharedString::from(error.to_string()))
                    }
                };
                this.hold();
                cx.notify();
            });
            let _ = landed;
        });
    }

    fn worth_looking_again(&self, moved_on: bool, wanted: &Wanted) -> bool {
        moved_on || self.looked_for.as_ref() != Some(wanted)
    }

    fn hold(&mut self) {
        let Some(lyrics) = self.found() else {
            return;
        };
        self.sheet = Sheet::of(lyrics);
    }

    fn rewind(&mut self) {
        self.read_at = None;
        self.laid_out = None;
        self.placed = false;
        self.arrived = None;
        self.turn = Turn::still(Reads::Evenly);
        self.light = Turn::still(None);
        self.glide = None;
        self.hand_at = None;
        self.opened_out = false;
        self.spread = Turn::still(self.falloff());
        self.sheet = Sheet::default();
        self.scroll.set_offset(point(px(0.0), px(0.0)));
    }
}

pub fn rising(standing: f32, through: f32) -> f32 {
    (LIT - standing).mul_add(through, standing)
}

impl Sheet {
    fn of(lyrics: &Lyrics) -> Self {
        let text = lyrics
            .lines()
            .iter()
            .map(|line| {
                if line.is_blank() {
                    SharedString::default()
                } else {
                    SharedString::from(line.text.clone())
                }
            })
            .collect();
        let moments = lyrics.lines().iter().map(|line| line.at).collect();
        let mut standing = 0;
        let written = lyrics
            .lines()
            .iter()
            .map(|line| {
                let ordinal = standing;
                if !line.is_blank() {
                    standing += 1;
                }

                ordinal
            })
            .collect();

        Self {
            text,
            moments,
            written,
        }
    }
}

#[cfg(test)]
mod tests {
    use resonate_engine::MediaLocation;
    use resonate_lyrics::LyricLine;

    use super::*;

    fn model() -> LyricsModel {
        LyricsModel::new(Arc::new(Lyricists::unsourced()))
    }

    fn wanting(title: Option<&str>) -> Wanted {
        Wanted {
            title: title.map(ToOwned::to_owned),
            ..Wanted::for_media(MediaLocation::local("/music/echoes.flac"))
        }
    }

    #[test]
    fn a_track_is_looked_up_again_only_where_what_is_searched_on_has_changed() {
        let mut model = model();

        assert!(
            model.worth_looking_again(true, &wanting(None)),
            "a track that has just started was not looked up"
        );
        model.looked_for = Some(wanting(None));

        assert!(
            !model.worth_looking_again(false, &wanting(None)),
            "a track was looked up twice on the same words"
        );
        assert!(
            model.worth_looking_again(false, &wanting(Some("Echoes"))),
            "a title the digest brought was never searched on"
        );
        assert!(
            model.worth_looking_again(true, &wanting(None)),
            "a track change did not look again"
        );
    }

    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn holding(lines: &[&str]) -> LyricsModel {
        let source = resonate_core::SourceId::new("held").expect("a lowercase name");
        let sung = lines
            .iter()
            .zip(0..)
            .map(|(text, line)| LyricLine::sung(at(line), *text))
            .collect();
        let mut model = model();
        model.look = Look::Found(Arc::new(
            Lyrics::synced(source, sung).expect("every line is timed"),
        ));
        model.hold();

        model
    }

    fn verse(lines: usize) -> LyricsModel {
        let sung: Vec<&str> = (0..lines).map(|_| "a line").collect();

        holding(&sung)
    }

    fn spread_settled(model: &mut LyricsModel) {
        model.spread = Turn::still(model.falloff());
    }

    #[test]
    fn a_turn_runs_from_nothing_to_everything_and_then_stays_there() {
        let start = Instant::now();
        let clock = Eased::from(start);

        assert!((clock.through(start) - 0.0).abs() < f32::EPSILON);
        assert!((clock.through(start + TURN / 2) - 0.5).abs() < f32::EPSILON);
        assert!((clock.through(start + TURN) - 1.0).abs() < f32::EPSILON);
        assert!((clock.through(start + TURN * 4) - 1.0).abs() < f32::EPSILON);
        assert!(!clock.settled(start));
        assert!(clock.settled(start + TURN));
    }

    #[test]
    fn a_line_crossfades_from_where_it_stood_to_where_it_is_going() {
        let start = Instant::now();
        let mut model = verse(6);
        model.read_as(Reading::Whole);
        spread_settled(&mut model);
        model.placed = true;

        model.follow_the_track(at(1), start);
        model.follow_the_track(at(2), start);

        assert!((model.standing(2, start) - Falloff::Across.ahead(1)).abs() < f32::EPSILON);
        assert!((model.standing(2, start + TURN) - LIT).abs() < f32::EPSILON);
        assert!((model.standing(1, start) - LIT).abs() < f32::EPSILON);
        assert!((model.standing(1, start + TURN) - Falloff::Across.ahead(1)).abs() < f32::EPSILON);
        assert!((model.lead(2, start) - 0.0).abs() < f32::EPSILON);
        assert!((model.lead(2, start + TURN) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_in_play_reading_is_the_line_being_sung_and_the_two_coming_after_it() {
        let now = Instant::now();
        let mut model = verse(8);

        model.follow_the_track(at(4), now);
        let settled = now + TURN;

        assert!((model.standing(4, settled) - LIT).abs() < f32::EPSILON);
        assert!((model.standing(5, settled) - Falloff::Around.ahead(1)).abs() < f32::EPSILON);
        assert!((model.standing(6, settled) - Falloff::Around.ahead(2)).abs() < f32::EPSILON);
        assert!(model.standing(7, settled).abs() < f32::EPSILON);
    }

    #[test]
    fn a_line_already_sung_is_out_of_an_in_play_reading_and_still_in_the_whole_set() {
        let now = Instant::now();
        let mut model = verse(8);

        model.follow_the_track(at(4), now);
        let settled = now + TURN;

        assert!(model.standing(3, settled).abs() < f32::EPSILON);
        assert!(model.standing(0, settled).abs() < f32::EPSILON);

        model.read_as(Reading::Whole);
        spread_settled(&mut model);
        assert!((model.standing(3, settled) - Falloff::Across.ahead(1)).abs() < f32::EPSILON);
        assert!(model.standing(0, settled) > 0.0);
    }

    #[test]
    fn the_pointer_opens_an_in_play_reading_out_without_changing_the_choice() {
        let now = Instant::now();
        let mut model = verse(8);
        model.follow_the_track(at(4), now);
        let settled = now + TURN;

        assert!(model.standing(7, settled).abs() < f32::EPSILON);

        model.open_out(true);
        spread_settled(&mut model);
        assert!(model.shows_every_line());
        assert_eq!(model.reading(), Reading::InPlay);
        assert!(model.standing(7, settled) > 0.0);
    }

    #[test]
    fn opening_the_pane_out_crossfades_the_lines_in_rather_than_snapping_them() {
        let now = Instant::now();
        let mut model = verse(8);
        model.follow_the_track(at(4), now);
        let settled = now + TURN;

        model.open_out(true);
        let opened = Instant::now();
        let across = Falloff::Across.ahead(3);

        assert!(model.standing(7, opened) < across / 2.0);
        assert!((model.standing(7, opened + TURN) - across).abs() < f32::EPSILON);
        assert!((model.standing(7, settled.max(opened + TURN)) - across).abs() < f32::EPSILON);
    }

    #[test]
    fn a_spring_runs_from_rest_to_rest_and_never_wanders_far_past_the_end() {
        assert!(spring(Duration::ZERO).abs() < f32::EPSILON);
        assert!((spring(GLIDE_SETTLES_IN) - 1.0).abs() < f32::EPSILON);
        assert!((spring(GLIDE_SETTLES_IN * 3) - 1.0).abs() < f32::EPSILON);

        let mut furthest: f32 = 0.0;
        for step in 0..=80 {
            let through = spring(GLIDE_SETTLES_IN * step / 80);
            furthest = furthest.max(through);
        }
        assert!(furthest > 0.99);
        assert!(furthest < 1.05);
        assert!(spring(Duration::from_millis(150)) > spring(Duration::from_millis(50)));
    }

    #[test]
    fn a_line_further_ahead_lags_further_behind_the_glide_and_catches_up_once_it_settles() {
        let now = Instant::now();
        let mut model = verse(8);
        model.placed = true;
        model.follow_the_track(at(2), now);
        model.glide_to(px(-300.0), now);

        let midway = now + Duration::from_millis(120);
        assert!(model.lag(2, midway).abs() < px(f32::EPSILON));
        assert!(model.lag(3, midway) > px(0.0));
        assert!(model.lag(5, midway) > model.lag(3, midway));
        assert!(model.lag(0, midway).abs() < px(f32::EPSILON));

        let settled = now + GLIDE_SETTLES_IN + LAG_PER_LINE * 8;
        assert!(model.lag(5, settled).abs() < px(f32::EPSILON));
        assert!(model.glide(midway));
        assert!(!model.glide(settled));
    }

    #[test]
    fn a_set_rises_into_place_from_the_read_line_outwards_once_it_has_been_placed() {
        let now = Instant::now();
        let mut model = verse(8);

        assert_eq!(model.rise(3, now), RISES_FROM);
        assert!(model.arrival(now).abs() < f32::EPSILON);

        model.follow_the_track(at(3), now);
        model.placed = true;
        model.arrived = Some(Sprung::from(now));

        let soon = now + Duration::from_millis(100);
        assert!(model.rise(3, soon) < model.rise(4, soon));
        assert!(model.rise(4, soon) < model.rise(7, soon));
        assert!(model.rise(2, soon) < model.rise(0, soon));
        assert!(model.arrival(soon) > 0.0);
        assert!(model.is_turning(soon));

        let there = now + GLIDE_SETTLES_IN + RISE_PER_LINE * 8;
        assert!(model.rise(7, there).abs() < px(f32::EPSILON));
        assert!((model.arrival(there) - 1.0).abs() < f32::EPSILON);
        assert!(!model.is_turning(there));
    }

    #[test]
    fn a_look_that_has_only_just_started_is_kept_quiet_before_it_is_announced() {
        let now = Instant::now();
        let mut model = model();
        model.look = Look::Searching;
        model.asked_at = now;

        assert!(model.looks_quietly(now));
        assert!(model.is_turning(now));
        assert!(!model.looks_quietly(now + LOOKS_QUIETLY_FOR));

        model.look = Look::Missing;
        assert!(!model.looks_quietly(now));
    }

    #[test]
    fn a_blank_line_between_verses_costs_a_written_line_no_standing() {
        let now = Instant::now();
        let mut model = holding(&["one", "two", "", "three", "four"]);
        model.follow_the_track(at(1), now);
        let settled = now + TURN;

        assert!((model.standing(3, settled) - Falloff::Around.ahead(1)).abs() < f32::EPSILON);
        assert!((model.standing(4, settled) - Falloff::Around.ahead(2)).abs() < f32::EPSILON);
        assert!(model.standing(0, settled).abs() < f32::EPSILON);
    }

    #[test]
    fn a_set_that_has_had_its_last_word_goes_out_without_the_pane_leaving_it() {
        let now = Instant::now();
        let mut model = verse(4);

        model.follow_the_track(at(3), now);
        assert!((model.standing(3, now + TURN) - LIT).abs() < f32::EPSILON);
        assert!((model.lead(3, now + TURN) - 1.0).abs() < f32::EPSILON);

        let over = now + TURN;
        model.follow_the_track(at(60), over);
        assert_eq!(model.read_at(), Some(3));
        assert!(model.standing(3, over + TURN).abs() < f32::EPSILON);
        assert!(model.lead(3, over + TURN).abs() < f32::EPSILON);
    }

    #[test]
    fn a_spent_set_is_still_readable_where_the_whole_of_it_was_asked_for() {
        let now = Instant::now();
        let mut model = verse(4);
        model.read_as(Reading::Whole);
        spread_settled(&mut model);

        model.follow_the_track(at(60), now);
        let standing = model.standing(3, now + TURN);

        assert!(standing > 0.0);
        assert!(standing < Falloff::Across.ahead(1));
    }

    #[test]
    fn a_gap_reads_at_the_line_it_is_waiting_on_rather_than_the_one_just_sung() {
        let now = Instant::now();
        let mut model = holding(&["one", "two"]);

        model.follow_the_track(at(1), now);
        assert_eq!(model.read_at(), Some(1));

        let mut wider = LyricsModel::new(Arc::new(Lyricists::unsourced()));
        let source = resonate_core::SourceId::new("held").expect("a lowercase name");
        wider.look = Look::Found(Arc::new(
            Lyrics::synced(
                source,
                vec![
                    LyricLine::sung(at(0), "one"),
                    LyricLine::sung(at(60), "two"),
                ],
            )
            .expect("every line is timed"),
        ));
        wider.hold();

        wider.follow_the_track(at(30), now);
        assert_eq!(wider.read_at(), Some(1));
        assert!(wider.standing(0, now + TURN).abs() < f32::EPSILON);
    }

    #[test]
    fn an_unsynced_set_stands_every_line_alike_however_far_the_track_has_run() {
        let now = Instant::now();
        let source = resonate_core::SourceId::new("held").expect("a lowercase name");
        let mut model = model();
        model.look = Look::Found(Arc::new(Lyrics::plain(
            source,
            vec!["one".to_owned(), "two".to_owned()],
        )));
        model.hold();

        model.follow_the_track(at(30), now);

        assert_eq!(model.read_at(), None);
        assert!((model.standing(0, now + TURN) - ADRIFT).abs() < f32::EPSILON);
        assert!((model.standing(1, now + TURN) - ADRIFT).abs() < f32::EPSILON);
    }

    #[test]
    fn a_pane_is_drawn_nowhere_until_the_layout_it_was_measured_from_is_the_one_it_has() {
        let now = Instant::now();
        let mut model = verse(4);

        assert!(!model.is_placed(), "a fresh look has not been placed yet");
        assert!(
            model.place(now),
            "an unplaced pane is still asking for frames"
        );
        assert!(
            !model.is_placed(),
            "nothing is known of the pane's height yet"
        );

        model.follow_the_track(at(2), now);
        assert!(
            (model.standing(2, now) - LIT).abs() < f32::EPSILON,
            "an unplaced pane arrives at its standing rather than fading into it"
        );
    }

    #[test]
    fn a_track_change_puts_the_pane_back_to_being_placed_rather_than_gliding_from_the_last_one() {
        let mut model = verse(4);
        model.placed = true;
        model.laid_out = Some(Size {
            width: px(900.0),
            height: px(200.0),
        });

        model.rewind();

        assert!(!model.is_placed());
        assert_eq!(model.laid_out, None);
        assert_eq!(model.read_at(), None);
    }

    #[test]
    fn a_target_that_drifts_under_a_line_bends_the_glide_rather_than_starting_it_again() {
        let now = Instant::now();
        let mut model = verse(4);
        model.placed = true;

        model.glide_to(px(400.0), now);
        let half = now + TURN / 2;
        assert!(model.glide(half));

        model.glide_to(px(412.0), half);
        assert!(
            !model
                .glide
                .expect("a glide is in flight")
                .clock
                .settled(half)
        );

        model.glide_to(px(-900.0), half);
        assert!(
            model
                .glide
                .expect("a glide is in flight")
                .clock
                .through(half)
                .abs()
                < f32::EPSILON,
            "a target a whole screen away should start a fresh glide"
        );
    }

    #[test]
    fn the_line_a_gap_is_waiting_on_comes_up_as_the_wait_runs_out() {
        assert!((rising(0.3, 0.0) - 0.3).abs() < f32::EPSILON);
        assert!((rising(0.3, 1.0) - LIT).abs() < f32::EPSILON);
        assert!(rising(0.3, 0.5) > 0.3);
        assert!(rising(0.3, 0.5) < LIT);
    }

    #[test]
    fn a_scroll_of_your_own_holds_the_pane_where_you_left_it_and_then_lets_go() {
        let now = Instant::now();
        let mut model = model();

        assert!(model.following(now));

        model.led_by_hand(now);
        assert!(!model.following(now));
        assert!(!model.following(now + HANDS_OFF / 2));
        assert!(model.following(now + HANDS_OFF));

        model.led_by_hand(now);
        model.follow_again();
        assert!(model.following(now));
    }

    #[test]
    fn the_bar_is_shown_only_for_a_moment_after_a_scroll_of_your_own() {
        let now = Instant::now();
        let mut model = model();
        assert!(!model.moved_by_hand_lately(now));

        model.led_by_hand(now);
        assert!(model.moved_by_hand_lately(now));
        assert!(model.is_turning(now));
        assert!(model.moved_by_hand_lately(now + BAR_LINGERS / 2));
        assert!(!model.moved_by_hand_lately(now + BAR_LINGERS));
    }
    fn pane() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(300.0), px(60.0)),
            size: Size {
                width: px(1000.0),
                height: px(700.0),
            },
        }
    }

    fn line_at(top: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(440.0), px(top)),
            size: Size {
                width: px(720.0),
                height: px(48.0),
            },
        }
    }

    #[test]
    fn the_sheet_opens_out_only_where_the_pointer_is_on_the_words() {
        let column = px(720.0);
        let read = Some(line_at(380.0));
        let middle = point(px(800.0), px(400.0));

        assert!(
            near_the_words(middle, pane(), column, read),
            "the pointer on the line being sung did not open the sheet out"
        );
        assert!(
            !near_the_words(point(px(340.0), px(400.0)), pane(), column, read),
            "the gutter beside the column opened the sheet out"
        );
        assert!(
            !near_the_words(point(px(800.0), px(90.0)), pane(), column, read),
            "the top of the pane, far from the read line, opened the sheet out"
        );
        assert!(
            !near_the_words(point(px(800.0), px(740.0)), pane(), column, read),
            "the foot of the pane, far from the read line, opened the sheet out"
        );
        assert!(
            !near_the_words(point(px(100.0), px(400.0)), pane(), column, read),
            "a pointer outside the pane altogether opened the sheet out"
        );
    }

    #[test]
    fn a_sheet_with_no_line_in_play_opens_out_anywhere_down_its_column() {
        let column = px(720.0);

        assert!(
            near_the_words(point(px(800.0), px(100.0)), pane(), column, None),
            "a sheet reading no line refused the column's top"
        );
        assert!(
            !near_the_words(point(px(1_290.0), px(100.0)), pane(), column, None),
            "a sheet reading no line still answered outside its column"
        );
    }

    #[test]
    fn a_pane_that_has_never_been_laid_out_opens_nothing() {
        let unmeasured = Bounds {
            origin: point(px(0.0), px(0.0)),
            size: Size {
                width: px(0.0),
                height: px(0.0),
            },
        };

        assert!(!near_the_words(
            point(px(0.0), px(0.0)),
            unmeasured,
            px(0.0),
            None
        ));
    }
}
