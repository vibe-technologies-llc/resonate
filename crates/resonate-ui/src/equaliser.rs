use std::{cell::Cell as Kept, collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use gpui::{Context, Task};
use resonate_core::{
    SampleRate,
    eq::{Band, BandGain, BandKind, Frequency, MAX_BANDS, Preamp, Profile, Q, Traced, TracedOn},
};
use resonate_engine::{Equalisation, NodeName};
use resonate_eq::{
    Binding, BuiltIn, Catalogue, Corrected, Device, DeviceId, Fitted, Fitting, Found, Measured,
    ProfileName, Response, Store, fit, search_preferring, suggest_preferring,
};
use resonate_listen::{Hearing, Listening};
use thiserror::Error;

use crate::{
    Bindings, Measurers,
    measure::{Measures, Unmeasured},
    models::Notice,
    toast,
};

pub const CURVE_COLUMNS: usize = 192;
pub const DRAWN_BETWEEN_MILLIBELS: i32 = 15_000;
const PROFILE_SETTLES: Duration = Duration::from_millis(600);
const FOUND_SHOWN: usize = 12;
const Q_PER_NOTCH: f64 = 1.122_462_048_309_373;
const Q_STEPS_PER_UNIT: f64 = 100.0;
const MILLI_PER_UNIT: f64 = 1_000.0;
const SEMITONES_AN_OCTAVE: f64 = 12.0;
const MILLIBELS_A_NUDGE: i32 = 500;
const NOTCHES_A_NUDGE: f64 = 1.0;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Curve {
    Kept(ProfileName),
    Own(Option<NodeName>),
}

impl Curve {
    pub fn of(owner: Option<&NodeName>, binding: &Binding) -> Self {
        match binding {
            Binding::Profile(name) => Self::Kept(name.clone()),
            Binding::Own => Self::Own(owner.cloned()),
        }
    }

    pub fn spoken(&self) -> String {
        match self {
            Self::Kept(name) => name.to_string(),
            Self::Own(Some(_)) => "This device's own curve".to_owned(),
            Self::Own(None) => "Every other device's own curve".to_owned(),
        }
    }

    pub fn file_name(&self) -> String {
        match self {
            Self::Kept(name) => format!("{name}.txt"),
            Self::Own(_) => "own curve.txt".to_owned(),
        }
    }

    fn read(&self, store: &Store) -> resonate_eq::Result<Option<Profile>> {
        match self {
            Self::Kept(name) => store.read(name),
            Self::Own(owner) => store.own(owner.as_ref().map(NodeName::as_str)).map(Some),
        }
    }

    fn file(&self, store: &Store) -> resonate_eq::Result<PathBuf> {
        match self {
            Self::Kept(name) => Ok(store.path_of(name)),
            Self::Own(owner) => store.own_path(owner.as_ref().map(NodeName::as_str)),
        }
    }

    fn keep(&self, store: &Store, profile: &Profile) -> resonate_eq::Result<()> {
        match self {
            Self::Kept(name) => store.keep(name, profile).map(|_| ()),
            Self::Own(owner) => store
                .keep_own(owner.as_ref().map(NodeName::as_str), profile)
                .map(|_| ()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub frequency: Frequency,
    pub gain: BandGain,
}

fn fitted_said(name: &ProfileName, aimed_at: &str, measured: &Measured) -> String {
    let passed_over = match measured.passed_over {
        0 => String::new(),
        lines => format!("; {lines} lines of the measurement were passed over"),
    };
    format!(
        "{}{passed_over}",
        fit_said(name, aimed_at, &measured.fitted)
    )
}

fn fit_said(name: &ProfileName, aimed_at: &str, fitted: &Fitted) -> String {
    format!(
        "Kept {name}, {} bands fitted toward {aimed_at}: {:.1} dB off before and {:.1} dB after",
        fitted.profile.bands().len(),
        fitted.off_by_db_before,
        fitted.off_by_db_after,
    )
}

const MEASURED_LIVE: &str = "measured";

#[derive(Debug, Error)]
enum Missed {
    #[error("the room could not be heard")]
    Heard(#[source] Unmeasured),

    #[error("the room's measurement could not be fitted or kept")]
    Fitted(#[source] resonate_eq::Error),
}

pub struct Sounding {
    pub rig: Arc<dyn Measures>,
    pub through: Option<NodeName>,
    pub from: Listening,
    pub called: Option<ProfileName>,
}

fn missed_the_room(error: &Missed) -> Notice {
    match error {
        Missed::Heard(unmeasured) if unmeasured.is_stopped() => {
            Notice::Noted("The measurement was stopped; nothing was kept".to_owned())
        }
        Missed::Heard(unmeasured) if unmeasured.is_silence() => Notice::Trouble(
            "Nothing was heard — check the microphone is on and the output can be heard".to_owned(),
        ),
        Missed::Heard(unmeasured) if unmeasured.is_a_missing_output() => {
            Notice::Trouble("The output to play the sweep through isn't there".to_owned())
        }
        Missed::Heard(_) => Notice::Trouble("Couldn't play the sweep and hear it".to_owned()),
        Missed::Fitted(error) => toast::eq_could_not("fit that measurement", error),
    }
}

fn caveats(kept: &resonate_eq::Kept) -> String {
    let mut said = String::new();
    if kept.passed_over > 0 {
        said.push_str(&format!(
            "; {} lines this build does not read were passed over",
            kept.passed_over
        ));
    }
    if kept.approximated > 0 {
        said.push_str(&format!(
            "; {} filters name a rolloff it reads as second order",
            kept.approximated
        ));
    }
    if kept.clamped > 0 {
        said.push_str(&format!(
            "; {} values past what it holds were held to its limits",
            kept.clamped
        ));
    }
    said
}

pub fn a_band_at(placed: Placed) -> Band {
    Band::peaking(placed.frequency, placed.gain, Q::BUTTERWORTH)
}

pub fn moved(band: Band, to: Placed) -> Band {
    Band {
        on: band.on,
        channels: band.channels,
        ..Band::new(band.kind, to.frequency, to.gain, band.q)
    }
}

pub fn narrowed(q: Q, notches: f64) -> Q {
    if !notches.is_finite() || notches == 0.0 {
        return q;
    }
    let lowest = f64::from(Q::WIDEST_MILLI) / MILLI_PER_UNIT;
    let highest = f64::from(Q::NARROWEST_MILLI) / MILLI_PER_UNIT;
    let steps_held = f64::from(q.milli()) / MILLI_PER_UNIT * Q_STEPS_PER_UNIT;
    let steps_turned = (q.units() * Q_PER_NOTCH.powf(notches) * Q_STEPS_PER_UNIT).round();

    let steps = if notches > 0.0 {
        steps_turned.max(steps_held.floor() + 1.0)
    } else {
        steps_turned.min(steps_held.ceil() - 1.0)
    };
    Q::from_units((steps / Q_STEPS_PER_UNIT).clamp(lowest, highest)).unwrap_or(q)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nudge {
    Higher,
    Lower,
    Louder,
    Quieter,
    Narrower,
    Wider,
}

pub fn nudged(band: Band, nudge: Nudge) -> Band {
    let placed = |frequency: Frequency, gain: BandGain| moved(band, Placed { frequency, gain });
    let semitone = |up: bool| {
        let step = if up { 1.0 } else { -1.0 } / SEMITONES_AN_OCTAVE;
        let hertz = (band.frequency.hertz() * 2_f64.powf(step))
            .clamp(Frequency::LOWEST.hertz(), Frequency::HIGHEST.hertz());
        Frequency::from_hertz(hertz).unwrap_or(band.frequency)
    };
    let louder = |by: i32| {
        let milli_decibels = (band.gain.milli_decibels() + by).clamp(
            -BandGain::WIDEST_MILLI_DECIBELS,
            BandGain::WIDEST_MILLI_DECIBELS,
        );
        BandGain::from_milli_decibels(milli_decibels).unwrap_or(band.gain)
    };

    match nudge {
        Nudge::Higher => placed(semitone(true), band.gain),
        Nudge::Lower => placed(semitone(false), band.gain),
        Nudge::Louder => placed(band.frequency, louder(MILLIBELS_A_NUDGE)),
        Nudge::Quieter => placed(band.frequency, louder(-MILLIBELS_A_NUDGE)),
        Nudge::Narrower => Band {
            q: narrowed(band.q, NOTCHES_A_NUDGE),
            ..band
        },
        Nudge::Wider => Band {
            q: narrowed(band.q, -NOTCHES_A_NUDGE),
            ..band
        },
    }
}

pub const fn chosen_after_dropping(chosen: Option<usize>, dropped: usize) -> Option<usize> {
    match chosen {
        Some(row) if row == dropped => None,
        Some(row) if row > dropped => Some(row - 1),
        held => held,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Frequency,
    Gain,
    Q,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Editing {
    pub row: usize,
    pub cell: Cell,
}

struct Drawn {
    rate: SampleRate,
    revision: u64,
    traced: Arc<[Traced]>,
}

#[derive(Clone, Copy)]
struct Peaked {
    rate: SampleRate,
    revision: u64,
    peak: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Goal {
    BuiltIn(BuiltIn),
    File(PathBuf),
}

impl Goal {
    pub const BY_DEFAULT: Self = Self::BuiltIn(BuiltIn::HarmanOverEar2018);

    fn response(&self) -> resonate_eq::Result<Response> {
        match self {
            Self::BuiltIn(built_in) => built_in.response(),
            Self::File(file) => Ok(Store::response_in(file)?.response),
        }
    }

    pub fn spoken(&self) -> String {
        match self {
            Self::BuiltIn(built_in) => built_in.label().to_owned(),
            Self::File(file) => file.file_name().map_or_else(
                || file.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
        }
    }
}

pub struct EqualiserModel {
    store: Store,
    corrections: Arc<Corrected>,
    bindings: Bindings,
    measurers: Measurers,
    looking_on: Option<NodeName>,
    kept: Vec<ProfileName>,
    shown: Option<(Curve, Profile)>,
    held: BTreeMap<Curve, Arc<Profile>>,
    unsaved: bool,
    revision: u64,
    drawn: Option<Drawn>,
    peaked: Kept<Option<Peaked>>,
    editing: Option<Editing>,
    shaping: Option<usize>,
    chosen: Option<usize>,
    catalogue: Option<Arc<Catalogue>>,
    found: Vec<Found>,
    reading_the_catalogue: bool,
    fetching: bool,
    goal: Goal,
    fitting_a_measurement: bool,
    measuring_the_room: bool,
    hearing: Option<Arc<Hearing>>,
    calibration: Option<PathBuf>,
    notice: Option<Notice>,
    untold: bool,
    _saved: Task<()>,
    _imported: Task<()>,
    _exported: Task<()>,
    _catalogued: Task<()>,
    _fetched: Task<()>,
    _measured: Task<()>,
    _sounded: Task<()>,
}

impl EqualiserModel {
    pub fn new(folder: PathBuf, corrections: Arc<Corrected>, bindings: Bindings) -> Self {
        let store = Store::at(folder);
        let kept = store.names().unwrap_or_default();

        let mut model = Self {
            store,
            corrections,
            bindings,
            measurers: Measurers::default(),
            looking_on: None,
            kept,
            shown: None,
            held: BTreeMap::new(),
            unsaved: false,
            revision: 0,
            drawn: None,
            peaked: Kept::new(None),
            editing: None,
            shaping: None,
            chosen: None,
            catalogue: None,
            found: Vec::new(),
            reading_the_catalogue: false,
            fetching: false,
            goal: Goal::BY_DEFAULT,
            fitting_a_measurement: false,
            measuring_the_room: false,
            hearing: None,
            calibration: None,
            notice: None,
            untold: false,
            _saved: Task::ready(()),
            _imported: Task::ready(()),
            _exported: Task::ready(()),
            _catalogued: Task::ready(()),
            _fetched: Task::ready(()),
            _measured: Task::ready(()),
            _sounded: Task::ready(()),
        };
        model.gather();
        model
    }

    fn gather(&mut self) {
        let bound: Vec<Curve> = self.bindings.curves().collect();
        self.held.retain(|curve, _| bound.contains(curve));
        for curve in bound {
            if self.held.contains_key(&curve) {
                continue;
            }
            match curve.read(&self.store) {
                Ok(Some(profile)) => {
                    self.held.insert(curve, Arc::new(profile));
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(%error, "a bound curve could not be read"),
            }
        }
    }

    pub const fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    pub fn kept(&self) -> &[ProfileName] {
        &self.kept
    }

    pub fn folder(&self) -> &std::path::Path {
        self.store.folder()
    }

    pub const fn editing(&self) -> Option<Editing> {
        self.editing
    }

    pub const fn shaping(&self) -> Option<usize> {
        self.shaping
    }

    pub const fn chosen(&self) -> Option<usize> {
        self.chosen
    }

    pub fn choose(&mut self, row: Option<usize>) {
        self.chosen = row.filter(|row| self.band(*row).is_some());
    }

    pub const fn is_looking(&self) -> bool {
        self.reading_the_catalogue || self.fetching
    }

    pub const fn goal(&self) -> &Goal {
        &self.goal
    }

    pub fn aim_at(&mut self, goal: Goal) {
        self.goal = goal;
    }

    pub const fn is_fitting_a_measurement(&self) -> bool {
        self.fitting_a_measurement
    }

    pub const fn is_measuring_the_room(&self) -> bool {
        self.measuring_the_room
    }

    pub fn calibration(&self) -> Option<&PathBuf> {
        self.calibration.as_ref()
    }

    pub fn calibrate_with(&mut self, file: Option<PathBuf>) {
        self.calibration = file;
    }

    pub fn stop_measuring_the_room(&mut self) {
        if let Some(hearing) = &self.hearing {
            hearing.stop();
        }
    }

    pub fn has_a_source(&self) -> bool {
        self.corrections.has_a_source()
    }

    pub fn take_notice(&mut self) -> Option<Notice> {
        self.notice.take()
    }

    pub const fn take_untold(&mut self) -> bool {
        let untold = self.untold;
        self.untold = false;
        untold
    }

    pub fn shown_curve(&self) -> Option<&Curve> {
        self.shown.as_ref().map(|(curve, _)| curve)
    }

    pub fn shown(&self) -> Option<&Profile> {
        self.shown.as_ref().map(|(_, profile)| profile)
    }

    pub fn shown_bands(&self) -> Vec<Band> {
        self.shown()
            .map(|profile| profile.bands().to_vec())
            .unwrap_or_default()
    }

    pub fn show(&mut self, sink: Option<&NodeName>) {
        let Some(curve) = self.bindings.bound_to(sink) else {
            if self.shown.is_some() {
                self.save_now();
                self.shown = None;
                self.forget_the_rows();
                self.step();
            }
            return;
        };
        if self.shown_curve() == Some(&curve) {
            return;
        }
        self.reread(curve);
    }

    fn reread(&mut self, curve: Curve) {
        self.save_now();
        self.forget_the_rows();
        let held = self
            .held
            .get(&curve)
            .map(|profile| Ok(Some(Profile::clone(profile))));
        match held.unwrap_or_else(|| curve.read(&self.store)) {
            Ok(Some(profile)) => self.shown = Some((curve, profile)),
            Ok(None) => {
                self.notice = Some(Notice::Trouble(format!(
                    "{} is bound but not kept here",
                    curve.spoken()
                )));
                self.shown = None;
            }
            Err(error) => {
                self.shown = None;
                self.notice = Some(toast::eq_could_not("read that profile", &error));
            }
        }
        self.step();
    }

    fn forget_the_rows(&mut self) {
        self.chosen = None;
        self.editing = None;
        self.shaping = None;
    }

    pub fn saved_on_leaving(
        folder: PathBuf,
        corrections: Arc<Corrected>,
        bindings: Bindings,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_app_quit(|model: &mut Self, _| {
            model.save_now();
            async {}
        })
        .detach();
        cx.on_release(|model: &mut Self, _| model.save_now())
            .detach();
        Self::new(folder, corrections, bindings)
    }

    fn save_now(&mut self) {
        if !self.unsaved {
            return;
        }
        self.unsaved = false;
        self._saved = Task::ready(());
        if let Some((curve, profile)) = self.shown.as_ref()
            && let Err(error) = curve.keep(&self.store, profile)
        {
            self.notice = Some(toast::eq_could_not("save the curve", &error));
        }
    }

    fn replaced(&mut self, name: &ProfileName) {
        let curve = Curve::Kept(name.clone());
        if self.shown_curve() == Some(&curve) {
            self.unsaved = false;
            self._saved = Task::ready(());
            self.shown = None;
            self.forget_the_rows();
        }
        let was_bound = self.held.remove(&curve).is_some();
        self.gather();
        self.untold |= was_bound;
    }

    fn step(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.drawn = None;
    }

    pub fn drawn(&mut self, rate: SampleRate) -> Arc<[Traced]> {
        if let Some(drawn) = self.drawn.as_ref()
            && drawn.rate == rate
            && drawn.revision == self.revision
        {
            return Arc::clone(&drawn.traced);
        }

        let traced: Arc<[Traced]> = match self.shown() {
            Some(profile) => profile.responses(rate, CURVE_COLUMNS).into(),
            None => [Traced {
                on: TracedOn::EveryChannel,
                decibels: vec![0.0; CURVE_COLUMNS],
            }]
            .into(),
        };
        self.drawn = Some(Drawn {
            rate,
            revision: self.revision,
            traced: Arc::clone(&traced),
        });
        traced
    }

    pub fn peak_db(&self, rate: SampleRate) -> f64 {
        if let Some(peaked) = self.peaked.get()
            && peaked.revision == self.revision
            && peaked.rate == rate
        {
            return peaked.peak;
        }
        let peak = self.shown().map_or(0.0, |profile| profile.peak_db(rate));
        self.peaked.set(Some(Peaked {
            rate,
            revision: self.revision,
            peak,
        }));
        peak
    }

    pub fn equalisation(&self) -> Equalisation {
        let held = |owner: Option<&NodeName>, binding: &Binding| {
            self.held.get(&Curve::of(owner, binding)).map(Arc::clone)
        };

        Equalisation {
            enabled: self.bindings.enabled,
            bound: self
                .bindings
                .by_sink
                .iter()
                .filter_map(|(sink, binding)| {
                    held(Some(sink), binding).map(|profile| (sink.clone(), profile))
                })
                .collect(),
            fallback: self
                .bindings
                .fallback
                .as_ref()
                .and_then(|binding| held(None, binding)),
        }
    }

    pub fn switch(&mut self, on: bool) {
        self.bindings.enabled = on;
    }

    pub fn prefer(&mut self, measurers: Measurers) {
        self.measurers = measurers;
    }

    pub fn measurer_for(&self, sink: Option<&NodeName>) -> Option<&str> {
        self.measurers.for_sink(sink)
    }

    pub fn rebound(&mut self, bindings: Bindings) {
        self.save_now();
        self.bindings = bindings;
        self.held.clear();
        self.shown = None;
        self.forget_the_rows();
        self.kept = self.store.names().unwrap_or_default();
        self.gather();
        self.step();
    }

    pub fn bind(&mut self, sink: Option<&NodeName>, binding: Option<Binding>) {
        self.save_now();
        self.bindings.bind(sink, binding);
        self.shown = None;
        self.forget_the_rows();
        self.gather();
        self.show(sink);
    }

    pub fn edit_cell(&mut self, row: usize, cell: Cell) {
        self.shaping = None;
        self.chosen = Some(row);
        self.editing = Some(Editing { row, cell });
    }

    pub fn leave_cell(&mut self) {
        self.editing = None;
    }

    pub fn shape(&mut self, row: Option<usize>) {
        self.editing = None;
        self.shaping = row;
    }

    pub fn band(&self, row: usize) -> Option<Band> {
        self.shown()?.bands().get(row).copied()
    }

    pub fn written(&self, editing: Editing) -> String {
        let Some(band) = self.band(editing.row) else {
            return String::new();
        };
        written_exactly(band, editing.cell)
    }

    pub fn take(&mut self, editing: Editing, typed: &str, cx: &mut Context<Self>) -> bool {
        let Some(mut band) = self.band(editing.row) else {
            return false;
        };
        let Some(number) = read_typed(typed, editing.cell) else {
            self.notice = Some(Notice::Trouble(refused(editing.cell)));
            return false;
        };

        let read = match editing.cell {
            Cell::Frequency => Frequency::from_hertz(number).map(|held| {
                band.frequency = held;
            }),
            Cell::Gain => BandGain::from_decibels(number).map(|held| band.gain = held),
            Cell::Q => Q::from_units(number).map(|held| band.q = held),
        };
        if read.is_err() {
            self.notice = Some(Notice::Trouble(refused(editing.cell)));
            return false;
        }

        self.put(editing.row, band, cx);
        self.editing = None;
        true
    }

    fn put(&mut self, row: usize, band: Band, cx: &mut Context<Self>) {
        if let Some((_, profile)) = self.shown.as_mut()
            && let Some(slot) = profile.band_mut(row)
        {
            *slot = band.normalised();
        }
        self.settle(cx);
    }

    pub fn add_at(&mut self, placed: Placed, cx: &mut Context<Self>) -> Option<usize> {
        let (_, profile) = self.shown.as_mut()?;
        if let Err(error) = profile.push(a_band_at(placed)) {
            tracing::debug!(%error, "a band was pressed onto a full curve");
            self.notice = Some(Notice::Trouble(format!(
                "a curve holds at most {MAX_BANDS} bands"
            )));
            return None;
        }
        let row = profile.bands().len().checked_sub(1)?;
        self.editing = None;
        self.shaping = None;
        self.chosen = Some(row);
        self.settle(cx);
        Some(row)
    }

    pub fn move_to(&mut self, row: usize, placed: Placed, cx: &mut Context<Self>) -> bool {
        let Some(band) = self.band(row) else {
            return false;
        };
        let landed = moved(band, placed);
        if landed == band {
            return false;
        }
        self.put(row, landed, cx);
        true
    }

    pub fn nudge(&mut self, row: usize, nudge: Nudge, cx: &mut Context<Self>) -> bool {
        let Some(band) = self.band(row) else {
            return false;
        };
        let next = nudged(band, nudge);
        if next == band {
            return false;
        }
        self.chosen = Some(row);
        self.put(row, next, cx);
        true
    }

    pub fn turn_the_q(&mut self, row: usize, notches: f64, cx: &mut Context<Self>) -> bool {
        let Some(band) = self.band(row) else {
            return false;
        };
        let q = narrowed(band.q, notches);
        if q == band.q {
            return false;
        }
        self.chosen = Some(row);
        self.put(row, Band { q, ..band }, cx);
        true
    }

    pub fn retype(&mut self, row: usize, kind: BandKind, cx: &mut Context<Self>) {
        let Some(band) = self.band(row) else {
            return;
        };
        self.put(
            row,
            Band {
                on: band.on,
                channels: band.channels,
                ..Band::new(kind, band.frequency, band.gain, band.q)
            },
            cx,
        );
        self.shaping = None;
    }

    pub fn switch_band(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(band) = self.band(row) else {
            return;
        };
        self.put(
            row,
            Band {
                on: !band.on,
                ..band
            },
            cx,
        );
    }

    pub fn add_a_band(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.shown() else {
            return;
        };
        let at = next_frequency(profile.bands());
        self.add_at(
            Placed {
                frequency: at,
                gain: BandGain::FLAT,
            },
            cx,
        );
    }

    pub fn drop_a_band(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some((_, profile)) = self.shown.as_mut()
            && profile.remove(row)
        {
            self.chosen = chosen_after_dropping(self.chosen, row);
        }
        self.editing = None;
        self.shaping = None;
        self.settle(cx);
    }

    pub fn fit_the_preamp(&mut self, rate: SampleRate, cx: &mut Context<Self>) {
        if let Some((_, profile)) = self.shown.as_mut() {
            let fitted = profile.fitted_preamp(rate);
            profile.set_preamp(fitted);
        }
        self.settle(cx);
    }

    pub fn fit_the_preamp_to_the_music(
        &mut self,
        measured: &Profile,
        fitted: Preamp,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((_, profile)) = self.shown.as_mut() else {
            return false;
        };
        if profile.bands() != measured.bands() {
            return false;
        }
        profile.set_preamp(fitted);
        self.settle(cx);
        true
    }

    pub fn set_preamp(&mut self, preamp: Preamp, cx: &mut Context<Self>) {
        if let Some((_, profile)) = self.shown.as_mut() {
            profile.set_preamp(preamp);
        }
        self.settle(cx);
    }

    fn settle(&mut self, cx: &mut Context<Self>) {
        self.step();
        let Some((curve, profile)) = self.shown.clone() else {
            return;
        };
        if self.held.contains_key(&curve) {
            self.held.insert(curve.clone(), Arc::new(profile.clone()));
        }
        self.unsaved = true;
        let folder = self.store.folder().to_path_buf();

        self._saved = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PROFILE_SETTLES).await;
            let written = cx
                .background_executor()
                .spawn(async move { curve.keep(&Store::at(folder), &profile) })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.unsaved = false;
                if let Err(error) = written {
                    this.notice = Some(toast::eq_could_not("save the curve", &error));
                    cx.notify();
                }
            });
            let _ = outcome;
        });
    }

    pub fn import(&mut self, from: PathBuf, cx: &mut Context<Self>) {
        let folder = self.store.folder().to_path_buf();
        self._imported = cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { Store::at(folder).import(&from, None) })
                .await;

            let outcome = this.update(cx, |this, cx| match read {
                Ok((name, kept)) => {
                    this.reload();
                    this.replaced(&name);
                    this.reread(Curve::Kept(name.clone()));
                    let said = if kept.converted {
                        format!(
                            "kept a graphic curve as {name}, fitted to {} bands here and again \
                             at the rate each stream plays at",
                            kept.profile.bands().len()
                        )
                    } else {
                        format!("Kept {name}, {} bands", kept.profile.bands().len())
                    };
                    this.notice = Some(Notice::Done(format!("{said}{}", caveats(&kept))));
                    cx.notify();
                }
                Err(error) => {
                    this.notice = Some(toast::eq_could_not("import that profile", &error));
                    cx.notify();
                }
            });
            let _ = outcome;
        });
    }

    pub fn fit_a_measurement(&mut self, from: PathBuf, cx: &mut Context<Self>) {
        let folder = self.store.folder().to_path_buf();
        let goal = self.goal.clone();
        self.fitting_a_measurement = true;

        self._measured = cx.spawn(async move |this, cx| {
            let aimed_at = goal.spoken();
            let fitted = cx
                .background_executor()
                .spawn(async move {
                    let store = Store::at(folder);
                    let target = goal.response()?;
                    let measured =
                        Store::fit_file(&from, &target, &resonate_eq::Fitting::default())?;
                    let name = Store::name_for_import(&from, None);
                    store.keep(&name, &measured.fitted.profile)?;
                    Ok::<_, resonate_eq::Error>((name, measured))
                })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.fitting_a_measurement = false;
                match fitted {
                    Ok((name, measured)) => {
                        this.reload();
                        this.replaced(&name);
                        this.reread(Curve::Kept(name.clone()));
                        this.notice = Some(Notice::Done(fitted_said(&name, &aimed_at, &measured)));
                    }
                    Err(error) => {
                        this.notice = Some(toast::eq_could_not("fit that measurement", &error));
                    }
                }
                cx.notify();
            });
            let _ = outcome;
        });
    }

    pub fn measure_the_room(&mut self, sounding: Sounding, cx: &mut Context<Self>) {
        if self.measuring_the_room {
            return;
        }
        let Sounding {
            rig,
            through,
            from,
            called,
        } = sounding;
        let folder = self.store.folder().to_path_buf();
        let goal = self.goal.clone();
        let calibration = self.calibration.clone();
        let hearing = Hearing::new();
        self.hearing = Some(Arc::clone(&hearing));
        self.measuring_the_room = true;

        self._sounded = cx.spawn(async move |this, cx| {
            let aimed_at = goal.spoken();
            let kept = cx
                .background_executor()
                .spawn(async move {
                    let microphone = calibration
                        .map(|file| Store::response_in(&file).map(|read| read.response))
                        .transpose()
                        .map_err(Missed::Fitted)?;
                    let heard = rig
                        .measure(through.as_ref(), &from, &hearing, microphone.as_ref())
                        .map_err(Missed::Heard)?;
                    let target = goal.response().map_err(Missed::Fitted)?;
                    let fitted =
                        fit(&heard, &target, &Fitting::default()).map_err(Missed::Fitted)?;
                    let name = called.unwrap_or_else(|| ProfileName::after(MEASURED_LIVE));
                    Store::at(folder)
                        .keep(&name, &fitted.profile)
                        .map_err(Missed::Fitted)?;
                    Ok::<_, Missed>((name, fitted))
                })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.measuring_the_room = false;
                this.hearing = None;
                match kept {
                    Ok((name, fitted)) => {
                        this.reload();
                        this.replaced(&name);
                        this.reread(Curve::Kept(name.clone()));
                        this.notice = Some(Notice::Done(fit_said(&name, &aimed_at, &fitted)));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "the room could not be measured");
                        this.notice = Some(missed_the_room(&error));
                    }
                }
                cx.notify();
            });
            let _ = outcome;
        });
    }

    pub fn export(&mut self, to: PathBuf, cx: &mut Context<Self>) {
        let Some((curve, profile)) = self.shown.clone() else {
            return;
        };
        let name = curve.spoken();
        let folder = self.store.folder().to_path_buf();

        self._exported = cx.spawn(async move |this, cx| {
            let written = cx
                .background_executor()
                .spawn(async move {
                    let store = Store::at(folder);
                    let standing_in = curve.file(&store)?;
                    store.export(&profile, Some(&standing_in), &to)
                })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.notice = Some(match written {
                    Ok(()) => Notice::Done(format!("Wrote {name} out")),
                    Err(error) => toast::eq_could_not("export the profile", &error),
                });
                cx.notify();
            });
            let _ = outcome;
        });
    }

    pub fn keep_as_a_profile(&mut self, called: &str, cx: &mut Context<Self>) {
        let Some((Curve::Own(_), profile)) = self.shown.clone() else {
            return;
        };
        self.save_now();
        let name = unused_name(&self.kept, called);
        match self.store.keep(&name, &profile) {
            Ok(_) => {
                self.reload();
                self.notice = Some(Notice::Done(format!("Kept the curve as {name}")));
            }
            Err(error) => {
                self.notice = Some(toast::eq_could_not("keep the curve as a profile", &error));
            }
        }
        cx.notify();
    }

    pub fn discard(&mut self, curve: &Curve, cx: &mut Context<Self>) {
        let discarded = match curve {
            Curve::Kept(name) => self.store.forget(name),
            Curve::Own(owner) => self.store.forget_own(owner.as_ref().map(NodeName::as_str)),
        };
        match discarded {
            Ok(_) => {
                if self.shown_curve() == Some(curve) {
                    self.unsaved = false;
                    self._saved = Task::ready(());
                    self.shown = None;
                    self.forget_the_rows();
                }
                self.held.remove(curve);
                self.reload();
                self.step();
                self.notice = Some(Notice::Done(match curve {
                    Curve::Kept(name) => format!("Discarded {name}"),
                    Curve::Own(_) => "Discarded the device's own curve".to_owned(),
                }));
            }
            Err(error) => self.notice = Some(toast::eq_could_not("discard that curve", &error)),
        }
        cx.notify();
    }

    fn reload(&mut self) {
        self.kept = self.store.names().unwrap_or_default();
    }

    pub fn look(&mut self, typed: &str, sink: Option<&NodeName>, cx: &mut Context<Self>) {
        self.looking_on = sink.cloned();
        if typed.trim().is_empty() {
            self.found.clear();
            cx.notify();
            return;
        }
        if let Some(catalogue) = self.catalogue.clone() {
            self.found = search_preferring(&catalogue, typed, self.measurers.for_sink(sink))
                .into_iter()
                .take(FOUND_SHOWN)
                .collect();
            cx.notify();
            return;
        }
        self.read_the_catalogue(Some(typed.to_owned()), cx);
    }

    pub fn read_the_catalogue(&mut self, then: Option<String>, cx: &mut Context<Self>) {
        if self.reading_the_catalogue {
            return;
        }
        self.reading_the_catalogue = true;
        let corrections = Arc::clone(&self.corrections);

        self._catalogued = cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { corrections.catalogue() })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.reading_the_catalogue = false;
                match read {
                    Ok(catalogue) => {
                        this.catalogue = Some(catalogue);
                        if let Some(typed) = then {
                            let sink = this.looking_on.clone();
                            this.look(&typed, sink.as_ref(), cx);
                        }
                    }
                    Err(error) => {
                        this.notice = Some(toast::eq_could_not("read the AutoEq catalogue", &error))
                    }
                }
                cx.notify();
            });
            let _ = outcome;
        });
    }

    pub fn found(&self) -> Vec<&Device> {
        let Some(catalogue) = self.catalogue.as_ref() else {
            return Vec::new();
        };
        self.found
            .iter()
            .filter_map(|found| catalogue.device(*found))
            .collect()
    }

    pub fn suggested(&self, description: &str, sink: Option<&NodeName>) -> Option<&Device> {
        let catalogue = self.catalogue.as_ref()?;
        suggest_preferring(catalogue, description, self.measurers.for_sink(sink))
            .and_then(|found| catalogue.device(found))
    }

    pub fn knows_the_catalogue(&self) -> bool {
        self.catalogue.is_some()
    }

    pub fn fetch(&mut self, device: DeviceId, label: String, cx: &mut Context<Self>) {
        let corrections = Arc::clone(&self.corrections);
        let folder = self.store.folder().to_path_buf();
        self.fetching = true;

        self._fetched = cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_executor()
                .spawn(async move {
                    let profile = corrections.profile(&device)?;
                    let name = ProfileName::after(&label);
                    match profile {
                        Some(profile) => {
                            Store::at(folder).keep(&name, &profile)?;
                            Ok(Some((name, profile)))
                        }
                        None => Ok(None),
                    }
                })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.fetching = false;
                match fetched {
                    Ok(Some((name, profile))) => {
                        this.reload();
                        this.notice = Some(Notice::Done(format!(
                            "kept {name}, {} bands",
                            profile.bands().len()
                        )));
                        this.replaced(&name);
                        this.step();
                    }
                    Ok(None) => {
                        this.notice = Some(Notice::Trouble(
                            "AutoEq has no measurement for that device".to_owned(),
                        ));
                    }
                    Err(error) => {
                        this.notice = Some(Notice::Trouble(resonate_eq::Error::to_string(&error)));
                    }
                }
                cx.notify();
            });
            let _ = outcome;
        });
    }
}

