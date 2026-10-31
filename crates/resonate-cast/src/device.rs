use std::{fmt, net::SocketAddr, time::Duration};

use resonate_audio::NodeName;
use resonate_core::{ChannelLayout, SampleRate, Volume};
use resonate_upnp::{Carriage, Wrapping};

use crate::{
    Asked, Error, Result,
    json::Json,
    link::{ANSWERED_WITHIN, Link, Namespace, RECEIVER},
};

pub const CAST_NODES: &str = "cast:";

pub const CAST_PORT: u16 = 8009;

pub fn receiver_address(text: &str) -> Option<SocketAddr> {
    let text = text.trim();
    text.parse::<SocketAddr>().ok().or_else(|| {
        text.trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .ok()
            .map(|ip| SocketAddr::new(ip, CAST_PORT))
    })
}

pub const MEDIA_RECEIVER: &str = "CC1AD845";

const LAUNCHED_WITHIN: Duration = Duration::from_secs(20);

const RATES_TAKEN: [SampleRate; 4] = [
    SampleRate::HZ_44100,
    SampleRate::HZ_48000,
    SampleRate::HZ_88200,
    SampleRate::HZ_96000,
];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CastId(Box<str>);

impl CastId {
    pub fn new(id: impl Into<Box<str>>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn at(at: SocketAddr) -> Self {
        let spelled = at.ip().to_string().replace(['.', ':'], "-");
        Self(format!("at-{spelled}-{}", at.port()).into())
    }
}

impl Found {
    pub fn at(at: SocketAddr) -> Self {
        Self {
            id: CastId::at(at),
            name: at.ip().to_string(),
            at,
        }
    }
}

impl fmt::Display for CastId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub id: CastId,
    pub name: String,
    pub at: SocketAddr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    found: Found,
    volume: Option<Volume>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Session {
    pub(crate) transport: String,
}

struct Status {
    volume: Option<Volume>,
    media: Option<Session>,
}

impl Device {
    pub fn reached(found: Found) -> Result<Self> {
        let link = Link::connect(found.at)?;
        let status = status(&link)?;
        Ok(Self {
            found,
            volume: status.volume,
        })
    }

    pub const fn id(&self) -> &CastId {
        &self.found.id
    }

    pub fn name(&self) -> &str {
        &self.found.name
    }

    pub const fn at(&self) -> SocketAddr {
        self.found.at
    }

    pub const fn volume(&self) -> Option<Volume> {
        self.volume
    }

    pub fn node_name(&self) -> NodeName {
        NodeName::new(format!("{CAST_NODES}{}", self.found.id))
    }

    pub fn carriage() -> Carriage {
        Carriage::new(
            Wrapping::Flac,
            RATES_TAKEN.to_vec(),
            vec![ChannelLayout::Stereo],
        )
    }
}

fn status(link: &Link) -> Result<Status> {
    let reply = link.ask(
        Namespace::Receiver,
        RECEIVER,
        Asked::Status,
        Json::Object(Vec::new()),
        ANSWERED_WITHIN,
    )?;
    status_in(&reply).ok_or(Error::Unreadable {
        asked: Asked::Status,
    })
}

fn status_in(reply: &Json) -> Option<Status> {
    if reply.kind() != Some("RECEIVER_STATUS") {
        return None;
    }
    let status = reply.get("status")?;
    let volume = status
        .get("volume")
        .and_then(|volume| volume.get("level"))
        .and_then(Json::as_number)
        .and_then(|level| Volume::new((level as f32).clamp(0.0, 1.0)).ok());
    let media = status
        .get("applications")
        .and_then(Json::as_list)
        .unwrap_or_default()
        .iter()
        .find(|application| application.get("appId").and_then(Json::as_str) == Some(MEDIA_RECEIVER))
        .and_then(|application| application.get("transportId"))
        .and_then(Json::as_str)
        .map(|transport| Session {
            transport: transport.to_owned(),
        });
    Some(Status { volume, media })
}

pub(crate) fn launched(link: &Link) -> Result<Session> {
    if let Some(session) = status(link)?.media {
        return Ok(session);
    }
    let reply = link.ask(
        Namespace::Receiver,
        RECEIVER,
        Asked::Launch,
        Json::object([("appId", Json::text(MEDIA_RECEIVER))]),
        LAUNCHED_WITHIN,
    )?;
    status_in(&reply)
        .and_then(|status| status.media)
        .ok_or(Error::Unreadable {
            asked: Asked::Launch,
        })
}

pub(crate) fn set_volume(link: &Link, volume: Volume) -> Result<()> {
    link.ask(
        Namespace::Receiver,
        RECEIVER,
        Asked::SetVolume,
        Json::object([(
            "volume",
            Json::object([("level", f64::from(volume.get()).into())]),
        )]),
        ANSWERED_WITHIN,
    )
    .map(drop)
}

pub(crate) fn set_mute(link: &Link, muted: bool) -> Result<()> {
    link.ask(
        Namespace::Receiver,
        RECEIVER,
        Asked::SetMute,
        Json::object([("volume", Json::object([("muted", muted.into())]))]),
        ANSWERED_WITHIN,
    )
    .map(drop)
}

#[cfg(test)]
mod tests {
    use resonate_core::{SampleFormat, StreamSpec};

