use std::{borrow::Cow, collections::HashMap};

use resonate_core::Mbid;
use resonate_library::Credit;
use rusqlite::{Connection, OptionalExtension};

use super::import::Building;
use crate::{
    Error, Result, Store, StoreOp,
    wire::{MBID_BYTES, mbid_of},
};

pub(crate) const NAMED_TABLES: [&str; 10] = [
    "artist_type",
    "gender",
    "area",
    "release_status",
    "release_group_primary_type",
    "release_group_secondary_type",
    "medium_format",
    "link_type",
    "label",
    "tag",
];

#[derive(Clone)]
pub(crate) struct CreditRow {
    artist: u32,
    name: Box<str>,
    joined_by: Box<str>,
}

#[derive(Clone)]
pub(crate) struct GroupInfo {
    pub gid: [u8; MBID_BYTES],
    pub title: Box<str>,
    pub kind: Option<u32>,
    pub secondary: Box<[u32]>,
    pub first_released: Option<Box<str>>,
}

#[derive(Clone)]
pub(crate) struct ReleaseInfo {
    pub gid: [u8; MBID_BYTES],
    pub title: Box<str>,
    pub group: u32,
    pub status: Option<u32>,
    pub date: Option<Box<str>>,
    pub country: Option<Box<str>>,
    pub discs: Box<[u32]>,
}

struct Held {
    artists: Vec<Option<[u8; MBID_BYTES]>>,
    credits: Vec<Option<Box<[CreditRow]>>>,
    groups: Vec<Option<GroupInfo>>,
    releases: Vec<Option<ReleaseInfo>>,
}

enum Rest<'a> {
    Held(Held),
    Asked(&'a Connection),
}

