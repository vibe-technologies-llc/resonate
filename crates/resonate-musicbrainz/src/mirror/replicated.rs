use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Read},
    path::Path,
};

use resonate_core::writer::Held;
use rusqlite::{Connection, types::Value};

use super::{
    Mirror,
    amended::{AMENDED_FILE, Amending, made_at},
    copy::{Table, fields, table_called, unescaped},
    documents::{Builders, rows_of},
    import::Building,
    looked_up::{LookedUp, gid_bytes, store_read},
    tables::{Change, TABLES_FILE, applied, opened, sequenced_at},
};
use crate::{
    Entity, Error, Result, Store, StoreOp,
    kept::now,
    wire::{MBID_BYTES, mbid_of},
};

pub const PACKETS_A_BATCH: usize = 24;
const PENDING_DATA: &str = "mbdump/pending_data";
const REPLICATION_SEQUENCE: &str = "REPLICATION_SEQUENCE";
const SCHEMA_SEQUENCE: &str = "SCHEMA_SEQUENCE";
const DBMIRROR_VERSION: &str = "DBMIRROR_VERSION";
const DBMIRROR2: u64 = 2;
const SCHEMA_OF_TABLES: &str = "musicbrainz.";
const PENDING_COLUMNS_AT_LEAST: usize = 6;
const LONGEST_SEQUENCE_FILE: u64 = 64;
const TRUE: &str = "t";
const FALSE: &str = "f";

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    Nested,
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Parser<'a> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Option<()> {
        self.space();
        (self.bytes.get(self.at) == Some(&byte)).then(|| self.at += 1)
    }

    fn word(&mut self, word: &[u8]) -> Option<()> {
        self.bytes[self.at..]
            .starts_with(word)
            .then(|| self.at += word.len())
    }

    fn hex(&mut self) -> Option<u32> {
        let digits = self.bytes.get(self.at..self.at + 4)?;
        let value = u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()?;
        self.at += 4;
        Some(value)
    }

    fn text(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut text = String::new();
        loop {
            let start = self.at;
            while let Some(byte) = self.bytes.get(self.at) {
                if *byte == b'"' || *byte == b'\\' {
                    break;
                }
                self.at += 1;
            }
            text.push_str(std::str::from_utf8(&self.bytes[start..self.at]).ok()?);
            match *self.bytes.get(self.at)? {
                b'"' => {
                    self.at += 1;
                    return Some(text);
                }
                _ => {
                    self.at += 1;
                    let escaped = *self.bytes.get(self.at)?;
                    self.at += 1;
                    match escaped {
                        b'"' => text.push('"'),
                        b'\\' => text.push('\\'),
                        b'/' => text.push('/'),
                        b'b' => text.push('\u{8}'),
                        b'f' => text.push('\u{c}'),
                        b'n' => text.push('\n'),
                        b'r' => text.push('\r'),
                        b't' => text.push('\t'),
                        b'u' => {
                            let high = self.hex()?;
                            let point = if (0xd800..0xdc00).contains(&high) {
                                self.word(b"\\u")?;
                                let low = self.hex()?;
                                if !(0xdc00..0xe000).contains(&low) {
                                    return None;
                                }
                                0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00)
                            } else {
                                high
                            };
                            text.push(char::from_u32(point).unwrap_or('\u{fffd}'));
                        }
                        _ => return None,
                    }
                }
            }
        }
    }

    fn number(&mut self) -> Option<String> {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_digit() || b"+-.eE".contains(byte))
        {
            self.at += 1;
        }
        (self.at > start).then(|| String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned())
    }

    fn skipped(&mut self, open: u8, close: u8) -> Option<()> {
        self.eat(open)?;
        self.space();
        if self.eat(close).is_some() {
            return Some(());
        }
        loop {
            if open == b'{' {
                self.text()?;
                self.eat(b':')?;
            }
            self.value()?;
            if self.eat(b',').is_some() {
                continue;
            }
            return self.eat(close);
        }
    }

    fn value(&mut self) -> Option<Json> {
        self.space();
        match *self.bytes.get(self.at)? {
            b'"' => self.text().map(Json::Text),
            b'n' => self.word(b"null").map(|()| Json::Null),
            b't' => self.word(b"true").map(|()| Json::Bool(true)),
            b'f' => self.word(b"false").map(|()| Json::Bool(false)),
            b'{' => self.skipped(b'{', b'}').map(|()| Json::Nested),
            b'[' => self.skipped(b'[', b']').map(|()| Json::Nested),
            _ => self.number().map(Json::Number),
        }
    }

    fn object(mut self) -> std::result::Result<Vec<(String, Json)>, usize> {
        let mut fields = Vec::new();
        self.eat(b'{').ok_or(self.at)?;
        self.space();
        if self.eat(b'}').is_none() {
            loop {
                let name = self.text().ok_or(self.at)?;
                self.eat(b':').ok_or(self.at)?;
                let value = self.value().ok_or(self.at)?;
                fields.push((name, value));
                if self.eat(b',').is_some() {
                    continue;
                }
                self.eat(b'}').ok_or(self.at)?;
                break;
            }
        }
        self.space();
        if self.at != self.bytes.len() {
            return Err(self.at);
        }
        Ok(fields)
    }
}

