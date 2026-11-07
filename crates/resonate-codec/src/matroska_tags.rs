use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom, Write};

use crate::{
    matroska::{
        ATTACHMENTS, CLUSTER, EBML_HEADER, Element, Length, MAX_LENGTH_BYTES, SEEK_HEAD, SEGMENT,
        TAGS, VOID, element,
    },
    tags::is_vorbis_comment_only,
    writing::TagField,
};

const DOC_TYPE: u32 = 0x4282;
const CHECKSUM: u32 = 0xBF;
const SEEK: u32 = 0x4DBB;
const SEEK_ID: u32 = 0x53AB;
const SEEK_POSITION: u32 = 0x53AC;
const TAG: u32 = 0x7373;
const TARGETS: u32 = 0x63C0;
const TARGET_TYPE_VALUE: u32 = 0x68CA;
const TARGET_TYPE: u32 = 0x63CA;
const TAG_EDITION_UID: u32 = 0x63C9;
const TAG_CHAPTER_UID: u32 = 0x63C4;
const TAG_ATTACHMENT_UID: u32 = 0x63C6;
const SIMPLE_TAG: u32 = 0x67C8;
const TAG_NAME: u32 = 0x45A3;
const TAG_STRING: u32 = 0x4487;
const SORT_WITH: &[u8] = b"SORT_WITH";
const ATTACHED_FILE: u32 = 0x61A7;
const FILE_NAME: u32 = 0x466E;
const FILE_MEDIA_TYPE: u32 = 0x4660;
const FILE_DATA: u32 = 0x465C;
const FILE_UID: u32 = 0x46AE;
const THE_PICTURE_UID: u64 = 1;

const DOC_TYPES: [&[u8]; 2] = [b"matroska", b"webm"];
const SEGMENT_ID_BYTES: u64 = 4;
const TAGS_BYTES_AT_MOST: u64 = 16 << 20;
const PADDING_LEFT_FOR_THE_NEXT_EDIT: u64 = 1024;
const SMALLEST_VOID_BYTES: u64 = 2;
const VOID_ID_BYTES: u64 = 1;
const BITS_IN_A_SIZE_BYTE: u32 = 7;

const PART_LEVEL: u64 = 40;
const SET_LEVELS: [u64; 3] = [PART_LEVEL, 60, 70];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    Track,
    Album,
}

