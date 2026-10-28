use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::RwLock;
use resonate_audio::{NodeName, SinkId};
use resonate_core::{Frames, Resumption, Span, Volume, ZoneId, ZoneName};

use crate::{
    Command, Error, Outcome, OutputMode, OutputSettings, OutputStatus, PlaybackState, Player,
    PlayerState, RepeatMode, Result, resumption_of,
};

pub const HANDED_OVER_WITHIN: Duration = Duration::from_secs(10);
const LOOKED_AT_EVERY: Duration = Duration::from_millis(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Within {
    TheZone,
    EveryZone,
}

#[derive(Clone)]
pub struct Zone {
    pub id: ZoneId,
    pub name: ZoneName,
    pub player: Arc<Player>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    pub id: ZoneId,
    pub name: ZoneName,
    pub sink: Option<NodeName>,
    pub volume: Volume,
}

impl Opening {
    pub fn anew(id: ZoneId) -> Self {
        Self {
            id,
            name: ZoneName::first_given_to(id),
            sink: None,
            volume: Volume::MAX,
        }
    }
}

pub trait Opens: Send + Sync + 'static {
    fn open(&self, opening: &Opening, alongside: &Player) -> Result<Arc<Player>>;

    fn opened(&self, _zones: &Arc<Zones>, _zone: &Zone) {}

    fn closed(&self, _zone: &Zone) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub sink: SinkId,
    pub by: ZoneId,
}

pub struct Handover {
    to: ZoneId,
    row: usize,
    at: Frames,
    sounding: bool,
    waiting: JoinHandle<Result<()>>,
}

impl Handover {
    pub const fn row(&self) -> usize {
        self.row
    }

    pub const fn at(&self) -> Frames {
        self.at
    }

    pub const fn sounding(&self) -> bool {
        self.sounding
    }

    pub fn wait(self) -> Result<()> {
        let to = self.to;
        self.waiting
            .join()
            .unwrap_or(Err(Error::HandoverUnheard { zone: to }))
    }
}

#[derive(Clone)]
struct Before {
    repeat: RepeatMode,
    resumption: Resumption,
}

impl Before {
    fn of(player: &Player) -> Self {
        let state = player.state();
        Self {
            repeat: state.repeat,
            resumption: resumption_of(&state, &player.queued()),
        }
    }
}

struct Handed {
    resumed: Outcome,
    played: Option<Outcome>,
    before: Before,
}

pub struct Zones {
    held: RwLock<Vec<Zone>>,
    opens: Arc<dyn Opens>,
    revision: AtomicU64,
}

impl Zones {
    pub fn start(first: Arc<Player>, name: ZoneName, opens: Arc<dyn Opens>) -> Arc<Self> {
        let first = Zone {
            id: ZoneId::FIRST,
            name,
            player: first,
        };
        let zones = Arc::new(Self {
            held: RwLock::new(vec![first.clone()]),
            opens: Arc::clone(&opens),
            revision: AtomicU64::new(0),
        });
        opens.opened(&zones, &first);
        zones
    }

    pub fn listed(&self) -> Vec<Zone> {
        self.held.read().clone()
    }

    pub fn count(&self) -> usize {
        self.held.read().len()
    }

    pub fn first(&self) -> Zone {
        self.held
            .read()
            .first()
            .cloned()
            .unwrap_or_else(|| unreachable!("the first zone is never removed"))
    }

