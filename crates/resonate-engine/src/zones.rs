use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use parking_lot::RwLock;
use resonate_audio::{NodeName, SinkId};
use resonate_core::{Volume, ZoneId, ZoneName};

use crate::{Command, Error, OutputMode, OutputSettings, OutputStatus, Player, Result};

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

pub fn holds_its_device(output: &OutputStatus, settings: &OutputSettings) -> bool {
    settings.prefer_bit_perfect
        && matches!(output.mode, OutputMode::BitPerfect | OutputMode::Repacked)
}
