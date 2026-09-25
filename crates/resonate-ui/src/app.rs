use std::{
    cell::RefCell,
    num::NonZeroUsize,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use gpui::{
    App, AppContext as _, Application, Bounds, Context, Global, Image, KeyBinding, Task,
    TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions, actions, px, size,
};
use resonate_core::{Appearance, FrameSpan, MediaLocation, Presence, ScrollbarMode};
use resonate_engine::{
    ArtRead, BitRate, Command, Event, MediaInfo, OutputSettings, Player, PlayerState, QueueItem,
    Queued, SinkInfo, StreamDigest, Tapped, TrackState,
};
use resonate_eq::Corrected;
use resonate_library::{Fingerprinters, Library, Reference};
use resonate_lyrics::Lyricists;

use crate::{
    AppIcon, Bindings, Error, Launcher, Result, RootView, Settings, WindowKind,
    drawing::Drawer,
    icons,
    listening::Listens,
    models::{Art, Drawn, FirstRead, Forget, Magnifying, held, whole_of},
    recent::Recent,
    settings::{Online, Places, Present, Sourcing, Stored, Tabs, WindowButtons},
    theme,
    views::field,
};

const APP_ID: &str = "resonate";
const WINDOW_TITLE: &str = "Resonate";

const POLL_INTERVAL: Duration = Duration::from_millis(16);

const QUIT_POLL: Duration = Duration::from_millis(100);

const CLOCK_STEP: Duration = Duration::from_secs(1);

const STEPS_PER_PIXEL: f32 = 4.0;

const PICTURES_HELD: NonZeroUsize = held(256);

const DECODES_AT_ONCE: usize = 4;

const SEEK_STEP_SECONDS: i64 = 5;

pub(crate) const WINDOW_CONTEXT: &str = "Resonate";
pub(crate) const SEARCH_CONTEXT: &str = "Search";
pub(crate) const CONTROL_CONTEXT: &str = "Control";

actions!(
    resonate,
    [
        TogglePlayPause,
        Pause,
        Stop,
        Next,
        Previous,
        SeekForward,
        SeekBackward,
        ToggleShuffle,
        CycleRepeat,
        VolumeUp,
        VolumeDown,
        ReachAbove,
        ReachBelow,
        ReachFirst,
        ReachLast,
        ReachPageAbove,
        ReachPageBelow,
        ReachEverything,
        DropReached,
        WidenAbove,
        WidenBelow,
        RaiseRow,
        LowerRow,
        PlayReached,
        UndoEdit,
        RedoEdit,
        FocusSearch,
        LeaveSearch,
        FocusFilter,
        ReachNext,
        ReachPrevious,
        NextPane,
        PreviousPane,
        PressControl,
        LeaveControl,
        GoToTheResults,
        TabOnward,
        Listen,
        Quit,
    ]
);

pub struct Lookups {
    pub lyricists: Arc<Lyricists>,
    pub fingerprinters: Arc<Fingerprinters>,
    pub reference: Option<Arc<dyn Reference>>,
    pub corrections: Arc<Corrected>,
    pub online: Online,
    pub bindings: Bindings,
    pub sourcing: Sourcing,
    pub listens: Listens,
}

pub struct ResonateApp {
    pub player: Arc<Player>,
    pub library: Arc<Library>,
    pub lyricists: Arc<Lyricists>,
    pub fingerprinters: Arc<Fingerprinters>,
    pub corrections: Arc<Corrected>,
    pub settings: Arc<dyn Settings>,
    pub online: Online,
    pub bindings: Bindings,
    pub reference: Option<Arc<dyn Reference>>,
    pub attention: Sender<bool>,
    pub places: Places,
    pub resume: bool,
    pub organise_as: String,
    pub notify: Arc<AtomicBool>,
    pub window_buttons: WindowButtons,
    pub scroll_volume: bool,
    pub scrollbars: ScrollbarMode,
    pub tabs: Tabs,
    pub presence: Presence,
    pub present: Arc<dyn Present>,
    pub launcher: Arc<dyn Launcher>,
    pub sourcing: Sourcing,
    pub listens: Listens,
    pub(crate) first_read: Option<FirstRead>,
}

impl Global for ResonateApp {}

pub(crate) fn attend(attention: &Sender<bool>, held: bool) {
    if attention.send(held).is_err() {
        tracing::debug!("nothing is watching whether the window is in front of the listener");
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Grain {
    #[default]
    EveryPoll,
    Stepped(Duration),
}

impl Grain {
    pub(crate) fn across(pixels: f32, duration: Option<Duration>) -> Self {
        let Some(duration) = duration else {
            return Self::Stepped(CLOCK_STEP);
        };
        let steps = pixels * STEPS_PER_PIXEL;
        if steps < 1.0 {
            return Self::EveryPoll;
        }
        match duration.div_f32(steps).min(CLOCK_STEP) {
            Duration::ZERO => Self::EveryPoll,
            step => Self::Stepped(step),
        }
    }

    fn shows(self, before: &PlayerState, after: &PlayerState) -> bool {
        let Self::Stepped(step) = self else {
            return true;
        };
        let (Some(was), Some(now)) = (before.current, after.current) else {
            return true;
        };
        if held_still(after, before) != *before {
            return true;
        }

        let moment = |track: TrackState, step: Duration| {
            track.position.to_duration(track.source.rate).as_nanos() / step.as_nanos()
        };
        moment(was, CLOCK_STEP) != moment(now, CLOCK_STEP) || moment(was, step) != moment(now, step)
    }
}

fn held_still(state: &PlayerState, like: &PlayerState) -> PlayerState {
    let mut still = state.clone();
    if let (Some(track), Some(was)) = (still.current.as_mut(), like.current) {
        track.position = was.position;
    }
    if let (Some(output), Some(was)) = (still.output.as_mut(), like.output) {
        output.latency = was.latency;
    }
    still
}

struct Graphed {
    columns: usize,
    series: Arc<[BitRate]>,
}

pub struct PlayerModel {
    state: PlayerState,
    settings: Arc<OutputSettings>,
    sinks: Arc<[SinkInfo]>,
    digest: Option<Arc<StreamDigest>>,
    graphed: Option<Graphed>,
    carried_lines: Option<usize>,
    queued: Queued,
    reads: u64,
    notice: Option<String>,
    pictures: Recent<MediaLocation, Option<Art>>,
    decoding: AHashSet<MediaLocation>,
    unsettled: AHashMap<MediaLocation, u64>,
    magnified: Option<Magnifying<MediaLocation>>,
    grain: Grain,
    player: Arc<Player>,
    _poll: Task<()>,
}

impl PlayerModel {
    pub fn new(player: Arc<Player>, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this.update(cx, Self::refresh).is_err() {
                    return;
                }
            }
        });

        Self {
            state: read_to_the_second(player.state()),
            settings: player.output_settings(),
            sinks: player.sinks(),
            digest: player.digest(),
            graphed: None,
            carried_lines: None,
            queued: player.queued(),
            reads: player.media_revision(),
            notice: None,
            pictures: Recent::new(PICTURES_HELD),
            decoding: AHashSet::new(),
            unsettled: AHashMap::new(),
            magnified: None,
            grain: Grain::default(),
            player,
            _poll: poll,
        }
    }

    pub const fn state(&self) -> &PlayerState {
        &self.state
    }

    pub fn output_settings(&self) -> &OutputSettings {
        &self.settings
    }

    pub fn sinks(&self) -> &[SinkInfo] {
        &self.sinks
    }

    pub fn digest(&self) -> Option<Arc<StreamDigest>> {
        self.digest.clone()
    }

    pub fn tap(&self) -> Tapped {
        self.player.tap()
    }

    pub(crate) const fn draw_at(&mut self, grain: Grain) {
        self.grain = grain;
    }

    pub fn listen_in(&self, listening: bool) {
        self.player.listen_in(listening);
    }

    pub const fn reads(&self) -> u64 {
        self.reads
    }

    pub fn carried_lines(&mut self) -> Option<usize> {
        if self.carried_lines.is_none()
            && let Some(lyrics) = self
                .digest
                .as_ref()
                .and_then(|digest| digest.info.tags.lyrics.as_deref())
        {
            self.carried_lines = Some(written_lines(lyrics));
        }

        self.carried_lines
    }

    pub fn condensed(&mut self, columns: usize) -> Arc<[BitRate]> {
        if let Some(graphed) = self.graphed.as_ref()
            && graphed.columns == columns
        {
            return Arc::clone(&graphed.series);
        }

        let series: Arc<[BitRate]> = self
            .digest
            .as_ref()
            .and_then(|digest| digest.profile.as_ref())
            .map(|profile| profile.condensed(columns))
            .unwrap_or_default()
            .into();
        self.graphed = Some(Graphed {
            columns,
            series: Arc::clone(&series),
        });
        series
    }

    pub fn queue(&self) -> Arc<Vec<QueueItem>> {
        Arc::clone(&self.queued.rows)
    }

    pub fn queued(&self) -> Queued {
        self.queued.clone()
    }

    pub(crate) fn engine(&self) -> Arc<Player> {
        Arc::clone(&self.player)
    }

    pub fn media(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Option<Arc<MediaInfo>> {
        self.player.media(location, span)
    }

    pub fn art(
        &mut self,
        location: &MediaLocation,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        self.art_of(location, cx).map(|art| art.drawn(drawn))
    }

    pub fn whole_art(
        &mut self,
        location: &MediaLocation,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        if let Some(magnified) = &self.magnified
            && magnified.names(location)
        {
            return magnified.whole();
        }
        self.magnified
            .replace(Magnifying::Reading(location.clone()))
            .forget(cx);

        let player = Arc::clone(&self.player);
        let wanted = location.clone();
        let asked = location.clone();
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { player.art(&asked).map(|art| whole_of(art.as_ref())) })
                .await;
            let landed = this.update(cx, |this, cx| {
                if this
                    .magnified
                    .as_ref()
                    .is_some_and(|held| held.names(&wanted))
                {
                    this.magnified
                        .replace(Magnifying::Read(wanted, read))
                        .forget(cx);
                    cx.notify();
                }
            });
            let _ = landed;
        })
        .detach();
        None
    }

    fn art_of(&mut self, location: &MediaLocation, cx: &mut Context<Self>) -> Option<Art> {
        if let Some(held) = self.pictures.get(location) {
            return held.clone();
        }
        if self.decoding.contains(location)
            || self.decoding.len() >= DECODES_AT_ONCE
            || self.unsettled.get(location) == Some(&self.reads)
        {
            return None;
        }
        self.decoding.insert(location.clone());

        let player = Arc::clone(&self.player);
        let wanted = location.clone();
        let asked = location.clone();
        let asked_at = self.reads;
        let drawing = cx
            .global::<Drawer>()
            .draw(move || match player.art_read(&asked) {
                ArtRead::Answered(art) => Some(Some(Art::of(art.as_ref()))),
                ArtRead::Nothing => Some(None),
                ArtRead::NotYet => None,
            });
        cx.spawn(async move |this, cx| {
            let drawn = drawing.await.unwrap_or(Some(None));
            let landed = this.update(cx, |this, cx| {
                this.decoding.remove(&wanted);
                match drawn {
                    Some(decoded) => {
                        this.unsettled.remove(&wanted);
                        this.pictures.insert(wanted, decoded).forget(cx);
                    }
                    None => {
                        this.unsettled.insert(wanted, asked_at);
                    }
                }
                cx.notify();
            });
            let _ = landed;
        })
        .detach();
        None
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn report(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    pub fn dismiss(&mut self) {
        self.notice = None;
    }

    pub fn send(&self, command: Command) {
        if let Err(error) = self.player.send(command) {
            tracing::error!(?error, "the engine refused a transport command");
        }
    }

    pub const fn poll_interval() -> Duration {
        POLL_INTERVAL
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;

        for event in self.player.events().try_iter() {
            changed = true;
            match event {
                Event::Failed { track, error } => {
                    tracing::error!(%error, %track, "playback failed");
                    self.notice = Some(format!("track {track} failed: {error}"));
                }
                Event::CommandFailed { command, error } => {
                    tracing::warn!(%error, ?command, "the engine refused a command");
                    self.notice = Some(format!("{command} was refused — {error}"));
                }
                Event::OutputChanged(_) | Event::TrackStarted(_) => self.notice = None,
                Event::TrackFinished(_) | Event::QueueFinished | Event::Underrun { .. } => {}
            }
        }

        let state = read_to_the_second(self.player.state());
        if state != self.state {
            changed |= self.grain.shows(&self.state, &state);
            self.state = state;
        }

        let settings = self.player.output_settings();
        if !Arc::ptr_eq(&settings, &self.settings) {
            self.settings = settings;
            changed = true;
        }

        let sinks = self.player.sinks();
        if !Arc::ptr_eq(&sinks, &self.sinks) {
            self.sinks = sinks;
            changed = true;
        }

        let digest = self.player.digest();
        if !is_same_digest(digest.as_ref(), self.digest.as_ref()) {
            self.digest = digest;
            self.graphed = None;
            self.carried_lines = None;
            changed = true;
        }

        let queued = self.player.queued();
        if queued.revision != self.queued.revision {
            self.queued = queued;
            changed = true;
        }

        let reads = self.player.media_revision();
        if reads != self.reads {
            self.reads = reads;
            changed = true;
        }

        if changed {
            cx.notify();
        }
    }
}

fn read_to_the_second(mut state: PlayerState) -> PlayerState {
    if let Some(asleep) = state.sleeping.as_mut() {
        asleep.left = asleep.left.map(whole_seconds);
    }

    state
}

fn whole_seconds(left: Duration) -> Duration {
    Duration::from_secs(left.as_secs() + u64::from(left.subsec_nanos() > 0))
}

fn written_lines(lyrics: &str) -> usize {
    lyrics
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

fn is_same_digest(left: Option<&Arc<StreamDigest>>, right: Option<&Arc<StreamDigest>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

const PLAY_KEY: &str = "xf86audioplay";

const PAUSE_KEY: &str = "xf86audiopause";

const STOP_KEY: &str = "xf86audiostop";

const NEXT_TRACK_KEY: &str = "xf86audionext";

const PREVIOUS_TRACK_KEY: &str = "xf86audioprev";

const FORWARD_KEY: &str = "forward";

const BACK_KEY: &str = "back";

fn answering_anywhere() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(PLAY_KEY, TogglePlayPause, None),
        KeyBinding::new(PAUSE_KEY, Pause, None),
        KeyBinding::new(STOP_KEY, Stop, None),
        KeyBinding::new(NEXT_TRACK_KEY, Next, None),
        KeyBinding::new(PREVIOUS_TRACK_KEY, Previous, None),
        KeyBinding::new(FORWARD_KEY, Next, None),
        KeyBinding::new(BACK_KEY, Previous, None),
        KeyBinding::new("ctrl-shift-right", Next, None),
        KeyBinding::new("ctrl-shift-left", Previous, None),
        KeyBinding::new("ctrl-f", FocusSearch, None),
        KeyBinding::new("tab", ReachNext, None),
        KeyBinding::new("shift-tab", ReachPrevious, None),
        KeyBinding::new("ctrl-tab", NextPane, None),
        KeyBinding::new("ctrl-shift-tab", PreviousPane, None),
        KeyBinding::new("ctrl-comma", FocusFilter, None),
        KeyBinding::new("ctrl-l", Listen, None),
        KeyBinding::new("ctrl-q", Quit, None),
    ]
}

fn answering_away_from_a_field(typed: Option<&str>) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("space", TogglePlayPause, typed),
        KeyBinding::new("s", Stop, typed),
        KeyBinding::new("right", SeekForward, typed),
        KeyBinding::new("left", SeekBackward, typed),
        KeyBinding::new("ctrl-right", Next, typed),
        KeyBinding::new("ctrl-left", Previous, typed),
        KeyBinding::new("h", ToggleShuffle, typed),
        KeyBinding::new("r", CycleRepeat, typed),
        KeyBinding::new("ctrl-up", VolumeUp, typed),
        KeyBinding::new("ctrl-down", VolumeDown, typed),
        KeyBinding::new("up", ReachAbove, typed),
        KeyBinding::new("down", ReachBelow, typed),
        KeyBinding::new("shift-up", WidenAbove, typed),
        KeyBinding::new("shift-down", WidenBelow, typed),
        KeyBinding::new("home", ReachFirst, typed),
        KeyBinding::new("end", ReachLast, typed),
        KeyBinding::new("pageup", ReachPageAbove, typed),
        KeyBinding::new("pagedown", ReachPageBelow, typed),
        KeyBinding::new("ctrl-a", ReachEverything, typed),
        KeyBinding::new("delete", DropReached, typed),
        KeyBinding::new("alt-up", RaiseRow, typed),
        KeyBinding::new("alt-down", LowerRow, typed),
        KeyBinding::new("enter", PlayReached, typed),
        KeyBinding::new("ctrl-z", UndoEdit, typed),
        KeyBinding::new("ctrl-shift-z", RedoEdit, typed),
        KeyBinding::new("ctrl-y", RedoEdit, typed),
    ]
}

