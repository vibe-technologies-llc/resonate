use resonate_core::Recording;

use crate::{
    DeviceId, Result,
    error::{Error, Malformed},
    wire::{Reader, Writer},
};

const SAID_HELLO: u8 = 0;

const SAID_PEOPLE: u8 = 1;

const SAID_NOW: u8 = 2;

const SAID_ASK: u8 = 3;

pub const SONGS_AHEAD_TOLD_AT_MOST: usize = 8;

const PEOPLE_TOLD_AT_MOST: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartySong {
    pub row: u64,
    pub recording: Recording,
    pub isrc: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyNow {
    pub song: Option<PartySong>,
    pub next: Vec<PartySong>,
    pub playing: bool,
    pub position_nanos: u64,
    pub anchor_nanos: u64,
    pub coarse: bool,
}

impl PartyNow {
    pub fn position_at(&self, server_nanos: u64) -> u64 {
        if self.playing {
            self.position_nanos
                .saturating_add(server_nanos.saturating_sub(self.anchor_nanos))
        } else {
            self.position_nanos
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartySaid {
    Hello { name: String },
    People(Vec<(DeviceId, String)>),
    Now(PartyNow),
    Ask(PartySong),
}

fn written_song(writer: &mut Writer, song: &PartySong) {
    writer.number(song.row);
    writer.recording(&song.recording);
    writer.flag(song.isrc.is_some());
    if let Some(isrc) = &song.isrc {
        writer.text(isrc);
    }
}

fn read_song(reader: &mut Reader<'_>) -> Result<PartySong> {
    let row = reader.number()?;
    let recording = reader.recording()?;
    let isrc = if reader.flag()? {
        Some(reader.text()?)
    } else {
        None
    };

    Ok(PartySong {
        row,
        recording,
        isrc,
    })
}

impl PartySaid {
    pub fn encoded(&self) -> Vec<u8> {
        let mut writer = Writer::new();

        match self {
            Self::Hello { name } => {
                writer.byte(SAID_HELLO);
                writer.text(name);
            }
            Self::People(people) => {
                writer.byte(SAID_PEOPLE);
                writer.length(people.len());
                for (member, name) in people {
                    writer.device(*member);
                    writer.text(name);
                }
            }
            Self::Now(now) => {
                writer.byte(SAID_NOW);
                writer.flag(now.song.is_some());
                if let Some(song) = &now.song {
                    written_song(&mut writer, song);
                }
                let next = &now.next[..now.next.len().min(SONGS_AHEAD_TOLD_AT_MOST)];
                writer.length(next.len());
                for song in next {
                    written_song(&mut writer, song);
                }
                writer.flag(now.playing);
                writer.number(now.position_nanos);
                writer.number(now.anchor_nanos);
                writer.flag(now.coarse);
            }
            Self::Ask(song) => {
                writer.byte(SAID_ASK);
                written_song(&mut writer, song);
            }
        }

        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let said = match reader.byte()? {
            SAID_HELLO => Self::Hello {
                name: reader.text()?,
            },
            SAID_PEOPLE => {
                let count = reader.length(PEOPLE_TOLD_AT_MOST)?;
                let mut people = Vec::with_capacity(count);
                for _ in 0..count {
                    let member = reader.device()?;
                    people.push((member, reader.text()?));
                }
                Self::People(people)
            }
            SAID_NOW => {
                let song = if reader.flag()? {
                    Some(read_song(&mut reader)?)
                } else {
                    None
                };
                let count = reader.length(SONGS_AHEAD_TOLD_AT_MOST)?;
                let mut next = Vec::with_capacity(count);
                for _ in 0..count {
                    next.push(read_song(&mut reader)?);
                }
                Self::Now(PartyNow {
                    song,
                    next,
                    playing: reader.flag()?,
                    position_nanos: reader.number()?,
                    anchor_nanos: reader.number()?,
                    coarse: reader.flag()?,
                })
            }
            SAID_ASK => Self::Ask(read_song(&mut reader)?),
            tag => return Err(Error::Malformed(Malformed::Tag { tag })),
        };
        reader.done()?;

        Ok(said)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn song(row: u64) -> PartySong {
        PartySong {
            row,
            recording: Recording {
                mbid: None,
                title: "Song".to_owned(),
                artist: "Artist".to_owned(),
                album: "Album".to_owned(),
                length: Some(Duration::from_secs(200)),
            },
            isrc: Some("USRC17607839".to_owned()),
        }
    }

    #[test]
    fn everything_said_in_a_party_reads_back_as_it_was_said() {
        let now = PartyNow {
            song: Some(song(1)),
            next: vec![song(2), song(3)],
            playing: true,
            position_nanos: 12,
            anchor_nanos: 34,
            coarse: true,
        };
        let said = [
            PartySaid::Hello {
                name: "Ada".to_owned(),
            },
            PartySaid::People(vec![(DeviceId([7; 16]), "Ada".to_owned())]),
            PartySaid::Now(now),
            PartySaid::Ask(song(4)),
        ];

        for one in said {
            assert_eq!(PartySaid::decoded(&one.encoded()).unwrap(), one);
        }
    }

    #[test]
    fn a_later_kind_of_saying_is_refused_rather_than_misread() {
        assert!(PartySaid::decoded(&[200]).is_err());
    }

    #[test]
    fn a_host_at_rest_stays_where_it_stopped_and_a_playing_host_moves_on() {
        let mut now = PartyNow {
            song: None,
            next: Vec::new(),
            playing: true,
            position_nanos: 100,
            anchor_nanos: 1_000,
            coarse: false,
        };

        assert_eq!(now.position_at(1_500), 600);
        assert_eq!(now.position_at(500), 100);

        now.playing = false;

        assert_eq!(now.position_at(9_000), 100);
    }
}
