use std::{fmt, time::Duration};

use resonate_audio::{NodeName, SinkFormats, Words};
use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec, Volume};

use crate::{
    Action, Error, Located, Result,
    description::{self, Service},
    soap::{self, Answered, escaped},
};

pub const RENDERER_NODES: &str = "upnp:";

const INSTANCE: (&str, &str) = ("InstanceID", "0");

const MASTER: (&str, &str) = ("Channel", "Master");

const WAVE_KINDS: [&str; 4] = ["audio/wav", "audio/x-wav", "audio/wave", "audio/vnd.wave"];

const L16_KIND: &str = "audio/l16";

const RATES_OFFERED: [SampleRate; 6] = [
    SampleRate::HZ_44100,
    SampleRate::HZ_48000,
    SampleRate::HZ_88200,
    SampleRate::HZ_96000,
    SampleRate::HZ_176400,
    SampleRate::HZ_192000,
];

const L16_RATES_UNSAID: [SampleRate; 2] = [SampleRate::HZ_44100, SampleRate::HZ_48000];

const PERCENT: f32 = 100.0;

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

impl fmt::Display for Udn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Carriage {
    Wave,
    L16(Vec<SampleRate>),
}

impl Carriage {
    pub fn formats(&self) -> Vec<SinkFormats> {
        let stereo = vec![ChannelLayout::Stereo];
        match self {
            Self::Wave => vec![
                SinkFormats {
                    format: SampleFormat::S16,
                    words: Words::Whole,
                    rates: RATES_OFFERED.to_vec(),
                    channels: stereo.clone(),
                },
                SinkFormats {
                    format: SampleFormat::S24,
                    words: Words::Packed,
                    rates: RATES_OFFERED.to_vec(),
                    channels: stereo,
                },
            ],
            Self::L16(rates) => vec![SinkFormats {
                format: SampleFormat::S16,
                words: Words::Whole,
                rates: rates.clone(),
                channels: stereo,
            }],
        }
    }

    pub fn rates(&self) -> Vec<SampleRate> {
        match self {
            Self::Wave => RATES_OFFERED.to_vec(),
            Self::L16(rates) => rates.clone(),
        }
    }

    pub fn carries(&self, spec: StreamSpec) -> bool {
        spec.channels == ChannelLayout::Stereo
            && self
                .formats()
                .iter()
                .any(|offered| offered.format == spec.format && offered.rates.contains(&spec.rate))
    }

