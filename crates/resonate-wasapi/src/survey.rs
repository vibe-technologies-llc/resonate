use std::collections::{BTreeMap, BTreeSet};

use resonate_audio::{SinkFormats, SinkInfo, SinkPort, SinkResult, Words, sink_id_of};
use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

use crate::{
    formats::{formats_taken, rates_of},
    ways::{Way, node_of},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mix {
    pub format: SampleFormat,
    pub rate: SampleRate,
    pub layout: ChannelLayout,
}

pub struct Endpoint<C> {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub mix: Option<Mix>,
    pub port: Option<SinkPort>,
    pub client: C,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    Taken,
    Refused,
    InUse,
    Barred,
}

pub trait Endpoints {
    type Client;

    fn listed(&mut self) -> SinkResult<Vec<Endpoint<Self::Client>>>;
    fn exclusive(
        &self,
        client: &Self::Client,
        format: SampleFormat,
        rate: SampleRate,
        layout: ChannelLayout,
    ) -> Probe;
}

#[derive(Default)]
pub struct Probed {
    taken_while_free: BTreeMap<String, Vec<SinkFormats>>,
    listed_last: BTreeSet<String>,
}

enum Unprobed {
    Free,
    Held,
    Barred,
}

impl Probed {
    pub fn survey<E: Endpoints>(&mut self, endpoints: &mut E) -> SinkResult<Vec<SinkInfo>> {
        let listed = endpoints.listed()?;

        let mut sinks = Vec::with_capacity(listed.len() * 2);

        for endpoint in &listed {
            let formats = self.exclusive_formats(endpoints, endpoint);

            sinks.extend(shared_sink(endpoint));
            sinks.extend(exclusive_sink(endpoint, formats));
        }
        self.listed_last = listed.into_iter().map(|endpoint| endpoint.id).collect();
        Ok(sinks)
    }

    fn exclusive_formats<E: Endpoints>(
        &mut self,
        endpoints: &E,
        endpoint: &Endpoint<E::Client>,
    ) -> Vec<SinkFormats> {
        let id = endpoint.id.as_str();

        if self.listed_last.contains(id)
            && let Some(kept) = self.taken_while_free.get(id)
        {
            return kept.clone();
        }

        let mut unprobed = Unprobed::Free;

        let probed = formats_taken(|format, rate, layout| {
            if !matches!(unprobed, Unprobed::Free) {
                return false;
            }
            match endpoints.exclusive(&endpoint.client, format, rate, layout) {
                Probe::Taken => true,
                Probe::Refused => false,
                Probe::InUse => {
                    unprobed = Unprobed::Held;
                    false
                }
                Probe::Barred => {
                    unprobed = Unprobed::Barred;
                    false
                }
            }
        });
        match unprobed {
            Unprobed::Free => self.taken_while_free(id, probed),
            Unprobed::Held => self.taken_while_free.get(id).cloned().unwrap_or_default(),
            Unprobed::Barred => Vec::new(),
        }
    }

    fn taken_while_free(&mut self, endpoint: &str, probed: Vec<SinkFormats>) -> Vec<SinkFormats> {
        self.taken_while_free
            .insert(endpoint.to_owned(), probed.clone());
        probed
    }
}

fn shared_sink<C>(endpoint: &Endpoint<C>) -> Option<SinkInfo> {
    let mix = endpoint.mix?;
    let node = node_of(Way::Shared, &endpoint.id);
    Some(SinkInfo {
        id: sink_id_of(&node),
        name: node,
        aliases: Vec::new(),
        description: format!("{}{}", endpoint.name, Way::Shared.described()),
        is_default: endpoint.is_default,
        is_hardware: true,
        port: endpoint.port.clone(),
        profile: None,
        formats: vec![SinkFormats {
            format: mix.format,
            words: Words::Whole,
            rates: vec![mix.rate],
            channels: vec![mix.layout],
        }],
        allowed_rates: vec![mix.rate],
        current_rate: Some(mix.rate),
    })
}

fn exclusive_sink<C>(endpoint: &Endpoint<C>, formats: Vec<SinkFormats>) -> Option<SinkInfo> {
    if formats.is_empty() {
        return None;
    }

    let node = node_of(Way::Exclusive, &endpoint.id);
    Some(SinkInfo {
        id: sink_id_of(&node),
        name: node,
        aliases: Vec::new(),
        description: format!("{}{}", endpoint.name, Way::Exclusive.described()),
        is_default: false,
        is_hardware: true,
        allowed_rates: rates_of(&formats),
        formats,
        port: endpoint.port.clone(),
        profile: None,
        current_rate: None,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use resonate_audio::{HardwareVolume, Plugged};
    use resonate_core::Gain;

    use super::*;
    use crate::PROBED_RATES;

    #[derive(Clone)]
    struct Present {
        id: &'static str,
        volume: f32,
        muted: bool,
        rate: SampleRate,
        answers: Probe,
    }

    impl Present {
        const fn free(id: &'static str) -> Self {
            Self {
                id,
                volume: 0.5,
                muted: false,
                rate: SampleRate::HZ_48000,
                answers: Probe::Taken,
            }
        }
    }

    #[derive(Default)]
    struct Fake {
        present: Vec<Present>,
        probes: Cell<usize>,
    }

    impl Fake {
        fn with(present: Vec<Present>) -> Self {
            Self {
                present,
                ..Self::default()
            }
        }

        fn probes_during(&mut self, probed: &mut Probed) -> (usize, Vec<SinkInfo>) {
            let before = self.probes.get();
            let sinks = probed.survey(self).expect("a survey");

            (self.probes.get() - before, sinks)
        }

        fn endpoint(&mut self, id: &str) -> &mut Present {
            self.present
                .iter_mut()
                .find(|present| present.id == id)
                .expect("the endpoint is present")
        }
    }

    impl Endpoints for Fake {
        type Client = Probe;

        fn listed(&mut self) -> SinkResult<Vec<Endpoint<Probe>>> {
            Ok(self
                .present
                .iter()
                .map(|present| Endpoint {
                    id: present.id.to_owned(),
                    name: present.id.to_uppercase(),
                    is_default: false,
                    mix: Some(Mix {
                        format: SampleFormat::F32,
                        rate: present.rate,
                        layout: ChannelLayout::Stereo,
                    }),
                    port: Some(SinkPort {
                        description: String::new(),
                        plugged: Plugged::Unsaid,
                        hardware_volume: HardwareVolume::No,
                        volume: Some(Gain::new(present.volume).expect("a gain")),
                        muted: present.muted,
                    }),
                    client: present.answers,
                })
                .collect())
        }

        fn exclusive(
            &self,
            client: &Probe,
            _: SampleFormat,
            rate: SampleRate,
            layout: ChannelLayout,
        ) -> Probe {
            self.probes.set(self.probes.get() + 1);
            match client {
                Probe::Taken if layout == ChannelLayout::Stereo && rate.hz() <= 96_000 => {
                    Probe::Taken
                }
                Probe::Taken => Probe::Refused,
                other => *other,
            }
        }
    }

    fn exclusive_of<'a>(sinks: &'a [SinkInfo], endpoint: &str) -> Option<&'a SinkInfo> {
        sinks
            .iter()
            .find(|sink| sink.name == node_of(Way::Exclusive, endpoint))
    }

    fn shared_of<'a>(sinks: &'a [SinkInfo], endpoint: &str) -> &'a SinkInfo {
        sinks
            .iter()
            .find(|sink| sink.name == node_of(Way::Shared, endpoint))
            .expect("the shared sink")
    }

    const EVERY_COMBINATION: usize =
        SampleFormat::ALL.len() * crate::PROBED_LAYOUTS.len() * PROBED_RATES.len();

    #[test]
    fn a_volume_turned_is_surveyed_without_a_probe_and_read_into_the_port() {
        let mut fake = Fake::with(vec![Present::free("dac")]);
        let mut probed = Probed::default();

        let (first_probes, first) = fake.probes_during(&mut probed);

        fake.endpoint("dac").volume = 0.1;
        fake.endpoint("dac").muted = true;

        let (nudged_probes, nudged) = fake.probes_during(&mut probed);

        assert_eq!(first_probes, EVERY_COMBINATION);
        assert_eq!(nudged_probes, 0);
        assert_eq!(
            exclusive_of(&nudged, "dac").map(|sink| &sink.formats),
            exclusive_of(&first, "dac").map(|sink| &sink.formats)
        );
        assert_eq!(
            shared_of(&nudged, "dac")
                .port
                .as_ref()
                .and_then(|port| port.volume),
            Gain::new(0.1).ok()
        );
        assert!(
            exclusive_of(&nudged, "dac")
                .and_then(|sink| sink.port.as_ref())
                .is_some_and(|port| port.muted)
        );
    }

    #[test]
    fn a_mix_rate_moved_is_read_without_a_probe() {
        let mut fake = Fake::with(vec![Present::free("dac")]);
        let mut probed = Probed::default();

        let _ = fake.probes_during(&mut probed);

        fake.endpoint("dac").rate = SampleRate::HZ_96000;

        let (probes, sinks) = fake.probes_during(&mut probed);

        assert_eq!(probes, 0);
        assert_eq!(
            shared_of(&sinks, "dac").current_rate,
            Some(SampleRate::HZ_96000)
        );
    }

    #[test]
    fn an_endpoint_added_or_back_after_leaving_is_probed_and_only_it() {
        let mut fake = Fake::with(vec![Present::free("dac")]);
        let mut probed = Probed::default();

        let _ = fake.probes_during(&mut probed);

        fake.present.push(Present::free("headphones"));

        let (added, sinks) = fake.probes_during(&mut probed);

        fake.present.retain(|present| present.id != "dac");

        let (left, _) = fake.probes_during(&mut probed);

        fake.present.push(Present::free("dac"));

        let (back, _) = fake.probes_during(&mut probed);

        assert_eq!(added, EVERY_COMBINATION);
        assert!(exclusive_of(&sinks, "headphones").is_some());
        assert_eq!(left, 0);
        assert_eq!(back, EVERY_COMBINATION);
    }

    #[test]
    fn an_endpoint_held_since_first_seen_is_asked_once_a_survey_until_it_is_free() {
        let mut fake = Fake::with(vec![Present {
            answers: Probe::InUse,
            ..Present::free("dac")
        }]);
        let mut probed = Probed::default();

        let (first, held) = fake.probes_during(&mut probed);
        let (second, _) = fake.probes_during(&mut probed);

        fake.endpoint("dac").answers = Probe::Taken;

        let (freed, free) = fake.probes_during(&mut probed);
        let (after, _) = fake.probes_during(&mut probed);

        assert_eq!((first, second), (1, 1));
        assert!(exclusive_of(&held, "dac").is_none());
        assert_eq!(freed, EVERY_COMBINATION);
        assert!(exclusive_of(&free, "dac").is_some());
        assert_eq!(after, 0);
    }

    #[test]
    fn an_endpoint_held_after_it_was_free_keeps_what_it_took_while_free() {
        let mut fake = Fake::with(vec![Present::free("dac")]);
        let mut probed = Probed::default();

        let (_, free) = fake.probes_during(&mut probed);

        fake.present.clear();

        let _ = fake.probes_during(&mut probed);

        fake.present.push(Present {
            answers: Probe::InUse,
            ..Present::free("dac")
        });

        let (probes, held) = fake.probes_during(&mut probed);

        assert_eq!(probes, 1);
        assert_eq!(
            exclusive_of(&held, "dac").map(|sink| &sink.formats),
            exclusive_of(&free, "dac").map(|sink| &sink.formats)
        );
    }

    #[test]
    fn an_endpoint_barred_from_exclusive_use_is_offered_shared_alone_and_asked_again_next_time() {
        let mut fake = Fake::with(vec![Present {
            answers: Probe::Barred,
            ..Present::free("dac")
        }]);
        let mut probed = Probed::default();

        let (barred, sinks) = fake.probes_during(&mut probed);

        fake.endpoint("dac").answers = Probe::Taken;

        let (allowed, after) = fake.probes_during(&mut probed);

        assert_eq!(barred, 1);
        assert!(exclusive_of(&sinks, "dac").is_none());
        assert_eq!(shared_of(&sinks, "dac").description, "DAC");
        assert_eq!(allowed, EVERY_COMBINATION);
        assert!(exclusive_of(&after, "dac").is_some());
    }
}
