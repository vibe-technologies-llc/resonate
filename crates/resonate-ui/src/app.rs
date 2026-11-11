use std::{
    cell::RefCell,
    collections::BTreeSet,
    future,
    num::NonZeroUsize,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use gpui::{
    App, AppContext as _, Bounds, Context, Global, KeyBinding, OperatingSystem, Task,
    TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions, actions, px, size,
};
use resonate_core::{
    Appearance, ArtistsDrawn, DesktopLook, FrameSpan, Frames, HeadYaw, MediaLocation, Presence,
    ScrollbarMode, SpeakerAngle, SpeakerElevation, Spectral, TrackId,
};
use resonate_engine::{
    ArtRead, BitRate, Command, CommandKind, Event, MediaInfo, NodeName, Outcome, OutputSettings,
    PlaybackState, Player, PlayerState, QueueRows, Queued, SinkId, SinkInfo, StreamDigest, Tapped,
    TrackState, Zones,
};
use resonate_eq::Corrected;
use resonate_library::{
    Fingerprinters, HistoryKept, Library, Reference, Scrobblers, SignsInToSpotify,
};
use resonate_lyrics::Lyricists;
use resonate_providers::SignsIn;
use resonate_words::LanguageChoice;

use crate::{
    AppIcon, Bindings, CaretBlink, Error, Launcher, Measurers, Notice, Result, RootView, Settings,
    WindowKind,
    drawing::Drawer,
    format, icons,
    listening::Listens,
    measure::Measures,
    models::{
        AtSide, Consulted, Drawn, FirstRead, Forget, Magnifying, Picture, Scale, drawn_within,
        held, magnified_of,
    },
    recent::Recent,
    served::Serving,
    settings::{
        ChecksYoutubeMusic, Followed, Online, Places, Present, SettingsCategory, SignsInToJellyfin,
        SignsInToYoutubeMusic, Sourcing, Stored, Tabs, WindowButtons, WindowSize,
    },
    theme, toast,
    views::{chrome, dropping::Copying, field},
};

const APP_ID: &str = "resonate";
pub(crate) const WINDOW_TITLE: &str = "Resonate";

const POLL_INTERVAL: Duration = Duration::from_millis(16);

const RESTING_POLL_INTERVAL: Duration = Duration::from_millis(64);

const QUIET_POLLS_BEFORE_RESTING: u32 = 30;

const QUIT_POLL: Duration = Duration::from_millis(100);

const WIND_DOWN_WITHIN: Duration = Duration::from_secs(10);

const CLOCK_STEP: Duration = Duration::from_secs(1);

const STEPS_PER_PIXEL: f32 = 4.0;

const PICTURES_HELD: NonZeroUsize = held(256);

const DECODES_AT_ONCE: usize = 4;

const A_FAILED_PICTURE_IS_ASKED_AGAIN_AFTER: Duration = Duration::from_secs(30);

enum Drew {
    Settled(Option<Picture>),
    NotYet,
    Failed,
}

const SEEK_STEP_SECONDS: i64 = 5;

const SEEK_FURTHER_SECONDS: i64 = 30;

pub(crate) const WINDOW_CONTEXT: &str = stringify!(Resonate);
pub(crate) const SEARCH_CONTEXT: &str = stringify!(Search);
pub(crate) const CONTROL_CONTEXT: &str = stringify!(Control);

pub(crate) const BAND_CONTEXT: &str = stringify!(Band);

pub(crate) const RAIL_CONTEXT: &str = stringify!(Rail);

pub(crate) const ORDER_CONTEXT: &str = stringify!(Ordering);

actions!(
    resonate,
    [
        TogglePlayPause,
        PlayPauseUnlessTyping,
        Pause,
        Stop,
        Next,
        Previous,
        SeekForward,
        SeekBackward,
        SeekFurtherForward,
        SeekFurtherBackward,
        ToggleMute,
        ToggleQueue,
        HoldThePicture,
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
        ChooseReached,
        HopAbove,
        HopBelow,
        DropReached,
        WidenAbove,
        WidenBelow,
        RaiseRow,
        LowerRow,
        PlayReached,
        OpenTheMenu,
        UndoEdit,
        RedoEdit,
        PasteAway,
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
        About,
        Hide,
        HideOthers,
        ShowAll,
        Minimise,
        Zoom,
        CloseTheWindow,
        ToggleFullScreen,
        BandHigher,
        BandLower,
        BandLouder,
        BandQuieter,
        BandNarrower,
        BandWider,
        RailOn,
        RailBack,
        OrderAbove,
        OrderBelow,
        OrderFirst,
        OrderLast,
        ReadTheFirstWay,
        ReadTheOtherWay,
        TakeTheOrder,
    ]
);

pub struct Lookups {
    pub lyricists: Arc<Lyricists>,
    pub fingerprinters: Arc<Fingerprinters>,
    pub reference: Option<Arc<dyn Reference>>,
    pub for_the_pass: Consulted,
    pub scrobblers: Option<Arc<dyn Scrobblers>>,
    pub signs_in: Option<Arc<dyn SignsIn>>,
    pub spotify: Option<Arc<dyn SignsInToSpotify>>,
    pub checks_youtube_music: Option<ChecksYoutubeMusic>,
    pub signs_in_to_youtube_music: Option<Arc<dyn SignsInToYoutubeMusic>>,
    pub signs_in_to_jellyfin: Option<Arc<dyn SignsInToJellyfin>>,
    pub corrections: Arc<Corrected>,
    pub online: Online,
    pub bindings: Bindings,
    pub measurers: Measurers,
    pub measures: Option<Arc<dyn Measures>>,
    pub sourcing: Sourcing,
    pub serving: Serving,
    pub listens: Listens,
    pub mirroring: Option<Arc<dyn crate::Mirroring>>,
}

pub struct ResonateApp {
    pub zones: Arc<Zones>,
    pub library: Arc<Library>,
    pub lyricists: Arc<Lyricists>,
    pub fingerprinters: Arc<Fingerprinters>,
    pub corrections: Arc<Corrected>,
    pub settings: Arc<dyn Settings>,
    pub online: Online,
    pub bindings: Bindings,
    pub measurers: Measurers,
    pub measures: Option<Arc<dyn Measures>>,
    pub reference: Option<Arc<dyn Reference>>,
    pub for_the_pass: Consulted,
    pub scrobblers: Option<Arc<dyn Scrobblers>>,
    pub signs_in: Option<Arc<dyn SignsIn>>,
    pub spotify: Option<Arc<dyn SignsInToSpotify>>,
    pub checks_youtube_music: Option<ChecksYoutubeMusic>,
    pub signs_in_to_youtube_music: Option<Arc<dyn SignsInToYoutubeMusic>>,
    pub signs_in_to_jellyfin: Option<Arc<dyn SignsInToJellyfin>>,
    pub attention: Sender<bool>,
    pub places: Places,
    pub resume: bool,
    pub history_kept: HistoryKept,
    pub organise_as: String,
    pub notify: Arc<AtomicBool>,
    pub by_sound: Arc<AtomicBool>,
    pub lyrics_by_the_locale: Arc<AtomicBool>,
    pub flac_above_96k_for: BTreeSet<NodeName>,
    pub convolution: Option<PathBuf>,
    pub speaker_angle: SpeakerAngle,
    pub speaker_elevation: SpeakerElevation,
    pub head_yaw: HeadYaw,
    pub music_folder: Option<PathBuf>,
    pub file_dropped: bool,
    pub window_buttons: WindowButtons,
    pub scroll_volume: bool,
    pub mouse_navigation: bool,
    pub reduced_motion: bool,
    pub caret: CaretBlink,
    pub scrollbars: ScrollbarMode,
    pub language: LanguageChoice,
    pub spectral: Spectral,
    pub tabs: Tabs,
    pub remember_tab: bool,
    pub last_tab: Option<crate::Pane>,
    pub artists_drawn: ArtistsDrawn,
    pub remember_window_size: bool,
    pub window_size: Option<WindowSize>,
    pub remember_settings_category: bool,
    pub last_settings_category: SettingsCategory,
    pub presence: Presence,
    pub present: Arc<dyn Present>,
    pub renderers: Option<Arc<dyn crate::Renderers>>,
    pub synchronised: crate::Synchronised,
    pub syncing: Option<Arc<dyn crate::Syncing>>,
    pub together: Option<Arc<dyn crate::Together>>,
    pub reads_as_a_code: Option<fn(&str) -> bool>,
    pub launcher: Arc<dyn Launcher>,
    pub updates: Option<Arc<dyn crate::Updates>>,
    pub check_for_updates: bool,
    pub update_dismissed: Option<String>,
    pub sourcing: Sourcing,
    pub serving: Serving,
    pub listens: Listens,
    pub mirroring: Option<Arc<dyn crate::Mirroring>>,
    pub(crate) first_read: Option<FirstRead>,
    pub(crate) copying: Copying,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Moved {
    Clock,
    #[default]
    More,
}

struct Graphed {
    columns: usize,
    series: Arc<[BitRate]>,
}

struct Seeking {
    track: TrackId,
    position: Frames,
    outcome: Outcome,
}

pub struct PlayerModel {
    state: PlayerState,
    seeking: Option<Seeking>,
    settings: Arc<OutputSettings>,
    sinks: Arc<[SinkInfo]>,
    digest: Option<Arc<StreamDigest>>,
    graphed: Option<Graphed>,
    carried_lines: Option<usize>,
    queued: Queued,
    reads: u64,
    pictures: Recent<AtSide<MediaLocation>, Option<Picture>>,
    decoding: AHashSet<AtSide<MediaLocation>>,
    unsettled: AHashMap<AtSide<MediaLocation>, u64>,
    unread: AHashMap<AtSide<MediaLocation>, Instant>,
    scale: Scale,
    magnified: Option<Magnifying<MediaLocation>>,
    grain: Grain,
    moved: Moved,
    player: Arc<Player>,
    _poll: Task<()>,
}

impl PlayerModel {
    pub fn new(player: Arc<Player>, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            let mut quiet = 0_u32;
            loop {
                let interval = if quiet >= QUIET_POLLS_BEFORE_RESTING {
                    RESTING_POLL_INTERVAL
                } else {
                    POLL_INTERVAL
                };
                cx.background_executor().timer(interval).await;
                match this.update(cx, Self::refresh) {
                    Ok(Poll::Quiet) => quiet = quiet.saturating_add(1),
                    Ok(Poll::Busy) => quiet = 0,
                    Err(_) => return,
                }
            }
        });

        Self {
            state: read_to_the_second(player.state()),
            seeking: None,
            settings: player.output_settings(),
            sinks: player.sinks(),
            digest: player.digest(),
            graphed: None,
            carried_lines: None,
            queued: player.queued(),
            reads: player.media_revision(),
            pictures: Recent::new(PICTURES_HELD),
            decoding: AHashSet::new(),
            unsettled: AHashMap::new(),
            unread: AHashMap::new(),
            scale: Scale::ONE,
            magnified: None,
            grain: Grain::default(),
            moved: Moved::default(),
            player,
            _poll: poll,
        }
    }

    pub const fn state(&self) -> &PlayerState {
        &self.state
    }

    pub(crate) fn shown_position(&self) -> Option<Frames> {
        let track = self.state.current?;
        Some(
            self.seeking
                .as_ref()
                .filter(|seek| seek.track == track.id)
                .map_or(track.position, |seek| seek.position),
        )
    }

    pub(crate) fn seek(&mut self, position: Frames, cx: &mut Context<Self>) {
        let Some(track) = self.state.current else {
            return;
        };
        match self.player.request(Command::Seek(position)) {
            Ok(outcome) => {
                self.seeking = Some(Seeking {
                    track: track.id,
                    position,
                    outcome,
                });
            }
            Err(error) => {
                self.seeking = None;
                Self::seek_refused(error, cx);
            }
        }
        self.moved = Moved::Clock;
        cx.notify();
    }

    fn seek_refused(error: resonate_engine::Error, cx: &mut Context<Self>) {
        let command = CommandKind::Seek;
        tracing::warn!(%error, ?command, "the engine refused a command");
        toast::tell(
            Notice::Trouble(toast::would_not_do(command, error.cause())),
            cx,
        );
    }

    pub fn output_settings(&self) -> &OutputSettings {
        &self.settings
    }

    pub fn sinks(&self) -> &[SinkInfo] {
        &self.sinks
    }

    pub fn sink_in_use(&self) -> Option<&SinkInfo> {
        sink_in_use(
            &self.sinks,
            self.state.output.map(|output| output.sink),
            self.settings.sink.as_ref(),
        )
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

    pub(crate) const fn moved(&self) -> Moved {
        self.moved
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

    pub fn queue(&self) -> QueueRows {
        self.queued.rows.clone()
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

    pub fn scaled_by(&mut self, scale: Scale) {
        self.scale = scale;
    }

    pub fn art_is_absent(&self, location: &MediaLocation, drawn: Drawn) -> bool {
        let wanted = AtSide {
            key: location.clone(),
            side: drawn.side(self.scale),
        };
        matches!(self.pictures.peek(&wanted), Some(None))
    }

    pub fn art(
        &mut self,
        location: &MediaLocation,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<Picture> {
        let wanted = AtSide {
            key: location.clone(),
            side: drawn.side(self.scale),
        };
        if let Some(held) = self.pictures.get(&wanted) {
            return held.clone();
        }
        if self.decoding.contains(&wanted)
            || self.decoding.len() >= DECODES_AT_ONCE
            || self.unsettled.get(&wanted) == Some(&self.reads)
            || self
                .unread
                .get(&wanted)
                .is_some_and(|failed| failed.elapsed() < A_FAILED_PICTURE_IS_ASKED_AGAIN_AFTER)
        {
            return None;
        }
        self.decoding.insert(wanted.clone());

        let player = Arc::clone(&self.player);
        let asked = location.clone();
        let asked_at = self.reads;
        let side = wanted.side;
        let drawing = cx
            .global::<Drawer>()
            .draw(move || match player.art_read(&asked) {
                ArtRead::Answered(art) => Drew::Settled(drawn_within(art.as_ref(), side)),
                ArtRead::Nothing => Drew::Settled(None),
                ArtRead::NotYet => Drew::NotYet,
                ArtRead::Failed => Drew::Failed,
            });
        cx.spawn(async move |this, cx| {
            let drawn = drawing.await.unwrap_or(Drew::Settled(None));
            let landed = this.update(cx, |this, cx| {
                this.decoding.remove(&wanted);
                match drawn {
                    Drew::Settled(decoded) => {
                        this.unsettled.remove(&wanted);
                        this.unread.remove(&wanted);
                        this.pictures.insert(wanted, decoded).forget(cx);
                    }
                    Drew::NotYet => {
                        this.unsettled.insert(wanted, asked_at);
                    }
                    Drew::Failed => {
                        this.unread.insert(wanted, Instant::now());
                    }
                }
                cx.notify();
            });
            let _ = landed;
        })
        .detach();
        None
    }

    pub fn whole_art(
        &mut self,
        location: &MediaLocation,
        cx: &mut Context<Self>,
    ) -> Option<Picture> {
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
        let drawing = cx.global::<Drawer>().draw(move || {
            player
                .art(&asked)
                .and_then(|art| magnified_of(art.as_ref()))
        });
        cx.spawn(async move |this, cx| {
            let read = drawing.await.flatten();
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
                } else {
                    read.forget(cx);
                }
            });
            let _ = landed;
        })
        .detach();
        None
    }

    pub fn send(&self, command: Command) {
        if let Err(error) = self.player.send(command) {
            tracing::error!(?error, "the engine refused a transport command");
        }
    }

    pub fn send_by_position(&self, command: Command) {
        if let Err(error) = self
            .player
            .send_if_the_queue_is_still(self.queued.revision, command)
        {
            tracing::error!(?error, "the engine refused a transport command");
        }
    }

    fn title_of(&self, track: TrackId) -> Option<String> {
        let queue = self.player.queue();
        let item = self
            .queued
            .rows
            .iter()
            .chain(queue.iter())
            .find(|item| item.id == track)?;

        Some(
            self.player
                .media(&item.location, item.span)
                .and_then(|info| info.tags.title.clone())
                .unwrap_or_else(|| format::stem(&item.location)),
        )
    }

    pub const fn poll_interval() -> Duration {
        POLL_INTERVAL
    }

    fn refresh(&mut self, cx: &mut Context<Self>) -> Poll {
        let mut moved = None;

        if let Some(outcome) = self.seeking.as_ref().and_then(|seek| seek.outcome.poll()) {
            self.seeking = None;
            moved = Some(Moved::More);
            if let Err(error) = outcome {
                Self::seek_refused(error, cx);
            }
        }

        for event in self.player.events().try_iter() {
            moved = Some(Moved::More);
            match event {
                Event::Failed { track, error } => {
                    tracing::error!(%error, %track, "playback failed");
                    let said = match self
                        .title_of(track)
                        .or_else(|| error.location().map(format::stem))
                    {
                        Some(title) => toast::would_not_play(error.cause(), &title),
                        None => toast::would_not_do(CommandKind::Play, error.cause()),
                    };
                    toast::tell(Notice::Trouble(said), cx);
                }
                Event::Waiting { track, error } => {
                    tracing::warn!(%error, %track, "playback waits for a device");
                    toast::tell(
                        Notice::Trouble(toast::waits_for_a_device(error.cause()).to_owned()),
                        cx,
                    );
                }
                Event::CommandFailed { command, error } => {
                    tracing::warn!(%error, ?command, "the engine refused a command");
                    toast::tell(
                        Notice::Trouble(toast::would_not_do(command, error.cause())),
                        cx,
                    );
                }
                Event::OutputChanged(_)
                | Event::TrackStarted(_)
                | Event::TrackFinished(_)
                | Event::QueueFinished
                | Event::Underrun { .. } => {}
            }
        }

        let state = read_to_the_second(self.player.state());
        if self
            .seeking
            .as_ref()
            .is_some_and(|seek| state.current.map(|track| track.id) != Some(seek.track))
        {
            self.seeking = None;
            moved = Some(Moved::More);
        }
        if state != self.state {
            if self.grain.shows(&self.state, &state) {
                moved = moved.max(Some(moved_by(&self.state, &state)));
            }
            self.state = state;
        }

        let settings = self.player.output_settings();
        if !Arc::ptr_eq(&settings, &self.settings) {
            self.settings = settings;
            moved = Some(Moved::More);
        }

        let sinks = self.player.sinks();
        if !Arc::ptr_eq(&sinks, &self.sinks) {
            self.sinks = sinks;
            moved = Some(Moved::More);
        }

        let digest = self.player.digest();
        if !is_same_digest(digest.as_ref(), self.digest.as_ref()) {
            let grown = only_the_profile_grew(self.digest.as_deref(), digest.as_deref());
            self.digest = digest;
            self.graphed = None;
            if !grown {
                self.carried_lines = None;
            }
            moved = moved.max(Some(if grown { Moved::Clock } else { Moved::More }));
        }

        let queued = self.player.queued();
        if queued.revision != self.queued.revision {
            self.queued = queued;
            moved = Some(Moved::More);
        }

        let reads = self.player.media_revision();
        if reads != self.reads {
            self.reads = reads;
            moved = Some(Moved::More);
        }

        let busy = moved.is_some()
            || self.seeking.is_some()
            || self.state.playback == PlaybackState::Playing
            || self.state.sleeping.is_some();
        if let Some(moved) = moved {
            self.moved = moved;
            cx.notify();
        }
        if busy { Poll::Busy } else { Poll::Quiet }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Poll {
    Busy,
    Quiet,
}

fn moved_by(before: &PlayerState, after: &PlayerState) -> Moved {
    if held_still(after, before) == *before {
        Moved::Clock
    } else {
        Moved::More
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

fn only_the_profile_grew(before: Option<&StreamDigest>, after: Option<&StreamDigest>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => after.only_grew_from(before),
        _ => false,
    }
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

const MENU_KEY: &str = "menu";

fn answering_to_the_keys_a_keyboard_carries() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(PLAY_KEY, TogglePlayPause, None),
        KeyBinding::new(PAUSE_KEY, Pause, None),
        KeyBinding::new(STOP_KEY, Stop, None),
        KeyBinding::new(NEXT_TRACK_KEY, Next, None),
        KeyBinding::new(PREVIOUS_TRACK_KEY, Previous, None),
        KeyBinding::new(FORWARD_KEY, Next, None),
        KeyBinding::new(BACK_KEY, Previous, None),
    ]
}

fn answering_as_a_mac_expects() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimise, None),
        KeyBinding::new("cmd-w", CloseTheWindow, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
    ]
}

fn answering_anywhere(system: OperatingSystem) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(chord!(next_anywhere).on(system), Next, None),
        KeyBinding::new(chord!(previous_anywhere).on(system), Previous, None),
        KeyBinding::new(chord!(find).on(system), FocusSearch, None),
        KeyBinding::new("tab", ReachNext, None),
        KeyBinding::new("shift-tab", ReachPrevious, None),
        KeyBinding::new(chord!(next_pane).on(system), NextPane, None),
        KeyBinding::new(chord!(previous_pane).on(system), PreviousPane, None),
        KeyBinding::new(chord!(find_a_setting).on(system), FocusFilter, None),
        KeyBinding::new(chord!(listen).on(system), Listen, None),
        KeyBinding::new(chord!(quit).on(system), Quit, None),
    ]
}

