use std::{collections::BTreeSet, fmt, time::Duration};

use resonate_audio::{NodeName, RENDERER_NODES};
use resonate_core::{StreamSpec, Volume};

use crate::{
    Action, Carriage, Error, Located, Result,
    carriage::FlacUnsaid,
    description::{self, Service},
    soap::{self, Answered, escaped},
};

const INSTANCE: (&str, &str) = ("InstanceID", "0");

const MASTER: (&str, &str) = ("Channel", "Master");

const WAVE_ASSUMED: &str = "http-get:*:audio/wav:*";

const PERCENT: f32 = 100.0;

const OPENHOME_STEPS_UNSAID: u32 = 100;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Udn(Box<str>);

impl Udn {
    pub fn new(udn: impl Into<Box<str>>) -> Self {
        Self(udn.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Udn {
    pub fn from_node(node: &NodeName) -> Option<Self> {
        node.as_str().strip_prefix(RENDERER_NODES).map(Self::new)
    }
}

impl fmt::Display for Udn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vouched {
    flac_at_any_rate: BTreeSet<Udn>,
}

impl Vouched {
    pub fn flac_at_any_rate(renderers: impl IntoIterator<Item = Udn>) -> Self {
        Self {
            flac_at_any_rate: renderers.into_iter().collect(),
        }
    }

    fn flac_unsaid_for(&self, udn: &Udn) -> FlacUnsaid {
        if self.flac_at_any_rate.contains(udn) {
            FlacUnsaid::AnyRate
        } else {
            FlacUnsaid::UpTo96kHz
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpenHomeSteps(u32);

impl OpenHomeSteps {
    fn of(self, volume: Volume) -> u32 {
        (volume.get() * self.0 as f32)
            .round()
            .clamp(0.0, self.0 as f32) as u32
    }

    fn volume_at(self, step: u32) -> Option<Volume> {
        Volume::new((step as f32 / self.0.max(1) as f32).clamp(0.0, 1.0)).ok()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct OpenHomeVolume {
    service: Service,
    steps: OpenHomeSteps,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Renderer {
    udn: Udn,
    name: String,
    location: Located,
    transport: Service,
    rendering: Option<Service>,
    openhome: Option<OpenHomeVolume>,
    carriages: Vec<Carriage>,
    volume: Option<Volume>,
}

impl Renderer {
    pub fn reached(location: Located, vouched: &Vouched) -> Result<Self> {
        let described = description::described(&location)?;
        let flac_unsaid = vouched.flac_unsaid_for(&described.udn);
        if flac_unsaid == FlacUnsaid::AnyRate {
            tracing::info!(udn = %described.udn, "FLAC is offered at any rate the renderer does not state, as the listener vouched");
        }
        let carriages = match described.connection.as_ref() {
            Some(connection) => match soap::call(connection, Action::GetProtocolInfo, &[]) {
                Ok(answered) => {
                    let carriages = Carriage::accepted_by(
                        answered.named("Sink").unwrap_or_default(),
                        flac_unsaid,
                    );
                    if carriages.is_empty() {
                        return Err(Error::TakesNoCarriage);
                    }
                    carriages
                }
                Err(error) => {
                    tracing::debug!(%error, udn = %described.udn, "a renderer did not say what it takes; WAVE is offered");
                    Carriage::accepted_by(WAVE_ASSUMED, flac_unsaid)
                }
            },
            None => Carriage::accepted_by(WAVE_ASSUMED, flac_unsaid),
        };
        let openhome = described.openhome_volume.map(|service| {
            let steps = soap::call(&service, Action::Characteristics, &[])
                .ok()
                .and_then(|answered| answered.named("VolumeMax").map(str::to_owned))
                .and_then(|max| max.trim().parse().ok())
                .filter(|max| *max > 0)
                .unwrap_or(OPENHOME_STEPS_UNSAID);
            OpenHomeVolume {
                service,
                steps: OpenHomeSteps(steps),
            }
        });
        let volume = match (openhome.as_ref(), described.rendering.as_ref()) {
            (Some(openhome), _) => soap::call(&openhome.service, Action::Volume, &[])
                .ok()
                .and_then(|answered| {
                    let step = answered.named("Value")?.trim().parse().ok()?;
                    openhome.steps.volume_at(step)
                }),
            (None, Some(rendering)) => {
                soap::call(rendering, Action::GetVolume, &[INSTANCE, MASTER])
                    .ok()
                    .and_then(|answered| volume_of(&answered))
            }
            (None, None) => None,
        };

        Ok(Self {
            udn: described.udn,
            name: described.name,
            location,
            transport: described.transport,
            rendering: described.rendering,
            openhome,
            carriages,
            volume,
        })
    }

    pub const fn udn(&self) -> &Udn {
        &self.udn
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn location(&self) -> &Located {
        &self.location
    }

    pub fn carriages(&self) -> &[Carriage] {
        &self.carriages
    }

    pub fn carriage_for(&self, spec: StreamSpec) -> Option<&Carriage> {
        self.carriages
            .iter()
            .find(|carriage| carriage.carries(spec))
    }

    pub const fn volume(&self) -> Option<Volume> {
        self.volume
    }

    pub const fn turns_its_own_volume(&self) -> bool {
        self.rendering.is_some() || self.openhome.is_some()
    }

    pub const fn speaks_openhome_volume(&self) -> bool {
        self.openhome.is_some()
    }

    pub fn node_name(&self) -> NodeName {
        NodeName::new(format!("{RENDERER_NODES}{}", self.udn))
    }

    pub(crate) fn transport_reached_at(&self) -> &Located {
        &self.transport.control
    }

    pub(crate) fn set_uri(
        &self,
        carriage: &Carriage,
        address: &str,
        title: &str,
        spec: StreamSpec,
    ) -> Result<()> {
        let metadata = didl(address, title, &carriage.kind_of(spec));
        soap::call(
            &self.transport,
            Action::SetAvTransportUri,
            &[
                INSTANCE,
                ("CurrentURI", address),
                ("CurrentURIMetaData", &metadata),
            ],
        )
        .map(drop)
    }

    pub(crate) fn play(&self) -> Result<()> {
        soap::call(&self.transport, Action::Play, &[INSTANCE, ("Speed", "1")]).map(drop)
    }

    pub(crate) fn pause(&self) -> Result<()> {
        soap::call(&self.transport, Action::Pause, &[INSTANCE]).map(drop)
    }

    pub(crate) fn stop(&self) -> Result<()> {
        soap::call(&self.transport, Action::Stop, &[INSTANCE]).map(drop)
    }

    pub(crate) fn heard(&self) -> Result<Option<Duration>> {
        let answered = soap::call(&self.transport, Action::GetPositionInfo, &[INSTANCE])?;
        let time = answered.named("RelTime").ok_or(Error::Unanswered {
            action: Action::GetPositionInfo,
        })?;
        Ok(clock_of(time))
    }

    pub(crate) fn set_volume(&self, volume: Volume) -> Result<()> {
        if let Some(openhome) = self.openhome.as_ref() {
            return soap::call(
                &openhome.service,
                Action::SetVolume,
                &[("Value", &openhome.steps.of(volume).to_string())],
            )
            .map(drop);
        }
        let Some(rendering) = self.rendering.as_ref() else {
            return Ok(());
        };
        let percent = (volume.get() * PERCENT).round().clamp(0.0, PERCENT) as u8;
        soap::call(
            rendering,
            Action::SetVolume,
            &[INSTANCE, MASTER, ("DesiredVolume", &percent.to_string())],
        )
        .map(drop)
    }

    pub(crate) fn set_mute(&self, muted: bool) -> Result<()> {
        if let Some(openhome) = self.openhome.as_ref() {
            let muted = if muted { "true" } else { "false" };
            return soap::call(&openhome.service, Action::SetMute, &[("Value", muted)]).map(drop);
        }
        let Some(rendering) = self.rendering.as_ref() else {
            return Ok(());
        };
        let muted = if muted { "1" } else { "0" };
        soap::call(
            rendering,
            Action::SetMute,
            &[INSTANCE, MASTER, ("DesiredMute", muted)],
        )
        .map(drop)
    }
}

fn volume_of(answered: &Answered) -> Option<Volume> {
    let percent: f32 = answered.named("CurrentVolume")?.trim().parse().ok()?;
    Volume::new((percent / PERCENT).clamp(0.0, 1.0)).ok()
}

fn didl(address: &str, title: &str, kind: &str) -> String {
    format!(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\"><item id=\"resonate\" \
         parentID=\"0\" restricted=\"1\"><dc:title>{}</dc:title><upnp:class>object.item.audioItem.\
         musicTrack</upnp:class><res protocolInfo=\"http-get:*:{}:*\">{}</res></item></DIDL-Lite>",
        escaped(title),
        escaped(kind),
        escaped(address),
    )
}

fn clock_of(time: &str) -> Option<Duration> {
    let mut fields = time.trim().rsplitn(3, ':');
    let seconds: f64 = fields.next()?.parse().ok()?;
    let minutes: u64 = fields.next()?.parse().ok()?;
    let hours: u64 = fields.next()?.trim_start_matches('+').parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let whole = hours
        .checked_mul(3_600)?
        .checked_add(minutes.checked_mul(60)?)?;
    Some(Duration::from_secs(whole) + Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renderers_clock_is_read_to_the_fraction_and_an_unread_one_is_nothing() {
        assert_eq!(clock_of("0:01:02.500"), Some(Duration::from_millis(62_500)));
        assert_eq!(clock_of("10:00:00"), Some(Duration::from_secs(36_000)));
        assert_eq!(clock_of("NOT_IMPLEMENTED"), None);
    }

    #[test]
    fn an_openhome_volume_is_counted_in_the_steps_the_renderer_has() {
        let steps = OpenHomeSteps(40);

        assert_eq!(steps.of(Volume::new(0.5).expect("a volume")), 20);
        assert_eq!(steps.of(Volume::new(1.0).expect("a volume")), 40);
        assert_eq!(steps.of(Volume::new(0.0).expect("a volume")), 0);
        assert_eq!(steps.volume_at(10), Volume::new(0.25).ok());
        assert_eq!(steps.volume_at(400), Volume::new(1.0).ok());
    }
}