fn answering_where_the_caret_is(on_a_control: Option<&str>) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("escape", LeaveSearch, Some(SEARCH_CONTEXT)),
        KeyBinding::new("down", GoToTheResults, Some(SEARCH_CONTEXT)),
        KeyBinding::new("tab", TabOnward, Some(SEARCH_CONTEXT)),
        KeyBinding::new("space", PressControl, on_a_control),
        KeyBinding::new("enter", PressControl, on_a_control),
        KeyBinding::new("escape", LeaveControl, on_a_control),
    ]
}

fn bindings() -> Vec<KeyBinding> {
    let away_from_search = format!("!{SEARCH_CONTEXT} && !{CONTROL_CONTEXT}");

    let mut bindings = answering_anywhere();
    bindings.extend(answering_away_from_a_field(Some(&away_from_search)));
    bindings.extend(answering_where_the_caret_is(Some(CONTROL_CONTEXT)));
    bindings.extend(field::bindings());
    bindings
}

pub const fn seek_step() -> i64 {
    SEEK_STEP_SECONDS
}

pub struct Bus {
    pub quit: Receiver<()>,
    pub raise: Receiver<()>,
    pub attention: Sender<bool>,
}

fn raise_when_asked(asked: Receiver<()>, cx: &mut App) {
    let waiting = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            match asked.try_recv() {
                Ok(()) => {
                    let raised = cx.update(|cx| {
                        for handle in cx.windows() {
                            let _ = handle.update(cx, |_, window, _| window.activate_window());
                        }
                    });
                    if raised.is_err() {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => waiting.timer(QUIT_POLL).await,
                Err(TryRecvError::Disconnected) => return,
            }
        }
    })
    .detach();
}