impl Level {
    const fn value(self) -> u64 {
        match self {
            Self::Track => 30,
            Self::Album => 50,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    NotMatroska,
    UnknownSize,
    NotWhole,
    SegmentChecksummed,
    UnreadableElement,
    TagsTooLarge,
    Unindexed,
    SecondSeekHead,
    AttachmentsStand,
    NoRoomInTheSeekHead,
    SegmentSizeTooNarrow,
}

type Refusable<T> = Result<T, Refused>;

struct Spelling {
    level: Level,
    name: &'static str,
    read_too: &'static [&'static str],
}

const fn spelled(level: Level, name: &'static str, read_too: &'static [&'static str]) -> Spelling {
    Spelling {
        level,
        name,
        read_too,
    }
}

const fn spelling(field: TagField) -> Spelling {
    use Level::{Album, Track};
    use TagField as F;

    match field {
        F::Title => spelled(Track, "TITLE", &[]),
        F::Artist => spelled(Track, "ARTIST", &[]),
        F::Album => spelled(Album, "ALBUM", &[]),
        F::AlbumArtist => spelled(Album, "ALBUM_ARTIST", &["ALBUMARTIST", "ALBUM ARTIST"]),
        F::TrackNumber => spelled(Album, "TRACKNUMBER", &["TRACK"]),
        F::TrackTotal => spelled(Album, "TRACKTOTAL", &["TOTALTRACKS"]),
        F::DiscNumber => spelled(Album, "DISCNUMBER", &["DISC"]),
        F::DiscTotal => spelled(Album, "DISCTOTAL", &["TOTALDISCS"]),
        F::Date => spelled(
            Album,
            "DATE_RECORDED",
            &[
                "DATE",
                "DATE_RELEASE",
                "DATE_RELEASED",
                "ORIGINALDATE",
                "ORIGINAL_DATE",
            ],
        ),
        F::Label => spelled(Album, "LABEL", &["PUBLISHER", "ORGANIZATION"]),
        F::CatalogNumber => spelled(Album, "CATALOG_NUMBER", &["CATALOGNUMBER"]),
        F::Barcode => spelled(Album, "BARCODE", &[]),
        F::Isrc => spelled(Album, "ISRC", &[]),
        F::MusicBrainzTrackId => spelled(Album, "MUSICBRAINZ_TRACKID", &[]),
        F::MusicBrainzReleaseTrackId => spelled(Album, "MUSICBRAINZ_RELEASETRACKID", &[]),
        F::MusicBrainzAlbumId => spelled(Album, "MUSICBRAINZ_ALBUMID", &[]),
        F::MusicBrainzArtistId => spelled(Album, "MUSICBRAINZ_ARTISTID", &[]),
        F::MusicBrainzAlbumArtistId => spelled(Album, "MUSICBRAINZ_ALBUMARTISTID", &[]),
        F::MusicBrainzReleaseGroupId => spelled(Album, "MUSICBRAINZ_RELEASEGROUPID", &[]),
        F::Genre => spelled(Album, "GENRE", &[]),
        F::Composer => spelled(Album, "COMPOSER", &[]),
        F::Conductor => spelled(Album, "CONDUCTOR", &[]),
        F::Lyricist => spelled(Album, "LYRICIST", &["WRITTEN_BY", "WRITER"]),
        F::Performer => spelled(Album, "LEAD_PERFORMER", &["PERFORMER"]),
        F::Remixer => spelled(Album, "REMIXED_BY", &["REMIXER", "MIXARTIST"]),
        F::Engineer => spelled(Album, "SOUND_ENGINEER", &["ENGINEER", "MASTERED_BY"]),
        F::Producer => spelled(Album, "PRODUCER", &[]),
        F::Comment => spelled(Album, "COMMENT", &[]),
        F::BeatsPerMinute => spelled(Album, "BPM", &[]),
        F::Compilation => spelled(Album, "COMPILATION", &[]),
        F::Grouping => spelled(Album, "CONTENTGROUP", &["GROUPING"]),
        F::Copyright => spelled(Album, "COPYRIGHT", &[]),
        F::ReplayGainTrackGain => spelled(Album, "REPLAYGAIN_TRACK_GAIN", &["R128_TRACK_GAIN"]),
        F::ReplayGainTrackPeak => spelled(Album, "REPLAYGAIN_TRACK_PEAK", &[]),
        F::ReplayGainAlbumGain => spelled(Album, "REPLAYGAIN_ALBUM_GAIN", &["R128_ALBUM_GAIN"]),
        F::ReplayGainAlbumPeak => spelled(Album, "REPLAYGAIN_ALBUM_PEAK", &[]),
        F::ArtistSort => spelled(Album, "ARTISTSORT", &[]),
        F::AlbumArtistSort => spelled(Album, "ALBUMARTISTSORT", &[]),
        F::TitleSort => spelled(Album, "TITLESORT", &[]),
        F::AlbumSort => spelled(Album, "ALBUMSORT", &[]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reading {
    level: Option<u64>,
    vorbis: bool,
    parted: bool,
}

impl Reading {
    const fn at_the_album(self) -> bool {
        matches!(self.level, Some(level) if level == Level::Album.value())
    }

    const fn at_the_part(self) -> bool {
        matches!(self.level, Some(PART_LEVEL))
    }

    fn of_a_set(self) -> bool {
        self.level.is_some_and(|level| SET_LEVELS.contains(&level))
    }

    fn field_of(self, name: &str) -> Option<TagField> {
        match name {
            "TITLE" if self.at_the_album() && !self.vorbis => Some(TagField::Album),
            "TITLE" if self.of_a_set() => None,
            "TITLE" => Some(TagField::Title),
            "ARTIST" if self.at_the_album() && !self.vorbis => Some(TagField::AlbumArtist),
            "ARTIST" => Some(TagField::Artist),
            "PART_NUMBER" if self.at_the_part() => Some(TagField::DiscNumber),
            "PART_NUMBER" => Some(TagField::TrackNumber),
            "TOTAL_PARTS" if self.at_the_album() && self.parted => Some(TagField::DiscTotal),
            "TOTAL_PARTS" => Some(TagField::TrackTotal),
            "REPLAYGAIN_GAIN" if self.at_the_album() => Some(TagField::ReplayGainAlbumGain),
            "REPLAYGAIN_GAIN" => Some(TagField::ReplayGainTrackGain),
            "REPLAYGAIN_PEAK" if self.at_the_album() => Some(TagField::ReplayGainAlbumPeak),
            "REPLAYGAIN_PEAK" => Some(TagField::ReplayGainTrackPeak),
            _ => TagField::ALL.into_iter().find(|field| {
                let spelled = spelling(*field);
                spelled.name == name || spelled.read_too.contains(&name)
            }),
        }
    }

    fn sort_field_of(self, name: &str) -> Option<TagField> {
        match name {
            "ARTIST" if self.at_the_album() => Some(TagField::AlbumArtistSort),
            "ARTIST" => Some(TagField::ArtistSort),
            "TITLE" if self.at_the_album() => Some(TagField::AlbumSort),
            "TITLE" if self.level == Some(Level::Track.value()) => Some(TagField::TitleSort),
            _ => None,
        }
    }

    fn turns_with_the_naming(self, name: &str) -> bool {
        self.at_the_album() && matches!(name, "TITLE" | "ARTIST")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Child {
    id: u32,
    at: u64,
    body: u64,
    end: u64,
}

struct Layout {
    size_at: u64,
    size_width: u32,
    body: u64,
    end: u64,
    children: Vec<Child>,
}

impl Layout {
    fn clusters(&self) -> impl Iterator<Item = &Child> {
        self.children.iter().filter(|child| child.id == CLUSTER)
    }

    fn all(&self, id: u32) -> Vec<usize> {
        self.children
            .iter()
            .enumerate()
            .filter(|(_, child)| child.id == id)
            .map(|(index, _)| index)
            .collect()
    }

    fn room_of(&self, index: usize) -> u64 {
        self.children[index + 1..]
            .iter()
            .take_while(|child| child.id == u32::from(VOID))
            .last()
            .map_or(self.children[index].end, |void| void.end)
    }

    fn is_the_tail(&self, index: usize) -> bool {
        self.room_of(index) == self.end
    }
}

fn laid_out<S: Read + Seek + ?Sized>(source: &mut S) -> Refusable<Layout> {
    let length = source
        .seek(SeekFrom::End(0))
        .map_err(|_| Refused::UnreadableElement)?;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| Refused::UnreadableElement)?;

    let header = element(source).ok_or(Refused::NotMatroska)?;
    if header.id != EBML_HEADER {
        return Err(Refused::NotMatroska);
    }
    let header_end = header.end().ok_or(Refused::NotMatroska)?;
    if !names_a_matroska(source, header_end) {
        return Err(Refused::NotMatroska);
    }
    source
        .seek(SeekFrom::Start(header_end))
        .map_err(|_| Refused::NotMatroska)?;

    let segment = element(source).ok_or(Refused::NotMatroska)?;
    if segment.id != SEGMENT {
        return Err(Refused::NotMatroska);
    }
    let end = segment.end().ok_or(Refused::UnknownSize)?;
    if end != length {
        return Err(Refused::NotWhole);
    }

    let size_at = segment.at + SEGMENT_ID_BYTES;
    let size_width = u32::try_from(segment.body - size_at).map_err(|_| Refused::NotMatroska)?;

    let mut children = Vec::new();
    let mut at = segment.body;
    while at < end {
        let child = child_at(source, at, end)?;
        if child.id == CHECKSUM {
            return Err(Refused::SegmentChecksummed);
        }
        at = child.end;
        children.push(child);
    }

    Ok(Layout {
        size_at,
        size_width,
        body: segment.body,
        end,
        children,
    })
}

fn names_a_matroska<S: Read + Seek + ?Sized>(source: &mut S, end: u64) -> bool {
    let Ok(mut at) = source.stream_position() else {
        return false;
    };
    while at < end {
        let Ok(child) = child_at(source, at, end) else {
            return false;
        };
        if child.id == DOC_TYPE {
            let Ok(held) = held_body(source, &child) else {
                return false;
            };
            let named = held.split(|byte| *byte == 0).next().unwrap_or_default();
            return DOC_TYPES.contains(&named);
        }
        at = child.end;
    }
    false
}

fn child_at<S: Read + Seek + ?Sized>(source: &mut S, at: u64, end: u64) -> Refusable<Child> {
    source
        .seek(SeekFrom::Start(at))
        .map_err(|_| Refused::UnreadableElement)?;
    let found = element(source).ok_or(Refused::UnreadableElement)?;
    let Length::Known(_) = found.length else {
        return Err(Refused::UnknownSize);
    };
    let child_end = found.end().ok_or(Refused::UnreadableElement)?;
    if child_end > end {
        return Err(Refused::UnreadableElement);
    }
    Ok(Child {
        id: found.id,
        at: found.at,
        body: found.body,
        end: child_end,
    })
}

fn held_body<S: Read + Seek + ?Sized>(source: &mut S, child: &Child) -> Refusable<Vec<u8>> {
    let bytes = child.end - child.body;
    if bytes > TAGS_BYTES_AT_MOST {
        return Err(Refused::TagsTooLarge);
    }
    source
        .seek(SeekFrom::Start(child.body))
        .map_err(|_| Refused::UnreadableElement)?;
    let mut held = vec![0_u8; usize::try_from(bytes).map_err(|_| Refused::TagsTooLarge)?];
    source
        .read_exact(&mut held)
        .map_err(|_| Refused::UnreadableElement)?;
    Ok(held)
}

struct Within<'a> {
    id: u32,
    whole: &'a [u8],
    body: &'a [u8],
}

fn within(bytes: &[u8]) -> Refusable<Vec<Within<'_>>> {
    let mut cursor = Cursor::new(bytes);
    let mut found = Vec::new();
    let length = bytes.len() as u64;
    let mut at = 0;
    while at < length {
        let Element {
            id,
            length: size,
            body,
            ..
        } = element(&mut cursor).ok_or(Refused::UnreadableElement)?;
        let Length::Known(size) = size else {
            return Err(Refused::UnknownSize);
        };
        let end = body
            .checked_add(size)
            .filter(|end| *end <= length)
            .ok_or(Refused::UnreadableElement)?;
        let span = |from: u64| usize::try_from(from).map_err(|_| Refused::UnreadableElement);
        let (from, starts, stops) = (span(at)?, span(body)?, span(end)?);
        found.push(Within {
            id,
            whole: &bytes[from..stops],
            body: &bytes[starts..stops],
        });
        at = end;
        cursor.set_position(at);
    }
    Ok(found)
}

fn number_in(bytes: &[u8]) -> Option<u64> {
    (bytes.len() <= MAX_LENGTH_BYTES as usize).then(|| {
        bytes
            .iter()
            .fold(0_u64, |number, byte| (number << 8) | u64::from(*byte))
    })
}

fn id_bytes(id: u32) -> Vec<u8> {
    id.to_be_bytes()
        .into_iter()
        .skip_while(|byte| *byte == 0)
        .collect()
}

fn number_bytes(number: u64) -> Vec<u8> {
    let held = number.to_be_bytes();
    let first = held
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(held.len() - 1);
    held[first..].to_vec()
}

fn largest_in(width: u32) -> Option<u64> {
    (1..=MAX_LENGTH_BYTES)
        .contains(&width)
        .then(|| (1_u64 << (BITS_IN_A_SIZE_BYTE * width)) - 2)
}

fn size_bytes(size: u64, width: u32) -> Option<Vec<u8>> {
    largest_in(width).filter(|largest| size <= *largest)?;
    let marked = (1_u64 << (BITS_IN_A_SIZE_BYTE * width)) | size;
    let held = marked.to_be_bytes();
    held.get(held.len() - width as usize..).map(<[u8]>::to_vec)
}

fn narrowest_width(size: u64) -> Option<u32> {
    (1..=MAX_LENGTH_BYTES).find(|width| largest_in(*width).is_some_and(|most| size <= most))
}

fn with_width(id: u32, body: &[u8], width: u32) -> Option<Vec<u8>> {
    let mut bytes = id_bytes(id);
    bytes.extend_from_slice(&size_bytes(body.len() as u64, width)?);
    bytes.extend_from_slice(body);
    Some(bytes)
}

fn sized(id: u32, body: &[u8]) -> Vec<u8> {
    narrowest_width(body.len() as u64)
        .and_then(|width| with_width(id, body, width))
        .expect("what this writer lays out fits a size field")
}

fn void_of(bytes: u64) -> Option<Vec<u8>> {
    if bytes < SMALLEST_VOID_BYTES {
        return None;
    }
    let (width, size) = (1..=MAX_LENGTH_BYTES).find_map(|width| {
        let size = bytes.checked_sub(VOID_ID_BYTES + u64::from(width))?;
        largest_in(width)
            .filter(|most| size <= *most)
            .map(|_| (width, size))
    })?;
    let mut void = vec![VOID];
    void.extend_from_slice(&size_bytes(size, width)?);
    void.resize(usize::try_from(bytes).ok()?, 0);
    Some(void)
}

fn filling(id: u32, body: &[u8], room: u64) -> Option<Vec<u8>> {
    let width = narrowest_width(body.len() as u64)?;
    let mut laid = with_width(id, body, width)?;
    let left = room.checked_sub(laid.len() as u64)?;
    match left {
        0 => Some(laid),
        1 => with_width(id, body, width + 1),
        left => {
            laid.extend_from_slice(&void_of(left)?);
            Some(laid)
        }
    }
}

struct SimpleTag<'a> {
    whole: &'a [u8],
    name: String,
    value: Option<&'a str>,
    sort: Option<&'a str>,
    unsorted: Vec<&'a [u8]>,
}

impl SimpleTag<'_> {
    fn laid_unsorted(&self) -> Vec<u8> {
        sized(SIMPLE_TAG, &self.unsorted.concat())
    }
}

struct HeldTag<'a> {
    whole: &'a [u8],
    targets: Option<&'a [u8]>,
    level: Option<u64>,
    aimed_elsewhere: bool,
    bare: bool,
    simple: Vec<SimpleTag<'a>>,
}

fn sort_within<'a>(nested: &Within<'a>) -> Refusable<Option<&'a str>> {
    let mut sorts = false;
    let mut value = None;
    for child in within(nested.body)? {
        match child.id {
            TAG_NAME => sorts = child.body == SORT_WITH,
            TAG_STRING => value = str::from_utf8(child.body).ok(),
            _ => {}
        }
    }
    Ok(value.filter(|_| sorts))
}

