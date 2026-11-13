use std::{collections::BTreeSet, time::Duration};

use resonate_core::{Isrc, Link, Mbid, folded_letters, words_of_a_name};
use resonate_library::{
    ArtistProfile, ArtistRelease, Credit, Genre, GroupRelease, Issued, LifeSpan, Medium, Recording,
    RecordingRelease, Release, ReleaseGroup, ReleaseTrack,
};
use rusqlite::{Connection, OptionalExtension, Row, Statement};

use super::{
    ArtistDoc, DISCOGRAPHY_KINDS, GroupDoc, ReleaseDoc,
    looked_up::{LookedUp, dated, gid_bytes, small, store_read},
    search::words_of_a_credited,
};
use crate::{
    Entity, Result, Store,
    wire::{MBID_BYTES, Writer, mbid_of},
};

pub const RELEASES_OF_A_RECORDING_KEPT: usize = 64;
pub const GROUPS_OF_AN_ARTIST_KEPT: usize = 10_000;
pub const GENRES_KEPT: usize = 32;
pub(crate) const CODES_JOINED_BY: &str = " ";
const STATED_AS_NOTHING: [&str; 2] = ["[none]", "[no label]"];
const COVER_PRESENT: &str = "present";
const ENDED: &str = "t";

pub(crate) const RELEASE_ROWS: &str = "SELECT id, artist_credit, barcode, comment FROM release";
pub(crate) const RECORDING_ROWS: &str =
    "SELECT id, gid, name, artist_credit, length FROM recording";
pub(crate) const GROUP_ROWS: &str = "SELECT id, artist_credit, comment FROM release_group";
pub(crate) const ARTIST_ROWS: &str = "SELECT id, gid, name, sort_name, by, bm, bd, ey, em, ed, \
     type, area, gender, comment, ended, begin_area FROM artist";

pub(crate) const fn rows_of(kind: Entity) -> &'static str {
    match kind {
        Entity::Release => RELEASE_ROWS,
        Entity::Recording => RECORDING_ROWS,
        Entity::Group => GROUP_ROWS,
        Entity::Artist => ARTIST_ROWS,
    }
}

pub(crate) const fn words_of(kind: Entity) -> &'static str {
    match kind {
        Entity::Release => "release_words",
        Entity::Recording => "recording_words",
        Entity::Group => "group_words",
        Entity::Artist => "artist_words",
    }
}

pub(crate) struct Document {
    pub gid: [u8; MBID_BYTES],
    pub bytes: Vec<u8>,
    pub words: String,
    pub codes: Vec<String>,
}

fn stated(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty() && !STATED_AS_NOTHING.contains(&text.as_str()))
}

fn present(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

fn credited_as(credit: &[Credit]) -> String {
    credit
        .iter()
        .map(|named| format!("{}{}", named.name, named.joined_by))
        .collect()
}

fn links_of(statement: &mut Statement<'_>, entity: i64) -> Result<Vec<Link>> {
    let failed = store_read(Store::Staging);
    let mut rows = statement.query([entity]).map_err(&failed)?;
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    while let Some(row) = rows.next().map_err(&failed)? {
        let kind: Option<String> = row.get(0).map_err(&failed)?;
        let url: Option<String> = row.get(1).map_err(&failed)?;
        if let Some(url) = url
            && seen.insert(url.clone())
        {
            links.push(Link::new(kind.as_deref().unwrap_or_default(), url));
        }
    }
    Ok(links)
}

fn links_query(table: &str) -> String {
    format!(
        "SELECT lt.name, u.url FROM {table} l JOIN link k ON k.id = l.link \
         LEFT JOIN link_type lt ON lt.id = k.link_type JOIN url u ON u.id = l.entity1 \
         WHERE l.entity0 = ?1 ORDER BY u.url"
    )
}

fn texts_of(statement: &mut Statement<'_>, id: i64) -> Result<Vec<String>> {
    let failed = store_read(Store::Staging);
    let mut rows = statement.query([id]).map_err(&failed)?;
    let mut texts = Vec::new();
    while let Some(row) = rows.next().map_err(&failed)? {
        if let Some(text) = row.get::<_, Option<String>>(0).map_err(&failed)? {
            texts.push(text);
        }
    }
    Ok(texts)
}

fn earliest_first<T>(found: &mut [T], key: impl Fn(&T) -> (&Option<String>, &str)) {
    found.sort_by(|one, other| {
        let (one_date, one_title) = key(one);
        let (other_date, other_title) = key(other);
        (one_date.is_none(), one_date, one_title).cmp(&(
            other_date.is_none(),
            other_date,
            other_title,
        ))
    });
}

type TrackRead = (
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<i64>,
    i64,
    Option<String>,
);

pub(crate) struct Releases<'a> {
    media: Statement<'a>,
    tracks: Statement<'a>,
    recording_links: Statement<'a>,
    release_links: Statement<'a>,
    label: Statement<'a>,
    cover: Statement<'a>,
}