fn answering_away_from_a_field(typed: Option<&str>, system: OperatingSystem) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(chord!(play_pause).on(system), PlayPauseUnlessTyping, typed),
        KeyBinding::new(chord!(stop).on(system), Stop, typed),
        KeyBinding::new(chord!(seek).on(system), SeekForward, typed),
        KeyBinding::new(chord!(seek_back).on(system), SeekBackward, typed),
        KeyBinding::new(chord!(seek_further).on(system), SeekFurtherForward, typed),
        KeyBinding::new(
            chord!(seek_further_back).on(system),
            SeekFurtherBackward,
            typed,
        ),
        KeyBinding::new(chord!(mute).on(system), ToggleMute, typed),
        KeyBinding::new(chord!(queue).on(system), ToggleQueue, typed),
        KeyBinding::new(chord!(hold_the_picture).on(system), HoldThePicture, typed),
        KeyBinding::new(chord!(next).on(system), Next, typed),
        KeyBinding::new(chord!(previous).on(system), Previous, typed),
        KeyBinding::new(chord!(shuffle).on(system), ToggleShuffle, typed),
        KeyBinding::new(chord!(repeat).on(system), CycleRepeat, typed),
        KeyBinding::new(chord!(louder).on(system), VolumeUp, typed),
        KeyBinding::new(chord!(quieter).on(system), VolumeDown, typed),
        KeyBinding::new(chord!(reach_above).on(system), ReachAbove, typed),
        KeyBinding::new(chord!(reach_below).on(system), ReachBelow, typed),
        KeyBinding::new(chord!(widen_above).on(system), WidenAbove, typed),
        KeyBinding::new(chord!(widen_below).on(system), WidenBelow, typed),
        KeyBinding::new(chord!(reach_first).on(system), ReachFirst, typed),
        KeyBinding::new(chord!(reach_last).on(system), ReachLast, typed),
        KeyBinding::new(chord!(reach_page_above).on(system), ReachPageAbove, typed),
        KeyBinding::new(chord!(reach_page_below).on(system), ReachPageBelow, typed),
        KeyBinding::new(chord!(reach_everything).on(system), ReachEverything, typed),
        KeyBinding::new(chord!(choose_reached).on(system), ChooseReached, typed),
        KeyBinding::new(chord!(hop_above).on(system), HopAbove, typed),
        KeyBinding::new(chord!(hop_below).on(system), HopBelow, typed),
        KeyBinding::new(chord!(drop_reached).on(system), DropReached, typed),
        KeyBinding::new(chord!(raise_row).on(system), RaiseRow, typed),
        KeyBinding::new(chord!(lower_row).on(system), LowerRow, typed),
        KeyBinding::new(chord!(play_reached).on(system), PlayReached, typed),
        KeyBinding::new(chord!(open_menu).on(system), OpenTheMenu, typed),
        KeyBinding::new(MENU_KEY, OpenTheMenu, typed),
        KeyBinding::new(chord!(undo).on(system), UndoEdit, typed),
        KeyBinding::new(chord!(redo).on(system), RedoEdit, typed),
        KeyBinding::new(chord!(redo_also).on(system), RedoEdit, typed),
        KeyBinding::new(chord!(paste).on(system), PasteAway, typed),
    ]
}