fn simple_tag_of<'a>(simple: &Within<'a>) -> Refusable<SimpleTag<'a>> {
    let mut name = String::new();
    let mut value = None;
    let mut sort = None;
    let mut unsorted = Vec::new();
    for child in within(simple.body)? {
        let nested_sort = match child.id {
            SIMPLE_TAG => sort_within(&child)?,
            _ => None,
        };
        match child.id {
            TAG_NAME => name = String::from_utf8_lossy(child.body).to_ascii_uppercase(),
            TAG_STRING => value = str::from_utf8(child.body).ok(),
            _ => {}
        }
        match nested_sort {
            Some(held) => sort = Some(held),
            None => unsorted.push(child.whole),
        }
    }
    Ok(SimpleTag {
        whole: simple.whole,
        name,
        value,
        sort,
        unsorted,
    })
}

fn held_tag_of<'a>(tag: &Within<'a>) -> Refusable<HeldTag<'a>> {
    let mut held = HeldTag {
        whole: tag.whole,
        targets: None,
        level: None,
        aimed_elsewhere: false,
        bare: true,
        simple: Vec::new(),
    };
    for child in within(tag.body)? {
        match child.id {
            TARGETS => {
                held.targets = Some(child.whole);
                held.level = Some(Level::Album.value());
                for target in within(child.body)? {
                    match target.id {
                        TARGET_TYPE_VALUE => {
                            held.level = number_in(target.body).or(held.level);
                        }
                        TARGET_TYPE => {}
                        TAG_EDITION_UID | TAG_CHAPTER_UID | TAG_ATTACHMENT_UID => {
                            held.aimed_elsewhere = true;
                            held.bare = false;
                        }
                        _ => held.bare = false,
                    }
                }
            }
            SIMPLE_TAG => held.simple.push(simple_tag_of(&child)?),
            _ => {}
        }
    }
    Ok(held)
}