pub(crate) struct LookedUp<'a> {
    pub named: HashMap<(&'static str, u32), Box<str>>,
    rest: Rest<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EventOrder {
    year: i64,
    month: i64,
    day: i64,
    country: i64,
}

impl EventOrder {
    fn of(year: Option<i64>, month: Option<i64>, day: Option<i64>, country: Option<i64>) -> Self {
        Self {
            year: year.unwrap_or(i64::MAX),
            month: month.unwrap_or(i64::MAX),
            day: day.unwrap_or(i64::MAX),
            country: country.unwrap_or(i64::MAX),
        }
    }
}

type Event = (EventOrder, Option<String>, Option<u32>);

const EVENTS_OF_EVERY_RELEASE: [&str; 2] = [
    "SELECT release, y, m, d, country FROM release_country",
    "SELECT release, y, m, d, NULL FROM release_unknown_country",
];

const EVENTS_OF_A_RELEASE: &str = "SELECT release, y, m, d, country FROM release_country \
     WHERE release = ?1 UNION ALL \
     SELECT release, y, m, d, NULL FROM release_unknown_country WHERE release = ?1";

pub(crate) fn at(id: i64) -> usize {
    usize::try_from(id).unwrap_or(0)
}

pub(crate) fn small(id: i64) -> u32 {
    u32::try_from(id).unwrap_or(0)
}

fn placed<T>(slots: &mut Vec<Option<T>>, id: i64, value: T) {
    let at = at(id);
    if slots.len() <= at {
        slots.resize_with(at + 1, || None);
    }
    slots[at] = Some(value);
}

pub fn gid_bytes(gid: &str) -> Option<[u8; MBID_BYTES]> {
    let mbid = Mbid::new(gid).ok()?;
    Some(crate::wire::mbid_bytes(&mbid))
}

pub fn dated(year: Option<i64>, month: Option<i64>, day: Option<i64>) -> Option<String> {
    let year = year?;
    Some(match (month, day) {
        (Some(month), Some(day)) => format!("{year:04}-{month:02}-{day:02}"),
        (Some(month), None) => format!("{year:04}-{month:02}"),
        _ => format!("{year:04}"),
    })
}

pub(crate) fn store_read(store: Store) -> impl Fn(rusqlite::Error) -> Error {
    move |source| Error::Store {
        store,
        op: StoreOp::Read,
        source: Box::new(source),
    }
}

fn earliest(events: &mut HashMap<u32, Event>, row: &rusqlite::Row<'_>) -> rusqlite::Result<()> {
    let release = small(row.get(0)?);
    let year: Option<i64> = row.get(1)?;
    let month: Option<i64> = row.get(2)?;
    let day: Option<i64> = row.get(3)?;
    let country: Option<i64> = row.get(4)?;
    let order = EventOrder::of(year, month, day, country);
    if events
        .get(&release)
        .is_none_or(|(held, _, _)| order < *held)
    {
        events.insert(
            release,
            (order, dated(year, month, day), country.map(small)),
        );
    }
    Ok(())
}

fn named_in(staging: &Connection) -> Result<HashMap<(&'static str, u32), Box<str>>> {
    let failed = store_read(Store::Staging);
    let mut named = HashMap::new();
    for table in NAMED_TABLES {
        let mut statement = staging
            .prepare(&format!("SELECT id, name FROM {table}"))
            .map_err(&failed)?;
        let mut rows = statement.query([]).map_err(&failed)?;
        while let Some(row) = rows.next().map_err(&failed)? {
            let id: i64 = row.get(0).map_err(&failed)?;
            let name: Option<String> = row.get(1).map_err(&failed)?;
            if let Some(name) = name {
                named.insert((table, small(id)), name.into_boxed_str());
            }
        }
    }
    let mut statement = staging
        .prepare("SELECT area, code FROM iso_3166_1")
        .map_err(&failed)?;
    let mut rows = statement.query([]).map_err(&failed)?;
    while let Some(row) = rows.next().map_err(&failed)? {
        let area: i64 = row.get(0).map_err(&failed)?;
        let code: Option<String> = row.get(1).map_err(&failed)?;
        if let Some(code) = code {
            named.insert(("iso_3166_1", small(area)), code.into_boxed_str());
        }
    }
    Ok(named)
}

fn credit_row(row: &rusqlite::Row<'_>, from: usize) -> rusqlite::Result<CreditRow> {
    Ok(CreditRow {
        artist: small(row.get(from)?),
        name: row
            .get::<_, Option<String>>(from + 1)?
            .unwrap_or_default()
            .into_boxed_str(),
        joined_by: row
            .get::<_, Option<String>>(from + 2)?
            .unwrap_or_default()
            .into_boxed_str(),
    })
}

fn sorted(mut kinds: Vec<u32>) -> Box<[u32]> {
    kinds.sort_unstable();
    kinds.into_boxed_slice()
}

impl LookedUp<'static> {
    pub(crate) fn read(staging: &Connection, building: &Building) -> Result<Self> {
        let failed = store_read(Store::Staging);
        let named = named_in(staging)?;
        building.looked_at()?;

        let mut artists = Vec::new();
        {
            let mut statement = staging
                .prepare("SELECT id, gid FROM artist")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let id: i64 = row.get(0).map_err(&failed)?;
                let gid: String = row.get(1).map_err(&failed)?;
                if let Some(gid) = gid_bytes(&gid) {
                    placed(&mut artists, id, gid);
                }
            }
        }
        building.looked_at()?;

        let mut credits: Vec<Option<Box<[CreditRow]>>> = Vec::new();
        {
            let mut statement = staging
                .prepare(
                    "SELECT artist_credit, artist, name, join_phrase FROM artist_credit_name \
                     ORDER BY artist_credit, position",
                )
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            let mut current: Option<(i64, Vec<CreditRow>)> = None;
            while let Some(row) = rows.next().map_err(&failed)? {
                let credit: i64 = row.get(0).map_err(&failed)?;
                let named = credit_row(row, 1).map_err(&failed)?;
                match &mut current {
                    Some((held, rows)) if *held == credit => rows.push(named),
                    _ => {
                        if let Some((held, rows)) = current.take() {
                            placed(&mut credits, held, rows.into_boxed_slice());
                        }
                        current = Some((credit, vec![named]));
                    }
                }
            }
            if let Some((held, rows)) = current {
                placed(&mut credits, held, rows.into_boxed_slice());
            }
        }
        building.looked_at()?;

        let mut secondary: HashMap<u32, Vec<u32>> = HashMap::new();
        {
            let mut statement = staging
                .prepare(
                    "SELECT release_group, secondary_type FROM release_group_secondary_type_join",
                )
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                secondary
                    .entry(small(row.get(0).map_err(&failed)?))
                    .or_default()
                    .push(small(row.get(1).map_err(&failed)?));
            }
        }
        let mut first_released: HashMap<u32, String> = HashMap::new();
        {
            let mut statement = staging
                .prepare("SELECT id, y, m, d FROM release_group_meta")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                if let Some(date) = dated(
                    row.get(1).map_err(&failed)?,
                    row.get(2).map_err(&failed)?,
                    row.get(3).map_err(&failed)?,
                ) {
                    first_released.insert(small(row.get(0).map_err(&failed)?), date);
                }
            }
        }
        let mut groups = Vec::new();
        {
            let mut statement = staging
                .prepare("SELECT id, gid, name, type FROM release_group")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let id: i64 = row.get(0).map_err(&failed)?;
                let gid: String = row.get(1).map_err(&failed)?;
                let Some(gid) = gid_bytes(&gid) else {
                    continue;
                };
                let kind: Option<i64> = row.get(3).map_err(&failed)?;
                placed(
                    &mut groups,
                    id,
                    GroupInfo {
                        gid,
                        title: row
                            .get::<_, Option<String>>(2)
                            .map_err(&failed)?
                            .unwrap_or_default()
                            .into_boxed_str(),
                        kind: kind.map(small),
                        secondary: sorted(secondary.remove(&small(id)).unwrap_or_default()),
                        first_released: first_released
                            .remove(&small(id))
                            .map(String::into_boxed_str),
                    },
                );
            }
        }
        building.looked_at()?;

        let mut events: HashMap<u32, Event> = HashMap::new();
        for query in EVENTS_OF_EVERY_RELEASE {
            let mut statement = staging.prepare(query).map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                earliest(&mut events, row).map_err(&failed)?;
            }
        }
        let mut discs: HashMap<u32, Vec<u32>> = HashMap::new();
        {
            let mut statement = staging
                .prepare("SELECT release, track_count FROM medium ORDER BY release, position")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let count: Option<i64> = row.get(1).map_err(&failed)?;
                discs
                    .entry(small(row.get(0).map_err(&failed)?))
                    .or_default()
                    .push(small(count.unwrap_or(0)));
            }
        }
        building.looked_at()?;
        let mut releases = Vec::new();
        {
            let mut statement = staging
                .prepare("SELECT id, gid, name, release_group, status FROM release")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let id: i64 = row.get(0).map_err(&failed)?;
                let gid: String = row.get(1).map_err(&failed)?;
                let Some(gid) = gid_bytes(&gid) else {
                    continue;
                };
                let status: Option<i64> = row.get(4).map_err(&failed)?;
                let (date, country) = events
                    .remove(&small(id))
                    .map(|(_, date, country)| (date, country))
                    .unwrap_or_default();
                let country = country
                    .and_then(|area| named.get(&("iso_3166_1", area)))
                    .cloned();
                placed(
                    &mut releases,
                    id,
                    ReleaseInfo {
                        gid,
                        title: row
                            .get::<_, Option<String>>(2)
                            .map_err(&failed)?
                            .unwrap_or_default()
                            .into_boxed_str(),
                        group: small(row.get(3).map_err(&failed)?),
                        status: status.map(small),
                        date: date.map(String::into_boxed_str),
                        country,
                        discs: discs
                            .remove(&small(id))
                            .unwrap_or_default()
                            .into_boxed_slice(),
                    },
                );
            }
        }
        building.looked_at()?;

        Ok(Self {
            named,
            rest: Rest::Held(Held {
                artists,
                credits,
                groups,
                releases,
            }),
        })
    }
}