impl<'a> Releases<'a> {
    pub(crate) fn prepared(staging: &'a Connection) -> Result<Self> {
        let failed = store_read(Store::Staging);
        Ok(Self {
            media: staging
                .prepare(
                    "SELECT id, position, format, name FROM medium WHERE release = ?1 \
                     ORDER BY position, id",
                )
                .map_err(&failed)?,
            tracks: staging
                .prepare(
                    "SELECT position, number, name, artist_credit, gid, length, recording_gid, \
                     recording_length, recording, isrcs FROM placed_tracks WHERE medium = ?1 \
                     ORDER BY position, gid",
                )
                .map_err(&failed)?,
            recording_links: staging
                .prepare(&links_query("l_recording_url"))
                .map_err(&failed)?,
            release_links: staging
                .prepare(&links_query("l_release_url"))
                .map_err(&failed)?,
            label: staging
                .prepare(
                    "SELECT l.name, rl.catalog_number FROM release_label rl \
                     LEFT JOIN label l ON l.id = rl.label WHERE rl.release = ?1 \
                     ORDER BY rl.label, rl.catalog_number LIMIT 1",
                )
                .map_err(&failed)?,
            cover: staging
                .prepare("SELECT cover_art_presence FROM release_meta WHERE id = ?1")
                .map_err(&failed)?,
        })
    }