fn held_tags_in(body: &[u8]) -> Refusable<Vec<HeldTag<'_>>> {
    within(body)?
        .iter()
        .filter(|child| child.id == TAG)
        .map(held_tag_of)
        .collect()
}

struct Kept {
    targets: Option<Vec<u8>>,
    level: Option<u64>,
    bare: bool,
    simple: Vec<Vec<u8>>,
    whole: Option<Vec<u8>>,
}

impl Kept {
    fn laid(&self) -> Vec<u8> {
        if let Some(whole) = &self.whole {
            return whole.clone();
        }
        let mut body = self.targets.clone().unwrap_or_default();
        for simple in &self.simple {
            body.extend_from_slice(simple);
        }
        sized(TAG, &body)
    }
}

fn simple_tag(name: &str, value: &str) -> Vec<u8> {
    let mut body = sized(TAG_NAME, name.as_bytes());
    body.extend_from_slice(&sized(TAG_STRING, value.as_bytes()));
    sized(SIMPLE_TAG, &body)
}

fn targets_at(level: Level) -> Vec<u8> {
    sized(
        TARGETS,
        &sized(TARGET_TYPE_VALUE, &number_bytes(level.value())),
    )
}

struct Asked<'a> {
    edits: &'a [(TagField, String)],
    taken: &'a [TagField],
}