impl<'a> LookedUp<'a> {
    pub(crate) fn asking(tables: &'a Connection) -> Result<Self> {
        Ok(Self {
            named: named_in(tables)?,
            rest: Rest::Asked(tables),
        })
    }

    pub(crate) fn name(&self, table: &'static str, id: Option<i64>) -> Option<String> {
        id.and_then(|id| self.named.get(&(table, small(id))))
            .map(|name| name.to_string())
    }

    fn credit_rows(&self, id: i64) -> Result<Cow<'_, [CreditRow]>> {
        match &self.rest {
            Rest::Held(held) => Ok(match held.credits.get(at(id)) {
                Some(Some(rows)) => Cow::Borrowed(&rows[..]),
                _ => Cow::Owned(Vec::new()),
            }),
            Rest::Asked(tables) => {
                let failed = store_read(Store::Staging);
                let mut statement = tables
                    .prepare_cached(
                        "SELECT artist, name, join_phrase FROM artist_credit_name \
                         WHERE artist_credit = ?1 ORDER BY position",
                    )
                    .map_err(&failed)?;
                let rows = statement
                    .query_map([id], |row| credit_row(row, 0))
                    .map_err(&failed)?;
                Ok(Cow::Owned(
                    rows.collect::<rusqlite::Result<Vec<_>>>()
                        .map_err(&failed)?,
                ))
            }
        }
    }

    fn artist_gid(&self, id: u32) -> Result<Option<[u8; MBID_BYTES]>> {
        match &self.rest {
            Rest::Held(held) => Ok(held.artists.get(id as usize).copied().flatten()),
            Rest::Asked(tables) => {
                let gid: Option<String> = tables
                    .prepare_cached("SELECT gid FROM artist WHERE id = ?1")
                    .and_then(|mut statement| {
                        statement.query_row([id], |row| row.get(0)).optional()
                    })
                    .map_err(store_read(Store::Staging))?;
                Ok(gid.as_deref().and_then(gid_bytes))
            }
        }
    }

    pub(crate) fn credit(&self, id: i64) -> Result<Vec<Credit>> {
        let rows = self.credit_rows(id)?;
        let mut credit = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            credit.push(Credit {
                name: row.name.to_string(),
                joined_by: row.joined_by.to_string(),
                mbid: self.artist_gid(row.artist)?.map(mbid_of),
            });
        }
        Ok(credit)
    }

    pub(crate) fn credited_as(&self, id: i64) -> Result<String> {
        let rows = self.credit_rows(id)?;
        let mut credited = String::new();
        for row in rows.iter() {
            credited.push_str(&row.name);
            credited.push_str(&row.joined_by);
        }
        Ok(credited)
    }

    pub(crate) fn group(&self, id: u32) -> Result<Option<Cow<'_, GroupInfo>>> {
        match &self.rest {
            Rest::Held(held) => Ok(held
                .groups
                .get(id as usize)
                .and_then(Option::as_ref)
                .map(Cow::Borrowed)),
            Rest::Asked(tables) => Ok(asked_group(tables, id)?.map(Cow::Owned)),
        }
    }

    pub(crate) fn release(&self, id: i64) -> Result<Option<Cow<'_, ReleaseInfo>>> {
        match &self.rest {
            Rest::Held(held) => Ok(held
                .releases
                .get(at(id))
                .and_then(Option::as_ref)
                .map(Cow::Borrowed)),
            Rest::Asked(tables) => Ok(self.asked_release(tables, id)?.map(Cow::Owned)),
        }
    }

    pub(crate) fn kind_of(&self, group: &GroupInfo) -> Option<String> {
        group
            .kind
            .and_then(|kind| self.named.get(&("release_group_primary_type", kind)))
            .map(|name| name.to_string())
    }

    pub(crate) fn secondary_of(&self, group: &GroupInfo) -> Vec<String> {
        group
            .secondary
            .iter()
            .filter_map(|kind| self.named.get(&("release_group_secondary_type", *kind)))
            .map(|name| name.to_string())
            .collect()
    }

    pub(crate) fn status_of(&self, release: &ReleaseInfo) -> Option<String> {
        release
            .status
            .and_then(|status| self.named.get(&("release_status", status)))
            .map(|name| name.to_string())
    }

    fn asked_release(&self, tables: &Connection, id: i64) -> Result<Option<ReleaseInfo>> {
        let failed = store_read(Store::Staging);
        let row: Option<(String, Option<String>, i64, Option<i64>)> = tables
            .prepare_cached("SELECT gid, name, release_group, status FROM release WHERE id = ?1")
            .and_then(|mut statement| {
                statement
                    .query_row([id], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })
                    .optional()
            })
            .map_err(&failed)?;
        let Some((gid, title, group, status)) = row else {
            return Ok(None);
        };
        let Some(gid) = gid_bytes(&gid) else {
            return Ok(None);
        };
        let mut events = HashMap::new();
        {
            let mut statement = tables
                .prepare_cached(EVENTS_OF_A_RELEASE)
                .map_err(&failed)?;
            let mut rows = statement.query([id]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                earliest(&mut events, row).map_err(&failed)?;
            }
        }
        let (date, country) = events
            .remove(&small(id))
            .map(|(_, date, country)| (date, country))
            .unwrap_or_default();
        let discs = {
            let mut statement = tables
                .prepare_cached(
                    "SELECT track_count FROM medium WHERE release = ?1 ORDER BY position",
                )
                .map_err(&failed)?;
            let counts = statement
                .query_map([id], |row| row.get::<_, Option<i64>>(0))
                .map_err(&failed)?;
            counts
                .map(|count| count.map(|count| small(count.unwrap_or(0))))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(&failed)?
        };
        Ok(Some(ReleaseInfo {
            gid,
            title: title.unwrap_or_default().into_boxed_str(),
            group: small(group),
            status: status.map(small),
            date: date.map(String::into_boxed_str),
            country: country
                .and_then(|area| self.named.get(&("iso_3166_1", area)))
                .cloned(),
            discs: discs.into_boxed_slice(),
        }))
    }
}

