use std::{
    mem,
    path::{Path, PathBuf},
};

use resonate_core::MediaLocation;

use crate::{
    PlaylistEntry, Result,
    sheet::{self, Sheet},
    store,
};

const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";

const NAMESPACE: &str = "http://xspf.org/ns/0/";

const XSPF_VERSION: u32 = 1;

const PLAYLIST: &str = "playlist";

const TRACK_LIST: &str = "trackList";

const TRACK: &str = "track";

const LOCATION: &str = "location";

const TITLE: &str = "title";

const CREATOR: &str = "creator";

const DURATION: &str = "duration";

const MILLISECONDS: u64 = 1_000;

const UNDER_THE_PLAYLIST: usize = 1;

const UNDER_A_TRACK: usize = 3;

const COMMENT_OPENS: &str = "!--";

const COMMENT_CLOSES: &str = "-->";

const CDATA_OPENS: &str = "![CDATA[";

const CDATA_CLOSES: &str = "]]>";

const INSTRUCTION: char = '?';

const MARKUP_DECLARATION: char = '!';

const XML_BASE: &str = "xml:base";

const PATH_SEPARATOR: char = '/';

const THE_SHEETS_OWN_FOLDER: usize = 1;

const QUOTES: [char; 2] = ['"', '\''];

const WHITESPACE: [char; 4] = [' ', '\t', '\r', '\n'];

const TAG_BREAKS: [char; 5] = [' ', '\t', '\r', '\n', '/'];

const ENTITIES: [(&str, char); 5] = [
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
];

const HEXADECIMAL: [char; 2] = ['x', 'X'];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Depth {
    Playlist,
    TrackList,
    Track,
}