pub fn unused_name(kept: &[ProfileName], called: &str) -> ProfileName {
    let taken = |name: &ProfileName| kept.iter().any(|held| held.folded() == name.folded());
    let plain = ProfileName::after(called);
    if !taken(&plain) {
        return plain;
    }

    (2..)
        .map(|nth| ProfileName::after(&format!("{called} {nth}")))
        .find(|name| !taken(name))
        .unwrap_or(plain)
}

pub const HERTZ_A_KILOHERTZ: f64 = 1_000.0;
const DIGITS_IN_A_THOUSANDS_GROUP: usize = 3;

const MOST_DECIMALS_WRITTEN: usize = 6;

fn written_exactly(band: Band, cell: Cell) -> String {
    let (value, fewest_decimals) = match cell {
        Cell::Frequency => (band.frequency.hertz(), 0),
        Cell::Gain => (band.gain.decibels(), 2),
        Cell::Q => (band.q.units(), 3),
    };

    (fewest_decimals..=MOST_DECIMALS_WRITTEN)
        .map(|decimals| format!("{value:.decimals$}"))
        .find(|written| reads_back_as(written, cell, band))
        .unwrap_or_else(|| format!("{value}"))
}

fn reads_back_as(written: &str, cell: Cell, band: Band) -> bool {
    let Some(number) = read_typed(written, cell) else {
        return false;
    };
    match cell {
        Cell::Frequency => Frequency::from_hertz(number).is_ok_and(|read| read == band.frequency),
        Cell::Gain => BandGain::from_decibels(number).is_ok_and(|read| read == band.gain),
        Cell::Q => Q::from_units(number).is_ok_and(|read| read == band.q),
    }
}

