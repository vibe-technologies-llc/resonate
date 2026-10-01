use std::{cell::Cell as Kept, collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use gpui::{Context, Task};
use resonate_core::{
    SampleRate,
    eq::{Band, BandGain, BandKind, Frequency, MAX_BANDS, Preamp, Profile, Q, Traced, TracedOn},
};
use resonate_engine::{Equalisation, NodeName};
use resonate_eq::{
    Binding, Catalogue, Corrected, Device, DeviceId, Found, ProfileName, Store, search, suggest,
};

use crate::{Bindings, models::Notice, toast};

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
        let millibels = (band.gain.millibels() + by)
            .clamp(-BandGain::WIDEST_MILLIBELS, BandGain::WIDEST_MILLIBELS);
        BandGain::from_millibels(millibels).unwrap_or(band.gain)
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

pub struct EqualiserModel {
    store: Store,
    corrections: Arc<Corrected>,
    bindings: Bindings,
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
    notice: Option<Notice>,
    untold: bool,
    _saved: Task<()>,
    _imported: Task<()>,
    _exported: Task<()>,
    _catalogued: Task<()>,
    _fetched: Task<()>,
}

impl EqualiserModel {
    pub fn new(folder: PathBuf, corrections: Arc<Corrected>, bindings: Bindings) -> Self {
        let store = Store::at(folder);
        let kept = store.names().unwrap_or_default();

        let mut model = Self {
            store,
            corrections,
            bindings,
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
            notice: None,
            untold: false,
            _saved: Task::ready(()),
            _imported: Task::ready(()),
            _exported: Task::ready(()),
            _catalogued: Task::ready(()),
            _fetched: Task::ready(()),
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
        match editing.cell {
            Cell::Frequency => format!("{:.0}", band.frequency.hertz()),
            Cell::Gain => format!("{:.2}", band.gain.decibels()),
            Cell::Q => format!("{:.3}", band.q.units()),
        }
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
            *slot = band;
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
                    this.notice = Some(Notice::Done(if kept.converted {
                        format!(
                            "kept a graphic curve as {name}, fitted to {} bands here and again \
                             at the rate each stream plays at",
                            kept.profile.bands().len()
                        )
                    } else {
                        format!("Kept {name}, {} bands", kept.profile.bands().len())
                    }));
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

    pub fn export(&mut self, to: PathBuf, cx: &mut Context<Self>) {
        let Some((curve, profile)) = self.shown.clone() else {
            return;
        };
        let name = curve.spoken();
        let folder = self.store.folder().to_path_buf();

        self._exported = cx.spawn(async move |this, cx| {
            let written = cx
                .background_executor()
                .spawn(async move { Store::at(folder).export(&profile, &to) })
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

    pub fn look(&mut self, typed: &str, cx: &mut Context<Self>) {
        if typed.trim().is_empty() {
            self.found.clear();
            cx.notify();
            return;
        }
        if let Some(catalogue) = self.catalogue.clone() {
            self.found = search(&catalogue, typed)
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
                            this.look(&typed, cx);
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

    pub fn suggested(&self, description: &str) -> Option<&Device> {
        let catalogue = self.catalogue.as_ref()?;
        suggest(catalogue, description).and_then(|found| catalogue.device(found))
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

const HERTZ_A_KILOHERTZ: f64 = 1_000.0;
const DIGITS_IN_A_THOUSANDS_GROUP: usize = 3;

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

        assert_eq!(nudged(band, Nudge::Louder).gain.millibels(), 3_500);
        assert_eq!(nudged(band, Nudge::Quieter).gain.millibels(), 2_500);
        assert!(nudged(band, Nudge::Narrower).q > band.q);
        assert!(nudged(band, Nudge::Wider).q < band.q);
        assert_eq!(nudged(band, Nudge::Narrower).frequency, band.frequency);
    }

    #[test]
    fn a_nudge_past_the_ends_of_a_band_stays_at_the_end() {
        let top = Band {
            frequency: Frequency::HIGHEST,
            gain: BandGain::from_millibels(BandGain::WIDEST_MILLIBELS).expect("the widest gain"),
            ..a_band()
        };

        assert_eq!(nudged(top, Nudge::Higher).frequency, Frequency::HIGHEST);
        assert_eq!(
            nudged(top, Nudge::Louder).gain.millibels(),
            BandGain::WIDEST_MILLIBELS
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
}
