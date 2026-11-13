use resonate_core::Recording;

use crate::{
    DeviceId, Result, Token,
    error::{Error, Malformed},
    key::TOKEN_BYTES,
    wire::{LONGEST_TEXT, Reader, Writer},
};

const SAID_HELLO: u8 = 0;

const SAID_PEOPLE: u8 = 1;

const SAID_NOW: u8 = 2;

const SAID_ASK: u8 = 3;

const SAID_CHAT: u8 = 4;

const SAID_CHATTED: u8 = 5;

const SAID_HAND_OVER: u8 = 6;

const CHAT_WORDS: u8 = 0;

const CHAT_REACTION: u8 = 1;

pub const SONGS_AHEAD_TOLD_AT_MOST: usize = 8;

const PEOPLE_TOLD_AT_MOST: usize = 64;

pub const LONGEST_NAME: usize = 64;

pub const LONGEST_LINE: usize = 280;

pub const LINES_TOLD_AT_MOST: usize = 64;

pub const LARGEST_SAID_OPENED: usize = 512 * 1024;

const _: () = assert!(PEOPLE_TOLD_AT_MOST * (LONGEST_TEXT + 32) <= LARGEST_SAID_OPENED);

const _: () =
    assert!(LINES_TOLD_AT_MOST * (LONGEST_LINE + LONGEST_NAME + 48) <= LARGEST_SAID_OPENED);

fn cut_to_a_name(mut text: String) -> String {
    text.truncate(text.floor_char_boundary(LONGEST_NAME));
    text
}