pub fn read_typed(typed: &str, cell: Cell) -> Option<f64> {
    let typed = typed.trim();
    match cell {
        Cell::Frequency => {
            if let Some(kilohertz) = without_unit(typed, "khz").or_else(|| without_unit(typed, "k"))
            {
                return resonate_eq::read_number(kilohertz)
                    .map(|number| number * HERTZ_A_KILOHERTZ);
            }
            let hertz = without_unit(typed, "hz").unwrap_or(typed);
            if grouped_in_thousands(hertz) {
                resonate_eq::read_number(&hertz.replace(',', ""))
            } else {
                resonate_eq::read_number(hertz)
            }
        }
        Cell::Gain => resonate_eq::read_number(without_unit(typed, "db").unwrap_or(typed)),
        Cell::Q => resonate_eq::read_number(typed),
    }
}

fn without_unit<'a>(typed: &'a str, unit: &str) -> Option<&'a str> {
    let cut = typed.len().checked_sub(unit.len())?;
    let spelled = typed.get(cut..)?;
    spelled
        .eq_ignore_ascii_case(unit)
        .then(|| typed[..cut].trim_end())
}

fn grouped_in_thousands(number: &str) -> bool {
    let whole = number.split_once('.').map_or(number, |(whole, _)| whole);
    let mut groups = whole.split(',');
    let leading = groups.next().unwrap_or_default();
    let rest: Vec<&str> = groups.collect();
    let digits = |group: &str| group.bytes().all(|byte| byte.is_ascii_digit());

    !rest.is_empty()
        && (1..=DIGITS_IN_A_THOUSANDS_GROUP).contains(&leading.len())
        && digits(leading)
        && rest
            .iter()
            .all(|group| group.len() == DIGITS_IN_A_THOUSANDS_GROUP && digits(group))
}

