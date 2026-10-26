use std::{fmt, ops::Range};

use resonate_core::{
    MediaLocation, TextEncoding,
    text::{UTF8_BOM, decoded_as, detected, encoded},
};

use crate::{Error, Result, TagField};

const REMARK: &str = "REM";
const TRACK: &str = "TRACK";
const INDEX: &str = "INDEX";
const FILE: &str = "FILE";
const FIRST_INDEX: u32 = 1;
const LINE_ENDING: &str = "\n";
const INDENTED_PAST_ITS_TRACK_BY: &str = "  ";
const QUOTE: char = '"';

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SheetScope {
    Disc,
    Track(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SheetField {
    Title,
    Performer,
    Songwriter,
    Isrc,
    Catalog,
    Date,
    Genre,
    Comment,
    DiscNumber,
    DiscTotal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placing {
    Track,
    Disc,
    Either,
}

impl SheetField {
    pub const ALL: [Self; 10] = [
        Self::Title,
        Self::Performer,
        Self::Songwriter,
        Self::Isrc,
        Self::Catalog,
        Self::Date,
        Self::Genre,
        Self::Comment,
        Self::DiscNumber,
        Self::DiscTotal,
    ];

    pub const fn of(field: TagField) -> Option<(Self, Placing)> {
        Some(match field {
            TagField::Title => (Self::Title, Placing::Track),
            TagField::Artist => (Self::Performer, Placing::Track),
            TagField::Composer => (Self::Songwriter, Placing::Track),
            TagField::Isrc => (Self::Isrc, Placing::Track),
            TagField::Comment => (Self::Comment, Placing::Track),
            TagField::Album => (Self::Title, Placing::Disc),
            TagField::AlbumArtist => (Self::Performer, Placing::Disc),
            TagField::Barcode => (Self::Catalog, Placing::Disc),
            TagField::Date => (Self::Date, Placing::Either),
            TagField::Genre => (Self::Genre, Placing::Either),
            TagField::DiscNumber => (Self::DiscNumber, Placing::Either),
            TagField::DiscTotal => (Self::DiscTotal, Placing::Either),
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "TITLE",
            Self::Performer => "PERFORMER",
            Self::Songwriter => "SONGWRITER",
            Self::Isrc => "ISRC",
            Self::Catalog => "CATALOG",
            Self::Date => "DATE",
            Self::Genre => "GENRE",
            Self::Comment => "COMMENT",
            Self::DiscNumber => "DISCNUMBER",
            Self::DiscTotal => "TOTALDISCS",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.as_str() == name)
    }

    const fn is_a_remark(self) -> bool {
        matches!(
            self,
            Self::Date | Self::Genre | Self::Comment | Self::DiscNumber | Self::DiscTotal
        )
    }

    const fn is_quoted(self) -> bool {
        matches!(
            self,
            Self::Title | Self::Performer | Self::Songwriter | Self::Genre | Self::Comment
        )
    }

    fn said_by(command: &str) -> Option<Self> {
        [
            Self::Title,
            Self::Performer,
            Self::Songwriter,
            Self::Isrc,
            Self::Catalog,
        ]
        .into_iter()
        .find(|field| command.eq_ignore_ascii_case(field.as_str()))
    }

    fn remarked_by(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("DISCTOTAL") {
            return Some(Self::DiscTotal);
        }
        Self::ALL
            .into_iter()
            .filter(|field| field.is_a_remark())
            .find(|field| name.eq_ignore_ascii_case(field.as_str()))
    }

    fn spelled(self, value: &str, quoted_before: bool) -> String {
        let quoted = quoted_before
            || self.is_quoted()
            || value.is_empty()
            || value.contains(char::is_whitespace)
            || value.starts_with(QUOTE);
        match quoted {
            true => format!("{QUOTE}{value}{QUOTE}"),
            false => value.to_owned(),
        }
    }

    fn line(self, value: &str) -> String {
        let spelled = self.spelled(value, false);
        match self.is_a_remark() {
            true => format!("{REMARK} {} {spelled}", self.as_str()),
            false => format!("{} {spelled}", self.as_str()),
        }
    }
}

impl fmt::Display for SheetField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetEdit {
    pub scope: SheetScope,
    pub field: SheetField,
    pub value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Said {
    Field {
        field: SheetField,
        value_at: usize,
        value: String,
    },
    Track(Option<u32>),
    Index,
    File,
    Other,
}

#[derive(Clone, Debug)]
struct Line {
    content: Range<usize>,
    ending: Range<usize>,
    text: String,
    scope: SheetScope,
    said: Said,
}

pub struct SheetText<'a> {
    bytes: &'a [u8],
    encoding: TextEncoding,
    body: usize,
    lines: Vec<Line>,
}

impl<'a> SheetText<'a> {
    pub fn read(bytes: &'a [u8]) -> Self {
        let encoding = detected(bytes);
        let body = body_of(bytes, encoding);
        let mut lines = Vec::new();
        let mut scope = SheetScope::Disc;
        let mut indexed = false;

        for (content, ending) in lines_of(bytes, body, encoding) {
            let text = decoded_as(&bytes[content.clone()], encoding);
            let said = said(&text);
            match &said {
                Said::Track(number) => {
                    scope = number.map_or(SheetScope::Disc, SheetScope::Track);
                    indexed = false;
                }
                Said::Index if first_index(&text) => indexed = true,
                Said::File if crosses_into_the_file(scope, indexed, &text) => {}
                Said::File => scope = SheetScope::Disc,
                Said::Index | Said::Field { .. } | Said::Other => {}
            }
            lines.push(Line {
                content,
                ending,
                text,
                scope,
                said,
            });
        }

        Self {
            bytes,
            encoding,
            body,
            lines,
        }
    }

    pub const fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    pub fn own(&self, scope: SheetScope, field: SheetField) -> Option<String> {
        self.lines
            .iter()
            .filter(|line| line.scope == scope)
            .filter_map(|line| match &line.said {
                Said::Field {
                    field: said, value, ..
                } if *said == field && !value.is_empty() => Some(value.clone()),
                _ => None,
            })
            .next_back()
    }

    pub fn names_the_track_once(&self, number: u32) -> bool {
        self.track_line(number).is_some()
    }

    pub fn edited(&self, location: &MediaLocation, edits: &[SheetEdit]) -> Result<Vec<u8>> {
        let count = self.lines.len();
        let mut replaced: Vec<Option<Vec<u8>>> = vec![None; count];
        let mut removed = vec![false; count];
        let mut inserted: Vec<Vec<Vec<u8>>> = vec![Vec::new(); count + 1];

        for edit in edits {
            if let Some(value) = &edit.value
                && value.contains(['\n', '\r'])
            {
                return Err(Error::SheetValueUnwritable {
                    location: location.clone(),
                    field: edit.field,
                });
            }
            if let SheetScope::Track(number) = edit.scope
                && self.track_line(number).is_none()
            {
                return Err(Error::SheetTrackNotNamedOnce {
                    location: location.clone(),
                    track: number,
                });
            }

            let owned: Vec<usize> = self.owned(edit.scope, edit.field).collect();
            match (&edit.value, owned.last()) {
                (Some(value), Some(&last)) => {
                    replaced[last] = Some(self.rewritten(location, last, value, edit.field)?);
                    removed[last] = false;
                }
                (Some(value), None) => {
                    let line = format!("{}{}", self.indent_of(edit.scope), edit.field.line(value));
                    inserted[self.anchor_of(edit.scope)].push(self.spelled(location, &line)?);
                }
                (None, _) => {
                    for at in owned {
                        removed[at] = true;
                        replaced[at] = None;
                    }
                }
            }
        }

        let ending = self.ending(location)?;
        let mut written = Vec::with_capacity(self.bytes.len() + 64);
        written.extend_from_slice(&self.bytes[..self.body]);
        for (at, line) in self.lines.iter().enumerate() {
            for new in &inserted[at] {
                written.extend_from_slice(new);
                written.extend_from_slice(&ending);
            }
            if removed[at] {
                continue;
            }
            match &replaced[at] {
                Some(new) => written.extend_from_slice(new),
                None => written.extend_from_slice(&self.bytes[line.content.clone()]),
            }
            written.extend_from_slice(&self.bytes[line.ending.clone()]);
        }

        if !inserted[count].is_empty() {
            let unended = self
                .lines
                .iter()
                .enumerate()
                .rev()
                .find(|(at, _)| !removed[*at])
                .is_some_and(|(_, line)| line.ending.is_empty());
            if unended {
                written.extend_from_slice(&ending);
            }
            for new in &inserted[count] {
                written.extend_from_slice(new);
                written.extend_from_slice(&ending);
            }
        }
        Ok(written)
    }

    fn owned(&self, scope: SheetScope, field: SheetField) -> impl Iterator<Item = usize> + '_ {
        self.lines
            .iter()
            .enumerate()
            .filter(move |(_, line)| {
                line.scope == scope
                    && matches!(&line.said, Said::Field { field: said, .. } if *said == field)
            })
            .map(|(at, _)| at)
    }

    fn track_line(&self, number: u32) -> Option<usize> {
        let mut found = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.said == Said::Track(Some(number)))
            .map(|(at, _)| at);
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }

    fn anchor_of(&self, scope: SheetScope) -> usize {
        match scope {
            SheetScope::Disc => self
                .lines
                .iter()
                .position(|line| matches!(line.said, Said::Track(_) | Said::File))
                .unwrap_or(self.lines.len()),
            SheetScope::Track(number) => {
                let Some(track) = self.track_line(number) else {
                    return self.lines.len();
                };
                let mut at = track + 1;
                while let Some(line) = self.lines.get(at)
                    && line.scope == scope
                    && !matches!(line.said, Said::Index | Said::File | Said::Track(_))
                {
                    at += 1;
                }
                at
            }
        }
    }

    fn indent_of(&self, scope: SheetScope) -> String {
        let SheetScope::Track(number) = scope else {
            return String::new();
        };
        let Some(track) = self.track_line(number) else {
            return INDENTED_PAST_ITS_TRACK_BY.to_owned();
        };
        match self.lines.get(track + 1) {
            Some(next) if next.scope == scope && !matches!(next.said, Said::File) => {
                indent(&next.text).to_owned()
            }
            _ => format!(
                "{}{INDENTED_PAST_ITS_TRACK_BY}",
                indent(&self.lines[track].text)
            ),
        }
    }

    fn rewritten(
        &self,
        location: &MediaLocation,
        at: usize,
        value: &str,
        field: SheetField,
    ) -> Result<Vec<u8>> {
        let line = &self.lines[at];
        let Said::Field { value_at, .. } = &line.said else {
            return self.spelled(location, &field.line(value));
        };
        let (before, after) = line.text.split_at(*value_at);
        let spelled = field.spelled(value, after.trim_start().starts_with(QUOTE));
        if before.is_ascii() && self.keeps_ascii() {
            let kept = before.len() * self.encoding.unit_bytes();
            let mut rewritten = self.bytes[line.content.start..line.content.start + kept].to_vec();
            rewritten.extend_from_slice(&self.spelled(location, &spelled)?);
            return Ok(rewritten);
        }
        self.spelled(location, &format!("{before}{spelled}"))
    }

    fn keeps_ascii(&self) -> bool {
        match self.encoding {
            TextEncoding::Legacy(legacy) => legacy.is_ascii_compatible(),
            TextEncoding::Utf8 | TextEncoding::Utf16Le | TextEncoding::Utf16Be => true,
        }
    }

    fn spelled(&self, location: &MediaLocation, text: &str) -> Result<Vec<u8>> {
        encoded(text, self.encoding).ok_or_else(|| Error::SheetUnspellable {
            location: location.clone(),
            encoding: self.encoding,
        })
    }

    fn ending(&self, location: &MediaLocation) -> Result<Vec<u8>> {
        match self.lines.iter().find(|line| !line.ending.is_empty()) {
            Some(line) => Ok(self.bytes[line.ending.clone()].to_vec()),
            None => self.spelled(location, LINE_ENDING),
        }
    }
}

fn body_of(bytes: &[u8], encoding: TextEncoding) -> usize {
    let mark = encoding.byte_order_mark();
    if !mark.is_empty() && bytes.starts_with(mark) {
        return mark.len();
    }
    if bytes.starts_with(&UTF8_BOM) {
        return UTF8_BOM.len();
    }
    0
}

fn lines_of(
    bytes: &[u8],
    body: usize,
    encoding: TextEncoding,
) -> Vec<(Range<usize>, Range<usize>)> {
    let unit = encoding.unit_bytes();
    let mut lines = Vec::new();
    let mut start = body;
    let mut at = body;
    while at + unit <= bytes.len() {
        let Some(ends) = line_break(&bytes[at..at + unit], encoding) else {
            at += unit;
            continue;
        };
        let mut ending = at + unit;
        if ends == '\r'
            && let Some(next) = bytes.get(ending..ending + unit)
            && line_break(next, encoding) == Some('\n')
        {
            ending += unit;
        }
        lines.push((start..at, at..ending));
        start = ending;
        at = ending;
    }
    if start < bytes.len() {
        lines.push((start..bytes.len(), bytes.len()..bytes.len()));
    }
    lines
}

fn line_break(unit: &[u8], encoding: TextEncoding) -> Option<char> {
    let held = match (encoding, unit) {
        (TextEncoding::Utf8 | TextEncoding::Legacy(_), [byte]) => u16::from(*byte),
        (TextEncoding::Utf16Le, [low, high]) => u16::from_le_bytes([*low, *high]),
        (TextEncoding::Utf16Be, [high, low]) => u16::from_be_bytes([*high, *low]),
        _ => return None,
    };
    match held {
        0x0A => Some('\n'),
        0x0D => Some('\r'),
        _ => None,
    }
}

fn indent(text: &str) -> &str {
    let kept = text.len() - text.trim_start().len();
    &text[..kept]
}

fn said(text: &str) -> Said {
    let Some((command, rest, rest_at)) = command_of(text) else {
        return Said::Other;
    };

    if command.eq_ignore_ascii_case(TRACK) {
        return Said::Track(
            rest.split_whitespace()
                .next()
                .and_then(|number| number.parse().ok()),
        );
    }
    if command.eq_ignore_ascii_case(INDEX) {
        return Said::Index;
    }
    if command.eq_ignore_ascii_case(FILE) {
        return Said::File;
    }
    if command.eq_ignore_ascii_case(REMARK) {
        let Some((named, value, value_at)) = command_of(rest) else {
            return Said::Other;
        };
        return match SheetField::remarked_by(named) {
            Some(field) => Said::Field {
                field,
                value_at: rest_at + value_at,
                value: quoted(value),
            },
            None => Said::Other,
        };
    }
    match SheetField::said_by(command) {
        Some(field) => Said::Field {
            field,
            value_at: rest_at,
            value: quoted(rest),
        },
        None => Said::Other,
    }
}

fn command_of(text: &str) -> Option<(&str, &str, usize)> {
    let lead = text.len() - text.trim_start().len();
    let line = text[lead..].trim_end();
    if line.is_empty() {
        return None;
    }
    match line.find(char::is_whitespace) {
        Some(ends) => {
            let rest = &line[ends..];
            let skipped = rest.len() - rest.trim_start().len();
            Some((&line[..ends], rest.trim_start(), lead + ends + skipped))
        }
        None => Some((line, "", lead + line.len())),
    }
}

fn first_index(text: &str) -> bool {
    command_of(text)
        .and_then(|(_, rest, _)| rest.split_whitespace().next())
        .and_then(|number| number.parse::<u32>().ok())
        == Some(FIRST_INDEX)
}

fn crosses_into_the_file(scope: SheetScope, indexed: bool, text: &str) -> bool {
    matches!(scope, SheetScope::Track(_))
        && !indexed
        && command_of(text).is_some_and(|(_, rest, _)| !rest.trim().is_empty())
}

fn quoted(value: &str) -> String {
    let value = value.trim();
    let Some(rest) = value.strip_prefix(QUOTE) else {
        return value.to_owned();
    };
    match rest.rsplit_once(QUOTE) {
        Some((held, _)) => held.to_owned(),
        None => rest.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cue::read;

    const MEDDLE: &str = "REM GENRE \"Progressive Rock\"\r\nPERFORMER \"Pink Floyd\"\r\nTITLE \"Meddle\"\r\nFILE \"Meddle.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    TITLE \"One of These Days\"\r\n    INDEX 01 00:00:00\r\n  TRACK 02 AUDIO\r\n    INDEX 01 05:57:00\r\n";

    fn location() -> MediaLocation {
        MediaLocation::local("/music/Meddle.cue")
    }

    fn edit(scope: SheetScope, field: SheetField, value: Option<&str>) -> SheetEdit {
        SheetEdit {
            scope,
            field,
            value: value.map(str::to_owned),
        }
    }

    fn titles(bytes: &[u8]) -> Vec<Option<String>> {
        read(bytes).files[0]
            .tracks
            .iter()
            .map(|track| track.tags.title.clone())
            .collect()
    }

    #[test]
    fn a_title_edited_in_place_leaves_every_other_byte_of_the_sheet() {
        let sheet = SheetText::read(MEDDLE.as_bytes());

        let written = sheet
            .edited(
                &location(),
                &[edit(
                    SheetScope::Track(1),
                    SheetField::Title,
                    Some("One of These Days (Live)"),
                )],
            )
            .expect("an edit the sheet can hold");

        assert_eq!(
            String::from_utf8(written).expect("still UTF-8"),
            MEDDLE.replace("One of These Days", "One of These Days (Live)")
        );
    }

    #[test]
    fn a_title_a_track_lacks_is_written_into_its_own_block_before_its_index() {
        let sheet = SheetText::read(MEDDLE.as_bytes());

        let written = sheet
            .edited(
                &location(),
                &[edit(
                    SheetScope::Track(2),
                    SheetField::Title,
                    Some("Echoes"),
                )],
            )
            .expect("an edit the sheet can hold");

        assert!(
            String::from_utf8_lossy(&written)
                .contains("  TRACK 02 AUDIO\r\n    TITLE \"Echoes\"\r\n    INDEX 01 05:57:00\r\n"),
            "{}",
            String::from_utf8_lossy(&written)
        );
        assert_eq!(
            titles(&written),
            vec![
                Some("One of These Days".to_owned()),
                Some("Echoes".to_owned())
            ]
        );
    }

    #[test]
    fn an_album_wide_field_is_written_ahead_of_the_first_file() {
        let sheet = SheetText::read(MEDDLE.as_bytes());

        let written = sheet
            .edited(
                &location(),
                &[
                    edit(SheetScope::Disc, SheetField::Date, Some("1971")),
                    edit(
                        SheetScope::Disc,
                        SheetField::Title,
                        Some("Meddle (Remastered)"),
                    ),
                ],
            )
            .expect("an edit the sheet can hold");
        let read_back = read(&written);

        assert!(
            String::from_utf8_lossy(&written)
                .contains("TITLE \"Meddle (Remastered)\"\r\nREM DATE 1971\r\nFILE"),
            "{}",
            String::from_utf8_lossy(&written)
        );
        let tags = &read_back.files[0].tracks[1].tags;
        assert_eq!(tags.album.as_deref(), Some("Meddle (Remastered)"));
        assert_eq!(tags.date.as_deref(), Some("1971"));
    }

    #[test]
    fn a_field_taken_away_is_the_line_that_said_it_gone() {
        let sheet = SheetText::read(MEDDLE.as_bytes());

        let written = sheet
            .edited(
                &location(),
                &[edit(SheetScope::Disc, SheetField::Genre, None)],
            )
            .expect("an edit the sheet can hold");

        assert_eq!(
            String::from_utf8(written).expect("still UTF-8"),
            MEDDLE.replace("REM GENRE \"Progressive Rock\"\r\n", "")
        );
    }

    #[test]
    fn an_edit_and_its_reverse_give_the_sheet_back_byte_for_byte() {
        let sheet = SheetText::read(MEDDLE.as_bytes());
        let was = sheet.own(SheetScope::Track(1), SheetField::Title);

        let written = sheet
            .edited(
                &location(),
                &[edit(
                    SheetScope::Track(1),
                    SheetField::Title,
                    Some("Echoes"),
                )],
            )
            .expect("an edit the sheet can hold");
        let rewritten = SheetText::read(&written);
        assert_eq!(
            rewritten
                .own(SheetScope::Track(1), SheetField::Title)
                .as_deref(),
            Some("Echoes")
        );
        let back = rewritten
            .edited(
                &location(),
                &[edit(
                    SheetScope::Track(1),
                    SheetField::Title,
                    was.as_deref(),
                )],
            )
            .expect("the edit walked back");

        assert_eq!(back, MEDDLE.as_bytes());
    }

    #[test]
    fn a_sheet_in_a_code_page_is_written_back_in_that_code_page() {
        let text = "TITLE \"Caf\u{e9} del Mar\"\nFILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Entr\u{e9}e\"\n    INDEX 01 00:00:00\n";
        let bytes = encoded(text, TextEncoding::WINDOWS_1252).expect("a Latin sheet");
        let sheet = SheetText::read(&bytes);
        assert!(matches!(sheet.encoding(), TextEncoding::Legacy(_)));

        let written = sheet
            .edited(
                &location(),
                &[edit(
                    SheetScope::Track(1),
                    SheetField::Title,
                    Some("Sortie \u{e0} l'aube"),
                )],
            )
            .expect("a title the code page can spell");

        assert_eq!(
            written,
            encoded(
                &text.replace("Entr\u{e9}e", "Sortie \u{e0} l'aube"),
                TextEncoding::WINDOWS_1252
            )
            .expect("the edited sheet in its code page")
        );
    }

    #[test]
    fn a_letter_the_sheets_code_page_lacks_is_refused_rather_than_carried_into_another() {
        let text = "TITLE \"Caf\u{e9}\"\nFILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n";
        let bytes = encoded(text, TextEncoding::WINDOWS_1252).expect("a Latin sheet");
        let sheet = SheetText::read(&bytes);

        let refused = sheet.edited(
            &location(),
            &[edit(
                SheetScope::Track(1),
                SheetField::Title,
                Some("\u{6771}\u{4eac}"),
            )],
        );

        assert!(
            matches!(refused, Err(Error::SheetUnspellable { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn a_wide_sheet_keeps_its_mark_and_its_width() {
        let text = "\u{feff}TITLE \"Meddle\"\r\nFILE \"one.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    INDEX 01 00:00:00\r\n";
        let bytes = encoded(text, TextEncoding::Utf16Le).expect("a wide sheet");
        let sheet = SheetText::read(&bytes);
        assert_eq!(sheet.encoding(), TextEncoding::Utf16Le);

        let written = sheet
            .edited(
                &location(),
                &[edit(
                    SheetScope::Track(1),
                    SheetField::Title,
                    Some("\u{6771}\u{4eac}"),
                )],
            )
            .expect("a title a wide sheet can spell");

        assert_eq!(
            written,
            encoded(
                &text.replace(
                    "  TRACK 01 AUDIO\r\n",
                    "  TRACK 01 AUDIO\r\n    TITLE \"\u{6771}\u{4eac}\"\r\n"
                ),
                TextEncoding::Utf16Le
            )
            .expect("the edited wide sheet")
        );
        assert_eq!(titles(&written), vec![Some("\u{6771}\u{4eac}".to_owned())]);
    }

    #[test]
    fn a_track_the_sheet_does_not_name_once_is_refused() {
        let sheet = SheetText::read(MEDDLE.as_bytes());

        let refused = sheet.edited(
            &location(),
            &[edit(
                SheetScope::Track(7),
                SheetField::Title,
                Some("Seamus"),
            )],
        );

        assert!(
            matches!(refused, Err(Error::SheetTrackNotNamedOnce { track: 7, .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn a_value_over_two_lines_is_refused() {
        let sheet = SheetText::read(MEDDLE.as_bytes());

        let refused = sheet.edited(
            &location(),
            &[edit(
                SheetScope::Track(1),
                SheetField::Title,
                Some("One\nTwo"),
            )],
        );

        assert!(
            matches!(refused, Err(Error::SheetValueUnwritable { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn a_line_said_after_a_file_line_but_before_a_track_is_the_discs() {
        let text =
            "FILE \"one.flac\" WAVE\nTITLE \"Meddle\"\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n";
        let sheet = SheetText::read(text.as_bytes());

        assert_eq!(
            sheet.own(SheetScope::Disc, SheetField::Title).as_deref(),
            Some("Meddle")
        );
        assert_eq!(sheet.own(SheetScope::Track(1), SheetField::Title), None);
    }

    #[test]
    fn a_sheet_with_no_last_line_ending_takes_a_line_at_its_end() {
        let text = "FILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00";
        let sheet = SheetText::read(text.as_bytes());

        let written = sheet
            .edited(
                &location(),
                &[edit(
                    SheetScope::Track(1),
                    SheetField::Performer,
                    Some("Ada"),
                )],
            )
            .expect("an edit the sheet can hold");

        assert_eq!(
            String::from_utf8(written).expect("still UTF-8"),
            "FILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    PERFORMER \"Ada\"\n    INDEX 01 00:00:00"
        );
    }
}