fn object_of(text: &str) -> std::result::Result<Vec<(String, Json)>, usize> {
    Parser {
        bytes: text.as_bytes(),
        at: 0,
    }
    .object()
}

fn valued(json: &Json) -> Value {
    match json {
        Json::Null | Json::Nested => Value::Null,
        Json::Bool(true) => Value::Text(TRUE.to_owned()),
        Json::Bool(false) => Value::Text(FALSE.to_owned()),
        Json::Number(number) => number
            .parse::<i64>()
            .map(Value::Integer)
            .or_else(|_| number.parse::<f64>().map(Value::Real))
            .unwrap_or(Value::Null),
        Json::Text(text) => Value::Text(text.clone()),
    }
}

fn projected(
    table: &'static Table,
    text: Option<String>,
    sequence: u64,
) -> Result<Option<Vec<Value>>> {
    let Some(text) = text else {
        return Ok(None);
    };
    let fields = object_of(&text).map_err(|at| Error::PacketChange {
        sequence,
        table: table.name,
        at,
    })?;
    let mut row = Vec::with_capacity(table.named.len());
    for column in table.named {
        let Some((_, value)) = fields.iter().find(|(name, _)| name == column) else {
            return Err(Error::PacketColumn {
                sequence,
                table: table.name,
                column,
            });
        };
        row.push(valued(value));
    }
    Ok(Some(row))
}

pub struct Packet {
    pub sequence: u64,
    pub schema: u32,
    changes: Vec<Change>,
}

impl Packet {
    pub fn changes(&self) -> usize {
        self.changes.len()
    }
}

fn sequence_in(entry: impl Read) -> Option<u64> {
    let mut text = String::new();
    entry
        .take(LONGEST_SEQUENCE_FILE)
        .read_to_string(&mut text)
        .ok()?;
    text.trim().parse().ok()
}

fn table_of(named: &str) -> Option<&'static Table> {
    let bare = named.replace('"', "");
    table_called(bare.strip_prefix(SCHEMA_OF_TABLES)?)
}

type Pending = (i64, i64, Change);

fn pending_in(rows: impl BufRead, sequence: u64) -> Result<Vec<Change>> {
    let mut pending: Vec<Pending> = Vec::new();
    for line in rows.split(b'\n') {
        let mut split = Vec::with_capacity(PENDING_COLUMNS_AT_LEAST + 2);
        let line = line.map_err(|_| Error::PacketPart {
            sequence,
            part: PENDING_DATA,
        })?;
        if line.is_empty() {
            continue;
        }
        fields(&line, &mut split);
        if split.len() < PENDING_COLUMNS_AT_LEAST {
            return Err(Error::PacketColumns {
                sequence,
                found: split.len(),
                wanted: PENDING_COLUMNS_AT_LEAST,
            });
        }
        let Some(table) = unescaped(split[1]).as_deref().and_then(table_of) else {
            continue;
        };
        let malformed = || Error::PacketChange {
            sequence,
            table: table.name,
            at: 0,
        };
        let seq: i64 = unescaped(split[0])
            .and_then(|seq| seq.parse().ok())
            .ok_or_else(malformed)?;
        let xid: i64 = unescaped(split[3])
            .and_then(|xid| xid.parse().ok())
            .ok_or_else(malformed)?;
        let old = projected(table, unescaped(split[4]), sequence)?;
        let new = projected(table, unescaped(split[5]), sequence)?;
        pending.push((xid, seq, Change { table, old, new }));
    }
    let mut last_of: BTreeMap<i64, i64> = BTreeMap::new();
    for (xid, seq, _) in &pending {
        let last = last_of.entry(*xid).or_insert(*seq);
        *last = (*last).max(*seq);
    }
    pending.sort_by_key(|(xid, seq, _)| (last_of.get(xid).copied().unwrap_or(*seq), *seq));
    Ok(pending.into_iter().map(|(_, _, change)| change).collect())
}

