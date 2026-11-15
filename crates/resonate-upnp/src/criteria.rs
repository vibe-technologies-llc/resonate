use std::{iter::Peekable, str::CharIndices};

use resonate_core::{folded_letters, words_of_a_name};

use crate::catalog::{Field, Kind, Query, Test};

pub(crate) const CRITERIA_BYTES_AT_MOST: usize = 4 << 10;

pub(crate) const DEPTH_AT_MOST: usize = 16;

pub(crate) const RELATIONS_AT_MOST: usize = 64;

const TOKENS_AT_MOST: usize = 512;

const EVERYTHING: &str = "*";

const DERIVED_APART_BY: char = '.';

pub(crate) const SEARCH_CAPABILITIES: &str =
    "dc:title,dc:creator,upnp:artist,upnp:album,upnp:genre,upnp:class";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Property {
    Title,
    Creator,
    Artist,
    Album,
    Genre,
    Class,
}

impl Property {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "dc:title" => Self::Title,
            "dc:creator" => Self::Creator,
            "upnp:artist" => Self::Artist,
            "upnp:album" => Self::Album,
            "upnp:genre" => Self::Genre,
            "upnp:class" => Self::Class,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Op {
    Is,
    IsNot,
    Contains,
    Lacks,
    DerivedFrom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Criterion {
    Every,
    All(Vec<Self>),
    Any(Vec<Self>),
    Compared {
        property: Property,
        op: Op,
        value: String,
    },
    Exists {
        property: Property,
        held: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    TooLong,
    TooDeep,
    TooMany,
    Unreadable,
    UnknownProperty,
    UnknownOperator,
}

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Open,
    Close,
    Quoted(String),
    Equal,
    Unequal,
    Ordered,
    Word(&'a str),
}

fn ends_a_word(letter: char) -> bool {
    letter.is_whitespace() || matches!(letter, '(' | ')' | '"' | '=' | '!' | '<' | '>')
}

fn quoted(letters: &mut Peekable<CharIndices<'_>>) -> Result<String, Refusal> {
    let mut value = String::new();
    loop {
        match letters.next().map(|(_, letter)| letter) {
            None => return Err(Refusal::Unreadable),
            Some('"') => return Ok(value),
            Some('\\') => match letters.next().map(|(_, letter)| letter) {
                Some(escaped @ ('"' | '\\')) => value.push(escaped),
                _ => return Err(Refusal::Unreadable),
            },
            Some(letter) => value.push(letter),
        }
    }
}

fn tokens(text: &str) -> Result<Vec<Token<'_>>, Refusal> {
    let mut found = Vec::new();
    let mut letters = text.char_indices().peekable();
    while let Some((at, letter)) = letters.next() {
        if found.len() >= TOKENS_AT_MOST {
            return Err(Refusal::TooMany);
        }
        let token = match letter {
            letter if letter.is_whitespace() => continue,
            '(' => Token::Open,
            ')' => Token::Close,
            '"' => Token::Quoted(quoted(&mut letters)?),
            '=' => Token::Equal,
            '!' => match letters.next() {
                Some((_, '=')) => Token::Unequal,
                _ => return Err(Refusal::Unreadable),
            },
            '<' | '>' => {
                letters.next_if(|(_, letter)| *letter == '=');
                Token::Ordered
            }
            _ => {
                let mut ends = at + letter.len_utf8();
                while let Some((next, letter)) =
                    letters.next_if(|(_, letter)| !ends_a_word(*letter))
                {
                    ends = next + letter.len_utf8();
                }
                Token::Word(text.get(at..ends).ok_or(Refusal::Unreadable)?)
            }
        };
        found.push(token);
    }
    Ok(found)
}

struct Reading<'a> {
    tokens: Vec<Token<'a>>,
    at: usize,
    relations: usize,
}

impl<'a> Reading<'a> {
    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.at)
    }

    fn take(&mut self) -> Option<&Token<'a>> {
        let token = self.tokens.get(self.at);
        self.at += 1;
        token
    }

    fn joined_by(&self, joiner: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(word)) if word.eq_ignore_ascii_case(joiner))
    }

    fn either(&mut self, depth: usize) -> Result<Criterion, Refusal> {
        let mut any = vec![self.every(depth)?];
        while self.joined_by("or") {
            self.at += 1;
            any.push(self.every(depth)?);
        }
        Ok(if any.len() == 1 {
            any.remove(0)
        } else {
            Criterion::Any(any)
        })
    }

    fn every(&mut self, depth: usize) -> Result<Criterion, Refusal> {
        let mut all = vec![self.one(depth)?];
        while self.joined_by("and") {
            self.at += 1;
            all.push(self.one(depth)?);
        }
        Ok(if all.len() == 1 {
            all.remove(0)
        } else {
            Criterion::All(all)
        })
    }

    fn one(&mut self, depth: usize) -> Result<Criterion, Refusal> {
        match self.take() {
            Some(Token::Open) => {
                if depth >= DEPTH_AT_MOST {
                    return Err(Refusal::TooDeep);
                }
                let inside = self.either(depth + 1)?;
                match self.take() {
                    Some(Token::Close) => Ok(inside),
                    _ => Err(Refusal::Unreadable),
                }
            }
            Some(Token::Word(name)) => {
                let name = *name;
                self.relations += 1;
                if self.relations > RELATIONS_AT_MOST {
                    return Err(Refusal::TooMany);
                }
                self.related(name)
            }
            _ => Err(Refusal::Unreadable),
        }
    }

    fn related(&mut self, name: &str) -> Result<Criterion, Refusal> {
        let op = match self.take() {
            Some(Token::Equal) => Op::Is,
            Some(Token::Unequal) => Op::IsNot,
            Some(Token::Ordered) => return Err(Refusal::UnknownOperator),
            Some(Token::Word(word)) if word.eq_ignore_ascii_case("contains") => Op::Contains,
            Some(Token::Word(word)) if word.eq_ignore_ascii_case("doesNotContain") => Op::Lacks,
            Some(Token::Word(word)) if word.eq_ignore_ascii_case("derivedfrom") => Op::DerivedFrom,
            Some(Token::Word(word)) if word.eq_ignore_ascii_case("exists") => {
                let held = match self.take() {
                    Some(Token::Word(word)) if word.eq_ignore_ascii_case("true") => true,
                    Some(Token::Word(word)) if word.eq_ignore_ascii_case("false") => false,
                    _ => return Err(Refusal::Unreadable),
                };
                let property = Property::named(name).ok_or(Refusal::UnknownProperty)?;
                return Ok(Criterion::Exists { property, held });
            }
            Some(Token::Word(_)) => return Err(Refusal::UnknownOperator),
            _ => return Err(Refusal::Unreadable),
        };
        let Some(Token::Quoted(value)) = self.take() else {
            return Err(Refusal::Unreadable);
        };
        let value = value.clone();
        let property = Property::named(name).ok_or(Refusal::UnknownProperty)?;
        if op == Op::DerivedFrom && property != Property::Class {
            return Err(Refusal::UnknownOperator);
        }
        Ok(Criterion::Compared {
            property,
            op,
            value,
        })
    }
}