fn answering_where_the_caret_is(
    on_a_control: Option<&str>,
    system: OperatingSystem,
) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(chord!(leave).on(system), LeaveSearch, Some(SEARCH_CONTEXT)),
        KeyBinding::new("down", GoToTheResults, Some(SEARCH_CONTEXT)),
        KeyBinding::new("tab", TabOnward, Some(SEARCH_CONTEXT)),
        KeyBinding::new("space", PressControl, on_a_control),
        KeyBinding::new("enter", PressControl, on_a_control),
        KeyBinding::new("escape", LeaveControl, on_a_control),
    ]
}

fn answering_on_a_band(on_a_band: Option<&str>, system: OperatingSystem) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(chord!(band_higher).on(system), BandHigher, on_a_band),
        KeyBinding::new(chord!(band_lower).on(system), BandLower, on_a_band),
        KeyBinding::new(chord!(band_louder).on(system), BandLouder, on_a_band),
        KeyBinding::new(chord!(band_quieter).on(system), BandQuieter, on_a_band),
        KeyBinding::new(chord!(band_narrower).on(system), BandNarrower, on_a_band),
        KeyBinding::new(chord!(band_wider).on(system), BandWider, on_a_band),
    ]
}

fn answering_on_a_rail(on_a_rail: Option<&str>, system: OperatingSystem) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(chord!(rail_on).on(system), RailOn, on_a_rail),
        KeyBinding::new(chord!(rail_back).on(system), RailBack, on_a_rail),
        KeyBinding::new(chord!(rail_up).on(system), RailOn, on_a_rail),
        KeyBinding::new(chord!(rail_down).on(system), RailBack, on_a_rail),
    ]
}

