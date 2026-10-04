use std::{
    f32::consts::TAU,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    Bounds, Context, Pixels, Point, ScrollHandle, SharedString, Size, Task, ease_in_out, point, px,
};
use resonate_core::{Frames, TrackId};
use resonate_engine::{PlaybackState, PlayerState, Seeks, StreamDigest};
use resonate_lyrics::{Lyricists, Lyrics, Sweep, Timing, Voice, Waiting, Wanted};

use crate::{models::Scale, theme};

pub(crate) fn near_the_words(
    pointer: Point<Pixels>,
    pane: Bounds<Pixels>,
    column: Pixels,
    slack: Pixels,
) -> bool {
    if pane.size.height <= px(0.0) || !pane.contains(&pointer) {
        return false;
    }

    let middle = pane.left() + pane.size.width / 2.0;

    (pointer.x - middle).abs() <= column / 2.0 + slack
}

fn column_within(pane: Pixels, measures: Measures) -> Pixels {
    let room = pane - measures.gutter * 2.0;

    room.clamp(px(0.0), measures.column)
}

const DRAWN_WITHIN_PANES: f32 = 1.0;

const TURN: Duration = Duration::from_millis(420);

const GLIDE: Duration = Duration::from_millis(600);

const LAUNCHED_AT_MOST: f32 = 4.0;

const LAG_PER_LINE: Duration = Duration::from_millis(14);

const LAGS_AT_MOST: usize = 6;

const ARRIVES_IN: Duration = Duration::from_millis(560);

const RISE: Duration = Duration::from_millis(600);

const RISES_FROM: f32 = 22.0;

const RISE_PER_LINE: Duration = Duration::from_millis(40);

const RISES_AT_MOST: usize = 8;

const LOOKS_QUIETLY_FOR: Duration = Duration::from_millis(450);

const BREATH: Duration = Duration::from_millis(2400);

const DRIFTS_AT_MOST: Duration = Duration::from_millis(250);

const STEERED_OVER: Duration = Duration::from_millis(550);

const HANDS_OFF: Duration = Duration::from_secs(6);

const BAR_LINGERS: Duration = Duration::from_millis(1_500);

const SETTLED: Pixels = px(0.5);

const LIT: f32 = 1.0;

const ADRIFT: f32 = 0.45;

const LEADING: f32 = 1.28;

const LABEL_LEADING: f32 = 1.5;

const LINE_PADDING: f32 = 8.0;

const LINE_SPACING: f32 = 4.0;

const PLAIN_MARGIN: f32 = 48.0;

const END_GAP: f32 = 12.0;

const END_PADDING: f32 = 16.0;

const PANE_AT_RESTING_SIZE: Size<Pixels> = Size {
    width: px(784.0),
    height: px(600.0),
};