impl Asked<'_> {
    fn names(&self, field: TagField) -> bool {
        self.taken.contains(&field) || self.edits.iter().any(|(edited, _)| *edited == field)
    }

    fn is_empty(&self) -> bool {
        self.edits.is_empty() && self.taken.is_empty()
    }
}

type Said = (TagField, DateStanding);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateStanding {
    Recorded,
    Released,
    Original,
}

impl DateStanding {
    fn of(name: &str) -> Self {
        match name {
            "DATE" | "DATE_RELEASE" | "DATE_RELEASED" => Self::Released,
            "ORIGINALDATE" | "ORIGINAL_DATE" => Self::Original,
            _ => Self::Recorded,
        }
    }
}

struct Gone<'a> {
    asked: &'a Asked<'a>,
    said_later: &'a [Said],
}

impl Gone<'_> {
    fn takes(&self, field: TagField, name: &str) -> bool {
        self.asked.names(field) || self.said_later.contains(&(field, DateStanding::of(name)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Naming {
    vorbis: bool,
    parted: bool,
}

impl Naming {
    fn of(tags: &[HeldTag<'_>]) -> Self {
        let read = || tags.iter().filter(|tag| !tag.aimed_elsewhere);
        Self {
            vorbis: read()
                .flat_map(|tag| &tag.simple)
                .any(|simple| is_vorbis_comment_only(&simple.name)),
            parted: read().any(|tag| tag.level == Some(PART_LEVEL)),
        }
    }

    const fn at(self, tag: &HeldTag<'_>) -> Reading {
        Reading {
            level: tag.level,
            vorbis: self.vorbis,
            parted: self.parted,
        }
    }
}

fn said_in(tags: &[HeldTag<'_>]) -> Vec<Said> {
    let naming = Naming::of(tags);
    let mut said = Vec::new();
    for tag in tags.iter().filter(|tag| !tag.aimed_elsewhere) {
        let reading = naming.at(tag);
        for simple in &tag.simple {
            let given = simple.value.is_some_and(|value| !value.trim().is_empty());
            if let (true, Some(field)) = (given, reading.field_of(&simple.name)) {
                said.push((field, DateStanding::of(&simple.name)));
            }
            if let (Some(_), Some(field)) = (simple.sort, reading.sort_field_of(&simple.name)) {
                said.push((field, DateStanding::Recorded));
            }
        }
    }
    said
}

fn merged(held: &[Vec<u8>], asked: &Asked<'_>) -> Refusable<Vec<u8>> {
    let mut kept = Vec::new();
    let mut carried = Vec::new();

    let bodies = held
        .iter()
        .map(|body| held_tags_in(body))
        .collect::<Refusable<Vec<_>>>()?;
    let said: Vec<Vec<Said>> = bodies.iter().map(|tags| said_in(tags)).collect();

    for (index, tags) in bodies.iter().enumerate() {
        let naming = Naming::of(tags);
        let said_later = said[index + 1..].concat();
        let gone = Gone {
            asked,
            said_later: &said_later,
        };
        for tag in tags {
            kept.extend(kept_of(tag, naming.at(tag), &gone, &mut carried));
        }
    }

    for (field, value) in asked.edits {
        carried.push((*field, value.clone()));
    }
    for level in [Level::Track, Level::Album] {
        let simple: Vec<Vec<u8>> = carried
            .iter()
            .map(|(field, value)| (spelling(*field), value))
            .filter(|(spelled, _)| spelled.level == level)
            .map(|(spelled, value)| simple_tag(spelled.name, value))
            .collect();
        if simple.is_empty() {
            continue;
        }
        let home = kept
            .iter_mut()
            .find(|tag: &&mut Kept| tag.bare && tag.level == Some(level.value()));
        match home {
            Some(home) => {
                home.whole = None;
                home.simple.extend(simple);
            }
            None => kept.push(Kept {
                targets: Some(targets_at(level)),
                level: Some(level.value()),
                bare: true,
                simple,
                whole: None,
            }),
        }
    }

    Ok(kept.iter().flat_map(Kept::laid).collect())
}

fn kept_of(
    tag: &HeldTag<'_>,
    reading: Reading,
    gone: &Gone<'_>,
    carried: &mut Vec<(TagField, String)>,
) -> Option<Kept> {
    let mut simple = Vec::new();
    let mut touched = false;
    for held in &tag.simple {
        if tag.aimed_elsewhere {
            simple.push(held.whole.to_vec());
            continue;
        }

        let field = reading.field_of(&held.name);
        let sort = held.sort.zip(reading.sort_field_of(&held.name));
        let turned = field
            .zip(held.value)
            .filter(|_| reading.turns_with_the_naming(&held.name));
        let replaced = field.is_some_and(|field| gone.takes(field, &held.name));

        if replaced || turned.is_some() {
            touched = true;
            if let (false, Some((field, value))) = (replaced, turned) {
                carried.push((field, value.to_owned()));
            }
            if let Some((value, field)) = sort.filter(|(_, field)| !gone.takes(*field, "")) {
                carried.push((field, value.to_owned()));
            }
        } else if sort.is_some_and(|(_, field)| gone.takes(field, "")) {
            touched = true;
            simple.push(held.laid_unsorted());
        } else {
            simple.push(held.whole.to_vec());
        }
    }
    if touched && simple.is_empty() {
        return None;
    }
    Some(Kept {
        targets: tag.targets.map(<[u8]>::to_vec),
        level: tag.level,
        bare: tag.bare,
        simple,
        whole: (!touched).then(|| tag.whole.to_vec()),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    id: Vec<u8>,
    position: u64,
}

fn entries_in(body: &[u8]) -> Refusable<Vec<Entry>> {
    let mut entries = Vec::new();
    for seek in within(body)?.iter().filter(|child| child.id == SEEK) {
        let mut id = None;
        let mut position = None;
        for child in within(seek.body)? {
            match child.id {
                SEEK_ID => id = Some(child.body.to_vec()),
                SEEK_POSITION => position = number_in(child.body),
                _ => {}
            }
        }
        let (Some(id), Some(position)) = (id, position) else {
            return Err(Refused::UnreadableElement);
        };
        entries.push(Entry { id, position });
    }
    Ok(entries)
}

fn laid_seek_head(entries: &[Entry]) -> Vec<u8> {
    let mut body = Vec::new();
    for entry in entries {
        let mut seek = sized(SEEK_ID, &entry.id);
        seek.extend_from_slice(&sized(SEEK_POSITION, &number_bytes(entry.position)));
        body.extend_from_slice(&sized(SEEK, &seek));
    }
    body
}

fn entries_with_the_element_at(
    entries: &[Entry],
    id: u32,
    position: Option<u64>,
    indexed: bool,
) -> Vec<Entry> {
    let element_id = id_bytes(id);
    let mut placed = false;
    let mut wanted = Vec::new();
    for entry in entries {
        if entry.id != element_id {
            wanted.push(entry.clone());
            continue;
        }
        if let (false, Some(position)) = (placed, position) {
            placed = true;
            wanted.push(Entry {
                id: entry.id.clone(),
                position,
            });
        }
    }
    if let (false, true, Some(position)) = (placed, indexed, position) {
        wanted.push(Entry {
            id: element_id,
            position,
        });
    }
    wanted
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Laid {
    at: u64,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rewrite {
    laid: Vec<Laid>,
    clusters: Vec<Child>,
}

impl Rewrite {
    pub(crate) const fn is_empty(&self) -> bool {
        self.laid.is_empty()
    }

    pub(crate) fn laid_on<F: Read + Write + Seek>(&self, file: &mut F) -> io::Result<()> {
        for laid in &self.laid {
            file.seek(SeekFrom::Start(laid.at))?;
            file.write_all(&laid.bytes)?;
        }
        file.flush()?;
        self.confirmed_on(file)
    }

    fn confirmed_on<F: Read + Seek>(&self, file: &mut F) -> io::Result<()> {
        let unconfirmed = || io::Error::from(io::ErrorKind::InvalidData);
        let layout = laid_out(&mut BufReader::new(file)).map_err(|_| unconfirmed())?;
        let moved = !layout.clusters().eq(self.clusters.iter());
        if moved || layout.all(TAGS).len() > 1 || layout.all(ATTACHMENTS).len() > 1 {
            return Err(unconfirmed());
        }
        Ok(())
    }
}

pub(crate) fn rewritten<S: Read + Seek + ?Sized>(
    source: &mut S,
    edits: &[(TagField, String)],
    taken: &[TagField],
) -> Refusable<Rewrite> {
    let layout = laid_out(source)?;
    let clusters: Vec<Child> = layout.clusters().copied().collect();
    let asked = Asked { edits, taken };
    if asked.is_empty() {
        return Ok(Rewrite {
            laid: Vec::new(),
            clusters,
        });
    }

    let olds = layout.all(TAGS);
    let held = olds
        .iter()
        .map(|index| held_body(source, &layout.children[*index]))
        .collect::<Refusable<Vec<_>>>()?;
    let body = merged(&held, &asked)?;
    if body.is_empty() && olds.is_empty() {
        return Ok(Rewrite {
            laid: Vec::new(),
            clusters,
        });
    }

    let placing = placed(&layout, &olds, &body);
    let mut laid: Vec<Laid> = placing.laid;
    for (index, child) in olds.iter().map(|index| (*index, &layout.children[*index])) {
        if Some(index) != placing.home {
            laid.push(Laid {
                at: child.at,
                bytes: void_of(child.end - child.at).ok_or(Refused::UnreadableElement)?,
            });
        }
    }

    laid.extend(indexed(
        source,
        &layout,
        TAGS,
        &olds,
        placing.at,
        placing.home,
    )?);
    if placing.end > layout.end {
        let size = placing.end - layout.body;
        laid.push(Laid {
            at: layout.size_at,
            bytes: size_bytes(size, layout.size_width).ok_or(Refused::SegmentSizeTooNarrow)?,
        });
    }

    Ok(Rewrite { laid, clusters })
}

pub(crate) struct Attached<'a> {
    pub(crate) name: &'a str,
    pub(crate) media_type: &'a str,
    pub(crate) bytes: &'a [u8],
}

fn attachments_of(file: &Attached<'_>) -> Vec<u8> {
    let mut body = sized(FILE_NAME, file.name.as_bytes());
    body.extend_from_slice(&sized(FILE_MEDIA_TYPE, file.media_type.as_bytes()));
    body.extend_from_slice(&sized(FILE_DATA, file.bytes));
    body.extend_from_slice(&sized(FILE_UID, &number_bytes(THE_PICTURE_UID)));
    sized(ATTACHMENTS, &sized(ATTACHED_FILE, &body))
}

pub(crate) fn attached<S: Read + Seek + ?Sized>(
    source: &mut S,
    file: &Attached<'_>,
) -> Refusable<Rewrite> {
    let layout = laid_out(source)?;
    let clusters: Vec<Child> = layout.clusters().copied().collect();
    if !layout.all(ATTACHMENTS).is_empty() {
        return Err(Refused::AttachmentsStand);
    }

    let at = layout.end;
    let element = attachments_of(file);
    let end = at + element.len() as u64;
    let mut laid = vec![Laid { at, bytes: element }];
    laid.extend(indexed(source, &layout, ATTACHMENTS, &[], Some(at), None)?);
    laid.push(Laid {
        at: layout.size_at,
        bytes: size_bytes(end - layout.body, layout.size_width)
            .ok_or(Refused::SegmentSizeTooNarrow)?,
    });

    Ok(Rewrite { laid, clusters })
}

struct Placing {
    laid: Vec<Laid>,
    at: Option<u64>,
    home: Option<usize>,
    end: u64,
}

fn placed(layout: &Layout, olds: &[usize], body: &[u8]) -> Placing {
    if body.is_empty() {
        return Placing {
            laid: Vec::new(),
            at: None,
            home: None,
            end: layout.end,
        };
    }

    let in_room = olds.iter().find_map(|index| {
        let at = layout.children[*index].at;
        let bytes = filling(TAGS, body, layout.room_of(*index) - at)?;
        Some((*index, at, bytes))
    });
    if let Some((index, at, bytes)) = in_room {
        return Placing {
            laid: vec![Laid { at, bytes }],
            at: Some(at),
            home: Some(index),
            end: layout.end,
        };
    }

    let at_the_tail = olds
        .iter()
        .copied()
        .find(|index| layout.is_the_tail(*index));
    let at = at_the_tail.map_or(layout.end, |index| layout.children[index].at);
    let mut bytes = sized(TAGS, body);
    bytes.extend_from_slice(
        &void_of(PADDING_LEFT_FOR_THE_NEXT_EDIT).expect("the padding is wider than a void's head"),
    );
    let end = at + bytes.len() as u64;
    Placing {
        laid: vec![Laid { at, bytes }],
        at: Some(at),
        home: at_the_tail,
        end,
    }
}

fn indexed<S: Read + Seek + ?Sized>(
    source: &mut S,
    layout: &Layout,
    id: u32,
    olds: &[usize],
    at: Option<u64>,
    home: Option<usize>,
) -> Refusable<Option<Laid>> {
    let first_cluster = layout.clusters().next().map(|cluster| cluster.at);
    let past_the_clusters = at.is_some_and(|at| first_cluster.is_some_and(|first| at > first));
    let heads = layout.all(SEEK_HEAD);
    let moved = home.is_none() || olds.len() > 1;

    let Some(index) = heads.first().copied() else {
        return if past_the_clusters {
            Err(Refused::Unindexed)
        } else {
            Ok(None)
        };
    };
    let head = layout.children[index];
    let read_by_a_reader = first_cluster.is_none_or(|first| head.at < first);
    if past_the_clusters && !read_by_a_reader {
        return Err(Refused::Unindexed);
    }
    if heads.len() > 1 && moved {
        return Err(Refused::SecondSeekHead);
    }

    let entries = entries_in(&held_body(source, &head)?)?;
    let position = at.map(|at| at - layout.body);
    let wanted = entries_with_the_element_at(&entries, id, position, past_the_clusters);
    if wanted == entries {
        return Ok(None);
    }
    let room = layout.room_of(index) - head.at;
    let bytes = laid_in_the_room(wanted, layout, id, room).ok_or(Refused::NoRoomInTheSeekHead)?;
    Ok(Some(Laid { at: head.at, bytes }))
}

fn laid_in_the_room(
    mut wanted: Vec<Entry>,
    layout: &Layout,
    id: u32,
    room: u64,
) -> Option<Vec<u8>> {
    let added = id_bytes(id);
    let first_cluster = layout.clusters().next().map(|cluster| cluster.at);
    loop {
        if let Some(bytes) = filling(SEEK_HEAD, &laid_seek_head(&wanted), room) {
            return Some(bytes);
        }
        let read_in_order_anyway = wanted.iter().position(|entry| {
            entry.id != added
                && first_cluster.is_some_and(|first| layout.body + entry.position < first)
        })?;
        wanted.remove(read_in_order_anyway);
    }
}