pub fn packet_read(packed: impl Read, asked: u64) -> Result<Packet> {
    let decoded = bzip2::read::MultiBzDecoder::new(packed);
    let mut archive = tar::Archive::new(decoded);
    let entries = archive.entries().map_err(|_| Error::PacketPart {
        sequence: asked,
        part: PENDING_DATA,
    })?;
    let mut sequence = None;
    let mut schema = None;
    let mut form = None;
    let mut changes = None;
    for entry in entries {
        let entry = entry.map_err(|_| Error::PacketPart {
            sequence: asked,
            part: PENDING_DATA,
        })?;
        let name = entry
            .path()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        match name.as_str() {
            REPLICATION_SEQUENCE => sequence = sequence_in(entry),
            SCHEMA_SEQUENCE => schema = sequence_in(entry),
            DBMIRROR_VERSION => form = sequence_in(entry),
            PENDING_DATA => changes = Some(pending_in(BufReader::new(entry), asked)?),
            _ => {}
        }
    }
    let sequence = sequence.ok_or(Error::PacketPart {
        sequence: asked,
        part: REPLICATION_SEQUENCE,
    })?;
    if sequence != asked {
        return Err(Error::PacketSequence {
            asked,
            found: sequence,
        });
    }
    if form != Some(DBMIRROR2) {
        return Err(Error::PacketForm { sequence });
    }
    let schema = schema
        .and_then(|schema| u32::try_from(schema).ok())
        .ok_or(Error::PacketPart {
            sequence,
            part: SCHEMA_SEQUENCE,
        })?;
    Ok(Packet {
        sequence,
        schema,
        changes: changes.unwrap_or_default(),
    })
}

#[derive(Default)]
pub(crate) struct Touched {
    documents: BTreeSet<(u8, i64)>,
    gids: BTreeMap<(u8, i64), [u8; MBID_BYTES]>,
}

impl Touched {
    fn touch(&mut self, kind: Entity, id: Option<i64>) {
        if let Some(id) = id {
            self.documents.insert((kind.code(), id));
        }
    }

    fn touch_all(&mut self, kind: Entity, ids: Vec<i64>) {
        for id in ids {
            self.touch(kind, Some(id));
        }
    }
}

fn integer(table: &Table, row: &[Value], column: &str) -> Option<i64> {
    let at = table.named.iter().position(|named| *named == column)?;
    match row.get(at)? {
        Value::Integer(value) => Some(*value),
        _ => None,
    }
}

fn texted<'a>(table: &Table, row: &'a [Value], column: &str) -> Option<&'a str> {
    let at = table.named.iter().position(|named| *named == column)?;
    match row.get(at)? {
        Value::Text(text) => Some(text),
        _ => None,
    }
}

fn ids(tables: &Connection, sql: &str, id: Option<i64>) -> Result<Vec<i64>> {
    let Some(id) = id else {
        return Ok(Vec::new());
    };
    let failed = store_read(Store::Tables);
    let mut statement = tables.prepare_cached(sql).map_err(&failed)?;
    let found = statement
        .query_map([id], |row| row.get::<_, Option<i64>>(0))
        .map_err(&failed)?;
    Ok(found
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(&failed)?
        .into_iter()
        .flatten()
        .collect())
}