enum Token<'a> {
    Opens(&'a str),
    Closes(&'a str),
    Empty,
    Text(&'a str),
}

#[derive(Clone, Debug)]
enum Base {
    Beside(PathBuf),
    Elsewhere,
}

impl Base {
    fn under(&self, declared: &str) -> Self {
        let Self::Beside(beside) = self else {
            return Self::Elsewhere;
        };
        let declared = sheet::forward_escaped(declared.trim());
        let folder = folder_of(&declared);

        match sheet::scheme_of(folder) {
            Some(_) => match sheet::local_file(folder) {
                Some(named) => Self::Beside(PathBuf::from(named)),
                None => Self::Elsewhere,
            },
            None => match sheet::unescaped(folder) {
                Some(relative) => Self::Beside(beside.join(relative)),
                None => Self::Elsewhere,
            },
        }
    }

    fn beside(&self) -> Option<&Path> {
        match self {
            Self::Beside(beside) => Some(beside),
            Self::Elsewhere => None,
        }
    }
}

#[derive(Debug, Default)]
struct Offered {
    chosen: Option<MediaLocation>,
    alternates: usize,
}

impl Offered {
    fn offer(&mut self, resolved: Option<MediaLocation>) {
        self.alternates += 1;
        if self.chosen.is_none() {
            self.chosen = resolved;
        }
    }

    fn count(self, sheet: &mut Sheet) {
        match self.chosen {
            Some(location) => sheet.locations.push(location),
            None if self.alternates > 0 => sheet.elsewhere += 1,
            None => {}
        }
    }
}

pub fn read(text: &str, beside: &Path) -> Sheet {
    let mut sheet = Sheet::default();
    let mut depth = Depth::Playlist;
    let mut bases = vec![Base::Beside(beside.to_path_buf())];
    let mut offered = Offered::default();
    let mut written = String::new();

    for token in Tokens::over(text) {
        match token {
            Token::Opens(tag) => {
                written.clear();
                let inner = inherited(&bases, tag);
                bases.push(inner);

                let element = element_of(tag);
                if element.eq_ignore_ascii_case(TRACK_LIST) {
                    depth = Depth::TrackList;
                } else if element.eq_ignore_ascii_case(TRACK) && depth == Depth::TrackList {
                    depth = Depth::Track;
                    offered = Offered::default();
                }
            }
            Token::Text(run) => written.push_str(run),
            Token::Empty => written.clear(),
            Token::Closes(tag) => {
                let base = closed(&mut bases);
                let element = element_of(tag);

                if element.eq_ignore_ascii_case(TRACK_LIST) {
                    depth = Depth::Playlist;
                } else if element.eq_ignore_ascii_case(TRACK) && depth == Depth::Track {
                    depth = Depth::TrackList;
                    mem::take(&mut offered).count(&mut sheet);
                } else if element.eq_ignore_ascii_case(LOCATION) && depth == Depth::Track {
                    offered.offer(referenced(&plain_text(&written), &base));
                } else if element.eq_ignore_ascii_case(TITLE) && depth == Depth::Playlist {
                    let named = plain_text(&written).trim().to_owned();
                    if !named.is_empty() {
                        sheet.declared = Some(named);
                    }
                }
                written.clear();
            }
        }
    }

    sheet
}

pub fn write(name: &str, entries: &[PlaylistEntry], beside: &Path) -> Result<String> {
    let mut text =
        format!("{DECLARATION}\n<{PLAYLIST} version=\"{XSPF_VERSION}\" xmlns=\"{NAMESPACE}\">\n");
    text.push_str(&element(
        UNDER_THE_PLAYLIST,
        TITLE,
        &marked_up(&sheet::one_line(name)),
    ));
    text.push_str(&format!("  <{TRACK_LIST}>\n"));

    for entry in entries {
        let Some(file) = entry.location().as_path() else {
            continue;
        };
        let named = sheet::beside_or_absolute(file, beside);
        let described = sheet::describe(entry);

        text.push_str(&format!("    <{TRACK}>\n"));
        text.push_str(&element(UNDER_A_TRACK, LOCATION, &uri(&named)?));
        text.push_str(&element(UNDER_A_TRACK, TITLE, &marked_up(&described.title)));
        if let Some(artist) = described.artist.as_deref() {
            text.push_str(&element(UNDER_A_TRACK, CREATOR, &marked_up(artist)));
        }
        if let Some(seconds) = described.seconds {
            text.push_str(&element(
                UNDER_A_TRACK,
                DURATION,
                &(seconds * MILLISECONDS).to_string(),
            ));
        }
        text.push_str(&format!("    </{TRACK}>\n"));
    }

    text.push_str(&format!("  </{TRACK_LIST}>\n</{PLAYLIST}>\n"));
    Ok(text)
}

struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    const fn over(text: &'a str) -> Self {
        Self { rest: text }
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(inside) = self.rest.strip_prefix('<') else {
            if self.rest.is_empty() {
                return None;
            }
            let ends = self.rest.find('<').unwrap_or(self.rest.len());
            let (run, rest) = self.rest.split_at(ends);
            self.rest = rest;
            return Some(Token::Text(run));
        };

        if let Some(comment) = inside.strip_prefix(COMMENT_OPENS) {
            self.rest = past(comment, COMMENT_CLOSES);
            return Some(Token::Empty);
        }
        if let Some(character_data) = inside.strip_prefix(CDATA_OPENS) {
            let ends = character_data.find(CDATA_CLOSES)?;
            self.rest = &character_data[ends + CDATA_CLOSES.len()..];
            return Some(Token::Text(&character_data[..ends]));
        }

        let ends = tag_ends(inside)?;
        let tag = &inside[..ends];
        self.rest = &inside[ends + 1..];

        if tag.starts_with([INSTRUCTION, MARKUP_DECLARATION]) {
            return Some(Token::Empty);
        }
        match tag.strip_prefix('/') {
            Some(closing) => Some(Token::Closes(closing)),
            None if tag.ends_with('/') => Some(Token::Empty),
            None => Some(Token::Opens(tag)),
        }
    }
}

struct Attributes<'a> {
    rest: &'a str,
}

impl<'a> Attributes<'a> {
    const fn of(tag: &'a str) -> Self {
        Self { rest: tag }
    }
}