    pub fn kind_of(&self, spec: StreamSpec) -> String {
        match self {
            Self::Wave => "audio/wav".to_owned(),
            Self::L16(_) => format!(
                "audio/L16;rate={};channels={}",
                spec.rate.hz(),
                spec.channel_count().get()
            ),
        }
    }

    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Wave => "wav",
            Self::L16(_) => "pcm",
        }
    }

    fn accepted_by(sink: &str) -> Option<Self> {
        let mut wave = false;
        let mut l16 = None::<Vec<SampleRate>>;
        for entry in sink.split(',') {
            let Some(kind) = entry.split(':').nth(2) else {
                continue;
            };
            let kind = kind.trim().to_ascii_lowercase();
            let mut parts = kind.split(';');
            let named = parts.next().unwrap_or_default().trim();
            if WAVE_KINDS.contains(&named) {
                wave = true;
            } else if named == L16_KIND {
                let rates = l16.get_or_insert_with(Vec::new);
                for part in parts {
                    if let Some(rate) = part.trim().strip_prefix("rate=")
                        && let Ok(hz) = rate.parse()
                        && let Ok(rate) = SampleRate::new(hz)
                        && !rates.contains(&rate)
                    {
                        rates.push(rate);
                    }
                }
            }
        }
        if wave {
            return Some(Self::Wave);
        }
        l16.map(|rates| {
            if rates.is_empty() {
                Self::L16(L16_RATES_UNSAID.to_vec())
            } else {
                Self::L16(rates)
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Renderer {
    udn: Udn,
    name: String,
    location: Located,
    transport: Service,
    rendering: Option<Service>,
    carriage: Carriage,
    volume: Option<Volume>,
}

impl Renderer {
    pub fn reached(location: Located) -> Result<Self> {
        let described = description::described(&location)?;
        let carriage = match described.connection.as_ref() {
            Some(connection) => match soap::call(connection, Action::GetProtocolInfo, &[]) {
                Ok(answered) => Carriage::accepted_by(answered.named("Sink").unwrap_or_default())
                    .ok_or(Error::TakesNoPcm)?,
                Err(error) => {
                    tracing::debug!(%error, udn = %described.udn, "a renderer did not say what it takes; WAVE is offered");
                    Carriage::Wave
                }
            },
            None => Carriage::Wave,
        };
        let volume = described
            .rendering
            .as_ref()
            .and_then(|rendering| {
                soap::call(rendering, Action::GetVolume, &[INSTANCE, MASTER]).ok()
            })
            .and_then(|answered| volume_of(&answered));

        Ok(Self {
            udn: described.udn,
            name: described.name,
            location,
            transport: described.transport,
            rendering: described.rendering,
            carriage,
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

    pub const fn carriage(&self) -> &Carriage {
        &self.carriage
    }

    pub const fn volume(&self) -> Option<Volume> {
        self.volume
    }

    pub const fn turns_its_own_volume(&self) -> bool {
        self.rendering.is_some()
    }

    pub fn node_name(&self) -> NodeName {
        NodeName::new(format!("{RENDERER_NODES}{}", self.udn))
    }

    pub(crate) fn transport_reached_at(&self) -> &Located {
        &self.transport.control
    }

    pub(crate) fn set_uri(&self, address: &str, title: &str, spec: StreamSpec) -> Result<()> {
        let metadata = didl(address, title, &self.carriage.kind_of(spec));
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
    fn a_renderer_taking_wave_is_offered_wave_and_one_taking_l16_alone_its_rates() {
        let wave = "http-get:*:audio/mpeg:*,http-get:*:audio/x-wav:*,http-get:*:audio/L16;rate=44100;channels=2:*";
        let l16 = "http-get:*:audio/L16;rate=48000;channels=2:*,http-get:*:audio/L16;rate=44100;channels=2:DLNA.ORG_PN=LPCM";
        let bare = "http-get:*:audio/L16:*";

        assert_eq!(Carriage::accepted_by(wave), Some(Carriage::Wave));
        assert_eq!(
            Carriage::accepted_by(l16),
            Some(Carriage::L16(vec![
                SampleRate::HZ_48000,
                SampleRate::HZ_44100
            ]))
        );
        assert_eq!(
            Carriage::accepted_by(bare),
            Some(Carriage::L16(L16_RATES_UNSAID.to_vec()))
        );
        assert_eq!(Carriage::accepted_by("http-get:*:audio/flac:*"), None);
    }

    #[test]
    fn a_wave_carriage_takes_sixteen_and_twenty_four_bit_stereo_and_nothing_else() {
        let wave = Carriage::Wave;

        assert!(wave.carries(StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Stereo,
            SampleFormat::S24
        )));
        assert!(!wave.carries(StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Stereo,
            SampleFormat::F32
        )));
        assert!(!wave.carries(StreamSpec::new(
            SampleRate::HZ_44100,
            ChannelLayout::Surround51,
            SampleFormat::S16
        )));
    }

    #[test]
    fn a_renderers_clock_is_read_to_the_fraction_and_an_unread_one_is_nothing() {
        assert_eq!(clock_of("0:01:02.500"), Some(Duration::from_millis(62_500)));
        assert_eq!(clock_of("10:00:00"), Some(Duration::from_secs(36_000)));
        assert_eq!(clock_of("NOT_IMPLEMENTED"), None);
    }
}