    pub(crate) fn built(
        &mut self,
        looked_up: &LookedUp<'_>,
        row: &Row<'_>,
    ) -> Result<Option<Document>> {
        let failed = store_read(Store::Staging);
        let id: i64 = row.get(0).map_err(&failed)?;
        let Some(info) = looked_up.release(id)? else {
            return Ok(None);
        };
        let credit = looked_up.credit(row.get(1).map_err(&failed)?)?;
        let credited = credited_as(&credit);
        let barcode = present(row.get(2).map_err(&failed)?);
        let disambiguation = present(row.get(3).map_err(&failed)?);
        let group = looked_up.group(info.group)?;

        let read_media = {
            let mut medium_rows = self.media.query([id]).map_err(&failed)?;
            let mut read_media = Vec::new();
            while let Some(medium) = medium_rows.next().map_err(&failed)? {
                read_media.push((
                    medium.get::<_, i64>(0).map_err(&failed)?,
                    medium.get::<_, Option<i64>>(1).map_err(&failed)?,
                    medium.get::<_, Option<i64>>(2).map_err(&failed)?,
                    medium.get::<_, Option<String>>(3).map_err(&failed)?,
                ));
            }
            read_media
        };
        let mut held_media = Vec::new();
        for (medium, position, format, title) in read_media {
            let read_tracks: Vec<TrackRead> = {
                let mut track_rows = self.tracks.query([medium]).map_err(&failed)?;
                let mut read_tracks = Vec::new();
                while let Some(track) = track_rows.next().map_err(&failed)? {
                    read_tracks.push((
                        track.get(0).map_err(&failed)?,
                        track.get(1).map_err(&failed)?,
                        track.get(2).map_err(&failed)?,
                        track.get(3).map_err(&failed)?,
                        track.get(4).map_err(&failed)?,
                        track.get(5).map_err(&failed)?,
                        track.get(6).map_err(&failed)?,
                        track.get(7).map_err(&failed)?,
                        track.get(8).map_err(&failed)?,
                        track.get(9).map_err(&failed)?,
                    ));
                }
                read_tracks
            };
            let mut held_tracks = Vec::new();
            for (
                place,
                number,
                title,
                track_credit,
                gid,
                length,
                recording,
                own,
                recording_id,
                codes,
            ) in read_tracks
            {
                let own_credit = match track_credit {
                    Some(credit) => Some(looked_up.credited_as(credit)?),
                    None => None,
                }
                .filter(|own| !own.is_empty() && *own != credited);
                let place = u32::try_from(place.unwrap_or(0)).unwrap_or(0);
                held_tracks.push(ReleaseTrack {
                    position: place,
                    number: number.unwrap_or_else(|| place.to_string()),
                    title: title.unwrap_or_default(),
                    artist: own_credit,
                    recording: recording.and_then(|gid| Mbid::new(&gid).ok()),
                    track: gid.and_then(|gid| Mbid::new(&gid).ok()),
                    length: length
                        .or(own)
                        .and_then(|millis| u64::try_from(millis).ok())
                        .map(Duration::from_millis),
                    isrcs: codes
                        .as_deref()
                        .unwrap_or_default()
                        .split(CODES_JOINED_BY)
                        .filter(|code| !code.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    links: links_of(&mut self.recording_links, recording_id)?,
                });
            }
            held_media.push(Medium {
                position: u32::try_from(position.unwrap_or(0)).unwrap_or(0),
                format: looked_up.name("medium_format", format),
                title: present(title),
                tracks: held_tracks,
            });
        }

        let (label_name, catalog_number) = self
            .label
            .query_row([id], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .optional()
            .map_err(&failed)?
            .unwrap_or_default();
        let covered: Option<String> = self
            .cover
            .query_row([id], |row| row.get(0))
            .optional()
            .map_err(&failed)?;

        let release = Release {
            id: mbid_of(info.gid),
            group: group.as_ref().map(|group| mbid_of(group.gid)),
            title: info.title.to_string(),
            credit,
            date: info.date.as_deref().map(str::to_owned),
            country: info.country.as_deref().map(str::to_owned),
            label: stated(label_name),
            catalog_number: stated(catalog_number),
            barcode: barcode.clone(),
            kind: group.as_ref().and_then(|group| looked_up.kind_of(group)),
            disambiguation,
            has_front_cover: covered.as_deref() == Some(COVER_PRESENT),
            links: links_of(&mut self.release_links, id)?,
            media: held_media,
        };
        let words = words_of_a_credited(&release.title, &release.credit);
        let doc = ReleaseDoc {
            release,
            status: looked_up.status_of(&info),
        };
        let mut writer = Writer::new();
        doc.write(&mut writer);
        Ok(Some(Document {
            gid: info.gid,
            bytes: writer.finish(),
            words,
            codes: barcode.into_iter().collect(),
        }))
    }
}

pub(crate) struct Recordings<'a> {
    isrcs: Statement<'a>,
    appearances: Statement<'a>,
}

impl<'a> Recordings<'a> {
    pub(crate) fn prepared(staging: &'a Connection) -> Result<Self> {
        let failed = store_read(Store::Staging);
        Ok(Self {
            isrcs: staging
                .prepare("SELECT isrc FROM isrc WHERE recording = ?1 ORDER BY isrc")
                .map_err(&failed)?,
            appearances: staging
                .prepare(
                    "SELECT m.release, m.position, t.position FROM track t \
                     JOIN medium m ON m.id = t.medium WHERE t.recording = ?1 \
                     ORDER BY m.release, m.position, t.position",
                )
                .map_err(&failed)?,
        })
    }