fn asked_group(tables: &Connection, id: u32) -> Result<Option<GroupInfo>> {
    let failed = store_read(Store::Staging);
    let row: Option<(String, Option<String>, Option<i64>)> = tables
        .prepare_cached("SELECT gid, name, type FROM release_group WHERE id = ?1")
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .optional()
        })
        .map_err(&failed)?;
    let Some((gid, title, kind)) = row else {
        return Ok(None);
    };
    let Some(gid) = gid_bytes(&gid) else {
        return Ok(None);
    };
    let secondary = {
        let mut statement = tables
            .prepare_cached(
                "SELECT secondary_type FROM release_group_secondary_type_join \
                 WHERE release_group = ?1",
            )
            .map_err(&failed)?;
        let kinds = statement
            .query_map([id], |row| row.get::<_, i64>(0))
            .map_err(&failed)?;
        kinds
            .map(|kind| kind.map(small))
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(&failed)?
    };
    let first_released = tables
        .prepare_cached("SELECT y, m, d FROM release_group_meta WHERE id = ?1")
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| Ok(dated(row.get(0)?, row.get(1)?, row.get(2)?)))
                .optional()
        })
        .map_err(&failed)?
        .flatten();
    Ok(Some(GroupInfo {
        gid,
        title: title.unwrap_or_default().into_boxed_str(),
        kind: kind.map(small),
        secondary: sorted(secondary),
        first_released: first_released.map(String::into_boxed_str),
    }))
}