fn one(tables: &Connection, sql: &str, id: Option<i64>) -> Result<Option<i64>> {
    Ok(ids(tables, sql, id)?.into_iter().next())
}

const RELEASES_OF_A_RECORDING: &str = "SELECT DISTINCT m.release FROM track t \
     JOIN medium m ON m.id = t.medium WHERE t.recording = ?1";
const RECORDINGS_OF_A_RELEASE: &str = "SELECT DISTINCT t.recording FROM medium m \
     JOIN track t ON t.medium = m.id WHERE m.release = ?1";
const RECORDINGS_OF_A_MEDIUM: &str = "SELECT DISTINCT recording FROM track WHERE medium = ?1";
const RELEASE_OF_A_MEDIUM: &str = "SELECT release FROM medium WHERE id = ?1";
const GROUP_OF_A_RELEASE: &str = "SELECT release_group FROM release WHERE id = ?1";
const RELEASES_OF_A_GROUP: &str = "SELECT id FROM release WHERE release_group = ?1";
const CREDIT_OF_A_GROUP: &str = "SELECT artist_credit FROM release_group WHERE id = ?1";
const ARTISTS_OF_A_CREDIT: &str = "SELECT artist FROM artist_credit_name WHERE artist_credit = ?1";
const RECORDINGS_OF_A_CREDIT: &str = "SELECT id FROM recording WHERE artist_credit = ?1";
const RELEASES_OF_A_CREDIT: &str = "SELECT id FROM release WHERE artist_credit = ?1";
const GROUPS_OF_A_CREDIT: &str = "SELECT id FROM release_group WHERE artist_credit = ?1";
const RELEASES_OF_A_TRACK_CREDIT: &str = "SELECT DISTINCT m.release FROM track t \
     JOIN medium m ON m.id = t.medium WHERE t.artist_credit = ?1";
const RELEASES_OF_A_LABEL: &str = "SELECT release FROM release_label WHERE label = ?1";

const LINKED_TO_A_URL: [(&str, Entity); 4] = [
    (
        "SELECT entity0 FROM l_artist_url WHERE entity1 = ?1",
        Entity::Artist,
    ),
    (
        "SELECT entity0 FROM l_release_url WHERE entity1 = ?1",
        Entity::Release,
    ),
    (
        "SELECT entity0 FROM l_release_group_url WHERE entity1 = ?1",
        Entity::Group,
    ),
    (
        "SELECT entity0 FROM l_recording_url WHERE entity1 = ?1",
        Entity::Recording,
    ),
];

fn recordings_and_their_releases(
    tables: &Connection,
    recording: Option<i64>,
    touched: &mut Touched,
) -> Result<()> {
    touched.touch(Entity::Recording, recording);
    touched.touch_all(
        Entity::Release,
        ids(tables, RELEASES_OF_A_RECORDING, recording)?,
    );
    Ok(())
}

fn a_release_and_what_it_stands_in(
    tables: &Connection,
    release: Option<i64>,
    touched: &mut Touched,
) -> Result<()> {
    touched.touch(Entity::Release, release);
    touched.touch(Entity::Group, one(tables, GROUP_OF_A_RELEASE, release)?);
    touched.touch_all(
        Entity::Recording,
        ids(tables, RECORDINGS_OF_A_RELEASE, release)?,
    );
    Ok(())
}

fn the_artists_of_a_group(
    tables: &Connection,
    group: Option<i64>,
    touched: &mut Touched,
) -> Result<()> {
    let credit = one(tables, CREDIT_OF_A_GROUP, group)?;
    touched.touch_all(Entity::Artist, ids(tables, ARTISTS_OF_A_CREDIT, credit)?);
    Ok(())
}

fn linked(
    tables: &Connection,
    kind: Entity,
    entity: Option<i64>,
    touched: &mut Touched,
) -> Result<()> {
    match kind {
        Entity::Recording => touched.touch_all(
            Entity::Release,
            ids(tables, RELEASES_OF_A_RECORDING, entity)?,
        ),
        other => touched.touch(other, entity),
    }
    Ok(())
}