    pub(crate) fn built(
        &mut self,
        looked_up: &LookedUp<'_>,
        row: &Row<'_>,
    ) -> Result<Option<Document>> {
        let failed = store_read(Store::Staging);
        let id: i64 = row.get(0).map_err(&failed)?;
        let gid: String = row.get(1).map_err(&failed)?;
        let Some(gid) = gid_bytes(&gid) else {
            return Ok(None);
        };
        let appeared: Vec<(i64, Option<i64>, Option<i64>)> = {
            let mut rows = self.appearances.query([id]).map_err(&failed)?;
            let mut appeared = Vec::new();
            while let Some(appearance) = rows.next().map_err(&failed)? {
                appeared.push((
                    appearance.get(0).map_err(&failed)?,
                    appearance.get(1).map_err(&failed)?,
                    appearance.get(2).map_err(&failed)?,
                ));
            }
            appeared
        };
        let mut on: Vec<RecordingRelease> = Vec::new();
        let mut seen = BTreeSet::new();
        for (release, disc, position) in appeared {
            if !seen.insert(release) {
                continue;
            }
            let Some(info) = looked_up.release(release)? else {
                continue;
            };
            let group = looked_up.group(info.group)?;
            on.push(RecordingRelease {
                id: mbid_of(info.gid),
                title: info.title.to_string(),
                date: info.date.as_deref().map(str::to_owned),
                disc: disc.and_then(|disc| u32::try_from(disc).ok()),
                position: position.and_then(|position| u32::try_from(position).ok()),
                issued: Issued {
                    kind: group.as_ref().and_then(|group| looked_up.kind_of(group)),
                    secondary: group
                        .as_ref()
                        .map(|group| looked_up.secondary_of(group))
                        .unwrap_or_default(),
                    status: looked_up.status_of(&info),
                },
            });
        }
        earliest_first(&mut on, |on| (&on.date, &on.title));
        on.truncate(RELEASES_OF_A_RECORDING_KEPT);
        let codes = texts_of(&mut self.isrcs, id)?;
        let length: Option<i64> = row.get(4).map_err(&failed)?;
        let recording = Recording {
            id: mbid_of(gid),
            title: row
                .get::<_, Option<String>>(2)
                .map_err(&failed)?
                .unwrap_or_default(),
            credit: looked_up.credit(row.get(3).map_err(&failed)?)?,
            length: length
                .and_then(|millis| u64::try_from(millis).ok())
                .map(Duration::from_millis),
            isrcs: codes
                .iter()
                .filter_map(|code| Isrc::new(code).ok())
                .collect(),
            releases: on,
        };
        let words = words_of_a_credited(&recording.title, &recording.credit);
        let mut writer = Writer::new();
        writer.recording(&recording);
        Ok(Some(Document {
            gid,
            bytes: writer.finish(),
            words,
            codes: recording
                .isrcs
                .iter()
                .map(|isrc| isrc.as_str().to_owned())
                .collect(),
        }))
    }
}

pub(crate) struct Groups<'a> {
    links: Statement<'a>,
    releases: Statement<'a>,
}

impl<'a> Groups<'a> {
    pub(crate) fn prepared(staging: &'a Connection) -> Result<Self> {
        let failed = store_read(Store::Staging);
        Ok(Self {
            links: staging
                .prepare(&links_query("l_release_group_url"))
                .map_err(&failed)?,
            releases: staging
                .prepare("SELECT id FROM release WHERE release_group = ?1 ORDER BY id")
                .map_err(&failed)?,
        })
    }