    pub fn zone(&self, id: ZoneId) -> Option<Zone> {
        self.held.read().iter().find(|zone| zone.id == id).cloned()
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn next_id(&self) -> Option<ZoneId> {
        self.held
            .read()
            .iter()
            .map(|zone| zone.id)
            .max()
            .unwrap_or(ZoneId::FIRST)
            .after()
    }

    pub fn add(self: &Arc<Self>) -> Result<Zone> {
        let id = self.next_id().ok_or(Error::NoZoneLeft)?;
        self.open(&Opening::anew(id))
    }

    pub fn open(self: &Arc<Self>, opening: &Opening) -> Result<Zone> {
        if let Some(held) = self.zone(opening.id) {
            return Ok(held);
        }

        let zone = Zone {
            id: opening.id,
            name: opening.name.clone(),
            player: self.opens.open(opening, &self.first().player)?,
        };
        {
            let mut held = self.held.write();
            held.push(zone.clone());
            held.sort_by_key(|zone| zone.id);
        }
        self.moved();
        self.opens.opened(self, &zone);
        Ok(zone)
    }

    pub fn remove(&self, id: ZoneId) -> Result<Zone> {
        if id.is_first() {
            return Err(Error::FirstZoneStays);
        }
        let zone = {
            let mut held = self.held.write();
            let at = held
                .iter()
                .position(|zone| zone.id == id)
                .ok_or(Error::NoSuchZone { zone: id })?;
            held.remove(at)
        };
        self.moved();

        if let Err(error) = zone.player.send(Command::Stop) {
            tracing::debug!(%error, zone = %id, "a zone taken away had already stopped");
        }
        self.opens.closed(&zone);
        Ok(zone)
    }

    pub fn rename(&self, id: ZoneId, name: ZoneName) -> Result<()> {
        {
            let mut held = self.held.write();
            let zone = held
                .iter_mut()
                .find(|zone| zone.id == id)
                .ok_or(Error::NoSuchZone { zone: id })?;
            zone.name = name;
        }
        self.moved();
        Ok(())
    }

    pub fn send(&self, id: ZoneId, command: Command) -> Result<()> {
        let zone = self.zone(id).ok_or(Error::NoSuchZone { zone: id })?;
        if let Command::SetSink(sink) = &command
            && let Some(held) = self.holder_of(id, sink.as_ref())
        {
            return Err(Error::DeviceHeld { by: held.by });
        }

        match command.kind().within() {
            Within::TheZone => zone.player.send(command),
            Within::EveryZone => self
                .listed()
                .iter()
                .map(|zone| zone.player.send(command.clone()))
                .fold(Ok(()), Result::and),
        }
    }

    pub fn send_if_the_queue_is_still(
        &self,
        id: ZoneId,
        revision: u64,
        command: Command,
    ) -> Result<()> {
        let zone = self.zone(id).ok_or(Error::NoSuchZone { zone: id })?;
        match command.kind().within() {
            Within::TheZone => zone.player.send_if_the_queue_is_still(revision, command),
            Within::EveryZone => self.send(id, command),
        }
    }

    pub fn hand_over(self: &Arc<Self>, from: ZoneId, to: ZoneId) -> Result<Handover> {
        if from == to {
            return Err(Error::HandedToItself { zone: from });
        }
        let outgoing = self.zone(from).ok_or(Error::NoSuchZone { zone: from })?;
        let incoming = self.zone(to).ok_or(Error::NoSuchZone { zone: to })?;
        let state = outgoing.player.state();
        let queued = outgoing.player.queued();
        if queued.rows.is_empty() {
            return Err(Error::NothingToHandOver { zone: from });
        }
        let sounding = means_to_sound(&state);

        let wanted = incoming.player.output_settings().sink.clone();
        if let Some(held) = self.holder_of(to, wanted.as_ref())
            && (held.by != from || sounding)
        {
            return Err(Error::DeviceHeld { by: held.by });
        }

        let resumption = resumption_of(&state, &queued);
        let (row, at) = (resumption.landing(), resumption.at);
        let before = Before::of(&incoming.player);

        self.send(to, Command::SetRepeat(state.repeat))?;
        let resumed = incoming.player.request(Command::Resume(resumption))?;
        let played = match sounding.then(|| incoming.player.request(Command::Play)) {
            Some(Err(error)) => {
                self.put_back(to, before, None);
                return Err(error);
            }
            Some(Ok(played)) => Some(played),
            None => None,
        };

        let zones = Arc::clone(self);
        let revision = queued.revision;
        let last = queued.rows.len() - 1;
        let kept = before.clone();
        let waiting = thread::Builder::new()
            .name("resonate-handover".to_owned())
            .spawn(move || {
                let handed = Handed {
                    resumed,
                    played,
                    before: kept,
                };
                let landed = zones.let_the_outgoing_go(from, to, handed, revision, last);
                if let Err(error) = &landed {
                    tracing::warn!(%error, %from, %to, "playback handed over did not land");
                }
                landed
            });
        let Ok(waiting) = waiting else {
            self.put_back(to, before, None);
            return Err(Error::HandoverUnheard { zone: to });
        };

        Ok(Handover {
            to,
            row,
            at,
            sounding,
            waiting,
        })
    }

    fn let_the_outgoing_go(
        &self,
        from: ZoneId,
        to: ZoneId,
        handed: Handed,
        revision: u64,
        last: usize,
    ) -> Result<()> {
        let Handed {
            resumed,
            played,
            before,
        } = handed;

        let resumed = resumed.wait_for(HANDED_OVER_WITHIN);
        let installed = self.zone(to).map(|zone| zone.player.queued().revision);
        let landed = match (resumed, played) {
            (Err(error), _) => Err(error),
            (Ok(()), Some(played)) => self.started(to, played),
            (Ok(()), None) => Ok(()),
        };
        if let Err(error) = landed {
            self.put_back(to, before, installed);
            return Err(error);
        }

        self.send(from, Command::Stop)?;
        self.send_if_the_queue_is_still(from, revision, Command::Remove(Span::between(0, last)))
    }

    fn started(&self, to: ZoneId, played: Outcome) -> Result<()> {
        played.wait_for(HANDED_OVER_WITHIN)?;
        if self.heard_from(to)? {
            Ok(())
        } else {
            Err(Error::HandoverUnheard { zone: to })
        }
    }

    fn put_back(&self, to: ZoneId, before: Before, installed: Option<u64>) {
        let Before { repeat, resumption } = before;
        let resume = Command::Resume(resumption);

        let put = self
            .send(to, Command::Stop)
            .and_then(|()| self.send(to, Command::SetRepeat(repeat)))
            .and_then(|()| match installed {
                Some(revision) => self.send_if_the_queue_is_still(to, revision, resume),
                None => self.send(to, resume),
            });
        if let Err(error) = put {
            tracing::warn!(%error, zone = %to, "the queue a handover replaced was not put back");
        }
    }

    fn heard_from(&self, id: ZoneId) -> Result<bool> {
        let deadline = Instant::now() + HANDED_OVER_WITHIN;
        while Instant::now() < deadline {
            let zone = self.zone(id).ok_or(Error::NoSuchZone { zone: id })?;
            if sounds(&zone.player.state()) {
                return Ok(true);
            }
            thread::sleep(LOOKED_AT_EVERY);
        }
        Ok(false)
    }

    pub fn holding(&self, besides: ZoneId) -> Vec<Held> {
        self.held
            .read()
            .iter()
            .filter(|zone| zone.id != besides)
            .filter_map(|zone| {
                let output = zone.player.state().output?;
                holds_its_device(&output, &zone.player.output_settings()).then_some(Held {
                    sink: output.sink,
                    by: zone.id,
                })
            })
            .collect()
    }

    pub fn holder_of(&self, asking: ZoneId, sink: Option<&NodeName>) -> Option<Held> {
        let held = self.holding(asking);
        if held.is_empty() {
            return None;
        }
        let surveyed = self.zone(asking)?.player.sinks();
        let wanted = surveyed
            .iter()
            .find(|known| match sink {
                Some(name) => &known.name == name,
                None => known.is_default,
            })
            .map(|known| known.id)?;

        held.into_iter().find(|held| held.sink == wanted)
    }

    pub fn leave(&self) {
        for zone in self.listed().iter().rev() {
            self.opens.closed(zone);
        }
    }

    fn moved(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }
}

fn means_to_sound(state: &PlayerState) -> bool {
    matches!(
        state.playback,
        PlaybackState::Playing | PlaybackState::Buffering
    )
}

fn sounds(state: &PlayerState) -> bool {
    state.playback == PlaybackState::Playing && state.output.is_some()
}

pub fn holds_its_device(output: &OutputStatus, settings: &OutputSettings) -> bool {
    settings.prefer_bit_perfect
        && matches!(output.mode, OutputMode::BitPerfect | OutputMode::Repacked)
}