    use super::*;

    #[test]
    fn the_media_receiver_is_found_among_the_applications_and_the_volume_read() {
        let reply = Json::read(
            r#"{"type":"RECEIVER_STATUS","status":{"volume":{"level":0.4,"muted":false},
               "applications":[{"appId":"E8C28D3C","transportId":"backdrop"},
               {"appId":"CC1AD845","sessionId":"s","transportId":"web-7"}]}}"#,
        )
        .expect("JSON");
        let status = status_in(&reply).expect("a status");

        assert_eq!(
            status.media,
            Some(Session {
                transport: "web-7".to_owned()
            })
        );
        assert!(
            status
                .volume
                .is_some_and(|volume| (volume.get() - 0.4).abs() < 1e-6)
        );
        assert!(
            status_in(&Json::read(r#"{"type":"MEDIA_STATUS","status":[]}"#).expect("JSON"))
                .is_none()
        );
    }

    #[test]
    fn a_receiver_is_addressed_by_an_ip_with_the_cast_port_unless_one_is_given() {
        assert_eq!(
            receiver_address(" 192.168.1.20 "),
            Some(SocketAddr::from(([192, 168, 1, 20], CAST_PORT)))
        );
        assert_eq!(
            receiver_address("192.168.1.20:8010"),
            Some(SocketAddr::from(([192, 168, 1, 20], 8010)))
        );
        assert_eq!(
            receiver_address("[fe80::1]"),
            Some(SocketAddr::new(
                "fe80::1".parse().expect("an address"),
                CAST_PORT
            ))
        );
        assert_eq!(
            receiver_address("[fe80::1]:8011").map(|at| at.port()),
            Some(8011)
        );
        assert_eq!(receiver_address("living-room.local"), None);
        assert_eq!(receiver_address(""), None);
    }

    #[test]
    fn a_receiver_given_by_address_is_named_by_it() {
        let at = SocketAddr::from(([192, 168, 1, 20], 8009));

        assert_eq!(CastId::at(at).as_str(), "at-192-168-1-20-8009");
        assert_eq!(Found::at(at).name, "192.168.1.20");
        assert_ne!(
            CastId::at(at),
            CastId::at(SocketAddr::from(([192, 168, 1, 20], 8010)))
        );
    }

    #[test]
    fn a_receiver_takes_stereo_flac_to_ninety_six_kilohertz() {
        let carriage = Device::carriage();
        let stereo = |rate, format| StreamSpec::new(rate, ChannelLayout::Stereo, format);

        assert!(carriage.carries(stereo(SampleRate::HZ_96000, SampleFormat::S24)));
        assert!(carriage.carries(stereo(SampleRate::HZ_44100, SampleFormat::S16)));
        assert!(!carriage.carries(stereo(SampleRate::HZ_192000, SampleFormat::S24)));
        assert!(!carriage.carries(StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S24
        )));
        assert_eq!(
            carriage.kind_of(stereo(SampleRate::HZ_48000, SampleFormat::S16)),
            "audio/flac"
        );
    }
}