impl<'a> Iterator for Attributes<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        let (before, rest) = self.rest.split_once('=')?;
        let name = before.trim_end().rsplit(WHITESPACE).next()?;
        let rest = rest.trim_start();
        let quote = rest.chars().next().filter(|first| QUOTES.contains(first))?;
        let (value, rest) = rest[quote.len_utf8()..].split_once(quote)?;

        self.rest = rest;
        Some((name, value))
    }
}

fn tag_ends(inside: &str) -> Option<usize> {
    let mut quoted: Option<char> = None;

    for (at, character) in inside.char_indices() {
        match (quoted, character) {
            (None, '>') => return Some(at),
            (None, _) if QUOTES.contains(&character) => quoted = Some(character),
            (Some(opened), _) if opened == character => quoted = None,
            _ => {}
        }
    }
    None
}

fn attribute<'a>(tag: &'a str, named: &str) -> Option<&'a str> {
    Attributes::of(tag)
        .find(|(name, _)| name.eq_ignore_ascii_case(named))
        .map(|(_, value)| value)
}

fn inherited(bases: &[Base], tag: &str) -> Base {
    let outer = current(bases);

    match attribute(tag, XML_BASE) {
        Some(declared) => outer.under(&plain_text(declared)),
        None => outer,
    }
}

fn closed(bases: &mut Vec<Base>) -> Base {
    let base = current(bases);
    if bases.len() > THE_SHEETS_OWN_FOLDER {
        bases.pop();
    }
    base
}

fn current(bases: &[Base]) -> Base {
    bases.last().cloned().unwrap_or(Base::Elsewhere)
}

fn folder_of(reference: &str) -> &str {
    match reference.rfind(PATH_SEPARATOR) {
        Some(at) => &reference[..=at],
        None => "",
    }
}

fn past<'a>(text: &'a str, closing: &str) -> &'a str {
    match text.find(closing) {
        Some(ends) => &text[ends + closing.len()..],
        None => "",
    }
}

fn referenced(reference: &str, base: &Base) -> Option<MediaLocation> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    if sheet::scheme_of(reference).is_some() {
        return sheet::from_uri(reference);
    }

    Some(sheet::resolved(
        PathBuf::from(sheet::unescaped(&sheet::forward_escaped(reference))?),
        base.beside()?,
    ))
}

fn uri(named: &Path) -> Result<String> {
    let escaped = sheet::escaped(store::path_text(named)?);
    if named.is_absolute() {
        return Ok(format!("file://{escaped}"));
    }
    Ok(escaped)
}

fn element(indent: usize, name: &str, content: &str) -> String {
    format!(
        "{:indent$}<{name}>{content}</{name}>\n",
        "",
        indent = indent * 2
    )
}

fn element_of(tag: &str) -> &str {
    let name = tag.split(TAG_BREAKS).next().unwrap_or(tag);

    match name.split_once(':') {
        Some((_, local)) => local,
        None => name,
    }
}

fn marked_up(text: &str) -> String {
    let mut written = String::with_capacity(text.len());

    for character in text.chars() {
        match ENTITIES.iter().find(|(_, marked)| *marked == character) {
            Some((named, _)) => written.push_str(&format!("&{named};")),
            None => written.push(character),
        }
    }
    written
}

fn plain_text(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }

    let mut written = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.find('&') {
        written.push_str(&rest[..at]);
        rest = &rest[at..];

        let Some(ends) = rest.find(';') else {
            written.push('&');
            rest = &rest[1..];
            continue;
        };
        match entity(&rest[1..ends]) {
            Some(character) => {
                written.push(character);
                rest = &rest[ends + 1..];
            }
            None => {
                written.push('&');
                rest = &rest[1..];
            }
        }
    }

    written.push_str(rest);
    written
}

fn entity(named: &str) -> Option<char> {
    if let Some(numbered) = named.strip_prefix('#') {
        let code = match numbered.strip_prefix(HEXADECIMAL) {
            Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16).ok()?,
            None => numbered.parse().ok()?,
        };
        return char::from_u32(code);
    }

    ENTITIES
        .iter()
        .find(|(name, _)| *name == named)
        .map(|(_, character)| *character)
}
