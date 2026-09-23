use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use gpui::{Context, Task};
use resonate_core::{
    SampleRate,
    eq::{Band, BandGain, BandKind, Frequency, MAX_BANDS, Preamp, Profile, Q, sweep},
};
use resonate_engine::{Equalisation, NodeName};
use resonate_eq::{
    Binding, Catalogue, Corrected, Device, DeviceId, Found, ProfileName, Store, search, suggest,
};

use crate::{Bindings, models::Notice};

pub const CURVE_COLUMNS: usize = 192;
pub const DRAWN_BETWEEN_MILLIBELS: i32 = 15_000;
const PROFILE_SETTLES: Duration = Duration::from_millis(600);
const DRAWN_AT: SampleRate = SampleRate::HZ_48000;
const FOUND_SHOWN: usize = 12;
const Q_PER_NOTCH: f64 = 1.122_462_048_309_373;
const Q_STEPS_PER_UNIT: f64 = 100.0;
const MILLI_PER_UNIT: f64 = 1_000.0;

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
    curve: Arc<[f64]>,
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
    editing: Option<Editing>,
    shaping: Option<usize>,
    chosen: Option<usize>,
    catalogue: Option<Arc<Catalogue>>,
    found: Vec<Found>,
    looking: bool,
    notice: Option<Notice>,
    _saved: Task<()>,
    _asked: Task<()>,
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
            editing: None,
            shaping: None,
            chosen: None,
            catalogue: None,
            found: Vec::new(),
            looking: false,
            notice: None,
            _saved: Task::ready(()),
            _asked: Task::ready(()),
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

    pub const fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn is_looking(&self) -> bool {
        self.looking
    }

    pub fn has_a_source(&self) -> bool {
        self.corrections.has_a_source()
    }

    pub fn told(&mut self, notice: Notice) {
        self.notice = Some(notice);
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
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
                self.notice = Some(Notice::Trouble(error.to_string()));
            }
        }
        self.step();
    }

    fn forget_the_rows(&mut self) {
        self.chosen = None;
        self.editing = None;
        self.shaping = None;
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
            self.notice = Some(Notice::Trouble(error.to_string()));
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
        self.held.remove(&curve);
        self.gather();
    }

    fn step(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.drawn = None;
    }

    pub fn drawn(&mut self, rate: SampleRate) -> Arc<[f64]> {
        if let Some(drawn) = self.drawn.as_ref()
            && drawn.rate == rate
            && drawn.revision == self.revision
        {
            return Arc::clone(&drawn.curve);
        }

        let curve: Arc<[f64]> = match self.shown() {
            Some(profile) => sweep(CURVE_COLUMNS)
                .map(|hertz| profile.magnitude_db(hertz, rate))
                .collect(),
            None => vec![0.0; CURVE_COLUMNS].into(),
        };
        self.drawn = Some(Drawn {
            rate,
            revision: self.revision,
            curve: Arc::clone(&curve),
        });
        curve
    }

    pub fn peak_db(&self) -> f64 {
        self.shown()
            .map_or(0.0, |profile| profile.peak_db(DRAWN_AT))
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
        let Some(number) = resonate_eq::read_number(strip(typed, editing.cell)) else {
            self.notice = Some(Notice::Trouble(refused(editing.cell)));
            return false;
        };

        let read = match editing.cell {
            Cell::Frequency => Frequency::from_hertz(number * scale(typed)).map(|held| {
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

    pub fn fit_the_preamp(&mut self, cx: &mut Context<Self>) {
        if let Some((_, profile)) = self.shown.as_mut() {
            let fitted = profile.fitted_preamp(DRAWN_AT);
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

            let outcome = this.update(cx, |this, _| {
                this.unsaved = false;
                if let Err(error) = written {
                    this.notice = Some(Notice::Trouble(error.to_string()));
                }
            });
            let _ = outcome;
        });
    }

    pub fn import(&mut self, from: PathBuf, cx: &mut Context<Self>) {
        let folder = self.store.folder().to_path_buf();
        self._asked = cx.spawn(async move |this, cx| {
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
                            "converted a graphic curve to {} bands, kept as {name}",
                            kept.profile.bands().len()
                        )
                    } else {
                        format!("kept {name}, {} bands", kept.profile.bands().len())
                    }));
                    cx.notify();
                }
                Err(error) => this.notice = Some(Notice::Trouble(error.to_string())),
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

        self._asked = cx.spawn(async move |this, cx| {
            let written = cx
                .background_executor()
                .spawn(async move { Store::at(folder).export(&profile, &to) })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.notice = Some(match written {
                    Ok(()) => Notice::Done(format!("wrote {name} out")),
                    Err(error) => Notice::Trouble(error.to_string()),
                });
                cx.notify();
            });
            let _ = outcome;
        });
    }

    pub fn forget(&mut self, name: &ProfileName, cx: &mut Context<Self>) {
        match self.store.forget(name) {
            Ok(_) => {
                let forgotten = Binding::Profile(name.clone());
                self.bindings.by_sink.retain(|(_, held)| *held != forgotten);
                if self.bindings.fallback.as_ref() == Some(&forgotten) {
                    self.bindings.fallback = None;
                }
                self.reload();
                self.replaced(name);
                self.step();
                self.notice = Some(Notice::Done(format!("forgot {name}")));
            }
            Err(error) => self.notice = Some(Notice::Trouble(error.to_string())),
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
        if self.looking {
            return;
        }
        self.looking = true;
        let corrections = Arc::clone(&self.corrections);

        self._asked = cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { corrections.catalogue() })
                .await;

            let outcome = this.update(cx, |this, cx| {
                this.looking = false;
                match read {
                    Ok(catalogue) => {
                        this.catalogue = Some(catalogue);
                        if let Some(typed) = then {
                            this.look(&typed, cx);
                        }
                    }
                    Err(error) => this.notice = Some(Notice::Trouble(error.to_string())),
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
        self.looking = true;

        self._asked = cx.spawn(async move |this, cx| {
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
                this.looking = false;
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
                        this.notice =
                            Some(Notice::Trouble("nothing is measured for that".to_owned()));
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

fn strip(typed: &str, cell: Cell) -> &str {
    let typed = typed.trim();
    let without = match cell {
        Cell::Frequency => typed
            .strip_suffix("khz")
            .or_else(|| typed.strip_suffix("kHz"))
            .or_else(|| typed.strip_suffix("kHZ"))
            .or_else(|| typed.strip_suffix("k"))
            .or_else(|| typed.strip_suffix("K"))
            .or_else(|| typed.strip_suffix("hz"))
            .or_else(|| typed.strip_suffix("Hz"))
            .or_else(|| typed.strip_suffix("HZ")),
        Cell::Gain => typed
            .strip_suffix("dB")
            .or_else(|| typed.strip_suffix("db"))
            .or_else(|| typed.strip_suffix("DB")),
        Cell::Q => None,
    };
    without.unwrap_or(typed).trim()
}

fn scale(typed: &str) -> f64 {
    let folded = typed.trim().to_lowercase();
    if folded.ends_with("khz") || folded.ends_with('k') {
        1_000.0
    } else {
        1.0
    }
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