fn close_when_asked(asked: Receiver<()>, cx: &mut App) {
    let waiting = cx.background_executor().clone();
    let heard = cx.background_executor().spawn(async move {
        loop {
            match asked.try_recv() {
                Ok(()) => return true,
                Err(TryRecvError::Empty) => waiting.timer(QUIT_POLL).await,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    });

    cx.spawn(async move |cx| {
        if heard.await {
            let _ = cx.update(close_every_window);
        }
    })
    .detach();
}

fn close_every_window(cx: &mut App) {
    for handle in cx.windows() {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

fn quit_once_the_last_window_closes(cx: &mut App) {
    cx.on_window_closed(|cx| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

pub fn run(
    player: Arc<Player>,
    library: Arc<Library>,
    lookups: Lookups,
    stored: Stored,
    appearance: Appearance,
    bus: Bus,
) -> Result<()> {
    let first_read = FirstRead::start(&library);
    theme::wear(appearance);
    stored.launcher.show(AppIcon::of(appearance));

    let failure = Rc::new(RefCell::new(None));
    let recorded = Rc::clone(&failure);

    let application = Application::new().with_assets(icons::Embedded);

    application.run(move |cx: &mut App| {
        crate::fonts::settle(&cx.text_system().all_font_names());
        cx.set_global(Drawer::new());
        cx.set_global(ResonateApp {
            player: Arc::clone(&player),
            library,
            lyricists: Arc::clone(&lookups.lyricists),
            fingerprinters: Arc::clone(&lookups.fingerprinters),
            corrections: Arc::clone(&lookups.corrections),
            settings: Arc::clone(&stored.settings),
            bindings: lookups.bindings.clone(),
            online: lookups.online.clone(),
            reference: lookups.reference.clone(),
            attention: bus.attention.clone(),
            places: stored.places.clone(),
            resume: stored.resume,
            organise_as: stored.organise_as.clone(),
            notify: Arc::clone(&stored.notify),
            window_buttons: stored.window_buttons,
            scroll_volume: stored.scroll_volume,
            scrollbars: stored.scrollbars,
            tabs: stored.tabs,
            presence: stored.presence.clone(),
            present: Arc::clone(&stored.present),
            launcher: Arc::clone(&stored.launcher),
            sourcing: lookups.sourcing.clone(),
            listens: lookups.listens.clone(),
            first_read,
        });
        cx.bind_keys(bindings());
        cx.activate(true);
        close_when_asked(bus.quit, cx);
        raise_when_asked(bus.raise, cx);
        quit_once_the_last_window_closes(cx);

        let bounds = Bounds::centered(None, size(px(1_280.0), px(820.0)), cx);
        let options = WindowOptions {
            app_id: Some(APP_ID.to_owned()),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(WINDOW_TITLE.into()),
                ..TitlebarOptions::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_min_size: Some(size(
                theme::width(theme::WINDOW_MIN_WIDTH),
                theme::width(theme::WINDOW_MIN_HEIGHT),
            )),
            ..WindowOptions::default()
        };

        let opened = cx.open_window(options, |window, cx| {
            window.set_window_title(WINDOW_TITLE);
            cx.new(|cx| RootView::new(window, cx))
        });

        if let Err(source) = opened {
            tracing::error!(window = ?WindowKind::Main, error = ?source, "gpui failed to open a window");
            *recorded.borrow_mut() = Some(Error::WindowOpen {
                kind: WindowKind::Main,
            });
            cx.quit();
        }
    });

    match failure.borrow_mut().take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use gpui::{KeyBindingContextPredicate, KeyContext, Keystroke, Modifiers};
    use resonate_core::{ChannelLayout, Frames, SampleFormat, SampleRate, StreamSpec, TrackId};
    use resonate_engine::PlaybackState;

    use super::*;

    fn at_rest() -> Vec<KeyContext> {
        vec![KeyContext::parse(WINDOW_CONTEXT).expect("the window names itself")]
    }

    fn in_the_search_field() -> Vec<KeyContext> {
        vec![
            KeyContext::parse(WINDOW_CONTEXT).expect("the window names itself"),
            KeyContext::parse(SEARCH_CONTEXT).expect("the field names itself"),
        ]
    }

    fn on_a_control() -> Vec<KeyContext> {
        vec![
            KeyContext::parse(WINDOW_CONTEXT).expect("the window names itself"),
            KeyContext::parse(CONTROL_CONTEXT).expect("the control names itself"),
        ]
    }

    fn away_from_search() -> KeyBindingContextPredicate {
        KeyBindingContextPredicate::parse(&format!("!{SEARCH_CONTEXT} && !{CONTROL_CONTEXT}"))
            .expect("the predicate every transport key carries")
    }

    #[test]
    fn the_transport_keys_answer_away_from_the_field_and_not_inside_it() {
        let away = away_from_search();

        assert!(
            away.depth_of(&at_rest()).is_some(),
            "space, s, h and r are dead wherever the window is the whole of the context"
        );
        assert!(
            away.depth_of(&in_the_search_field()).is_none(),
            "a transport key fired while the caret was in the search field"
        );
        assert!(
            away.depth_of(&on_a_control()).is_none(),
            "a transport key fired while a settings control held the caret, so space would play              rather than press what was reached"
        );
    }

    #[test]
    fn the_control_keys_answer_only_where_a_control_holds_the_caret() {
        let reached = KeyBindingContextPredicate::parse(CONTROL_CONTEXT)
            .expect("the predicate space, enter and escape carry on a control");

        assert!(reached.depth_of(&on_a_control()).is_some());
        assert!(reached.depth_of(&at_rest()).is_none());
        assert!(reached.depth_of(&in_the_search_field()).is_none());
    }

    #[test]
    fn nothing_names_itself_both_a_field_and_a_control() {
        assert_ne!(SEARCH_CONTEXT, CONTROL_CONTEXT);
        assert_ne!(WINDOW_CONTEXT, CONTROL_CONTEXT);
    }

    #[test]
    fn a_window_that_does_not_name_itself_disables_every_negated_binding() {
        assert!(
            away_from_search().depth_of(&[]).is_none(),
            "gpui now matches a negated predicate against no context at all, so WINDOW_CONTEXT \
             may no longer be what keeps the transport keys alive"
        );
    }

    #[test]
    fn the_field_keys_answer_only_where_the_caret_is() {
        let inside = KeyBindingContextPredicate::parse(SEARCH_CONTEXT)
            .expect("the predicate every editing key carries");

        assert!(inside.depth_of(&in_the_search_field()).is_some());
        assert!(inside.depth_of(&at_rest()).is_none());
    }

    #[test]
    fn the_transport_keys_a_keyboard_carries_are_named_the_way_gpui_reads_them() {
        for named in [
            PLAY_KEY,
            PAUSE_KEY,
            STOP_KEY,
            NEXT_TRACK_KEY,
            PREVIOUS_TRACK_KEY,
            FORWARD_KEY,
            BACK_KEY,
        ] {
            let stroke = Keystroke::parse(named)
                .unwrap_or_else(|_| panic!("{named} is not a keystroke gpui can read"));

            assert_eq!(
                stroke.key, named,
                "gpui reads a keysym's name lowercased and a media key reaches the window under                  that name alone, so a binding that is not it is bound to nothing"
            );
            assert_eq!(stroke.modifiers, Modifiers::none());
        }
    }

    #[test]
    fn every_binding_the_window_carries_is_one_gpui_can_read() {
        assert!(bindings().len() > 20);
    }

    #[test]
    fn every_key_not_named_to_answer_anywhere_is_dead_while_a_caret_is_held() {
        let away_from_search = format!("!{SEARCH_CONTEXT} && !{CONTROL_CONTEXT}");

        for binding in answering_away_from_a_field(Some(&away_from_search)) {
            let keys = format!("{:?}", binding.keystrokes());
            let predicate = binding
                .predicate()
                .unwrap_or_else(|| panic!("{keys} is bound away from a field with no predicate"));

            assert!(
                predicate.depth_of(&in_the_search_field()).is_none(),
                "{keys} fires while the caret is in a field"
            );
            assert!(
                predicate.depth_of(&on_a_control()).is_none(),
                "{keys} fires while a settings control holds the caret"
            );
            assert!(
                predicate.depth_of(&at_rest()).is_some(),
                "{keys} answers nowhere at all"
            );
        }
    }

    #[test]
    fn down_and_tab_in_a_field_are_the_fields_own_before_they_are_the_windows() {
        let keymap = gpui::Keymap::new(bindings());
        for (key, wanted) in [("down", "GoToTheResults"), ("tab", "TabOnward")] {
            let typed = [Keystroke::parse(key).expect("a key gpui reads")];
            let (found, _) = keymap.bindings_for_input(&typed, &in_the_search_field());
            let first = found
                .first()
                .unwrap_or_else(|| panic!("{key} does nothing in a field"));
            assert!(
                first.action().name().ends_with(wanted),
                "{key} in a field is {}",
                first.action().name()
            );
        }
    }

    #[test]
    fn the_keys_that_answer_anywhere_are_the_ones_a_field_could_never_want() {
        for binding in answering_anywhere() {
            assert!(
                binding.predicate().is_none(),
                "a key named to answer anywhere carries a predicate"
            );
        }
    }

    #[test]
    fn the_desktop_entry_matches_the_window_against_the_app_id_it_sends() {
        let entry = include_str!("../../../packaging/resonate.desktop");
        let declared = entry
            .lines()
            .find_map(|line| line.strip_prefix("StartupWMClass="))
            .expect("the desktop entry declares the class it matches on");

        assert_eq!(declared, APP_ID);
    }

    #[test]
    fn the_desktop_entry_names_its_icon_after_the_app_id_the_window_sends() {
        let entry = include_str!("../../../packaging/resonate.desktop");
        let named = entry
            .lines()
            .find_map(|line| line.strip_prefix("Icon="))
            .expect("the desktop entry names an icon");

        assert_eq!(named, APP_ID);
    }

    fn playing_at(seconds: f64) -> PlayerState {
        let rate = SampleRate::HZ_44100;
        PlayerState {
            playback: PlaybackState::Playing,
            current: Some(TrackState {
                id: TrackId::new(1).expect("1 is not zero"),
                source: StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::S16),
                position: Frames::from_duration(Duration::from_secs_f64(seconds), rate),
                duration: Some(Frames::from_duration(Duration::from_secs(200), rate)),
            }),
            ..PlayerState::default()
        }
    }

    #[test]
    fn a_track_as_long_as_the_rail_is_wide_redraws_once_a_step() {
        let grain = Grain::across(200.0, Some(Duration::from_secs(200)));

        assert_eq!(grain, Grain::Stepped(Duration::from_millis(250)));
        assert!(!grain.shows(&playing_at(10.01), &playing_at(10.2)));
        assert!(grain.shows(&playing_at(10.2), &playing_at(10.26)));
    }

    #[test]
    fn the_clock_turns_over_whatever_the_rail_would_draw() {
        let grain = Grain::Stepped(Duration::from_millis(300));

        assert!(grain.shows(&playing_at(10.95), &playing_at(11.01)));
    }

    #[test]
    fn a_long_track_still_redraws_every_second() {
        assert_eq!(
            Grain::across(100.0, Some(Duration::from_secs(7_200))),
            Grain::Stepped(CLOCK_STEP)
        );
        assert_eq!(Grain::across(100.0, None), Grain::Stepped(CLOCK_STEP));
    }

    #[test]
    fn a_rail_not_yet_painted_redraws_every_poll() {
        assert_eq!(
            Grain::across(0.0, Some(Duration::from_secs(200))),
            Grain::EveryPoll
        );
    }

    #[test]
    fn anything_but_the_moment_moving_is_drawn_at_once() {
        let grain = Grain::Stepped(CLOCK_STEP);
        let before = playing_at(10.1);
        let paused = PlayerState {
            playback: PlaybackState::Paused,
            ..playing_at(10.2)
        };

        assert!(grain.shows(&before, &paused));
        assert!(Grain::EveryPoll.shows(&before, &playing_at(10.11)));
    }
}