pub(crate) fn read(text: &str) -> Result<Criterion, Refusal> {
    if text.len() > CRITERIA_BYTES_AT_MOST {
        return Err(Refusal::TooLong);
    }
    if text.trim() == EVERYTHING {
        return Ok(Criterion::Every);
    }
    let mut reading = Reading {
        tokens: tokens(text)?,
        at: 0,
        relations: 0,
    };
    let read = reading.either(0)?;
    if reading.peek().is_some() {
        return Err(Refusal::Unreadable);
    }
    Ok(read)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Described<'a> {
    pub class: &'static str,
    pub kind: Option<Kind>,
    pub title: Option<&'a str>,
}

impl Described<'_> {
    pub(crate) const fn of(kind: Kind) -> Self {
        Self {
            class: kind.class(),
            kind: Some(kind),
            title: None,
        }
    }

    fn field_of(&self, property: Property) -> Option<Field> {
        match self.kind {
            Some(kind) => kind.field_of(property),
            None => (property == Property::Title).then_some(Field::Title),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reduced {
    Always,
    Never,
    Asks(Query),
}

impl Reduced {
    const fn held(holds: bool) -> Self {
        if holds { Self::Always } else { Self::Never }
    }
}

const fn holds_when_missing(test: Test) -> bool {
    matches!(test, Test::IsNot | Test::Lacks)
}

const fn test_of(op: Op) -> Option<Test> {
    match op {
        Op::Is => Some(Test::Is),
        Op::IsNot => Some(Test::IsNot),
        Op::Contains => Some(Test::Contains),
        Op::Lacks => Some(Test::Lacks),
        Op::DerivedFrom => None,
    }
}

fn class_holds(class: &str, op: Op, value: &str) -> bool {
    let class = class.to_ascii_lowercase();
    let value = value.trim().to_ascii_lowercase();
    match op {
        Op::Is => class == value,
        Op::IsNot => class != value,
        Op::Contains => class.contains(&value),
        Op::Lacks => !class.contains(&value),
        Op::DerivedFrom => {
            class == value
                || class
                    .strip_prefix(&value)
                    .is_some_and(|rest| rest.starts_with(DERIVED_APART_BY))
        }
    }
}

pub(crate) fn text_holds(held: &str, test: Test, asked: &str) -> bool {
    match test {
        Test::Is => words_of_a_name(held) == words_of_a_name(asked),
        Test::IsNot => words_of_a_name(held) != words_of_a_name(asked),
        Test::Contains => folded_letters(held).contains(&folded_letters(asked)),
        Test::Lacks => !folded_letters(held).contains(&folded_letters(asked)),
    }
}

pub(crate) fn reduced(criterion: &Criterion, described: &Described<'_>) -> Reduced {
    match criterion {
        Criterion::Every => Reduced::Always,
        Criterion::All(all) => {
            let mut asked = Vec::new();
            for one in all {
                match reduced(one, described) {
                    Reduced::Never => return Reduced::Never,
                    Reduced::Always => {}
                    Reduced::Asks(Query::All(inner)) => asked.extend(inner),
                    Reduced::Asks(query) => asked.push(query),
                }
            }
            match asked.len() {
                0 => Reduced::Always,
                1 => Reduced::Asks(asked.remove(0)),
                _ => Reduced::Asks(Query::All(asked)),
            }
        }
        Criterion::Any(any) => {
            let mut asked = Vec::new();
            for one in any {
                match reduced(one, described) {
                    Reduced::Always => return Reduced::Always,
                    Reduced::Never => {}
                    Reduced::Asks(Query::Any(inner)) => asked.extend(inner),
                    Reduced::Asks(query) => asked.push(query),
                }
            }
            match asked.len() {
                0 => Reduced::Never,
                1 => Reduced::Asks(asked.remove(0)),
                _ => Reduced::Asks(Query::Any(asked)),
            }
        }
        Criterion::Compared {
            property: Property::Class,
            op,
            value,
        } => Reduced::held(class_holds(described.class, *op, value)),
        Criterion::Exists {
            property: Property::Class | Property::Title,
            held,
        } => Reduced::held(*held),
        Criterion::Compared {
            property,
            op,
            value,
        } => {
            let Some(test) = test_of(*op) else {
                return Reduced::Never;
            };
            match (described.field_of(*property), described.title) {
                (None, _) => Reduced::held(holds_when_missing(test)),
                (Some(Field::Title), Some(title)) => Reduced::held(text_holds(title, test, value)),
                (Some(field), _) => Reduced::Asks(Query::Text {
                    field,
                    test,
                    text: value.clone(),
                }),
            }
        }
        Criterion::Exists { property, held } => match described.field_of(*property) {
            None => Reduced::held(!held),
            Some(field) => Reduced::Asks(Query::Present { field, held: *held }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compared(property: Property, op: Op, value: &str) -> Criterion {
        Criterion::Compared {
            property,
            op,
            value: value.to_owned(),
        }
    }

    #[test]
    fn a_criterion_is_read_with_and_binding_tighter_than_or_and_brackets_over_both() {
        let read = read(
            "upnp:class derivedfrom \"object.item.audioItem\" and (dc:title contains \"Say \\\"hi\\\"\" \
             or upnp:artist = \"A\\\\B\") or upnp:genre exists false",
        );

        assert_eq!(
            read,
            Ok(Criterion::Any(vec![
                Criterion::All(vec![
                    compared(Property::Class, Op::DerivedFrom, "object.item.audioItem"),
                    Criterion::Any(vec![
                        compared(Property::Title, Op::Contains, "Say \"hi\""),
                        compared(Property::Artist, Op::Is, "A\\B"),
                    ]),
                ]),
                Criterion::Exists {
                    property: Property::Genre,
                    held: false
                },
            ]))
        );
        assert_eq!(read_ok("*"), Criterion::Every);
        assert_eq!(
            read_ok("dc:title!=\"x\" AND upnp:album doesNotContain \"y\""),
            Criterion::All(vec![
                compared(Property::Title, Op::IsNot, "x"),
                compared(Property::Album, Op::Lacks, "y"),
            ])
        );
    }

    fn read_ok(text: &str) -> Criterion {
        read(text).expect("a criterion")
    }

    #[test]
    fn what_cannot_be_read_or_answered_is_refused_and_nothing_long_or_deep_is_read() {
        for (text, refused) in [
            ("", Refusal::Unreadable),
            ("dc:title", Refusal::Unreadable),
            ("dc:title contains", Refusal::Unreadable),
            ("dc:title contains \"open", Refusal::Unreadable),
            ("dc:title contains \"\\n\"", Refusal::Unreadable),
            ("dc:title contains \"a\" and", Refusal::Unreadable),
            ("(dc:title contains \"a\"", Refusal::Unreadable),
            ("dc:title contains \"a\")", Refusal::Unreadable),
            ("dc:title ! \"a\"", Refusal::Unreadable),
            ("dc:title exists maybe", Refusal::Unreadable),
            ("res@size > \"1\"", Refusal::UnknownOperator),
            ("dc:title <= \"a\"", Refusal::UnknownOperator),
            ("dc:title derivedfrom \"a\"", Refusal::UnknownOperator),
            ("dc:title like \"a\"", Refusal::UnknownOperator),
            ("@id = \"song-1\"", Refusal::UnknownProperty),
            ("upnp:artist@role = \"Composer\"", Refusal::UnknownProperty),
        ] {
            assert_eq!(read(text), Err(refused), "{text:?}");
        }

        let deep = format!(
            "{}dc:title = \"a\"{}",
            "(".repeat(DEPTH_AT_MOST + 1),
            ")".repeat(DEPTH_AT_MOST + 1)
        );
        let held = format!(
            "{}dc:title = \"a\"{}",
            "(".repeat(DEPTH_AT_MOST),
            ")".repeat(DEPTH_AT_MOST)
        );
        let many = vec!["dc:title = \"a\""; RELATIONS_AT_MOST + 1].join(" or ");
        let long = format!("dc:title = \"{}\"", "a".repeat(CRITERIA_BYTES_AT_MOST));

        assert_eq!(read(&deep), Err(Refusal::TooDeep));
        assert!(read(&held).is_ok());
        assert_eq!(read(&many), Err(Refusal::TooMany));
        assert_eq!(read(&long), Err(Refusal::TooLong));
        assert_eq!(read(&"(".repeat(200)), Err(Refusal::TooDeep));
        assert_eq!(read(&"(".repeat(3000)), Err(Refusal::TooMany));
    }

    #[test]
    fn a_criterion_is_reduced_for_each_kind_to_what_the_catalog_is_asked() {
        let songs_by = read_ok(
            "upnp:class derivedfrom \"object.item.audioItem\" and upnp:artist contains \"band\"",
        );
        let titled = read_ok("dc:title contains \"songs\"");
        let albums = read_ok("upnp:class = \"object.container.album.musicAlbum\"");
        let without_genre = read_ok("upnp:genre exists false and dc:title exists true");
        let folder = Described {
            class: crate::didl::FOLDER,
            kind: None,
            title: Some("Songs"),
        };

        assert_eq!(
            reduced(&songs_by, &Described::of(Kind::Song)),
            Reduced::Asks(Query::Text {
                field: Field::Artist,
                test: Test::Contains,
                text: "band".to_owned()
            })
        );
        assert_eq!(
            reduced(&songs_by, &Described::of(Kind::Album)),
            Reduced::Never
        );
        assert_eq!(reduced(&titled, &folder), Reduced::Always);
        assert_eq!(
            reduced(&read_ok("dc:title = \"albums\""), &folder),
            Reduced::Never
        );
        assert_eq!(
            reduced(&albums, &Described::of(Kind::Album)),
            Reduced::Always
        );
        assert_eq!(reduced(&albums, &Described::of(Kind::Song)), Reduced::Never);
        assert_eq!(
            reduced(&without_genre, &Described::of(Kind::Artist)),
            Reduced::Always
        );
        assert_eq!(
            reduced(&without_genre, &Described::of(Kind::Song)),
            Reduced::Asks(Query::Present {
                field: Field::Genre,
                held: false
            })
        );
        assert_eq!(
            reduced(
                &read_ok("upnp:album != \"x\""),
                &Described::of(Kind::Artist)
            ),
            Reduced::Always
        );
        assert_eq!(
            reduced(
                &read_ok("upnp:class derivedfrom \"object.item.audio\""),
                &Described::of(Kind::Song)
            ),
            Reduced::Never
        );
    }
}