fn refused(cell: Cell) -> String {
    match cell {
        Cell::Frequency => "a band sits between 1 Hz and 40 kHz".to_owned(),
        Cell::Gain => "a band's gain sits between -40 and +40 dB".to_owned(),
        Cell::Q => "a Q sits between 0.10 and 40.00".to_owned(),
    }
}

fn next_frequency(bands: &[Band]) -> Frequency {
    const FIRST: f64 = 1_000.0;

    let mut held: Vec<f64> = bands.iter().map(|band| band.frequency.hertz()).collect();
    if held.is_empty() {
        return Frequency::from_hertz(FIRST).unwrap_or(Frequency::LOWEST);
    }
    held.sort_by(f64::total_cmp);
    held.insert(0, 20.0);
    held.push(20_000.0);

    let widest = held
        .windows(2)
        .max_by(|one, other| (one[1] / one[0].max(1.0)).total_cmp(&(other[1] / other[0].max(1.0))))
        .map_or(FIRST, |pair| (pair[0].max(1.0) * pair[1]).sqrt());

    Frequency::from_hertz(widest).unwrap_or(Frequency::LOWEST)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(text: &str) -> ProfileName {
        ProfileName::new(text).expect("a usable name")
    }

    fn a_band() -> Band {
        Band::peaking(
            Frequency::from_hertz(1_000.0).expect("a frequency"),
            BandGain::from_decibels(3.0).expect("a gain"),
            Q::BUTTERWORTH,
        )
    }

    #[gpui::test]
    fn a_curve_changed_just_before_the_window_closes_is_kept(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-leaving-{}",
            std::process::id()
        ));
        let store = Store::at(folder.clone());
        let name = named("Harman");
        let dac = NodeName::new("alsa_output.dac");
        store
            .keep(
                &name,
                &Profile::new(Preamp::NONE, vec![a_band()]).expect("a profile"),
            )
            .expect("the profile is kept");
        let model = cx.new(|cx| {
            EqualiserModel::saved_on_leaving(
                folder.clone(),
                Arc::new(Corrected::uncorrected()),
                Bindings {
                    enabled: true,
                    fallback: None,
                    by_sink: vec![(dac.clone(), Binding::Profile(name.clone()))],
                },
                cx,
            )
        });
        let quieter = Preamp::from_decibels(-3.0).expect("a preamp");

        model.update(cx, |model, cx| {
            model.show(Some(&dac));
            model.set_preamp(quieter, cx);
        });
        drop(model);
        cx.update(|_| {});
        cx.run_until_parked();

        let kept = store
            .read(&name)
            .expect("the profile reads")
            .expect("the profile is there");
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(
            kept.preamp(),
            quieter,
            "a change made within the settle of closing was lost"
        );
    }

    #[gpui::test]
    fn a_gain_typed_into_a_band_that_has_none_is_not_kept(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-gainless-{}",
            std::process::id()
        ));
        let store = Store::at(folder.clone());
        let name = named("Notched");
        let dac = NodeName::new("alsa_output.dac");
        let notch = Band::new(
            BandKind::Notch,
            Frequency::from_hertz(1_000.0).expect("a frequency"),
            BandGain::FLAT,
            Q::BUTTERWORTH,
        );
        store
            .keep(
                &name,
                &Profile::new(Preamp::NONE, vec![notch]).expect("a profile"),
            )
            .expect("the profile is kept");
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.clone(),
                Arc::new(Corrected::uncorrected()),
                Bindings {
                    enabled: true,
                    fallback: None,
                    by_sink: vec![(dac.clone(), Binding::Profile(name.clone()))],
                },
            )
        });

        let taken = model.update(cx, |model, cx| {
            model.show(Some(&dac));
            let editing = Editing {
                row: 0,
                cell: Cell::Gain,
            };
            model.take(editing, "4.5", cx)
        });
        let bands = model.read_with(cx, |model, _| model.shown_bands());
        let _ = std::fs::remove_dir_all(&folder);

        assert!(taken);
        assert_eq!(bands, vec![notch]);
    }

    #[gpui::test]
    fn a_catalogue_read_is_not_dropped_by_an_import_started_beside_it(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-beside-{}",
            std::process::id()
        ));
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.clone(),
                Arc::new(Corrected::uncorrected()),
                Bindings::default(),
            )
        });

        model.update(cx, |model, cx| {
            model.read_the_catalogue(None, cx);
            model.import(folder.join("not-there.txt"), cx);
        });
        cx.run_until_parked();

        let (looking, known) = model.read_with(cx, |model, _| {
            (model.is_looking(), model.knows_the_catalogue())
        });
        let _ = std::fs::remove_dir_all(&folder);
        assert!(
            !looking,
            "the catalogue read was dropped and left the pane looking"
        );
        assert!(known, "the catalogue was never read");
    }

    fn dipped_measurement(folder: &std::path::Path) -> PathBuf {
        let text: String = (0..60)
            .map(|step| {
                let hertz = 20.0 * 1.12_f64.powi(step);
                let level = if (3_000.0..6_000.0).contains(&hertz) {
                    -5.0
                } else {
                    0.0
                };
                format!("{hertz:.2}\t{level:.2}\n")
            })
            .collect();
        std::fs::create_dir_all(folder).expect("a writable temporary directory");
        let file = folder.join("dipped.txt");
        std::fs::write(&file, format!("* REW\n{text}")).expect("a writable temporary directory");
        file
    }

    #[gpui::test]
    fn a_measurement_fitted_in_the_pane_is_kept_and_shown_as_the_profile_it_became(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-measured-{}",
            std::process::id()
        ));
        let measurement = dipped_measurement(&folder);
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.join("equaliser"),
                Arc::new(Corrected::uncorrected()),
                Bindings::default(),
            )
        });

        model.update(cx, |model, cx| {
            model.aim_at(Goal::BuiltIn(BuiltIn::Flat));
            model.fit_a_measurement(measurement, cx);
        });
        assert!(model.read_with(cx, |model, _| model.is_fitting_a_measurement()));
        cx.run_until_parked();

        let (fitting, shown, bands, notice, kept) = model.update(cx, |model, _| {
            (
                model.is_fitting_a_measurement(),
                model.shown_curve().cloned(),
                model.shown_bands(),
                model.take_notice(),
                model.kept().to_vec(),
            )
        });
        let _ = std::fs::remove_dir_all(&folder);

        assert!(!fitting);
        assert_eq!(shown, Some(Curve::Kept(named("dipped"))));
        assert_eq!(kept, vec![named("dipped")]);
        assert!(bands.iter().any(|band| band.gain.decibels() > 2.0));
        assert!(matches!(notice, Some(Notice::Done(said)) if said.contains("fitted toward Flat")));
    }

    #[gpui::test]
    fn a_target_file_that_is_not_there_leaves_the_pane_with_a_notice_and_nothing_kept(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-no-target-{}",
            std::process::id()
        ));
        let measurement = dipped_measurement(&folder);
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.join("equaliser"),
                Arc::new(Corrected::uncorrected()),
                Bindings::default(),
            )
        });

        model.update(cx, |model, cx| {
            model.aim_at(Goal::File(folder.join("missing.csv")));
            model.fit_a_measurement(measurement, cx);
        });
        cx.run_until_parked();

        let (fitting, kept, notice) = model.update(cx, |model, _| {
            (
                model.is_fitting_a_measurement(),
                model.kept().to_vec(),
                model.take_notice(),
            )
        });
        let _ = std::fs::remove_dir_all(&folder);

        assert!(!fitting);
        assert!(kept.is_empty());
        assert!(matches!(notice, Some(Notice::Trouble(_))));
    }

    struct Dipped;

    impl Measures for Dipped {
        fn measure(
            &self,
            _through: Option<&NodeName>,
            _from: &Listening,
            _hearing: &Hearing,
            _calibration: Option<&Response>,
        ) -> Result<Response, Unmeasured> {
            let points = (0..60)
                .map(|step| {
                    let hertz = 20.0 * 1.12_f64.powi(step);
                    let decibels = if (3_000.0..6_000.0).contains(&hertz) {
                        -5.0
                    } else {
                        0.0
                    };
                    resonate_eq::Point { hertz, decibels }
                })
                .collect();
            Ok(Response::from_points(points).expect("a response"))
        }
    }

    fn sounding(rig: Arc<dyn Measures>, from: Listening, called: Option<ProfileName>) -> Sounding {
        Sounding {
            rig,
            through: None,
            from,
            called,
        }
    }

    struct Deaf;

    impl Measures for Deaf {
        fn measure(
            &self,
            _through: Option<&NodeName>,
            _from: &Listening,
            _hearing: &Hearing,
            _calibration: Option<&Response>,
        ) -> Result<Response, Unmeasured> {
            Err(Unmeasured::Deconvolved(
                resonate_engine::DspError::NothingHeard,
            ))
        }
    }

    #[gpui::test]
    fn a_room_measured_in_the_pane_is_fitted_kept_and_shown_as_the_profile_it_became(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder =
            std::env::temp_dir().join(format!("resonate-ui-equaliser-room-{}", std::process::id()));
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.join("equaliser"),
                Arc::new(Corrected::uncorrected()),
                Bindings::default(),
            )
        });

        model.update(cx, |model, cx| {
            model.aim_at(Goal::BuiltIn(BuiltIn::Flat));
            model.measure_the_room(sounding(Arc::new(Dipped), Listening::Desktop, None), cx);
        });
        assert!(model.read_with(cx, |model, _| model.is_measuring_the_room()));
        cx.run_until_parked();

        let (measuring, shown, bands, notice) = model.update(cx, |model, _| {
            (
                model.is_measuring_the_room(),
                model.shown_curve().cloned(),
                model.shown_bands(),
                model.take_notice(),
            )
        });
        let _ = std::fs::remove_dir_all(&folder);

        assert!(!measuring);
        assert_eq!(shown, Some(Curve::Kept(named("measured"))));
        assert!(bands.iter().any(|band| band.gain.decibels() > 2.0));
        assert!(matches!(notice, Some(Notice::Done(said)) if said.contains("fitted toward Flat")));
    }

    #[gpui::test]
    fn a_room_that_was_not_heard_leaves_nothing_kept_and_says_so(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;

        let folder =
            std::env::temp_dir().join(format!("resonate-ui-equaliser-deaf-{}", std::process::id()));
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.join("equaliser"),
                Arc::new(Corrected::uncorrected()),
                Bindings::default(),
            )
        });

        model.update(cx, |model, cx| {
            model.measure_the_room(
                sounding(Arc::new(Deaf), Listening::Microphone(None), None),
                cx,
            );
        });
        cx.run_until_parked();

        let (measuring, kept, notice) = model.update(cx, |model, _| {
            (
                model.is_measuring_the_room(),
                model.kept().to_vec(),
                model.take_notice(),
            )
        });
        let _ = std::fs::remove_dir_all(&folder);

        assert!(!measuring);
        assert!(kept.is_empty());
        assert!(
            matches!(notice, Some(Notice::Trouble(said)) if said.starts_with("Nothing was heard"))
        );
    }

    struct Stoppable;

    impl Measures for Stoppable {
        fn measure(
            &self,
            _through: Option<&NodeName>,
            _from: &Listening,
            hearing: &Hearing,
            _calibration: Option<&Response>,
        ) -> Result<Response, Unmeasured> {
            if hearing.stopped() {
                return Err(Unmeasured::Heard(Box::new(resonate_listen::Error::Stopped)));
            }
            Err(Unmeasured::Heard(Box::new(
                resonate_listen::Error::NothingHeard,
            )))
        }
    }

    struct Calibrated(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl Measures for Calibrated {
        fn measure(
            &self,
            through: Option<&NodeName>,
            from: &Listening,
            hearing: &Hearing,
            calibration: Option<&Response>,
        ) -> Result<Response, Unmeasured> {
            self.0.store(
                calibration.map_or(0, |response| response.points().len()),
                std::sync::atomic::Ordering::Relaxed,
            );
            Dipped.measure(through, from, hearing, calibration)
        }
    }

    #[gpui::test]
    fn a_sweep_stopped_part_way_keeps_nothing_and_says_it_was_stopped(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-stopped-{}",
            std::process::id()
        ));
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.join("equaliser"),
                Arc::new(Corrected::uncorrected()),
                Bindings::default(),
            )
        });

        model.update(cx, |model, cx| {
            model.measure_the_room(sounding(Arc::new(Stoppable), Listening::Desktop, None), cx);
            model.stop_measuring_the_room();
        });
        cx.run_until_parked();

        let (measuring, kept, notice) = model.update(cx, |model, _| {
            (
                model.is_measuring_the_room(),
                model.kept().to_vec(),
                model.take_notice(),
            )
        });
        let _ = std::fs::remove_dir_all(&folder);

        assert!(!measuring);
        assert!(kept.is_empty());
        assert!(
            matches!(notice, Some(Notice::Noted(said)) if said.starts_with("The measurement was stopped"))
        );
    }

    #[gpui::test]
    fn a_calibration_chosen_is_taken_off_what_was_heard_and_a_name_given_is_the_profile_kept(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-calibrated-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&folder).expect("a scratch folder");
        let microphone = folder.join("microphone.csv");
        let rows: String = (0..12)
            .map(|step| format!("{},0.5\n", 100 * (step + 1)))
            .collect();
        std::fs::write(&microphone, format!("frequency,raw\n{rows}")).expect("a calibration");
        let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.join("equaliser"),
                Arc::new(Corrected::uncorrected()),
                Bindings::default(),
            )
        });

        model.update(cx, |model, cx| {
            model.aim_at(Goal::BuiltIn(BuiltIn::Flat));
            model.calibrate_with(Some(microphone.clone()));
            model.measure_the_room(
                sounding(
                    Arc::new(Calibrated(read.clone())),
                    Listening::Desktop,
                    Some(named("Living room")),
                ),
                cx,
            );
        });
        cx.run_until_parked();

        let (calibration, shown) = model.update(cx, |model, _| {
            (model.calibration().cloned(), model.shown_curve().cloned())
        });
        let held = read.load(std::sync::atomic::Ordering::Relaxed);
        let _ = std::fs::remove_dir_all(&folder);

        assert_eq!(calibration, Some(microphone));
        assert!(held >= 8, "{held}");
        assert_eq!(shown, Some(Curve::Kept(named("Living room"))));
    }

    #[gpui::test]
    fn a_calibration_that_will_not_read_leaves_the_sweep_unplayed_and_nothing_kept(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-uncalibrated-{}",
            std::process::id()
        ));
        let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(99));
        let model = cx.new(|_| {
            EqualiserModel::new(
                folder.join("equaliser"),
                Arc::new(Corrected::uncorrected()),
                Bindings::default(),
            )
        });

        model.update(cx, |model, cx| {
            model.calibrate_with(Some(folder.join("missing.csv")));
            model.measure_the_room(
                sounding(Arc::new(Calibrated(read.clone())), Listening::Desktop, None),
                cx,
            );
        });
        cx.run_until_parked();

        let (kept, notice) =
            model.update(cx, |model, _| (model.kept().to_vec(), model.take_notice()));
        let _ = std::fs::remove_dir_all(&folder);

        assert!(kept.is_empty());
        assert_eq!(read.load(std::sync::atomic::Ordering::Relaxed), 99);
        assert!(matches!(notice, Some(Notice::Trouble(_))));
    }

    #[test]
    fn bindings_changed_from_outside_are_what_the_pane_shows_and_tells_the_engine_next() {
        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-rebound-{}",
            std::process::id()
        ));
        let store = Store::at(folder.clone());
        let before = named("Harman");
        let after = named("Flat-ish");
        let dac = NodeName::new("alsa_output.dac");
        let louder = Band {
            gain: BandGain::from_decibels(6.0).expect("a gain"),
            ..a_band()
        };
        for (name, band) in [(&before, a_band()), (&after, louder)] {
            store
                .keep(
                    name,
                    &Profile::new(Preamp::NONE, vec![band]).expect("a profile"),
                )
                .expect("the profile is kept");
        }
        let mut model = EqualiserModel::new(
            folder.clone(),
            Arc::new(Corrected::uncorrected()),
            Bindings {
                enabled: true,
                fallback: None,
                by_sink: vec![(dac.clone(), Binding::Profile(before.clone()))],
            },
        );
        model.show(Some(&dac));

        model.rebound(Bindings {
            enabled: true,
            fallback: None,
            by_sink: vec![(dac.clone(), Binding::Profile(after.clone()))],
        });
        model.show(Some(&dac));
        let told = model.equalisation();
        let _ = std::fs::remove_dir_all(&folder);

        assert_eq!(model.shown_curve(), Some(&Curve::Kept(after)));
        assert_eq!(model.shown_bands(), vec![louder]);
        assert_eq!(
            told.bound.get(&dac).map(|profile| profile.bands().to_vec()),
            Some(vec![louder])
        );
    }

    #[test]
    fn the_pane_suggests_from_the_measurer_the_settings_prefer_before_the_fixed_ranking() {
        let by = |measured_by: &str| Device {
            id: DeviceId::new(&format!("{measured_by}/over-ear/Maker Thing 100")).expect("a path"),
            label: "Maker Thing 100".to_owned(),
            measured_by: measured_by.to_owned(),
            rig: None,
        };
        let folder = std::env::temp_dir().join("resonate-ui-equaliser-measurer");
        let mut model = EqualiserModel::new(
            folder,
            Arc::new(Corrected::uncorrected()),
            Bindings::default(),
        );
        model.catalogue = Some(Arc::new(Catalogue::of(vec![
            by("Rtings"),
            by("oratory1990"),
        ])));
        let measurer_of = |model: &EqualiserModel| {
            model
                .suggested("Maker Thing 100", None)
                .map(|device| device.measured_by.clone())
        };

        let ranked = measurer_of(&model);
        model.prefer(Measurers {
            fallback: Some("rtings".to_owned()),
            by_sink: BTreeMap::new(),
        });
        let preferred = measurer_of(&model);

        assert_eq!(ranked.as_deref(), Some("oratory1990"));
        assert_eq!(preferred.as_deref(), Some("Rtings"));
    }

    #[test]
    fn the_pane_suggests_from_the_measurer_preferred_for_the_device_in_use_before_the_one_for_every_device()
     {
        let by = |measured_by: &str| Device {
            id: DeviceId::new(&format!("{measured_by}/over-ear/Maker Thing 100")).expect("a path"),
            label: "Maker Thing 100".to_owned(),
            measured_by: measured_by.to_owned(),
            rig: None,
        };
        let folder = std::env::temp_dir().join("resonate-ui-equaliser-measurer-for");
        let mut model = EqualiserModel::new(
            folder,
            Arc::new(Corrected::uncorrected()),
            Bindings::default(),
        );
        model.catalogue = Some(Arc::new(Catalogue::of(vec![
            by("Rtings"),
            by("oratory1990"),
            by("crinacle"),
        ])));
        let dac = NodeName::new("alsa_output.dac");
        let speakers = NodeName::new("alsa_output.speakers");
        model.prefer(Measurers {
            fallback: Some("rtings".to_owned()),
            by_sink: BTreeMap::from([(dac.clone(), "crinacle".to_owned())]),
        });
        let measurer_of = |sink: Option<&NodeName>| {
            model
                .suggested("Maker Thing 100", sink)
                .map(|device| device.measured_by.clone())
        };

        assert_eq!(measurer_of(Some(&dac)).as_deref(), Some("crinacle"));
        assert_eq!(measurer_of(Some(&speakers)).as_deref(), Some("Rtings"));
        assert_eq!(measurer_of(None).as_deref(), Some("Rtings"));
    }

    #[test]
    fn a_bound_profile_kept_again_over_itself_is_what_the_engine_is_told_next() {
        let folder =
            std::env::temp_dir().join(format!("resonate-ui-equaliser-{}", std::process::id()));
        let store = Store::at(folder.clone());
        let name = named("Harman");
        let dac = NodeName::new("alsa_output.dac");
        let louder = Band {
            gain: BandGain::from_decibels(6.0).expect("a gain"),
            ..a_band()
        };

        store
            .keep(
                &name,
                &Profile::new(Preamp::NONE, vec![a_band()]).expect("a profile"),
            )
            .expect("the first profile is kept");
        let mut model = EqualiserModel::new(
            folder.clone(),
            Arc::new(Corrected::uncorrected()),
            Bindings {
                enabled: true,
                fallback: None,
                by_sink: vec![(dac.clone(), Binding::Profile(name.clone()))],
            },
        );
        store
            .keep(
                &name,
                &Profile::new(Preamp::NONE, vec![louder]).expect("a profile"),
            )
            .expect("the second profile is kept over the first");
        model.replaced(&name);
        let untold = model.take_untold();
        let told = model.equalisation();
        let _ = std::fs::remove_dir_all(&folder);

        assert!(
            untold,
            "a bound profile replaced was not marked for the engine"
        );
        assert!(!model.take_untold());
        assert_eq!(
            told.bound
                .iter()
                .find(|(sink, _)| **sink == dac)
                .map(|(_, profile)| profile.bands().to_vec()),
            Some(vec![louder])
        );

        model.replaced(&named("Unbound"));
        assert!(
            !model.take_untold(),
            "a profile nothing binds was told to the engine"
        );
    }

    #[test]
    fn a_nudge_moves_a_band_a_semitone_half_a_decibel_or_a_notch_of_q_and_nothing_else() {
        let band = a_band();

        let higher = nudged(band, Nudge::Higher);
        assert!((higher.frequency.hertz() - 1_059.46).abs() < 0.01);
        assert_eq!(
            (higher.gain, higher.q, higher.kind),
            (band.gain, band.q, band.kind)
        );
        let back = nudged(higher, Nudge::Lower);
        assert!((back.frequency.hertz() - 1_000.0).abs() < 0.02);

        assert_eq!(nudged(band, Nudge::Louder).gain.milli_decibels(), 3_500);
        assert_eq!(nudged(band, Nudge::Quieter).gain.milli_decibels(), 2_500);
        assert!(nudged(band, Nudge::Narrower).q > band.q);
        assert!(nudged(band, Nudge::Wider).q < band.q);
        assert_eq!(nudged(band, Nudge::Narrower).frequency, band.frequency);
    }

    #[test]
    fn a_nudge_past_the_ends_of_a_band_stays_at_the_end() {
        let top = Band {
            frequency: Frequency::HIGHEST,
            gain: BandGain::from_milli_decibels(BandGain::WIDEST_MILLI_DECIBELS)
                .expect("the widest gain"),
            ..a_band()
        };

        assert_eq!(nudged(top, Nudge::Higher).frequency, Frequency::HIGHEST);
        assert_eq!(
            nudged(top, Nudge::Louder).gain.milli_decibels(),
            BandGain::WIDEST_MILLI_DECIBELS
        );
    }

    #[test]
    fn a_curve_kept_as_a_profile_takes_the_devices_name_or_the_next_one_free() {
        assert_eq!(
            unused_name(&[], "Studio Monitors"),
            named("Studio Monitors")
        );
        assert_eq!(
            unused_name(&[named("studio monitors")], "Studio Monitors"),
            named("Studio Monitors 2")
        );
        assert_eq!(
            unused_name(
                &[named("Studio Monitors"), named("Studio Monitors 2")],
                "Studio Monitors"
            ),
            named("Studio Monitors 3")
        );
    }

    #[test]
    fn a_frequency_typed_with_a_thousands_comma_or_any_spelling_of_a_unit_reads_as_meant() {
        let hertz = |typed: &str| read_typed(typed, Cell::Frequency);

        assert_eq!(hertz("1,000 Hz"), Some(1_000.0));
        assert_eq!(hertz("1,000"), Some(1_000.0));
        assert_eq!(hertz("12,500.5 hz"), Some(12_500.5));
        assert_eq!(hertz("12,5"), Some(12.5));
        assert_eq!(hertz("2 KHz"), Some(2_000.0));
        assert_eq!(hertz("2KHZ"), Some(2_000.0));
        assert_eq!(hertz("1,5 kHz"), Some(1_500.0));
        assert_eq!(hertz("3K"), Some(3_000.0));
        assert_eq!(hertz("440 hZ"), Some(440.0));
        assert_eq!(hertz("1,00,000"), None);
        assert_eq!(read_typed("-3 DB", Cell::Gain), Some(-3.0));
        assert_eq!(read_typed("1,5", Cell::Q), Some(1.5));
    }

    #[gpui::test]
    fn the_peak_and_the_fitted_preamp_are_worked_at_the_rate_the_curve_is_drawn_at(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder =
            std::env::temp_dir().join(format!("resonate-ui-equaliser-peak-{}", std::process::id()));
        let store = Store::at(folder.clone());
        let name = named("Treble");
        let treble = Band::new(
            BandKind::HighShelf,
            Frequency::from_hertz(16_000.0).expect("a frequency"),
            BandGain::from_decibels(6.0).expect("a gain"),
            Q::BUTTERWORTH,
        );
        let profile = Profile::new(Preamp::NONE, vec![treble]).expect("a profile");
        store.keep(&name, &profile).expect("the profile is kept");
        let model = cx.new(|_| {
            let mut model = EqualiserModel::new(
                folder.clone(),
                Arc::new(Corrected::uncorrected()),
                Bindings {
                    enabled: true,
                    fallback: Some(Binding::Profile(name)),
                    by_sink: Vec::new(),
                },
            );
            model.show(None);
            model
        });

        let (at_48, at_96) = model.read_with(cx, |model, _| {
            (
                model.peak_db(SampleRate::HZ_48000),
                model.peak_db(SampleRate::HZ_96000),
            )
        });
        model.update(cx, |model, cx| {
            model.fit_the_preamp(SampleRate::HZ_96000, cx);
        });
        let fitted = model.read_with(cx, |model, _| model.shown().map(Profile::preamp));
        let _ = std::fs::remove_dir_all(&folder);

        assert!((at_48 - profile.peak_db(SampleRate::HZ_48000)).abs() < 1e-9);
        assert!((at_96 - profile.peak_db(SampleRate::HZ_96000)).abs() < 1e-9);
        assert!(
            (at_48 - at_96).abs() > 0.1,
            "a 16 kHz shelf peaks alike at 48 and 96 kHz, so the rate is not weighed"
        );
        assert_eq!(fitted, Some(profile.fitted_preamp(SampleRate::HZ_96000)));
        assert_ne!(fitted, Some(profile.fitted_preamp(SampleRate::HZ_48000)));
    }

    #[gpui::test]
    fn a_measured_preamp_lands_only_on_the_bands_it_was_measured_for(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;

        let folder = std::env::temp_dir().join(format!(
            "resonate-ui-equaliser-music-{}",
            std::process::id()
        ));
        let store = Store::at(folder.clone());
        let name = named("Bass");
        let bass = Band::new(
            BandKind::Peaking,
            Frequency::from_hertz(100.0).expect("a frequency"),
            BandGain::from_decibels(6.0).expect("a gain"),
            Q::BUTTERWORTH,
        );
        let profile = Profile::new(Preamp::NONE, vec![bass]).expect("a profile");
        store.keep(&name, &profile).expect("the profile is kept");
        let model = cx.new(|_| {
            let mut model = EqualiserModel::new(
                folder.clone(),
                Arc::new(Corrected::uncorrected()),
                Bindings {
                    enabled: true,
                    fallback: Some(Binding::Profile(name)),
                    by_sink: Vec::new(),
                },
            );
            model.show(None);
            model
        });
        let measured = Preamp::from_decibels(-2.5).expect("a preamp");
        let moved = Profile::new(Preamp::NONE, Vec::new()).expect("a profile");

        let refused = model.update(cx, |model, cx| {
            model.fit_the_preamp_to_the_music(&moved, measured, cx)
        });
        let kept_by_refusal = model.read_with(cx, |model, _| model.shown().map(Profile::preamp));
        let landed = model.update(cx, |model, cx| {
            model.fit_the_preamp_to_the_music(&profile, measured, cx)
        });
        let fitted = model.read_with(cx, |model, _| model.shown().map(Profile::preamp));
        let _ = std::fs::remove_dir_all(&folder);

        assert!(!refused);
        assert_eq!(kept_by_refusal, Some(Preamp::NONE));
        assert!(landed);
        assert_eq!(fitted, Some(measured));
    }
}
