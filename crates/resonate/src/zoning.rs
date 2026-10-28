use std::{collections::BTreeMap, sync::Arc};

use parking_lot::Mutex;
use resonate_codec::Sources;
use resonate_core::{KeptZone, Volume, ZoneId, ZoneName};
use resonate_engine::{Command, EngineConfig, NodeName, Opening, Opens, Player, Zone, Zones};
use resonate_library::Library;
use resonate_mpris::{Mpris, Teller};

use crate::mpris::{self, Serving};

pub(crate) struct Opener {
    base: EngineConfig,
    sources: Arc<Sources>,
    serving: Serving,
    services: Mutex<BTreeMap<ZoneId, Mpris>>,
}

impl Opener {
    pub(crate) fn new(base: EngineConfig, sources: &Arc<Sources>, serving: Serving) -> Arc<Self> {
        Arc::new(Self {
            base,
            sources: Arc::clone(sources),
            serving,
            services: Mutex::new(BTreeMap::new()),
        })
    }

    pub(crate) fn teller(&self) -> Option<Teller> {
        self.services.lock().get(&ZoneId::FIRST).map(Mpris::teller)
    }
}

impl Opens for Opener {
    fn open(&self, opening: &Opening, alongside: &Player) -> resonate_engine::Result<Arc<Player>> {
        let config = EngineConfig {
            sink: opening.sink.clone(),
            volume: opening.volume,
            ..alongside.output_settings().applied_to(self.base.clone())
        };

        Ok(Arc::new(Player::with_sources(
            config,
            Arc::clone(&self.sources),
        )?))
    }

    fn opened(&self, zones: &Arc<Zones>, zone: &Zone) {
        if let Some(served) = mpris::start_zone(&self.serving, zones, zone) {
            self.services.lock().insert(zone.id, served);
        }
    }

    fn closed(&self, zone: &Zone) {
        let served = self.services.lock().remove(&zone.id);
        if let Some(served) = served {
            served.shutdown();
        }
    }
}

pub(crate) fn kept_zones(library: &Library, resumes: bool) -> Vec<KeptZone> {
    if !resumes {
        return Vec::new();
    }
    match library.zones() {
        Ok(kept) => kept,
        Err(error) => {
            tracing::warn!(%error, "the zones kept from the last run could not be read");
            Vec::new()
        }
    }
}

pub(crate) fn first_named(kept: &[KeptZone]) -> ZoneName {
    kept.iter().find(|zone| zone.id.is_first()).map_or_else(
        || ZoneName::first_given_to(ZoneId::FIRST),
        |zone| zone.name.clone(),
    )
}

pub(crate) fn opening_of(kept: &KeptZone) -> Opening {
    Opening {
        id: kept.id,
        name: kept.name.clone(),
        sink: kept.device.as_deref().map(NodeName::new),
        volume: kept.volume.unwrap_or(Volume::MAX),
    }
}

pub(crate) fn restore(zones: &Arc<Zones>, library: &Library, kept: &[KeptZone]) {
    for kept in kept.iter().filter(|zone| !zone.id.is_first()) {
        let zone = match zones.open(&opening_of(kept)) {
            Ok(zone) => zone,
            Err(error) => {
                tracing::warn!(%error, zone = %kept.id, "a zone kept from the last run did not open");
                continue;
            }
        };
        let resumed = match library.resumption_of(zone.id) {
            Ok(resumed) => resumed,
            Err(error) => {
                tracing::warn!(%error, zone = %zone.id, "a zone's queue kept from the last run could not be read");
                None
            }
        };
        if let Some(resumption) = resumed
            && let Err(error) = zone.player.send(Command::Resume(resumption))
        {
            tracing::warn!(%error, zone = %zone.id, "a zone's queue kept from the last run was not taken up");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zone_kept_is_opened_on_its_own_device_at_its_own_volume_and_the_first_keeps_its_name() {
        let second = ZoneId::new(2).expect("a second zone");
        let half = Volume::new(0.5).expect("half way");
        let kept = vec![
            KeptZone {
                id: ZoneId::FIRST,
                name: ZoneName::given("Kitchen").expect("a name"),
                device: None,
                volume: None,
            },
            KeptZone {
                id: second,
                name: ZoneName::given("Study").expect("a name"),
                device: Some("alsa_output.usb".into()),
                volume: Some(half),
            },
        ];

        assert_eq!(first_named(&kept).as_str(), "Kitchen");
        assert_eq!(first_named(&[]).as_str(), "Zone 1");
        assert_eq!(
            opening_of(&kept[1]),
            Opening {
                id: second,
                name: ZoneName::given("Study").expect("a name"),
                sink: Some(NodeName::new("alsa_output.usb")),
                volume: half,
            }
        );
        assert_eq!(
            opening_of(&KeptZone {
                volume: None,
                device: None,
                ..kept[1].clone()
            })
            .volume,
            Volume::MAX
        );
    }
}