    pub(crate) fn built(
        &mut self,
        looked_up: &LookedUp<'_>,
        row: &Row<'_>,
    ) -> Result<Option<Document>> {
        let failed = store_read(Store::Staging);
        let id: i64 = row.get(0).map_err(&failed)?;
        let Some(info) = looked_up.group(small(id))? else {
            return Ok(None);
        };
        let pressed: Vec<i64> = {
            let ids = self
                .releases
                .query_map([id], |row| row.get(0))
                .map_err(&failed)?;
            ids.collect::<rusqlite::Result<_>>().map_err(&failed)?
        };
        let mut pressings = Vec::new();
        for release in pressed {
            let Some(release) = looked_up.release(release)? else {
                continue;
            };
            pressings.push(GroupRelease {
                id: mbid_of(release.gid),
                title: release.title.to_string(),
                date: release.date.as_deref().map(str::to_owned),
                country: release.country.as_deref().map(str::to_owned),
                track_count: (!release.discs.is_empty()).then(|| release.discs.iter().sum()),
                discs: release.discs.to_vec(),
            });
        }
        earliest_first(&mut pressings, |pressing| (&pressing.date, &pressing.title));
        let group = ReleaseGroup {
            id: mbid_of(info.gid),
            title: info.title.to_string(),
            credit: looked_up.credit(row.get(1).map_err(&failed)?)?,
            kind: looked_up.kind_of(&info),
            first_released: info.first_released.as_deref().map(str::to_owned),
            disambiguation: present(row.get(2).map_err(&failed)?),
            links: links_of(&mut self.links, id)?,
            releases: pressings,
        };
        let words = words_of_a_credited(&group.title, &group.credit);
        let doc = GroupDoc {
            group,
            secondary: looked_up.secondary_of(&info),
        };
        let mut writer = Writer::new();
        doc.write(&mut writer);
        Ok(Some(Document {
            gid: info.gid,
            bytes: writer.finish(),
            words,
            codes: Vec::new(),
        }))
    }
}

pub(crate) struct Artists<'a> {
    links: Statement<'a>,
    aliases: Statement<'a>,
    tags: Statement<'a>,
    credited: Statement<'a>,
}

impl<'a> Artists<'a> {
    pub(crate) fn prepared(staging: &'a Connection) -> Result<Self> {
        let failed = store_read(Store::Staging);
        Ok(Self {
            links: staging
                .prepare(&links_query("l_artist_url"))
                .map_err(&failed)?,
            aliases: staging
                .prepare("SELECT name FROM artist_alias WHERE artist = ?1 ORDER BY name")
                .map_err(&failed)?,
            tags: staging
                .prepare(&format!(
                    "SELECT t.name, a.count FROM artist_tag a JOIN tag t ON t.id = a.tag \
                     WHERE a.artist = ?1 AND a.count > 0 ORDER BY a.count DESC, t.name \
                     LIMIT {GENRES_KEPT}"
                ))
                .map_err(&failed)?,
            credited: staging
                .prepare(
                    "SELECT DISTINCT rg.id FROM artist_credit_name acn \
                     JOIN release_group rg ON rg.artist_credit = acn.artist_credit \
                     WHERE acn.artist = ?1 ORDER BY rg.id",
                )
                .map_err(&failed)?,
        })
    }