const GROWS_AT_MOST: f32 = 2.5;

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

    fn spent(self, read: Option<usize>, line: usize) -> f32 {
        match (self, read) {
            (Self::Around, Some(read)) if line >= read => self.ahead(line - read + 1),
            (Self::Around, _) => 0.0,
            (Self::Across, Some(read)) => self.ahead(read.abs_diff(line).max(1)),
            (Self::Across, None) => self.ahead(usize::MAX),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    At(usize),
    Evenly,
    Spent(Option<usize>),
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
struct FadingBreath {
    line: usize,
    started: Instant,
}

impl FadingBreath {
    fn through(self, now: Instant) -> f32 {
        1.0 - (now.saturating_duration_since(self.started).as_secs_f32() / TURN.as_secs_f32())
            .clamp(0.0, 1.0)
    }

    fn settled(self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= TURN
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heard {
    pub track: TrackId,
    pub seeks: Seeks,
    pub at: Duration,
    pub playing: bool,
}

impl Heard {
    pub fn of(state: &PlayerState) -> Option<Self> {
        let current = state.current?;
        Some(Self {
            track: current.id,
            seeks: state.seeks,
            at: current.position.to_duration(current.source.rate),
            playing: state.playback == PlaybackState::Playing,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Clock {
    track: TrackId,
    seeks: Seeks,
    from: Duration,
    at: Instant,
}

impl Clock {
    fn started(heard: Heard, now: Instant) -> Self {
        Self {
            track: heard.track,
            seeks: heard.seeks,
            from: heard.at,
            at: now,
        }
    }

    fn keeps_time_with(self, heard: Heard) -> bool {
        heard.playing && self.track == heard.track && self.seeks == heard.seeks
    }

    fn runs_to(self, now: Instant) -> Duration {
        self.from
            .saturating_add(now.saturating_duration_since(self.at))
    }

    fn steered_by(self, heard: Duration, now: Instant) -> Duration {
        let ran = self.runs_to(now);
        let over =
            now.saturating_duration_since(self.at).as_secs_f32() / STEERED_OVER.as_secs_f32();
        let share = 1.0 - (-over).exp();
        let steered = if heard >= ran {
            ran.saturating_add((heard - ran).mul_f32(share))
        } else {
            ran.saturating_sub((ran - heard).mul_f32(share))
        };

        steered.max(self.from)
    }
}

#[derive(Clone, Copy)]
struct Breathing {
    line: usize,
    started: Instant,
}

impl Breathing {
    fn opacity(self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();

        ease_in_out((elapsed / TURN.as_secs_f32()).clamp(0.0, 1.0))
    }

    fn settled(self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= TURN
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breath {
    pub through: f32,
    pub opacity: f32,
}

fn share_of(elapsed: Duration, span: Duration) -> f32 {
    (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0)
}

fn landed(through: f32) -> f32 {
    let left = 1.0 - through;

    3.0f32.mul_add(through, 1.0).mul_add(-left.powi(3), 1.0)
}

fn landing_pace(through: f32) -> f32 {
    12.0 * through * (1.0 - through).powi(2)
}

fn launched(through: f32) -> f32 {
    through * (1.0 - through).powi(3)
}

fn launch_pace(through: f32) -> f32 {
    (1.0 - through).powi(2) * 4.0f32.mul_add(-through, 1.0)
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Growth(f32);

impl Growth {
    pub const NONE: Self = Self(1.0);

    pub fn of(pane: Size<Pixels>) -> Self {
        let across = pane.width / PANE_AT_RESTING_SIZE.width;
        let down = pane.height / PANE_AT_RESTING_SIZE.height;
        let grown = across.min(down);

        if grown.is_finite() {
            Self(grown.clamp(1.0, GROWS_AT_MOST))
        } else {
            Self::NONE
        }
    }

    fn grown(self, length: f32) -> Pixels {
        px(length * self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measures {
    pub words: Pixels,
    pub leading: Pixels,
    pub label_words: Pixels,
    pub label: Pixels,
    pub padding: Pixels,
    pub spacing: Pixels,
    pub breath: Pixels,
    pub pause: Pixels,
    pub margin: Pixels,
    pub column: Pixels,
    pub gutter: Pixels,
    pub pad: Pixels,
    pub dot: Pixels,
    pub dot_gap: Pixels,
    pub end_rule: Pixels,
    pub end_gap: Pixels,
    pub end_padding: Pixels,
    pub dissolve: Pixels,
    pub rise: Pixels,
}

impl Measures {
    pub fn at(scale: Scale, growth: Growth) -> Self {
        let words = scale.snapped(growth.grown(theme::text_lyric()));
        let label_words = scale.snapped(growth.grown(theme::text_xs()));

        Self {
            words,
            leading: scale.snapped(words * LEADING),
            label_words,
            label: scale.snapped(label_words * LABEL_LEADING),
            padding: scale.snapped(growth.grown(LINE_PADDING)),
            spacing: scale.snapped(growth.grown(LINE_SPACING)),
            breath: scale.snapped(growth.grown(theme::lyric_breath())),
            pause: scale.snapped(growth.grown(theme::lyric_break())),
            margin: scale.snapped(growth.grown(PLAIN_MARGIN)),
            column: growth.grown(theme::lyric_column()),
            gutter: growth.grown(theme::lyric_gutter()),
            pad: growth.grown(theme::lyric_pad()),
            dot: growth.grown(theme::lyric_dot()),
            dot_gap: growth.grown(theme::lyric_dot_gap()),
            end_rule: growth.grown(theme::lyric_end_rule()),
            end_gap: growth.grown(END_GAP),
            end_padding: scale.snapped(growth.grown(END_PADDING)),
            dissolve: growth.grown(theme::lyric_edge()),
            rise: growth.grown(RISES_FROM),
        }
    }
}

struct Sheet {
    text: Arc<[SharedString]>,
    moments: Arc<[Option<Duration>]>,
    voices: Arc<[Voice]>,
    breathes: Arc<[bool]>,
    written: Arc<[usize]>,
}

impl Default for Sheet {
    fn default() -> Self {
        Self {
            text: Arc::from([] as [SharedString; 0]),
            moments: Arc::from([] as [Option<Duration>; 0]),
            voices: Arc::from([] as [Voice; 0]),
            breathes: Arc::from([] as [bool; 0]),
            written: Arc::from([] as [usize; 0]),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Leg {
    was: Pixels,
    lands: Pixels,
    launch: Pixels,
    started: Instant,
}

impl Leg {
    fn after(self, elapsed: Duration) -> Pixels {
        let through = share_of(elapsed, GLIDE);

        self.was + (self.lands - self.was) * landed(through) + self.launch * launched(through)
    }

    fn pace_after(self, elapsed: Duration) -> Pixels {
        let through = share_of(elapsed, GLIDE);

        (self.lands - self.was) * landing_pace(through) + self.launch * launch_pace(through)
    }

    fn elapsed(self, now: Instant) -> Duration {
        now.saturating_duration_since(self.started)
    }
}

#[derive(Clone, Copy, Debug)]
struct Glide {
    leg: Leg,
    before: Option<Leg>,
}

impl Glide {
    fn from_rest(was: Pixels, lands: Pixels, now: Instant) -> Self {
        Self {
            leg: Leg {
                was,
                lands,
                launch: px(0.0),
                started: now,
            },
            before: None,
        }
    }

    fn at(self, now: Instant) -> Pixels {
        self.leg.after(self.leg.elapsed(now))
    }

    fn lagging(self, lag: Duration, now: Instant) -> Pixels {
        let into = self.leg.elapsed(now);
        match self.before {
            Some(before) if into < lag => before.after(before.elapsed(now).saturating_sub(lag)),
            _ => self.leg.after(into.saturating_sub(lag)),
        }
    }

    fn redirected(self, lands: Pixels, now: Instant) -> Self {
        let into = self.leg.elapsed(now);
        let was = self.leg.after(into);
        let reach = (lands - was).abs() * LAUNCHED_AT_MOST;
        let launch = self.leg.pace_after(into).clamp(-reach, reach);

        Self {
            leg: Leg {
                was,
                lands,
                launch,
                started: now,
            },
            before: Some(self.leg),
        }
    }

    fn settled(self, now: Instant) -> bool {
        self.leg.elapsed(now) >= GLIDE + lagged(LAGS_AT_MOST, LAG_PER_LINE, LAGS_AT_MOST)
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
    scale: Scale,
    laid_out: Option<(Size<Pixels>, Measures)>,
    steady: bool,
    placed: bool,
    arrived: Option<Instant>,
    turn: Turn<Reads>,
    light: Turn<[Option<usize>; 2]>,
    spread: Turn<Falloff>,
    glide: Option<Glide>,
    hand_at: Option<Instant>,
    breathing_for: Option<Breathing>,
    fading_breath: Option<FadingBreath>,
    clock: Option<Clock>,
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
            scale: Scale::ONE,
            laid_out: None,
            steady: false,
            placed: false,
            arrived: None,
            turn: Turn::still(Reads::Evenly),
            light: Turn::still([None; 2]),
            spread: Turn::still(Falloff::Around),
            glide: None,
            hand_at: None,
            breathing_for: None,
            fading_breath: None,
            clock: None,
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

    pub fn voices(&self) -> Arc<[Voice]> {
        Arc::clone(&self.sheet.voices)
    }

    pub fn breathes(&self) -> Arc<[bool]> {
        Arc::clone(&self.sheet.breathes)
    }

    pub fn has_two_voices(&self) -> bool {
        self.sheet.voices.contains(&Voice::Two)
    }

    pub fn has_a_source(&self) -> bool {
        self.lyricists.has_a_source()
    }

    pub fn waiting_at(&self, position: Duration) -> Option<Waiting> {
        self.found()?.waiting_at(position)
    }

    pub fn keep_time(&mut self, heard: Heard, now: Instant) -> Duration {
        let running = self
            .clock
            .filter(|clock| clock.keeps_time_with(heard))
            .filter(|clock| clock.runs_to(now).abs_diff(heard.at) <= DRIFTS_AT_MOST);
        let Some(clock) = running else {
            self.clock = heard.playing.then(|| Clock::started(heard, now));
            return heard.at;
        };
        let kept = clock.steered_by(heard.at, now);
        self.clock = Some(Clock {
            from: kept,
            at: now,
            ..clock
        });

        kept
    }

    pub fn in_play(&self, position: Duration) -> [Option<usize>; 2] {
        self.found()
            .map_or([None; 2], |lyrics| lyrics.voices_in_play(position))
    }

    pub fn sweep(&self, index: usize, position: Duration) -> Option<Sweep> {
        self.found()?.lines().get(index)?.sweep_at(position)
    }

    pub fn breath_at(
        &self,
        waiting: Option<Waiting>,
        index: usize,
        now: Instant,
    ) -> Option<Breath> {
        if let Some(waiting) = waiting.filter(|waiting| waiting.next == index) {
            let opacity = self
                .breathing_for
                .filter(|breathing| breathing.line == index)
                .map_or(1.0, |breathing| breathing.opacity(now));
            return Some(Breath {
                through: waiting.through,
                opacity,
            });
        }

        self.fading_breath
            .filter(|breath| breath.line == index && !breath.settled(now))
            .map(|breath| Breath {
                through: 1.0,
                opacity: breath.through(now),
            })
    }

    fn is_breathing_in_or_out(&self, now: Instant) -> bool {
        self.breathing_for
            .is_some_and(|breathing| !breathing.settled(now))
            || self
                .fading_breath
                .is_some_and(|breath| !breath.settled(now))
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
        self.spread_out(Instant::now());
    }

    pub fn open_out(&mut self, opened: bool) -> bool {
        let was = self.opened_out;
        self.opened_out = opened;
        self.spread_out(Instant::now()) || was != opened
    }

    pub fn shows_every_line(&self, now: Instant) -> bool {
        self.opened_out || matches!(self.reading, Reading::Whole) || !self.following(now)
    }

    fn falloff(&self, now: Instant) -> Falloff {
        if self.shows_every_line(now) {
            Falloff::Across
        } else {
            Falloff::Around
        }
    }

    fn spread_out(&mut self, now: Instant) -> bool {
        self.spread.onto(self.falloff(now), now)
    }

    pub const fn read_at(&self) -> Option<usize> {
        self.read_at
    }

    pub fn follow_the_track(&mut self, position: Duration, now: Instant) {
        self.spread_out(now);
        let Some(lyrics) = self.found() else {
            self.turn.onto(Reads::Evenly, now);
            self.light.onto([None; 2], now);
            return;
        };
        let waiting = lyrics.waiting_at(position);
        let lit = lyrics.voices_in_play(position);
        let ended = lyrics.has_ended(position).then(|| self.end_of_the_sheet());
        let read_at = waiting
            .map(|waiting| waiting.next)
            .or(ended)
            .or_else(|| lyrics.line_at(position));
        let timing = lyrics.timing();
        match waiting {
            Some(waiting) => {
                if self
                    .breathing_for
                    .is_none_or(|breathing| breathing.line != waiting.next)
                {
                    self.breathing_for = Some(Breathing {
                        line: waiting.next,
                        started: now,
                    });
                }
                self.fading_breath = None;
            }
            None => {
                self.fading_breath = self
                    .breathing_for
                    .take()
                    .filter(|breathing| lit.contains(&Some(breathing.line)))
                    .map(|breathing| FadingBreath {
                        line: breathing.line,
                        started: now,
                    })
                    .or_else(|| self.fading_breath.filter(|breath| !breath.settled(now)));
            }
        }
        let reads = if timing == Timing::Unsynced {
            Reads::Evenly
        } else {
            lit.into_iter()
                .flatten()
                .max()
                .or(ended)
                .or_else(|| waiting.is_none().then_some(read_at).flatten())
                .map_or(Reads::Spent(read_at), Reads::At)
        };

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

        let focus = self.spread.blended(now, |falloff| {
            self.turn.blended(now, |reads| match reads {
                Reads::At(sung) => falloff.between(self.ordinal(sung), line),
                Reads::Evenly => ADRIFT,
                Reads::Spent(read) => falloff.spent(read.map(|read| self.ordinal(read)), line),
            })
        });

        focus.max(self.lead(index, now))
    }

    pub fn lead(&self, index: usize, now: Instant) -> f32 {
        self.light
            .blended(now, |lit| f32::from(u8::from(lit.contains(&Some(index)))))
    }

    pub fn inset(&self, index: usize, now: Instant) -> Pixels {
        let rise = self.rise(index, now);
        let Some(glide) = self.glide else {
            return self.scale.snapped(rise);
        };
        let lines_ahead = self.read_at.map_or(0, |read| {
            self.ordinal(index).saturating_sub(self.ordinal(read))
        });
        let lag = lagged(lines_ahead, LAG_PER_LINE, LAGS_AT_MOST);

        self.scale.snapped(glide.lagging(lag, now) + rise) - self.scale.snapped(glide.at(now))
    }

    fn rise(&self, index: usize, now: Instant) -> Pixels {
        let rises_from = self.measures().rise;
        let Some(arrived) = self.arrived else {
            return rises_from;
        };
        let read = self.read_at.map_or(0, |read| self.ordinal(read));
        let away = self.ordinal(index).abs_diff(read);
        let lag = lagged(away, RISE_PER_LINE, RISES_AT_MOST);
        let risen = now.saturating_duration_since(arrived).saturating_sub(lag);

        rises_from * (1.0 - landed(share_of(risen, RISE)))
    }

    pub fn arrival(&self, now: Instant) -> f32 {
        let Some(arrived) = self.arrived else {
            return 0.0;
        };

        ease_in_out(share_of(now.saturating_duration_since(arrived), ARRIVES_IN))
    }

    pub fn breath(&self, now: Instant) -> f32 {
        let since = self.arrived.map_or(Duration::ZERO, |arrived| {
            now.saturating_duration_since(arrived)
        });
        let phase = since.as_secs_f32() / BREATH.as_secs_f32();

        (phase * TAU).sin().mul_add(0.5, 0.5)
    }

    pub fn end_of_the_sheet(&self) -> usize {
        self.sheet.text.len()
    }

    pub fn has_ended(&self, position: Duration) -> bool {
        self.found()
            .is_some_and(|lyrics| lyrics.has_ended(position))
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

    pub fn resting_height(&self, index: usize) -> Option<Pixels> {
        let pane = self.scroll.bounds();
        if !self.steady || pane.size.height <= px(0.0) {
            return None;
        }
        let line = self.scroll.bounds_for_item(index)?;
        let scrolled = self.scroll.offset().y;
        let reach = pane.size.height * DRAWN_WITHIN_PANES;
        let far_above = line.bottom() + scrolled < pane.top() - reach;
        let far_below = line.top() + scrolled > pane.bottom() + reach;

        (far_above || far_below).then_some(line.size.height)
    }

    pub fn pane_height(&self) -> Pixels {
        self.scroll.bounds().size.height
    }

    pub fn column_width(&self) -> Pixels {
        column_within(self.scroll.bounds().size.width, self.measures())
    }

    pub fn edge(&self) -> Pixels {
        self.scale.snapped(self.pane_height() / 2.0)
    }

    pub fn measures(&self) -> Measures {
        Measures::at(self.scale, Growth::of(self.scroll.bounds().size))
    }

    pub fn scaled_by(&mut self, scale: Scale) {
        self.scale = scale;
    }

    pub fn opened_by(&self, pointer: Point<Pixels>) -> bool {
        near_the_words(
            pointer,
            self.scroll.bounds(),
            self.column_width(),
            self.measures().pad,
        )
    }

    pub const fn is_placed(&self) -> bool {
        self.placed
    }

    pub fn place(&mut self, now: Instant) -> bool {
        let laid_out = Some((self.scroll.bounds().size, self.measures()));
        self.steady = self.laid_out == laid_out;
        self.laid_out = laid_out;

        if self.following(now)
            && let Some(lands) = self.landing()
        {
            if self.placed {
                self.glide_to(lands, now);
            } else if self.steady {
                self.scroll.set_offset(point(self.scroll.offset().x, lands));
                self.glide = None;
                self.placed = true;
                self.arrived = Some(now);
            }
        }

        !self.placed || self.glide(now)
    }

    pub fn landing(&self) -> Option<Pixels> {
        match self.read_at {
            Some(line) => {
                let room = if self.sheet.breathes.get(line).copied().unwrap_or(false) {
                    self.measures().breath
                } else {
                    px(0.0)
                };
                Some(self.scale.snapped(self.centre_of(line)? - room / 2.0))
            }
            None if self.placed && !self.is_synced() => None,
            None => Some(px(0.0)),
        }
    }

    pub fn glide_to(&mut self, lands: Pixels, now: Instant) {
        self.glide = match self.glide {
            None => Some(Glide::from_rest(self.scroll.offset().y, lands, now)),
            Some(glide) if (glide.leg.lands - lands).abs() < SETTLED => return,
            Some(glide) => Some(glide.redirected(lands, now)),
        };
    }

    pub fn glide(&mut self, now: Instant) -> bool {
        let Some(glide) = self.glide else {
            return false;
        };
        self.scroll.set_offset(point(
            self.scroll.offset().x,
            self.scale.snapped(glide.at(now)),
        ));

        !glide.settled(now)
    }

    pub fn is_turning(&self, now: Instant) -> bool {
        !self.turn.clock.settled(now)
            || !self.light.clock.settled(now)
            || !self.spread.clock.settled(now)
            || self.is_arriving(now)
            || self.looks_quietly(now)
            || self.moved_by_hand_lately(now)
            || self.is_breathing_in_or_out(now)
    }

    fn is_arriving(&self, now: Instant) -> bool {
        self.arrived.is_some_and(|arrived| {
            now.saturating_duration_since(arrived)
                < RISE + lagged(RISES_AT_MOST, RISE_PER_LINE, RISES_AT_MOST)
        })
    }

    pub fn led_by_hand(&mut self, now: Instant) {
        self.hand_at = Some(now);
        self.glide = None;
        self.spread_out(now);
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
        self.spread_out(Instant::now());
    }

    pub fn follow(&mut self, asked: Option<Asked>, wanted: Option<Wanted>, cx: &mut Context<Self>) {
        if self.asked_about == asked {
            return;
        }
        let moved_on = self.asked_about.map(|held| held.track) != asked.map(|asked| asked.track);
        self.asked_about = asked;

        let Some(wanted) = wanted else {
            self._find = Task::ready(());
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
                let look = match found {
                    Ok(Some(lyrics)) => Look::Found(Arc::new(lyrics)),
                    Ok(None) => Look::Missing,
                    Err(error) => {
                        tracing::warn!(%error, "no lyric provider answered");
                        Look::Refused(SharedString::from(error.to_string()))
                    }
                };
                if moved_on || !this.already_shows(&look) {
                    this.rewind();
                }
                this.look = look;
                this.hold();
                cx.notify();
            });
            let _ = landed;
        });
    }

    fn already_shows(&self, look: &Look) -> bool {
        match (self.found(), look) {
            (Some(held), Look::Found(landed)) => held == landed.as_ref(),
            _ => false,
        }
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
        self.steady = false;
        self.placed = false;
        self.arrived = None;
        self.turn = Turn::still(Reads::Evenly);
        self.light = Turn::still([None; 2]);
        self.glide = None;
        self.hand_at = None;
        self.breathing_for = None;
        self.fading_breath = None;
        self.opened_out = false;
        self.spread = Turn::still(self.falloff(Instant::now()));
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
        let voices = lyrics.lines().iter().map(|line| line.voice).collect();
        let breathes = (0..lyrics.lines().len())
            .map(|line| lyrics.breathes_before(line))
            .collect();
        let mut standing = 0;
        let mut written: Vec<usize> = lyrics
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
        written.push(standing);

        Self {
            text,
            moments,
            voices,
            breathes,
            written: written.into(),
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

    #[test]
    fn a_lookup_answering_what_is_already_shown_leaves_the_sheet_where_it_is() {
        let model = holding(&["one", "two"]);
        let Look::Found(shown) = model.look.clone() else {
            panic!("the model holds a sheet");
        };

        assert!(
            model.already_shows(&Look::Found(Arc::new(shown.as_ref().clone()))),
            "the same sheet landing again was read as another"
        );
        assert!(!model.already_shows(&holding(&["three"]).look));
        assert!(!model.already_shows(&Look::Missing));
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

    #[test]
    fn a_plain_sheet_is_placed_at_its_top_once_and_then_left_where_it_was_read_to() {
        let source = resonate_core::SourceId::new("held").expect("a lowercase name");
        let mut model = model();
        model.look = Look::Found(Arc::new(Lyrics::plain(
            source,
            vec!["one".to_owned(), "two".to_owned()],
        )));
        model.hold();

        assert_eq!(model.landing(), Some(px(0.0)));

        model.placed = true;

        assert_eq!(
            model.landing(),
            None,
            "a plain sheet read down the page was pulled back up"
        );

        let mut synced = verse(3);
        synced.placed = true;

        assert_eq!(
            synced.landing(),
            Some(px(0.0)),
            "a synced sheet before its first line did not wait at the top"
        );
    }

    fn verse(lines: usize) -> LyricsModel {
        let sung: Vec<&str> = (0..lines).map(|_| "a line").collect();

        holding(&sung)
    }

    fn spread_settled(model: &mut LyricsModel) {
        model.spread = Turn::still(model.falloff(Instant::now()));
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
        assert!(model.shows_every_line(now));
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

    const FRAME_AT_240_HZ: Duration = Duration::from_micros(4_167);

    const LAST_STEP_WITHIN: Duration = Duration::from_millis(60);

    fn frames_through(span: Duration) -> impl Iterator<Item = Duration> {
        let frames = u32::try_from(span.as_nanos() / FRAME_AT_240_HZ.as_nanos() + 2).expect("few");

        (0..=frames).map(|frame| FRAME_AT_240_HZ * frame)
    }

    #[test]
    fn a_glide_runs_from_rest_to_its_landing_and_never_passes_it() {
        assert!(landed(0.0).abs() < f32::EPSILON);
        assert!((landed(1.0) - 1.0).abs() < f32::EPSILON);
        assert!(landing_pace(0.0).abs() < f32::EPSILON);
        assert!(landing_pace(1.0).abs() < f32::EPSILON);
        assert!(launched(0.0).abs() < f32::EPSILON);
        assert!(launched(1.0).abs() < f32::EPSILON);
        assert!((launch_pace(0.0) - 1.0).abs() < f32::EPSILON);

        let mut was = 0.0;
        for step in 1..=1_000 {
            let through = landed(step as f32 / 1_000.0);
            assert!(through >= was && through <= 1.0);
            was = through;
        }
    }

    #[test]
    fn a_glide_lands_on_a_whole_pixel_with_its_last_step_close_behind_the_one_before() {
        let start = Instant::now();
        for travel in [
            px(22.0),
            px(66.0),
            px(120.0),
            px(400.0),
            px(-900.0),
            px(2_400.0),
        ] {
            let glide = Glide::from_rest(px(0.0), travel, start);
            let mut drawn = px(0.0);
            let mut stepped = [Duration::ZERO; 2];
            for elapsed in frames_through(GLIDE) {
                let now = Scale::ONE.snapped(glide.at(start + elapsed));
                assert!(
                    (travel - now).abs() <= (travel - drawn).abs(),
                    "a glide of {travel:?} turned back at {elapsed:?}"
                );
                if now != drawn {
                    stepped = [stepped[1], elapsed];
                }
                drawn = now;
            }

            assert_eq!(drawn, travel, "a glide of {travel:?} did not land");
            assert!(
                stepped[1] - stepped[0] <= LAST_STEP_WITHIN,
                "a glide of {travel:?} took its last pixel {:?} after the one before",
                stepped[1] - stepped[0]
            );
            assert!(glide.settled(start + GLIDE + LAG_PER_LINE * 6));
        }
    }

    #[test]
    fn a_new_landing_in_flight_carries_the_glide_on_without_a_jump_or_a_stop() {
        let start = Instant::now();
        let first = Glide::from_rest(px(0.0), px(-400.0), start);
        let turned = start + GLIDE / 3;
        let then = first.redirected(px(-466.0), turned);

        assert!((then.at(turned) - first.at(turned)).abs() < px(1e-3));
        for lines in 0..=LAGS_AT_MOST {
            let lag = lagged(lines, LAG_PER_LINE, LAGS_AT_MOST);
            assert!(
                (then.lagging(lag, turned) - first.lagging(lag, turned)).abs() < px(1e-3),
                "a line {lines} ahead jumped when the glide turned"
            );
        }

        let before = first.at(turned) - first.at(turned - FRAME_AT_240_HZ);
        let after = then.at(turned + FRAME_AT_240_HZ) - then.at(turned);
        assert!(
            (after - before).abs() < before.abs() * 0.1,
            "the glide went from {before:?} to {after:?} a frame as it turned"
        );

        let near = first.redirected(first.at(turned) - px(3.0), turned);
        for elapsed in frames_through(GLIDE) {
            assert!(near.at(turned + elapsed) >= near.leg.lands - px(1e-3));
        }
    }

    fn arrived_and_placed(lines: usize, scale: Scale, now: Instant) -> LyricsModel {
        let mut model = verse(lines);
        model.scaled_by(scale);
        model.placed = true;
        model.arrived = Some(now);

        model
    }

    #[test]
    fn every_line_is_drawn_on_whole_pixels_and_never_steps_back_through_a_glide() {
        let factor = 1.25;
        let scale = Scale::of(factor);
        let start = Instant::now() + RISE * 4;
        let mut model = arrived_and_placed(10, scale, Instant::now());

        model.follow_the_track(at(2), start);
        model.glide_to(px(-266.4), start);

        let mut drawn = [px(f32::MAX); 11];
        for elapsed in frames_through(GLIDE + LAG_PER_LINE * 8) {
            let now = start + elapsed;
            model.glide(now);
            let offset = model.scroll.offset().y;
            for (line, was) in drawn.iter_mut().enumerate() {
                let at = offset + model.inset(line, now);
                let pixels = f32::from(at) * factor;
                assert!(
                    (pixels - pixels.round()).abs() < 1e-3,
                    "line {line} was drawn {pixels} pixels down at {elapsed:?}"
                );
                assert!(
                    at <= *was + px(1e-3),
                    "line {line} stepped back from {was:?} to {at:?} at {elapsed:?}"
                );
                *was = at;
            }
        }

        assert!(!model.glide(start + GLIDE + LAG_PER_LINE * 8));
        let home = scale.snapped(px(-266.4));
        assert!(drawn.iter().all(|at| (*at - home).abs() < px(1e-3)));
    }

    #[test]
    fn a_line_further_ahead_lags_further_behind_the_glide_and_catches_up_once_it_settles() {
        let now = Instant::now() + RISE * 4;
        let midway = now + Duration::from_millis(120);
        let settled = now + GLIDE + LAG_PER_LINE * 8;
        let mut model = arrived_and_placed(8, Scale::ONE, Instant::now());

        model.follow_the_track(at(2), now);
        model.glide_to(px(-300.0), now);
        model.glide(midway);

        assert_eq!(model.inset(2, midway), px(0.0));
        assert!(model.inset(3, midway) > px(0.0));
        assert!(model.inset(5, midway) > model.inset(3, midway));
        assert_eq!(model.inset(0, midway), px(0.0));
        assert_eq!(model.inset(5, settled), px(0.0));
        assert!(model.glide(midway));
        assert!(!model.glide(settled));
    }

    #[test]
    fn a_set_rises_into_place_from_the_read_line_outwards_once_it_has_been_placed() {
        let now = Instant::now();
        let mut model = verse(8);

        assert_eq!(model.rise(3, now), model.measures().rise);
        assert!(model.arrival(now).abs() < f32::EPSILON);

        model.follow_the_track(at(3), now);
        model.placed = true;
        model.arrived = Some(now);

        let soon = now + Duration::from_millis(100);
        assert!(model.rise(3, soon) < model.rise(4, soon));
        assert!(model.rise(4, soon) < model.rise(7, soon));
        assert!(model.rise(2, soon) < model.rise(0, soon));
        assert!(model.arrival(soon) > 0.0);
        assert!(model.is_turning(soon));

        let there = now + RISE + RISE_PER_LINE * 8;
        assert_eq!(model.rise(7, there), px(0.0));
        assert!((model.arrival(there) - 1.0).abs() < f32::EPSILON);
        assert!(!model.is_turning(there));
    }

    #[test]
    fn a_line_is_held_open_only_once_two_layouts_running_were_measured_alike() {
        let now = Instant::now();
        let mut model = verse(4);

        model.place(now);
        assert!(!model.steady, "a first layout was taken as measured");

        model.place(now);
        assert!(model.steady);

        model.scaled_by(Scale::of(1.5));
        model.place(now);
        assert!(
            !model.steady,
            "lines were held open at heights measured at another scale"
        );

        model.place(now);
        assert!(model.steady);
    }

    #[test]
    fn the_measures_of_a_line_are_whole_device_pixels_at_any_scale() {
        for factor in [1.0, 1.25, 1.5, 1.75, 2.0, 2.25] {
            for growth in [1.0, 1.3, 1.77, GROWS_AT_MOST] {
                let measures = Measures::at(Scale::of(factor), Growth(growth));
                for length in [
                    measures.words,
                    measures.leading,
                    measures.label_words,
                    measures.label,
                    measures.padding,
                    measures.spacing,
                    measures.breath,
                    measures.pause,
                    measures.margin,
                    measures.end_padding,
                ] {
                    let pixels = f32::from(length) * factor;
                    assert!(
                        (pixels - pixels.round()).abs() < 1e-3,
                        "{length:?} is {pixels} pixels at {factor} grown {growth}"
                    );
                }
            }
        }
    }

    fn sized(width: f32, height: f32) -> Size<Pixels> {
        Size {
            width: px(width),
            height: px(height),
        }
    }

    #[test]
    fn the_sheet_grows_with_its_pane_by_the_tighter_of_its_two_sides() {
        assert_eq!(Growth::of(sized(0.0, 0.0)), Growth::NONE);
        assert_eq!(Growth::of(sized(600.0, 400.0)), Growth::NONE);
        assert_eq!(Growth::of(PANE_AT_RESTING_SIZE), Growth::NONE);
        assert_eq!(Growth::of(sized(784.0 * 3.0, 600.0 * 1.5)), Growth(1.5));
        assert_eq!(Growth::of(sized(784.0 * 1.5, 600.0 * 3.0)), Growth(1.5));
        assert_eq!(Growth::of(sized(3840.0, 2160.0)), Growth(GROWS_AT_MOST));

        let resting = Measures::at(Scale::ONE, Growth::NONE);
        let grown = Measures::at(Scale::ONE, Growth(2.0));

        assert_eq!(resting.words, px(theme::text_lyric()));
        assert_eq!(grown.words, resting.words * 2.0);
        assert_eq!(grown.column, resting.column * 2.0);
        assert_eq!(grown.gutter, resting.gutter * 2.0);
        assert_eq!(grown.spacing, resting.spacing * 2.0);
        assert_eq!(grown.dot, resting.dot * 2.0);
    }

    #[test]
    fn the_column_widens_with_the_pane_and_never_outruns_its_gutters() {
        let resting = Measures::at(Scale::ONE, Growth::NONE);
        let wide = sized(1568.0, 1200.0);
        let grown = Measures::at(Scale::ONE, Growth::of(wide));

        assert_eq!(column_within(px(900.0), resting), resting.column);
        assert_eq!(
            column_within(px(500.0), resting),
            px(500.0) - resting.gutter * 2.0
        );
        assert_eq!(column_within(wide.width, grown), resting.column * 2.0);
        assert!(column_within(wide.width, grown) + grown.gutter * 2.0 <= wide.width);
        assert_eq!(column_within(px(10.0), grown), px(0.0));
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

    fn a_pause(model: &LyricsModel, position: Duration, now: Instant) -> Option<Breath> {
        model.breath_at(model.waiting_at(position), 1, now)
    }

    #[test]
    fn the_dots_of_a_pause_fade_in_and_out_in_a_room_of_their_own_that_never_moves() {
        let source = resonate_core::SourceId::new("held").expect("a lowercase name");
        let mut model = model();
        model.look = Look::Found(Arc::new(
            Lyrics::synced(
                source,
                vec![
                    LyricLine::sung(at(0), "touch"),
                    LyricLine::sung(at(60), "see"),
                ],
            )
            .expect("every line is timed"),
        ));
        model.hold();
        assert_eq!(model.breathes().as_ref(), [true, true]);
        let frame = Duration::from_millis(16);
        let frames = u32::try_from(TURN.as_millis() / 16 + 1).expect("few frames");

        let began = Instant::now();
        model.follow_the_track(at(20), began);
        let mut was = 0.0;
        for step in 0..=frames {
            let now = began + frame * step;
            model.follow_the_track(at(20), now);
            let shown = a_pause(&model, at(20), now)
                .expect("the pause breathes")
                .opacity;
            assert!(
                shown >= was && shown - was < 0.1,
                "the dots popped in by {}",
                shown - was
            );
            was = shown;
        }
        assert!((was - 1.0).abs() < f32::EPSILON);

        let sung = began + TURN * 3;
        model.follow_the_track(at(60), sung);
        let mut was = 1.0;
        for step in 0..=frames {
            let now = sung + frame * step;
            model.follow_the_track(at(60), now);
            let shown = a_pause(&model, at(60), now).map_or(0.0, |breath| breath.opacity);
            assert!(
                shown <= was && was - shown < 0.1,
                "the dots popped out by {}",
                was - shown
            );
            was = shown;
        }
        assert_eq!(a_pause(&model, at(60), sung + TURN), None);
        assert_eq!(model.breathes().as_ref(), [true, true]);
    }

    fn heard(at: Duration, seeks: Seeks, playing: bool) -> Heard {
        Heard {
            track: TrackId::new(1).expect("a track id"),
            seeks,
            at,
            playing,
        }
    }

    #[test]
    fn the_clock_runs_smoothly_through_a_position_that_arrives_a_decoded_block_at_a_time() {
        let block = Duration::from_millis(93);
        let starts = at(10);
        for frame in [Duration::from_micros(16_667), FRAME_AT_240_HZ] {
            let mut model = model();
            let began = Instant::now();
            let frames = u32::try_from(at(4).as_nanos() / frame.as_nanos()).expect("few");

            let mut was = model.keep_time(heard(starts, Seeks::default(), true), began);
            assert_eq!(was, starts);
            for step in 1..=frames {
                let elapsed = frame * step;
                let blocks = u32::try_from(elapsed.as_nanos() / block.as_nanos()).expect("few");
                let sampled = starts + block * blocks;
                let kept = model.keep_time(heard(sampled, Seeks::default(), true), began + elapsed);
                let stepped = kept.saturating_sub(was);
                assert!(kept >= was, "the clock ran backwards at frame {step}");
                assert!(
                    stepped.abs_diff(frame) < frame / 4,
                    "a frame of {frame:?} moved the clock by {stepped:?}"
                );
                assert!((starts + elapsed).abs_diff(kept) < block);
                was = kept;
            }
        }
    }

    #[test]
    fn a_clock_steered_back_by_a_position_behind_it_slows_rather_than_running_backwards() {
        let mut model = model();
        let began = Instant::now();
        model.keep_time(heard(at(10), Seeks::default(), true), began);
        let stalled = began + Duration::from_millis(200);

        let kept = model.keep_time(
            heard(at(10) - Duration::from_millis(40), Seeks::default(), true),
            stalled,
        );

        assert!(kept >= at(10), "the clock ran back to {kept:?}");
        assert!(kept < at(10) + Duration::from_millis(200));
    }

    #[test]
    fn a_seek_or_a_pause_is_taken_as_it_stands() {
        let mut model = model();
        let frame = Duration::from_micros(16_667);
        let began = Instant::now();
        model.keep_time(heard(at(10), Seeks::default(), true), began);

        let later = began + frame * 241;
        let sought = model.keep_time(heard(at(90), Seeks::default().stepped(), true), later);
        assert_eq!(sought, at(90), "a seek was smoothed rather than followed");
        let paused = model.keep_time(heard(at(91), Seeks::default().stepped(), false), later);
        assert_eq!(paused, at(91));
        assert_eq!(
            model.keep_time(
                heard(at(91), Seeks::default().stepped(), false),
                later + at(5)
            ),
            at(91),
            "a paused clock ran on"
        );
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
        assert_eq!(model.read_at(), Some(model.end_of_the_sheet()));
        assert!(model.standing(3, over + TURN).abs() < f32::EPSILON);
        assert!(model.lead(3, over + TURN).abs() < f32::EPSILON);
        assert!(
            (model.standing(model.end_of_the_sheet(), over + TURN) - LIT).abs() < f32::EPSILON,
            "the end of the sheet was not lit once the last line had had its word"
        );
    }

    #[test]
    fn a_spent_set_is_still_readable_where_the_whole_of_it_was_asked_for() {
        let now = Instant::now();
        let mut model = verse(6);
        model.read_as(Reading::Whole);
        spread_settled(&mut model);

        model.follow_the_track(at(60), now);
        let settled = now + TURN;

        assert!((model.standing(5, settled) - Falloff::Across.ahead(1)).abs() < f32::EPSILON);
        assert!((model.standing(4, settled) - Falloff::Across.ahead(2)).abs() < f32::EPSILON);
        assert!((model.standing(2, settled) - Falloff::Across.ahead(4)).abs() < f32::EPSILON);
        assert!(model.standing(5, settled) < LIT);
        assert!(model.lead(5, settled).abs() < f32::EPSILON);
    }

    #[test]
    fn a_scroll_of_your_own_after_the_last_word_brings_the_whole_set_back() {
        let now = Instant::now();
        let mut model = verse(6);
        model.follow_the_track(at(60), now);
        let over = now + TURN;
        assert!(model.standing(1, over).abs() < f32::EPSILON);

        model.led_by_hand(over);
        assert!(model.shows_every_line(over));
        assert!((model.standing(1, over + TURN) - Falloff::Across.ahead(5)).abs() < f32::EPSILON);

        let let_go = over + HANDS_OFF;
        model.follow_the_track(at(61), let_go);
        assert!(!model.shows_every_line(let_go));
        assert!(model.standing(1, let_go + TURN).abs() < f32::EPSILON);
    }

    #[test]
    fn the_pointer_opens_out_a_spent_set_it_is_held_over() {
        let now = Instant::now();
        let mut model = verse(4);
        model.follow_the_track(at(60), now);
        let over = now + TURN;
        assert!(model.standing(3, over).abs() < f32::EPSILON);

        model.open_out(true);
        spread_settled(&mut model);
        assert!(model.standing(3, over) >= Falloff::Across.ahead(1));
        assert!(model.standing(0, over) >= Falloff::Across.ahead(4));
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

    fn sung(lines: Vec<LyricLine>) -> LyricsModel {
        let source = resonate_core::SourceId::new("held").expect("a lowercase name");
        let mut model = model();
        model.look = Look::Found(Arc::new(
            Lyrics::synced(source, lines).expect("every line is timed"),
        ));
        model.hold();

        model
    }

    #[test]
    fn a_gap_shorter_than_a_breath_holds_the_line_just_sung_rather_than_putting_the_sheet_out() {
        let now = Instant::now();
        let settled = now + TURN;
        let mut model = sung(vec![
            LyricLine::sung(at(4), "a line the sheet ends").ending(at(6)),
            LyricLine::sung(Duration::from_millis(6_400), "the next comes soon"),
            LyricLine::sung(at(9), "and one after"),
        ]);

        model.follow_the_track(Duration::from_millis(6_200), now);

        assert_eq!(model.in_play(Duration::from_millis(6_200)), [None, None]);
        assert!((model.standing(0, settled) - LIT).abs() < f32::EPSILON);
        assert!(model.lead(0, settled).abs() < f32::EPSILON);
        assert!((model.standing(1, settled) - Falloff::Around.ahead(1)).abs() < f32::EPSILON);
    }

    #[test]
    fn a_pause_leaves_the_line_it_waits_on_and_the_one_after_readable_under_the_dots() {
        let now = Instant::now();
        let settled = now + TURN;
        let mut model = sung(vec![
            LyricLine::sung(at(0), "one"),
            LyricLine::sung(at(60), "two"),
            LyricLine::sung(at(62), "three"),
            LyricLine::sung(at(64), "four"),
        ]);

        model.follow_the_track(at(30), now);

        assert_eq!(model.read_at(), Some(1));
        assert!(model.standing(0, settled).abs() < f32::EPSILON);
        assert!((model.standing(1, settled) - Falloff::Around.ahead(1)).abs() < f32::EPSILON);
        assert!((model.standing(2, settled) - Falloff::Around.ahead(2)).abs() < f32::EPSILON);
        assert!(model.standing(3, settled).abs() < f32::EPSILON);
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
        model.steady = true;
        model.laid_out = Some((
            Size {
                width: px(900.0),
                height: px(200.0),
            },
            model.measures(),
        ));

        model.rewind();

        assert!(!model.is_placed());
        assert!(!model.steady);
        assert_eq!(model.laid_out, None);
        assert_eq!(model.read_at(), None);
    }

    #[test]
    fn a_landing_moved_by_less_than_half_a_pixel_leaves_the_glide_alone() {
        let now = Instant::now();
        let mut model = verse(4);
        model.placed = true;

        model.glide_to(px(400.0), now);
        let half = now + GLIDE / 2;
        model.glide_to(px(400.2), half);
        let glide = model.glide.expect("a glide is in flight");

        assert!(glide.before.is_none(), "a sub-pixel move turned the glide");
        assert_eq!(glide.leg.lands, px(400.0));

        model.glide_to(px(-900.0), half);
        let turned = model.glide.expect("a glide is in flight");
        assert_eq!(turned.leg.lands, px(-900.0));
        assert!((turned.at(half) - glide.at(half)).abs() < px(1e-3));
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

    #[test]
    fn the_sheet_opens_out_anywhere_down_its_column_the_lines_already_sung_included() {
        let column = px(720.0);
        let slack = px(16.0);

        assert!(
            near_the_words(point(px(800.0), px(400.0)), pane(), column, slack),
            "the pointer on the line being sung did not open the sheet out"
        );
        assert!(
            near_the_words(point(px(800.0), px(90.0)), pane(), column, slack),
            "the pointer over the lines already sung did not open the sheet out"
        );
        assert!(
            near_the_words(point(px(800.0), px(740.0)), pane(), column, slack),
            "the pointer over the lines still to come did not open the sheet out"
        );
        assert!(
            !near_the_words(point(px(340.0), px(400.0)), pane(), column, slack),
            "the gutter beside the column opened the sheet out"
        );
        assert!(
            !near_the_words(point(px(100.0), px(400.0)), pane(), column, slack),
            "a pointer outside the pane altogether opened the sheet out"
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
            px(0.0)
        ));
    }
}