fn cut_to_a_line(text: &str) -> String {
    let mut line: String = text
        .chars()
        .map(|letter| if letter.is_control() { ' ' } else { letter })
        .collect();
    line.truncate(line.floor_char_boundary(LONGEST_LINE));
    line
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartyReaction {
    Love,
    Fire,
    Laugh,
    Clap,
    Dance,
    Wow,
}

impl PartyReaction {
    pub const ALL: [Self; 6] = [
        Self::Love,
        Self::Fire,
        Self::Laugh,
        Self::Clap,
        Self::Dance,
        Self::Wow,
    ];

    pub const fn code(self) -> u8 {
        match self {
            Self::Love => 0,
            Self::Fire => 1,
            Self::Laugh => 2,
            Self::Clap => 3,
            Self::Dance => 4,
            Self::Wow => 5,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Love),
            1 => Some(Self::Fire),
            2 => Some(Self::Laugh),
            3 => Some(Self::Clap),
            4 => Some(Self::Dance),
            5 => Some(Self::Wow),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartyChat {
    Words(String),
    Reaction(PartyReaction),
}

impl PartyChat {
    pub fn words(text: &str) -> Option<Self> {
        let line = cut_to_a_line(text.trim());
        let line = line.trim();

        (!line.is_empty()).then(|| Self::Words(line.to_owned()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyLine {
    pub id: u64,
    pub from: DeviceId,
    pub name: String,
    pub chat: PartyChat,
}

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
    Chat(PartyChat),
    Chatted(Vec<PartyLine>),
    HandOver(Token),
}

fn written_chat(writer: &mut Writer, chat: &PartyChat) {
    match chat {
        PartyChat::Words(words) => {
            writer.byte(CHAT_WORDS);
            writer.text(&cut_to_a_line(words));
        }
        PartyChat::Reaction(reaction) => {
            writer.byte(CHAT_REACTION);
            writer.byte(reaction.code());
        }
    }
}

enum ChatRead {
    Known(PartyChat),
    LaterReaction(u8),
}

fn read_chat(reader: &mut Reader<'_>) -> Result<ChatRead> {
    match reader.byte()? {
        CHAT_WORDS => Ok(ChatRead::Known(PartyChat::Words(cut_to_a_line(
            &reader.text()?,
        )))),
        CHAT_REACTION => {
            let code = reader.byte()?;
            Ok(
                PartyReaction::from_code(code).map_or(ChatRead::LaterReaction(code), |reaction| {
                    ChatRead::Known(PartyChat::Reaction(reaction))
                }),
            )
        }
        tag => Err(Error::Malformed(Malformed::Tag { tag })),
    }
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
            Self::Chat(chat) => {
                writer.byte(SAID_CHAT);
                written_chat(&mut writer, chat);
            }
            Self::Chatted(lines) => {
                writer.byte(SAID_CHATTED);
                let lines = &lines[..lines.len().min(LINES_TOLD_AT_MOST)];
                writer.length(lines.len());
                for line in lines {
                    writer.number(line.id);
                    writer.device(line.from);
                    writer.text(&cut_to_a_name(line.name.clone()));
                    written_chat(&mut writer, &line.chat);
                }
            }
            Self::HandOver(token) => {
                writer.byte(SAID_HAND_OVER);
                writer.fixed(&token.0);
            }
        }

        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let said = match reader.byte()? {
            SAID_HELLO => Self::Hello {
                name: cut_to_a_name(reader.text()?),
            },
            SAID_PEOPLE => {
                let count = reader.length(PEOPLE_TOLD_AT_MOST)?;
                let mut people = crate::wire::reserved(count);
                for _ in 0..count {
                    let member = reader.device()?;
                    people.push((member, cut_to_a_name(reader.text()?)));
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
                let mut next = crate::wire::reserved(count);
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
            SAID_CHAT => match read_chat(&mut reader)? {
                ChatRead::Known(chat) => Self::Chat(chat),
                ChatRead::LaterReaction(code) => {
                    return Err(Error::Malformed(Malformed::Reaction { code }));
                }
            },
            SAID_CHATTED => {
                let count = reader.length(LINES_TOLD_AT_MOST)?;
                let mut lines = crate::wire::reserved(count);
                for _ in 0..count {
                    let id = reader.number()?;
                    let from = reader.device()?;
                    let name = cut_to_a_name(reader.text()?);
                    if let ChatRead::Known(chat) = read_chat(&mut reader)? {
                        lines.push(PartyLine {
                            id,
                            from,
                            name,
                            chat,
                        });
                    }
                }
                Self::Chatted(lines)
            }
            SAID_HAND_OVER => Self::HandOver(Token(reader.fixed::<TOKEN_BYTES>()?)),
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
            PartySaid::Chat(PartyChat::Words("Lovely".to_owned())),
            PartySaid::Chat(PartyChat::Reaction(PartyReaction::Fire)),
            PartySaid::HandOver(Token([9; TOKEN_BYTES])),
            PartySaid::Chatted(vec![
                PartyLine {
                    id: u64::MAX,
                    from: DeviceId([3; 16]),
                    name: "Ada".to_owned(),
                    chat: PartyChat::Words("Turn it up".to_owned()),
                },
                PartyLine {
                    id: 1,
                    from: DeviceId([4; 16]),
                    name: String::new(),
                    chat: PartyChat::Reaction(PartyReaction::Love),
                },
            ]),
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
    fn every_reaction_reads_back_from_its_code_and_a_code_past_them_reads_as_none() {
        for reaction in PartyReaction::ALL {
            assert_eq!(PartyReaction::from_code(reaction.code()), Some(reaction));
        }

        assert_eq!(PartyReaction::from_code(6), None);
    }

    #[test]
    fn a_reaction_of_a_later_build_is_refused_alone_and_passed_over_among_lines() {
        let lone = [SAID_CHAT, CHAT_REACTION, 200];
        let mut among = vec![SAID_CHATTED, 2];
        for (id, code) in [(1, 200), (2, PartyReaction::Clap.code())] {
            among.push(id);
            among.extend([7; 16]);
            among.extend([0, CHAT_REACTION, code]);
        }

        assert!(matches!(
            PartySaid::decoded(&lone),
            Err(Error::Malformed(Malformed::Reaction { code: 200 }))
        ));
        assert_eq!(
            PartySaid::decoded(&among).unwrap(),
            PartySaid::Chatted(vec![PartyLine {
                id: 2,
                from: DeviceId([7; 16]),
                name: String::new(),
                chat: PartyChat::Reaction(PartyReaction::Clap),
            }])
        );
    }

    #[test]
    fn a_line_is_cut_on_a_letter_its_controls_read_as_spaces_and_a_blank_one_is_not_said() {
        let long = format!("a\nb{}", "é".repeat(LONGEST_LINE));
        let said = PartySaid::Chat(PartyChat::Words(long)).encoded();

        let PartySaid::Chat(PartyChat::Words(line)) = PartySaid::decoded(&said).unwrap() else {
            panic!("words");
        };

        assert!(line.len() <= LONGEST_LINE);
        assert!(line.starts_with("a b"));
        assert_eq!(PartyChat::words("  \t "), None);
        assert_eq!(
            PartyChat::words("  hi there "),
            Some(PartyChat::Words("hi there".to_owned()))
        );
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

    #[test]
    fn a_name_is_cut_short_where_it_is_read_on_a_letter() {
        let long = "é".repeat(LONGEST_NAME);
        let hello = PartySaid::Hello { name: long.clone() }.encoded();
        let people = PartySaid::People(vec![(DeviceId([1; 16]), long)]).encoded();

        let PartySaid::Hello { name } = PartySaid::decoded(&hello).unwrap() else {
            panic!("a hello");
        };
        let PartySaid::People(people) = PartySaid::decoded(&people).unwrap() else {
            panic!("people");
        };

        assert_eq!(name.len(), LONGEST_NAME);
        assert_eq!(people[0].1.len(), LONGEST_NAME);
        assert!(name.chars().all(|letter| letter == 'é'));
    }
}
