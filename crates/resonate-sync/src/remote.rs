use std::time::{Duration, Instant};

use resonate_core::{RemoteTrust, Volume, ZoneId};

use crate::{
    Error, Malformed, Result,
    wire::{Reader, Writer},
};

pub const ZONES_MIRRORED_AT_MOST: usize = 16;
pub const ROWS_MIRRORED_AT_MOST: usize = 500;
pub const MIRROR_LOOKED_AT_EVERY: Duration = Duration::from_millis(250);
pub const MIRROR_TOLD_AGAIN_AFTER: Duration = Duration::from_secs(5);
pub const MIRROR_GOES_STALE_AFTER: Duration = Duration::from_secs(15);
pub const CONTROL_ANSWERED_WITHIN: Duration = Duration::from_secs(10);
const POSITION_DRIFTS_AT_MOST: Duration = Duration::from_secs(1);
const PERMILLE: f32 = 1_000.0;
const ZONE_AT_LEAST: usize = 8;
const ROW_AT_LEAST: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Permille(u16);

impl Permille {
    pub const WHOLE: Self = Self(1_000);

    pub fn of(volume: Volume) -> Self {
        Self((volume.get() * PERMILLE).round().clamp(0.0, PERMILLE) as u16)
    }

    pub const fn new(thousandths: u16) -> Self {
        if thousandths > Self::WHOLE.0 {
            Self::WHOLE
        } else {
            Self(thousandths)
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub fn volume(self) -> Volume {
        Volume::new(f32::from(self.0) / PERMILLE).unwrap_or(Volume::MAX)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek(Duration),
    Volume(Permille),
    Move { from: usize, to: usize },
    Remove(usize),
    Jump(usize),
}

impl RemoteCommand {
    pub const fn edits_the_queue(self) -> bool {
        matches!(self, Self::Move { .. } | Self::Remove(_))
    }

    pub const fn names_a_row(self) -> bool {
        matches!(self, Self::Move { .. } | Self::Remove(_) | Self::Jump(_))
    }

    pub const fn trusted_by(self, trust: RemoteTrust) -> bool {
        trust.shows() && (trust.edits_the_queue() || !self.edits_the_queue())
    }

    pub(crate) fn write(self, writer: &mut Writer) {
        match self {
            Self::Play => writer.byte(0),
            Self::Pause => writer.byte(1),
            Self::Toggle => writer.byte(2),
            Self::Next => writer.byte(3),
            Self::Previous => writer.byte(4),
            Self::Seek(at) => {
                writer.byte(5);
                writer.duration(at);
            }
            Self::Volume(volume) => {
                writer.byte(6);
                writer.number(u64::from(volume.get()));
            }
            Self::Move { from, to } => {
                writer.byte(7);
                writer.length(from);
                writer.length(to);
            }
            Self::Remove(row) => {
                writer.byte(8);
                writer.length(row);
            }
            Self::Jump(row) => {
                writer.byte(9);
                writer.length(row);
            }
        }
    }

    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self> {
        Ok(match reader.byte()? {
            0 => Self::Play,
            1 => Self::Pause,
            2 => Self::Toggle,
            3 => Self::Next,
            4 => Self::Previous,
            5 => Self::Seek(reader.duration()?),
            6 => Self::Volume(Permille::new(
                u16::try_from(reader.number()?).map_err(|_| Error::Malformed(Malformed::Number))?,
            )),
            7 => Self::Move {
                from: row_of(reader)?,
                to: row_of(reader)?,
            },
            8 => Self::Remove(row_of(reader)?),
            9 => Self::Jump(row_of(reader)?),
            tag => return Err(Error::Malformed(Malformed::Tag { tag })),
        })
    }
}

fn row_of(reader: &mut Reader<'_>) -> Result<usize> {
    usize::try_from(reader.number()?).map_err(|_| Error::Malformed(Malformed::Number))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controlling {
    Done,
    Untrusted,
    Uncontrollable,
    NoSuchZone,
    NoSuchRow,
    Failed,
    Unanswered,
}

impl Controlling {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::Done => 0,
            Self::Untrusted => 1,
            Self::Uncontrollable => 2,
            Self::NoSuchZone => 3,
            Self::NoSuchRow => 4,
            Self::Failed => 5,
            Self::Unanswered => 255,
        }
    }

    pub(crate) const fn tagged(tag: u8) -> Self {
        match tag {
            0 => Self::Done,
            1 => Self::Untrusted,
            2 => Self::Uncontrollable,
            3 => Self::NoSuchZone,
            4 => Self::NoSuchRow,
            5 => Self::Failed,
            _ => Self::Unanswered,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uncontrolled {
    NoSuchZone,
    NoSuchRow,
    QueueMoved,
    Failed,
}

impl From<Uncontrolled> for Controlling {
    fn from(uncontrolled: Uncontrolled) -> Self {
        match uncontrolled {
            Uncontrolled::NoSuchZone => Self::NoSuchZone,
            Uncontrolled::NoSuchRow | Uncontrolled::QueueMoved => Self::NoSuchRow,
            Uncontrolled::Failed => Self::Failed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RemotePlayback {
    #[default]
    Idle,
    Playing,
    Paused,
    Stopped,
}

impl RemotePlayback {
    const fn tag(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Playing => 1,
            Self::Paused => 2,
            Self::Stopped => 3,
        }
    }

    const fn tagged(tag: u8) -> Self {
        match tag {
            1 => Self::Playing,
            2 => Self::Paused,
            3 => Self::Stopped,
            _ => Self::Idle,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteRow {
    pub title: String,
    pub artist: String,
    pub length: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteZone {
    pub zone: ZoneId,
    pub name: String,
    pub playback: RemotePlayback,
    pub playing: Option<usize>,
    pub position: Duration,
    pub length: Option<Duration>,
    pub volume: Permille,
    pub queue_length: usize,
    pub first_row: usize,
    pub rows: Vec<RemoteRow>,
    pub queue_revision: Option<u64>,
}

impl RemoteZone {
    pub fn playing_row(&self) -> Option<&RemoteRow> {
        let playing = self.playing?.checked_sub(self.first_row)?;
        self.rows.get(playing)
    }

    pub fn row(&self, row: usize) -> Option<&RemoteRow> {
        self.rows.get(row.checked_sub(self.first_row)?)
    }

    pub(crate) fn write(&self, writer: &mut Writer, revisions: Revisions) {
        writer.number(u64::from(self.zone.get()));
        writer.text(&self.name);
        writer.byte(self.playback.tag());
        writer.number(self.playing.map_or(0, |row| {
            u64::try_from(row).unwrap_or(u64::MAX).saturating_add(1)
        }));
        writer.duration(self.position);
        written_length(writer, self.length);
        writer.number(u64::from(self.volume.get()));
        writer.length(self.queue_length);
        writer.length(self.first_row);
        let rows = &self.rows[..self.rows.len().min(ROWS_MIRRORED_AT_MOST)];
        writer.length(rows.len());
        for row in rows {
            writer.text(&row.title);
            writer.text(&row.artist);
            written_length(writer, row.length);
        }
        if revisions == Revisions::Carried {
            match self.queue_revision {
                Some(revision) => {
                    writer.flag(true);
                    writer.number(revision);
                }
                None => writer.flag(false),
            }
        }
    }

    pub(crate) fn read(reader: &mut Reader<'_>, revisions: Revisions) -> Result<Self> {
        let zone = ZoneId::new(reader.small()?).ok_or(Error::Malformed(Malformed::Number))?;
        let name = reader.text()?;
        let playback = RemotePlayback::tagged(reader.byte()?);
        let playing = match reader.number()? {
            0 => None,
            row => Some(usize::try_from(row - 1).map_err(|_| Error::Malformed(Malformed::Number))?),
        };
        let position = reader.duration()?;
        let length = read_length(reader)?;
        let volume = Permille::new(
            u16::try_from(reader.number()?).map_err(|_| Error::Malformed(Malformed::Number))?,
        );
        let queue_length = row_of(reader)?;
        let first_row = row_of(reader)?;
        let count = reader.count_of(ROW_AT_LEAST)?;
        if count > ROWS_MIRRORED_AT_MOST {
            return Err(Error::Malformed(Malformed::Overlong { length: count }));
        }
        let mut rows = crate::wire::reserved(count);
        for _ in 0..count {
            rows.push(RemoteRow {
                title: reader.text()?,
                artist: reader.text()?,
                length: read_length(reader)?,
            });
        }
        let queue_revision = match revisions {
            Revisions::Carried if reader.flag()? => Some(reader.number()?),
            Revisions::Carried | Revisions::Unsaid => None,
        };
        Ok(Self {
            zone,
            name,
            playback,
            playing,
            position,
            length,
            volume,
            queue_length,
            first_row,
            rows,
            queue_revision,
        })
    }
}

fn written_length(writer: &mut Writer, length: Option<Duration>) {
    match length {
        Some(length) => {
            writer.flag(true);
            writer.duration(length);
        }
        None => writer.flag(false),
    }
}

fn read_length(reader: &mut Reader<'_>) -> Result<Option<Duration>> {
    if reader.flag()? {
        reader.duration().map(Some)
    } else {
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Revisions {
    Unsaid,
    Carried,
}

pub(crate) fn write_zones(writer: &mut Writer, zones: &[RemoteZone], revisions: Revisions) {
    let zones = &zones[..zones.len().min(ZONES_MIRRORED_AT_MOST)];
    writer.length(zones.len());
    for zone in zones {
        zone.write(writer, revisions);
    }
}

pub(crate) fn read_zones(reader: &mut Reader<'_>, revisions: Revisions) -> Result<Vec<RemoteZone>> {
    let count = reader.count_of(ZONE_AT_LEAST)?;
    if count > ZONES_MIRRORED_AT_MOST {
        return Err(Error::Malformed(Malformed::Overlong { length: count }));
    }
    let mut zones = crate::wire::reserved(count);
    for _ in 0..count {
        zones.push(RemoteZone::read(reader, revisions)?);
    }
    Ok(zones)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mirror {
    pub zones: Vec<RemoteZone>,
    pub heard_at: Instant,
}

impl Mirror {
    pub fn position_of(&self, zone: &RemoteZone, now: Instant) -> Duration {
        if zone.playback != RemotePlayback::Playing {
            return zone.position;
        }
        let heard = zone.position + now.saturating_duration_since(self.heard_at);
        zone.length.map_or(heard, |length| heard.min(length))
    }

    pub fn stale(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.heard_at) >= MIRROR_GOES_STALE_AFTER
    }

    pub fn zone(&self, zone: ZoneId) -> Option<&RemoteZone> {
        self.zones.iter().find(|held| held.zone == zone)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Watched {
    #[default]
    Waiting,
    Mirrored(Mirror),
    Refused(Controlling),
}

impl Watched {
    pub const fn mirror(&self) -> Option<&Mirror> {
        match self {
            Self::Mirrored(mirror) => Some(mirror),
            Self::Waiting | Self::Refused(_) => None,
        }
    }
}

pub fn told_alike(was: &[RemoteZone], told_at: Instant, now: &[RemoteZone], at: Instant) -> bool {
    let since = at.saturating_duration_since(told_at);
    was.len() == now.len()
        && was.iter().zip(now).all(|(was, now)| {
            let expected = if was.playback == RemotePlayback::Playing {
                was.position + since
            } else {
                was.position
            };
            let drift = expected.abs_diff(now.position);
            drift < POSITION_DRIFTS_AT_MOST
                && RemoteZone {
                    position: now.position,
                    ..was.clone()
                } == *now
        })
}

pub trait Controlled: Send + Sync + 'static {
    fn zones(&self) -> Vec<RemoteZone>;

    fn control(
        &self,
        zone: ZoneId,
        command: RemoteCommand,
    ) -> std::result::Result<(), Uncontrolled>;

    fn control_if_the_queue_is_still(
        &self,
        zone: ZoneId,
        revision: u64,
        command: RemoteCommand,
    ) -> std::result::Result<(), Uncontrolled>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{FromPeer, ToPeer};

    fn zone(playback: RemotePlayback, position: Duration) -> RemoteZone {
        RemoteZone {
            zone: ZoneId::FIRST,
            name: "Living room".into(),
            playback,
            playing: Some(1),
            position,
            length: Some(Duration::from_secs(200)),
            volume: Permille::new(640),
            queue_length: 3,
            first_row: 0,
            rows: vec![
                RemoteRow {
                    title: "Echoes".into(),
                    artist: "Pink Floyd".into(),
                    length: Some(Duration::from_secs(1_403)),
                },
                RemoteRow {
                    title: "One of These Days".into(),
                    artist: "Pink Floyd".into(),
                    length: None,
                },
                RemoteRow::default(),
            ],
            queue_revision: Some(41),
        }
    }

    #[test]
    fn every_command_a_device_may_send_another_reads_back_as_written() {
        let commands = [
            RemoteCommand::Play,
            RemoteCommand::Pause,
            RemoteCommand::Toggle,
            RemoteCommand::Next,
            RemoteCommand::Previous,
            RemoteCommand::Seek(Duration::from_millis(61_250)),
            RemoteCommand::Volume(Permille::new(250)),
            RemoteCommand::Move { from: 4, to: 0 },
            RemoteCommand::Remove(7),
            RemoteCommand::Jump(2),
        ];

        for command in commands {
            for queue_seen in [None, Some(17)] {
                let asked = ToPeer::Control {
                    zone: ZoneId::new(2).expect("a zone"),
                    command,
                    queue_seen,
                };
                assert_eq!(ToPeer::decoded(&asked.encoded()).expect("decoded"), asked);
            }
        }
        for revisions in [Revisions::Unsaid, Revisions::Carried] {
            let watch = ToPeer::Watch { revisions };
            assert_eq!(ToPeer::decoded(&watch.encoded()).expect("decoded"), watch);
        }
    }

    #[test]
    fn what_an_older_device_reads_is_written_as_it_always_was_and_carries_no_revision() {
        let plain = ToPeer::Control {
            zone: ZoneId::new(2).expect("a zone"),
            command: RemoteCommand::Remove(7),
            queue_seen: None,
        };
        let guarded = ToPeer::Control {
            zone: ZoneId::new(2).expect("a zone"),
            command: RemoteCommand::Remove(7),
            queue_seen: Some(17),
        };
        let unsaid = FromPeer::Zones {
            zones: vec![zone(RemotePlayback::Playing, Duration::from_secs(42))],
            revisions: Revisions::Unsaid,
        };

        assert_eq!(plain.encoded(), [8, 2, 8, 7]);
        assert_eq!(guarded.encoded(), [13, 2, 8, 7, 17]);
        assert_eq!(
            ToPeer::Watch {
                revisions: Revisions::Unsaid
            }
            .encoded(),
            [9]
        );
        assert_eq!(unsaid.encoded()[0], 14);
        let FromPeer::Zones { zones, .. } = FromPeer::decoded(&unsaid.encoded()).expect("decoded")
        else {
            panic!("a mirror");
        };
        assert_eq!(zones[0].queue_revision, None);
    }

    #[test]
    fn a_mirror_of_the_zones_and_every_answer_to_a_command_read_back_as_written() {
        let mirrored = FromPeer::Zones {
            zones: vec![
                zone(RemotePlayback::Playing, Duration::from_secs(42)),
                RemoteZone {
                    queue_revision: None,
                    ..zone(RemotePlayback::Paused, Duration::ZERO)
                },
            ],
            revisions: Revisions::Carried,
        };
        let answers = [
            Controlling::Done,
            Controlling::Untrusted,
            Controlling::Uncontrollable,
            Controlling::NoSuchZone,
            Controlling::NoSuchRow,
            Controlling::Failed,
            Controlling::Unanswered,
        ];

        assert_eq!(
            FromPeer::decoded(&mirrored.encoded()).expect("decoded"),
            mirrored
        );
        for answer in answers {
            let answered = FromPeer::Controlled(answer);
            assert_eq!(
                FromPeer::decoded(&answered.encoded()).expect("decoded"),
                answered
            );
        }
    }

    #[test]
    fn a_command_or_answer_of_a_kind_a_later_build_adds_is_refused_or_read_as_unanswered() {
        let unknown_command = [8, 1, 42];
        let unknown_answer = [13, 77];

        assert!(ToPeer::decoded(&unknown_command).is_err());
        assert_eq!(
            FromPeer::decoded(&unknown_answer).expect("decoded"),
            FromPeer::Controlled(Controlling::Unanswered)
        );
    }

    #[test]
    fn more_rows_than_a_mirror_carries_are_cut_by_the_writer() {
        let mut long = zone(RemotePlayback::Playing, Duration::ZERO);
        long.rows = vec![RemoteRow::default(); ROWS_MIRRORED_AT_MOST + 1];
        let mirrored = FromPeer::Zones {
            zones: vec![long],
            revisions: Revisions::Carried,
        };

        let decoded = FromPeer::decoded(&mirrored.encoded()).expect("decoded");

        let FromPeer::Zones { zones, .. } = decoded else {
            panic!("a mirror");
        };
        assert_eq!(zones[0].rows.len(), ROWS_MIRRORED_AT_MOST);
    }

    #[test]
    fn a_trusted_device_may_do_everything_a_transport_one_no_queue_edit_and_an_untrusted_nothing() {
        assert!(RemoteCommand::Remove(0).trusted_by(RemoteTrust::Everything));
        assert!(RemoteCommand::Seek(Duration::ZERO).trusted_by(RemoteTrust::Transport));
        assert!(RemoteCommand::Jump(3).trusted_by(RemoteTrust::Transport));
        assert!(!RemoteCommand::Move { from: 0, to: 1 }.trusted_by(RemoteTrust::Transport));
        assert!(RemoteCommand::Jump(3).names_a_row());
        assert!(!RemoteCommand::Next.names_a_row());
        assert!(!RemoteCommand::Play.trusted_by(RemoteTrust::Nothing));
        assert!(RemoteTrust::Transport.shows());
        assert!(!RemoteTrust::Nothing.shows());
    }

    #[test]
    fn a_playing_zone_is_told_again_only_where_it_moved_other_than_by_playing_on() {
        let now = Instant::now();
        let later = now + Duration::from_secs(3);
        let was = vec![zone(RemotePlayback::Playing, Duration::from_secs(10))];
        let played_on = vec![zone(RemotePlayback::Playing, Duration::from_secs(13))];
        let sought = vec![zone(RemotePlayback::Playing, Duration::from_secs(90))];
        let paused = vec![zone(RemotePlayback::Paused, Duration::from_secs(13))];

        assert!(told_alike(&was, now, &played_on, later));
        assert!(!told_alike(&was, now, &sought, later));
        assert!(!told_alike(&was, now, &paused, later));
    }

    #[test]
    fn a_mirror_plays_on_from_where_it_was_heard_and_stops_at_the_end_of_the_song() {
        let heard_at = Instant::now();
        let playing = zone(RemotePlayback::Playing, Duration::from_secs(199));
        let paused = zone(RemotePlayback::Paused, Duration::from_secs(20));
        let mirror = Mirror {
            zones: vec![playing.clone(), paused.clone()],
            heard_at,
        };

        let played_on = mirror.position_of(&playing, heard_at + Duration::from_secs(5));
        let held = mirror.position_of(&paused, heard_at + Duration::from_secs(5));

        assert_eq!(played_on, Duration::from_secs(200));
        assert_eq!(held, Duration::from_secs(20));
        assert_eq!(
            playing.playing_row().map(|row| row.title.as_str()),
            Some("One of These Days")
        );
    }
}