fn answering_in_the_order(in_the_order: Option<&str>, system: OperatingSystem) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(chord!(order_above).on(system), OrderAbove, in_the_order),
        KeyBinding::new(chord!(order_below).on(system), OrderBelow, in_the_order),
        KeyBinding::new(chord!(order_first).on(system), OrderFirst, in_the_order),
        KeyBinding::new(chord!(order_last).on(system), OrderLast, in_the_order),
        KeyBinding::new(
            chord!(read_the_first_way).on(system),
            ReadTheFirstWay,
            in_the_order,
        ),
        KeyBinding::new(
            chord!(read_the_other_way).on(system),
            ReadTheOtherWay,
            in_the_order,
        ),
        KeyBinding::new(
            chord!(take_the_order).on(system),
            TakeTheOrder,
            in_the_order,
        ),
    ]
}

pub(crate) fn bindings(system: OperatingSystem) -> Vec<KeyBinding> {
    let away_from_search = format!("!{SEARCH_CONTEXT} && !{CONTROL_CONTEXT}");

    let mut bindings = if system == OperatingSystem::Mac {
        answering_as_a_mac_expects()
    } else {
        answering_to_the_keys_a_keyboard_carries()
    };
    bindings.extend(answering_anywhere(system));
    bindings.extend(answering_away_from_a_field(Some(&away_from_search), system));
    bindings.extend(answering_where_the_caret_is(Some(CONTROL_CONTEXT), system));
    bindings.extend(answering_on_a_band(Some(BAND_CONTEXT), system));
    bindings.extend(answering_on_a_rail(Some(RAIL_CONTEXT), system));
    bindings.extend(answering_in_the_order(Some(ORDER_CONTEXT), system));
    bindings.extend(field::bindings(system));
    bindings
}