    pub(crate) fn built(
        &mut self,
        looked_up: &LookedUp<'_>,
        row: &Row<'_>,
    ) -> Result<Option<Document>> {
        let failed = store_read(Store::Staging);
        let id: i64 = row.get(0).map_err(&failed)?;
        let gid: String = row.get(1).map_err(&failed)?;
        let Some(gid) = gid_bytes(&gid) else {
            return Ok(None);
        };
        let name: String = row
            .get::<_, Option<String>>(2)
            .map_err(&failed)?
            .unwrap_or_default();
        let billed = folded_letters(&name);
        let mut folds = vec![billed];
        let mut known_as = Vec::new();
        for alias in texts_of(&mut self.aliases, id)? {
            let fold = folded_letters(&alias);
            if alias.trim().is_empty() || folds.contains(&fold) {
                continue;
            }
            folds.push(fold);
            known_as.push(alias);
        }
        let mut genres = Vec::new();
        {
            let mut tagged = self.tags.query([id]).map_err(&failed)?;
            while let Some(tag) = tagged.next().map_err(&failed)? {
                let named: Option<String> = tag.get(0).map_err(&failed)?;
                let count: i64 = tag.get(1).map_err(&failed)?;
                if let Some(named) = present(named) {
                    genres.push(Genre {
                        name: named,
                        weight: u32::try_from(count).unwrap_or(u32::MAX),
                    });
                }
            }
        }
        let credited_groups: Vec<i64> = {
            let ids = self
                .credited
                .query_map([id], |row| row.get(0))
                .map_err(&failed)?;
            ids.collect::<rusqlite::Result<_>>().map_err(&failed)?
        };
        let mut groups = Vec::new();
        for group in credited_groups {
            let Some(info) = looked_up.group(small(group))? else {
                continue;
            };
            let kind = looked_up.kind_of(&info);
            if !kind
                .as_deref()
                .is_some_and(|kind| DISCOGRAPHY_KINDS.contains(&kind))
            {
                continue;
            }
            groups.push(ArtistRelease {
                mbid: mbid_of(info.gid),
                title: info.title.to_string(),
                kind,
                secondary: looked_up.secondary_of(&info),
                first_released: info.first_released.as_deref().map(str::to_owned),
            });
        }
        earliest_first(&mut groups, |group| (&group.first_released, &group.title));
        let credited_count = u32::try_from(groups.len()).unwrap_or(u32::MAX);
        groups.truncate(GROUPS_OF_AN_ARTIST_KEPT);

        let area: Option<i64> = row.get(11).map_err(&failed)?;
        let ended: Option<String> = row.get(14).map_err(&failed)?;
        let sort_name = present(row.get(3).map_err(&failed)?);
        let artist = ArtistProfile {
            mbid: mbid_of(gid),
            name,
            sort_name: sort_name.clone(),
            kind: looked_up.name("artist_type", row.get(10).map_err(&failed)?),
            gender: looked_up.name("gender", row.get(12).map_err(&failed)?),
            country: looked_up.name("iso_3166_1", area),
            area: looked_up.name("area", area),
            began_in: looked_up.name("area", row.get(15).map_err(&failed)?),
            span: LifeSpan {
                begin: dated(
                    row.get(4).map_err(&failed)?,
                    row.get(5).map_err(&failed)?,
                    row.get(6).map_err(&failed)?,
                ),
                end: dated(
                    row.get(7).map_err(&failed)?,
                    row.get(8).map_err(&failed)?,
                    row.get(9).map_err(&failed)?,
                ),
                ended: ended.as_deref() == Some(ENDED),
            },
            disambiguation: present(row.get(13).map_err(&failed)?),
            aliases: known_as,
            genres,
            links: links_of(&mut self.links, id)?,
        };
        let mut words = vec![artist.name.clone()];
        words.extend(sort_name);
        words.extend(artist.aliases.iter().cloned());
        let words = words
            .iter()
            .flat_map(|named| words_of_a_name(named))
            .collect::<Vec<_>>()
            .join(" ");
        let doc = ArtistDoc {
            artist,
            groups,
            credited: credited_count,
        };
        let mut writer = Writer::new();
        doc.write(&mut writer);
        Ok(Some(Document {
            gid,
            bytes: writer.finish(),
            words,
            codes: Vec::new(),
        }))
    }
}

pub(crate) struct Builders<'a> {
    pub releases: Releases<'a>,
    pub recordings: Recordings<'a>,
    pub groups: Groups<'a>,
    pub artists: Artists<'a>,
}

impl<'a> Builders<'a> {
    pub(crate) fn prepared(staging: &'a Connection) -> Result<Self> {
        Ok(Self {
            releases: Releases::prepared(staging)?,
            recordings: Recordings::prepared(staging)?,
            groups: Groups::prepared(staging)?,
            artists: Artists::prepared(staging)?,
        })
    }

    pub(crate) fn built(
        &mut self,
        kind: Entity,
        looked_up: &LookedUp<'_>,
        row: &Row<'_>,
    ) -> Result<Option<Document>> {
        match kind {
            Entity::Release => self.releases.built(looked_up, row),
            Entity::Recording => self.recordings.built(looked_up, row),
            Entity::Group => self.groups.built(looked_up, row),
            Entity::Artist => self.artists.built(looked_up, row),
        }
    }
}