pub(crate) fn touched_by(
    tables: &Connection,
    table: &Table,
    row: &[Value],
    touched: &mut Touched,
) -> Result<()> {
    let id = integer(table, row, "id");
    let gid = texted(table, row, "gid").and_then(gid_bytes);
    let main = match table.name {
        "artist" => Some(Entity::Artist),
        "recording" => Some(Entity::Recording),
        "release" => Some(Entity::Release),
        "release_group" => Some(Entity::Group),
        _ => None,
    };
    if let (Some(kind), Some(id), Some(gid)) = (main, id, gid) {
        touched.gids.insert((kind.code(), id), gid);
    }
    match table.name {
        "artist" => touched.touch(Entity::Artist, id),
        "artist_alias" | "artist_tag" => {
            touched.touch(Entity::Artist, integer(table, row, "artist"));
        }
        "l_artist_url" => linked(
            tables,
            Entity::Artist,
            integer(table, row, "entity0"),
            touched,
        )?,
        "l_release_url" => linked(
            tables,
            Entity::Release,
            integer(table, row, "entity0"),
            touched,
        )?,
        "l_release_group_url" => {
            linked(
                tables,
                Entity::Group,
                integer(table, row, "entity0"),
                touched,
            )?;
        }
        "l_recording_url" => {
            linked(
                tables,
                Entity::Recording,
                integer(table, row, "entity0"),
                touched,
            )?;
        }
        "url" => {
            for (query, kind) in LINKED_TO_A_URL {
                for entity in ids(tables, query, id)? {
                    linked(tables, kind, Some(entity), touched)?;
                }
            }
        }
        "artist_credit_name" => {
            let credit = integer(table, row, "artist_credit");
            touched.touch(Entity::Artist, integer(table, row, "artist"));
            touched.touch_all(
                Entity::Recording,
                ids(tables, RECORDINGS_OF_A_CREDIT, credit)?,
            );
            touched.touch_all(Entity::Release, ids(tables, RELEASES_OF_A_CREDIT, credit)?);
            touched.touch_all(Entity::Group, ids(tables, GROUPS_OF_A_CREDIT, credit)?);
            touched.touch_all(
                Entity::Release,
                ids(tables, RELEASES_OF_A_TRACK_CREDIT, credit)?,
            );
        }
        "recording" => recordings_and_their_releases(tables, id, touched)?,
        "isrc" => {
            recordings_and_their_releases(tables, integer(table, row, "recording"), touched)?;
        }
        "track" => {
            touched.touch(
                Entity::Release,
                one(tables, RELEASE_OF_A_MEDIUM, integer(table, row, "medium"))?,
            );
            touched.touch(Entity::Recording, integer(table, row, "recording"));
        }
        "medium" => {
            let release = integer(table, row, "release");
            touched.touch(Entity::Release, release);
            touched.touch(Entity::Group, one(tables, GROUP_OF_A_RELEASE, release)?);
            touched.touch_all(Entity::Recording, ids(tables, RECORDINGS_OF_A_MEDIUM, id)?);
        }
        "release" => {
            a_release_and_what_it_stands_in(tables, id, touched)?;
            touched.touch(Entity::Group, integer(table, row, "release_group"));
        }
        "release_country" | "release_unknown_country" => {
            a_release_and_what_it_stands_in(tables, integer(table, row, "release"), touched)?;
        }
        "release_label" => touched.touch(Entity::Release, integer(table, row, "release")),
        "release_meta" => touched.touch(Entity::Release, id),
        "label" => touched.touch_all(Entity::Release, ids(tables, RELEASES_OF_A_LABEL, id)?),
        "release_group" => {
            touched.touch(Entity::Group, id);
            for release in ids(tables, RELEASES_OF_A_GROUP, id)? {
                touched.touch(Entity::Release, Some(release));
                touched.touch_all(
                    Entity::Recording,
                    ids(tables, RECORDINGS_OF_A_RELEASE, Some(release))?,
                );
            }
            let credit = integer(table, row, "artist_credit");
            touched.touch_all(Entity::Artist, ids(tables, ARTISTS_OF_A_CREDIT, credit)?);
        }
        "release_group_meta" => {
            touched.touch(Entity::Group, id);
            the_artists_of_a_group(tables, id, touched)?;
        }
        "release_group_secondary_type_join" => {
            let group = integer(table, row, "release_group");
            touched.touch(Entity::Group, group);
            for release in ids(tables, RELEASES_OF_A_GROUP, group)? {
                touched.touch_all(
                    Entity::Recording,
                    ids(tables, RECORDINGS_OF_A_RELEASE, Some(release))?,
                );
            }
            the_artists_of_a_group(tables, group, touched)?;
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn changed(tables: &Connection, change: &Change, touched: &mut Touched) -> Result<()> {
    if let Some(old) = &change.old {
        touched_by(tables, change.table, old, touched)?;
    }
    applied(tables, change)?;
    if let Some(new) = &change.new {
        touched_by(tables, change.table, new, touched)?;
    }
    Ok(())
}

pub(crate) fn rewritten(
    tables: &Connection,
    mirror: &Mirror,
    amending: &Amending<'_>,
    touched: &Touched,
    building: &Building,
) -> Result<u64> {
    let failed = store_read(Store::Tables);
    let looked_up = LookedUp::asking(tables)?;
    let mut builders = Builders::prepared(tables)?;
    let mut rewritten = 0_u64;
    for (code, id) in &touched.documents {
        let Some(kind) = Entity::coded(*code) else {
            continue;
        };
        let document = {
            let mut statement = tables
                .prepare_cached(&format!("{} WHERE id = ?1", rows_of(kind)))
                .map_err(&failed)?;
            let mut rows = statement.query([id]).map_err(&failed)?;
            match rows.next().map_err(&failed)? {
                Some(row) => builders.built(kind, &looked_up, row)?,
                None => None,
            }
        };
        match document {
            Some(document) => amending.rewritten(kind, &document)?,
            None => {
                if let Some(gid) = touched.gids.get(&(*code, *id)) {
                    let built_holds = mirror.holds_built(&mbid_of(*gid), kind)?;
                    amending.gone(kind, gid, built_holds)?;
                }
            }
        }
        rewritten += 1;
        if rewritten.is_multiple_of(1024) {
            building.looked_at()?;
        }
    }
    Ok(rewritten)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Replicated {
    pub from: u64,
    pub to: u64,
    pub changes: u64,
    pub documents: u64,
}

pub fn replicated(
    folder: &Path,
    mirror: &Mirror,
    mut packet: impl FnMut(u64) -> Result<Option<Packet>>,
    building: &Building,
    _held: &Held,
) -> Result<Replicated> {
    let (tables, kept) = opened(&folder.join(TABLES_FILE))?;
    if kept.dump != mirror.made().dump {
        return Err(Error::TablesOfAnotherExport {
            tables: kept.dump,
            mirror: mirror.made().dump.clone(),
        });
    }
    let amended_path = folder.join(AMENDED_FILE);
    made_at(&amended_path, &kept.dump, kept.sequence, now())?;
    tables
        .execute(
            "ATTACH DATABASE ?1 AS amended",
            [amended_path.to_string_lossy().into_owned()],
        )
        .map_err(Error::store(Store::Amended, StoreOp::Open))?;
    let amending = Amending {
        connection: &tables,
        dictionaries: mirror.dictionaries(),
    };
    let mut done = Replicated {
        from: kept.sequence,
        to: kept.sequence,
        ..Replicated::default()
    };
    let mut ended = false;
    while !ended {
        building.looked_at()?;
        let mut batch = Vec::new();
        while batch.len() < PACKETS_A_BATCH {
            match packet(done.to + 1 + batch.len() as u64)? {
                Some(read) => {
                    if read.schema != kept.schema {
                        ended = true;
                        if batch.is_empty() {
                            return Err(Error::PacketSchema {
                                sequence: read.sequence,
                                found: read.schema,
                                reads: kept.schema,
                            });
                        }
                        break;
                    }
                    batch.push(read);
                }
                None => {
                    ended = true;
                    break;
                }
            }
        }
        let Some(last) = batch.last().map(|read| read.sequence) else {
            break;
        };
        tables
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(Error::store(Store::Tables, StoreOp::Write))?;
        let outcome = (|| {
            let mut touched = Touched::default();
            let mut changes = 0_u64;
            for read in &batch {
                for change in &read.changes {
                    changed(&tables, change, &mut touched)?;
                    changes += 1;
                }
                building.looked_at()?;
            }
            let documents = rewritten(&tables, mirror, &amending, &touched, building)?;
            sequenced_at(&tables, last)?;
            amending.sequenced_at(last, now())?;
            Ok::<_, Error>((changes, documents))
        })();
        match outcome {
            Ok((changes, documents)) => {
                tables
                    .execute_batch("COMMIT")
                    .map_err(Error::store(Store::Tables, StoreOp::Write))?;
                done.to = last;
                done.changes += changes;
                done.documents += documents;
                tracing::info!(
                    sequence = last,
                    changes,
                    documents,
                    "replication packets applied to the copy of MusicBrainz"
                );
            }
            Err(error) => {
                let _ = tables.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_as_json_reads_its_text_numbers_booleans_and_nulls() {
        let fields = object_of(
            r#"{"id":12,"name":"Café \"live\"\n","ended":true,"type":null,"x":{"a":[1,{"b":2}]},"len":-3.5e2,"emoji":"🎵"}"#,
        )
        .expect("an object");

        assert_eq!(
            fields,
            [
                ("id".to_owned(), Json::Number("12".to_owned())),
                (
                    "name".to_owned(),
                    Json::Text("Caf\u{e9} \"live\"\n".to_owned())
                ),
                ("ended".to_owned(), Json::Bool(true)),
                ("type".to_owned(), Json::Null),
                ("x".to_owned(), Json::Nested),
                ("len".to_owned(), Json::Number("-3.5e2".to_owned())),
                ("emoji".to_owned(), Json::Text("\u{1f3b5}".to_owned())),
            ]
        );
        assert_eq!(valued(&Json::Bool(false)), Value::Text("f".to_owned()));
        assert_eq!(valued(&Json::Number("7".to_owned())), Value::Integer(7));
    }

    #[test]
    fn malformed_json_is_refused_at_the_byte_it_went_wrong() {
        assert!(object_of(r#"{"id":12"#).is_err());
        assert!(object_of(r#"{"id":12} trailing"#).is_err());
        assert!(object_of(r#"{"id":"\ud83c"}"#).is_err());
        assert!(object_of(r#"{"id":tru}"#).is_err());
        assert_eq!(object_of("{}"), Ok(Vec::new()));
    }

    #[test]
    fn changes_are_ordered_by_their_transactions_last_change_then_their_own() {
        let lines = concat!(
            "5\tmusicbrainz.label\tu\t200\t{\"id\":1,\"name\":\"B\"}\t{\"id\":1,\"name\":\"C\"}\t\\N\t\\N\n",
            "3\tmusicbrainz.label\ti\t100\t\\N\t{\"id\":1,\"name\":\"A\"}\t\\N\t\\N\n",
            "4\tmusicbrainz.label\tu\t100\t{\"id\":1,\"name\":\"A\"}\t{\"id\":1,\"name\":\"B\"}\t\\N\t\\N\n",
            "6\tmusicbrainz.editor\ti\t300\t\\N\t{\"id\":9}\t\\N\t\\N\n",
        );

        let changes = pending_in(lines.as_bytes(), 1).expect("changes");

        let names: Vec<Option<Value>> = changes
            .iter()
            .map(|change| change.new.as_ref().and_then(|row| row.get(1).cloned()))
            .collect();
        assert_eq!(
            names,
            [
                Some(Value::Text("A".to_owned())),
                Some(Value::Text("B".to_owned())),
                Some(Value::Text("C".to_owned())),
            ]
        );
    }

    #[test]
    fn a_change_lacking_a_column_kept_is_refused_and_names_it() {
        let line = "1\tmusicbrainz.label\ti\t1\t\\N\t{\"id\":1}\t\\N\t\\N\n";

        let refused = pending_in(line.as_bytes(), 9).err();

        assert!(matches!(
            refused,
            Some(Error::PacketColumn {
                sequence: 9,
                table: "label",
                column: "name"
            })
        ));
    }
}