pub const fn seek_step() -> i64 {
    SEEK_STEP_SECONDS
}

pub(crate) const fn seek_further() -> i64 {
    SEEK_FURTHER_SECONDS
}

pub struct Bus {
    pub quit: Receiver<()>,
    pub raise: Receiver<()>,
    pub attention: Sender<bool>,
    pub followed: Receiver<Followed>,
    pub look: DesktopLook,
    pub desktop: Receiver<DesktopLook>,
    pub opened: Sender<Vec<String>>,
}

pub type Leaving = Box<dyn FnOnce()>;

struct LeftOnce(RefCell<Option<Leaving>>);

impl LeftOnce {
    fn by(leaving: Leaving) -> Rc<Self> {
        Rc::new(Self(RefCell::new(Some(leaving))))
    }

    fn leave(&self) {
        let leaving = self.0.borrow_mut().take();
        if let Some(leaving) = leaving {
            leaving();
        }
    }
}

fn follow_the_desktop_when_told(told: Receiver<DesktopLook>, cx: &mut App) {
    let waiting = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            match told.try_recv() {
                Ok(look) => {
                    cx.update(|cx| {
                        theme::hear_the_desktop(look);
                        cx.global::<ResonateApp>()
                            .launcher
                            .show(AppIcon::of(theme::worn()));
                        cx.refresh_windows();
                    });
                }
                Err(TryRecvError::Empty) => waiting.timer(QUIT_POLL).await,
                Err(TryRecvError::Disconnected) => return,
            }
        }
    })
    .detach();
}

fn follow_when_told(told: Receiver<Followed>, cx: &mut App) {
    let waiting = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            match told.try_recv() {
                Ok(followed) => {
                    cx.update(|cx| {
                        for handle in cx.windows() {
                            let Some(root) = handle.downcast::<RootView>() else {
                                continue;
                            };
                            let arrives = matches!(
                                followed,
                                Followed::PartyOffered(_) | Followed::ListOffered(_)
                            );
                            let _ = root.update(cx, |root, window, cx| {
                                root.follow(followed.clone(), cx);
                                if arrives {
                                    window.activate_window();
                                }
                            });
                        }
                    });
                }
                Err(TryRecvError::Empty) => waiting.timer(QUIT_POLL).await,
                Err(TryRecvError::Disconnected) => return,
            }
        }
    })
    .detach();
}

fn raise_when_asked(asked: Receiver<()>, cx: &mut App) {
    let waiting = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            match asked.try_recv() {
                Ok(()) => {
                    cx.update(|cx| {
                        for handle in cx.windows() {
                            let _ = handle.update(cx, |_, window, _| window.activate_window());
                        }
                    });
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
            cx.update(close_every_window);
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
    cx.on_window_closed(|cx, _window| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

pub fn run(
    zones: Arc<Zones>,
    library: Arc<Library>,
    lookups: Lookups,
    stored: Stored,
    appearance: Appearance,
    bus: Bus,
    leaving: Leaving,
) -> Result<()> {
    let first_read = FirstRead::start(&library);
    let winding_down = Arc::clone(&library);
    let copying = Copying::default();
    let copying_when_closed = copying.clone();
    theme::wear(appearance);
    theme::hear_the_desktop(bus.look);
    stored.launcher.show(AppIcon::of(theme::worn()));

    let failure = Rc::new(RefCell::new(None));
    let recorded = Rc::clone(&failure);

    let left = LeftOnce::by(Box::new(move || {
        if !copying_when_closed.wind_down(WIND_DOWN_WITHIN) {
            tracing::warn!("a copy of dropped songs was still running when the window left");
        }
        if !winding_down.wind_down(WIND_DOWN_WITHIN) {
            tracing::warn!("a library pass was still running when the window left");
        }
        leaving();
    }));
    let left_as_the_app_quits = Rc::clone(&left);

    let application = gpui_platform::application().with_assets(icons::Embedded);
    let opened = bus.opened.clone();
    application.on_open_urls(move |links| {
        if opened.send(links).is_err() {
            tracing::warn!("nothing takes what the system opens any more");
        }
    });

    application.run(move |cx: &mut App| {
        cx.on_app_quit(move |_| {
            left_as_the_app_quits.leave();
            future::ready(())
        })
        .detach();
        let carried = crate::fonts::bundled();
        if !carried.is_empty()
            && let Err(error) = cx.text_system().add_fonts(carried)
        {
            tracing::warn!(?error, "the fonts the application carries could not be loaded");
        }
        crate::fonts::settle(&cx.text_system().all_font_names());
        cx.set_global(Drawer::new());
        cx.set_global(ResonateApp {
            zones: Arc::clone(&zones),
            library,
            lyricists: Arc::clone(&lookups.lyricists),
            fingerprinters: Arc::clone(&lookups.fingerprinters),
            corrections: Arc::clone(&lookups.corrections),
            settings: Arc::clone(&stored.settings),
            bindings: lookups.bindings.clone(),
            measurers: lookups.measurers.clone(),
            measures: lookups.measures.clone(),
            online: lookups.online.clone(),
            reference: lookups.reference.clone(),
            for_the_pass: lookups.for_the_pass.clone(),
            scrobblers: lookups.scrobblers.clone(),
            signs_in: lookups.signs_in.clone(),
            spotify: lookups.spotify.clone(),
            checks_youtube_music: lookups.checks_youtube_music.clone(),
            signs_in_to_youtube_music: lookups.signs_in_to_youtube_music.clone(),
            signs_in_to_jellyfin: lookups.signs_in_to_jellyfin.clone(),
            attention: bus.attention.clone(),
            places: stored.places.clone(),
            resume: stored.resume,
            history_kept: stored.history_kept,
            organise_as: stored.organise_as.clone(),
            notify: Arc::clone(&stored.notify),
            by_sound: Arc::clone(&stored.by_sound),
            lyrics_by_the_locale: Arc::clone(&stored.lyrics_by_the_locale),
            flac_above_96k_for: stored.flac_above_96k_for.clone(),
            convolution: stored.convolution.clone(),
            speaker_angle: stored.speaker_angle,
            speaker_elevation: stored.speaker_elevation,
            head_yaw: stored.head_yaw,
            music_folder: stored.music_folder.clone(),
            file_dropped: stored.file_dropped,
            window_buttons: stored.window_buttons,
            scroll_volume: stored.scroll_volume,
            mouse_navigation: stored.mouse_navigation,
            reduced_motion: stored.reduced_motion,
            caret: stored.caret.clone(),
            scrollbars: stored.scrollbars,
            language: stored.language,
            spectral: stored.spectral,
            tabs: stored.tabs,
            remember_tab: stored.remember_tab,
            last_tab: stored.last_tab,
            artists_drawn: stored.artists_drawn,
            remember_window_size: stored.remember_window_size,
            window_size: stored.window_size,
            remember_settings_category: stored.remember_settings_category,
            last_settings_category: stored.last_settings_category,
            presence: stored.presence.clone(),
            present: Arc::clone(&stored.present),
            renderers: stored.renderers.clone(),
            synchronised: stored.synchronised.clone(),
            syncing: stored.syncing.clone(),
            together: stored.together.clone(),
            reads_as_a_code: stored.reads_as_a_code,
            launcher: Arc::clone(&stored.launcher),
            updates: stored.updates.clone(),
            check_for_updates: stored.check_for_updates,
            update_dismissed: stored.update_dismissed.clone(),
            sourcing: lookups.sourcing.clone(),
            serving: lookups.serving.clone(),
            listens: lookups.listens.clone(),
            mirroring: lookups.mirroring.clone(),
            first_read,
            copying,
        });
        cx.set_reduce_motion(stored.reduced_motion);
        cx.bind_keys(bindings(cx.platform_info().os));
        crate::app_menu::lay_out(cx);
        cx.activate(true);
        close_when_asked(bus.quit, cx);
        raise_when_asked(bus.raise, cx);
        follow_when_told(bus.followed, cx);
        follow_the_desktop_when_told(bus.desktop, cx);
        quit_once_the_last_window_closes(cx);

        let opening_size = stored
            .window_size
            .filter(|_| stored.remember_window_size)
            .map(WindowSize::pixels)
            .unwrap_or_else(|| size(px(1_280.0), px(820.0)));
        let bounds = Bounds::centered(None, opening_size, cx);
        let options = WindowOptions {
            app_id: Some(APP_ID.to_owned()),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(WINDOW_TITLE.into()),
                appears_transparent: chrome::THE_HEADER_STANDS_IN_THE_TITLE_BAR,
                traffic_light_position: chrome::traffic_lights_placed(),
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

    left.leave();

    match failure.borrow_mut().take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub(crate) fn sink_in_use<'s>(
    sinks: &'s [SinkInfo],
    open: Option<SinkId>,
    named: Option<&NodeName>,
) -> Option<&'s SinkInfo> {
    sinks
        .iter()
        .find(|sink| open == Some(sink.id))
        .or_else(|| {
            sinks
                .iter()
                .find(|sink| named.is_some_and(|named| sink.is_named(named)))
        })
        .or_else(|| sinks.iter().find(|sink| sink.is_default))
        .or_else(|| sinks.first())
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

    fn device(id: u32, name: &str, is_default: bool) -> SinkInfo {
        SinkInfo {
            id: SinkId::new(id),
            name: NodeName::new(name),
            aliases: Vec::new(),
            description: name.to_owned(),
            is_default,
            is_hardware: true,
            port: None,
            profile: None,
            formats: Vec::new(),
            allowed_rates: Vec::new(),
            current_rate: None,
        }
    }

    #[test]
    fn the_sink_in_use_is_the_one_open_else_the_one_the_engine_would_choose() {
        let sinks = [
            device(1, "speakers", true),
            device(2, "dac", false),
            device(3, "hdmi", false),
        ];
        let dac = NodeName::new("dac");
        let gone = NodeName::new("unplugged");

        let named = |sink: Option<&SinkInfo>| sink.map(|sink| sink.name.as_str().to_owned());

        assert_eq!(
            named(sink_in_use(&sinks, Some(SinkId::new(3)), Some(&dac))),
            Some("hdmi".to_owned())
        );
        assert_eq!(
            named(sink_in_use(&sinks, None, Some(&dac))),
            Some("dac".to_owned())
        );
        assert_eq!(
            named(sink_in_use(&sinks, Some(SinkId::new(9)), Some(&gone))),
            Some("speakers".to_owned())
        );
        assert_eq!(
            named(sink_in_use(&sinks[1..], None, None)),
            Some("dac".to_owned())
        );
        assert_eq!(named(sink_in_use(&[], None, None)), None);
    }

    #[test]
    fn what_is_done_on_leaving_is_done_once_whichever_way_the_window_leaves() {
        let done = Rc::new(std::cell::Cell::new(0));
        let counted = Rc::clone(&done);
        let left = LeftOnce::by(Box::new(move || counted.set(counted.get() + 1)));
        let as_the_app_quits = Rc::clone(&left);

        as_the_app_quits.leave();
        left.leave();
        as_the_app_quits.leave();

        assert_eq!(done.get(), 1);
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

    const SYSTEMS: [OperatingSystem; 3] = [
        OperatingSystem::Linux,
        OperatingSystem::Windows,
        OperatingSystem::Mac,
    ];

    fn spelt(binding: &KeyBinding, system: OperatingSystem) -> String {
        binding
            .keystrokes()
            .iter()
            .map(|stroke| stroke.unparse(system))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn done_by(binding: &KeyBinding) -> &'static str {
        binding
            .action()
            .name()
            .rsplit("::")
            .next()
            .expect("an action is named")
    }

    fn bound_to(keys: &'static str, system: OperatingSystem) -> Vec<&'static str> {
        let named = Keystroke::parse(keys)
            .unwrap_or_else(|_| panic!("{keys} is not a keystroke gpui can read"))
            .unparse(system);

        bindings(system)
            .iter()
            .filter(|binding| spelt(binding, system) == named)
            .map(done_by)
            .collect()
    }

    #[test]
    fn every_binding_the_window_carries_is_one_gpui_can_read() {
        for system in SYSTEMS {
            assert!(bindings(system).len() > 20);
        }
    }

    #[test]
    fn every_key_not_named_to_answer_anywhere_is_dead_while_a_caret_is_held() {
        let away_from_search = format!("!{SEARCH_CONTEXT} && !{CONTROL_CONTEXT}");

        for system in SYSTEMS {
            for binding in answering_away_from_a_field(Some(&away_from_search), system) {
                let keys = format!("{:?}", binding.keystrokes());
                let predicate = binding.predicate().unwrap_or_else(|| {
                    panic!("{keys} is bound away from a field with no predicate")
                });

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
    }

    #[test]
    fn no_chord_answers_twice_where_the_same_things_hold_the_caret_on_any_system() {
        for system in SYSTEMS {
            let mut seen = AHashSet::new();

            for binding in bindings(system) {
                let held = binding
                    .predicate()
                    .map(|predicate| format!("{predicate:?}"))
                    .unwrap_or_default();
                let keys = spelt(&binding, system);

                assert!(
                    seen.insert((held.clone(), keys.clone())),
                    "{keys} is bound twice under {held:?} on {system:?}"
                );
            }
        }
    }

    #[test]
    fn the_chords_a_mac_keeps_for_itself_do_what_the_system_says_and_nothing_else() {
        let mac = OperatingSystem::Mac;

        assert_eq!(bound_to("cmd-h", mac), ["Hide"]);
        assert_eq!(bound_to("alt-cmd-h", mac), ["HideOthers"]);
        assert_eq!(bound_to("cmd-m", mac), ["Minimise"]);
        assert_eq!(bound_to("cmd-w", mac), ["CloseTheWindow"]);
        assert_eq!(bound_to("cmd-q", mac), ["Quit"]);
        assert_eq!(bound_to("cmd-,", mac), ["FocusFilter"]);
        assert_eq!(bound_to("ctrl-cmd-f", mac), ["ToggleFullScreen"]);
        assert!(bound_to("cmd-space", mac).is_empty());
        assert!(bound_to("cmd-tab", mac).is_empty());
        assert!(bound_to("cmd-shift-tab", mac).is_empty());
    }

    #[test]
    fn a_mac_steers_with_command_and_leaves_control_to_the_chords_command_cannot_take() {
        let mac = OperatingSystem::Mac;

        assert_eq!(bound_to("cmd-u", mac), ["ToggleQueue"]);
        assert_eq!(bound_to("cmd-s", mac), ["Stop"]);
        assert_eq!(bound_to("cmd-f", mac), ["FocusSearch"]);
        assert_eq!(bound_to("cmd-up", mac), ["VolumeUp"]);
        assert_eq!(bound_to("ctrl-m", mac), ["ToggleMute"]);
        assert_eq!(bound_to("ctrl-space", mac), ["ChooseReached"]);
        assert_eq!(bound_to("ctrl-tab", mac), ["NextPane"]);
        assert!(bound_to("ctrl-u", mac).is_empty());
        assert!(bound_to("ctrl-s", mac).is_empty());
        assert!(bound_to("ctrl-q", mac).is_empty());
    }

    #[test]
    fn nothing_a_mac_alone_binds_is_bound_anywhere_else() {
        for system in [OperatingSystem::Linux, OperatingSystem::Windows] {
            for binding in bindings(system) {
                assert!(
                    binding
                        .keystrokes()
                        .iter()
                        .all(|stroke| !stroke.modifiers().platform),
                    "{} takes the system's own key on {system:?}",
                    spelt(&binding, system)
                );
            }

            assert_eq!(bound_to("ctrl-u", system), ["ToggleQueue"]);
            assert_eq!(bound_to("ctrl-,", system), ["FocusFilter"]);
            assert_eq!(bound_to("ctrl-h", system), ["ToggleShuffle"]);
        }
    }

    #[test]
    fn a_mac_binds_none_of_the_keys_only_another_keyboard_carries() {
        for named in [
            PLAY_KEY,
            PAUSE_KEY,
            STOP_KEY,
            NEXT_TRACK_KEY,
            PREVIOUS_TRACK_KEY,
        ] {
            assert!(bound_to(named, OperatingSystem::Mac).is_empty());
            assert_eq!(bound_to(named, OperatingSystem::Linux).len(), 1);
        }
    }

    #[test]
    fn the_arrows_in_the_sort_pop_up_are_its_own_before_they_are_the_windows() {
        let keymap = gpui::Keymap::new(bindings(OperatingSystem::Linux));
        let window = KeyContext::parse(WINDOW_CONTEXT).expect("the window names itself");
        let pop_up = KeyContext::parse(ORDER_CONTEXT).expect("the pop-up names itself");
        let control = KeyContext::parse(CONTROL_CONTEXT).expect("the control names itself");
        let on_the_pop_up = [window.clone(), pop_up.clone()];
        let on_one_of_its_orders = [window, pop_up, control];

        for (key, wanted) in [
            ("up", "OrderAbove"),
            ("down", "OrderBelow"),
            ("home", "OrderFirst"),
            ("end", "OrderLast"),
            ("left", "ReadTheFirstWay"),
            ("right", "ReadTheOtherWay"),
        ] {
            let typed = [Keystroke::parse(key).expect("a key gpui reads")];
            for held in [&on_the_pop_up[..], &on_one_of_its_orders[..]] {
                let (found, _) = keymap.bindings_for_input(&typed, held);
                let first = found
                    .first()
                    .unwrap_or_else(|| panic!("{key} does nothing in the sort pop-up"));
                assert!(
                    first.action().name().ends_with(wanted),
                    "{key} in the sort pop-up is {}",
                    first.action().name()
                );
            }
        }

        let enter = [Keystroke::parse("enter").expect("a key gpui reads")];
        let (on_the_panel, _) = keymap.bindings_for_input(&enter, &on_the_pop_up);
        let (on_an_order, _) = keymap.bindings_for_input(&enter, &on_one_of_its_orders);
        assert!(on_the_panel[0].action().name().ends_with("TakeTheOrder"));
        assert!(on_an_order[0].action().name().ends_with("PressControl"));
    }

    #[test]
    fn down_and_tab_in_a_field_are_the_fields_own_before_they_are_the_windows() {
        let keymap = gpui::Keymap::new(bindings(OperatingSystem::Linux));
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
        for system in SYSTEMS {
            for binding in answering_anywhere(system) {
                assert!(
                    binding.predicate().is_none(),
                    "a key named to answer anywhere carries a predicate"
                );
            }
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

    #[test]
    fn only_the_moment_moving_is_told_as_the_clock() {
        let before = playing_at(10.1);
        let paused = PlayerState {
            playback: PlaybackState::Paused,
            ..playing_at(10.2)
        };

        assert_eq!(moved_by(&before, &playing_at(10.2)), Moved::Clock);
        assert_eq!(moved_by(&before, &paused), Moved::More);
    }
}
