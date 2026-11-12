use std::{num::NonZeroU32, path::PathBuf, time::Duration};

use ahash::AHashMap;
use resonate_codec::Popularity;
use resonate_core::{
    AlbumId, ArtistId, ChannelCount, ChannelLayout, FrameSpan, Frames, MediaLocation,
    ReleaseTrackId, SampleRate, StreamSpec, TrackId, WantId,
};
use resonate_vault::{Encoding, Replacing};
use rusqlite::{Connection, OptionalExtension as _, Row, params, params_from_iter, types::Value};

use super::{Inner, ordered::Reading, statements::FIRST_DISC};
use crate::{
    Album, Artist, ArtistDetail, Codec, CoverArt, Direction, Error, Favoured, Genre, HeldMedium,
    HeldReleaseTrack, ImageFormat, KeyColumn, LifeSpan, Link, Mbid, MissingTrack, ReleaseDetail,
    Result, StoreOp, Track, UnheldRelease, VaultKey, VaultObject, Want, enrich,
    organise::TrackToFile, retag::TrackToTag, store,
};

pub(super) struct RawToTag {
    pub(super) id: i64,
    pub(super) path: String,
    pub(super) span_start: i64,
    pub(super) span_frames: Option<i64>,
    pub(super) answered: bool,
    pub(super) title: String,
    pub(super) artist: Option<String>,
    pub(super) artist_mbid: Option<String>,
    pub(super) mbid: Option<String>,
    pub(super) release_track_mbid: Option<String>,
    pub(super) isrc: Option<String>,
    pub(super) track_number: Option<i64>,
    pub(super) disc_number: Option<i64>,
    pub(super) album: Option<i64>,
    pub(super) album_answered: bool,
    pub(super) release_title: Option<String>,
    pub(super) album_artist_answered: bool,
    pub(super) album_artist: Option<String>,
    pub(super) album_artist_mbid: Option<String>,
    pub(super) album_mbid: Option<String>,
    pub(super) release_group: Option<String>,
    pub(super) date: Option<String>,
    pub(super) label: Option<String>,
    pub(super) catalog_number: Option<String>,
    pub(super) barcode: Option<String>,
    pub(super) vaulted: bool,
    pub(super) favourite: bool,
    pub(super) plays: i64,
    pub(super) artist_sort: Option<String>,
    pub(super) album_artist_sort: Option<String>,
    pub(super) pressing_fitted: bool,
    pub(super) sample_rate: i64,
    pub(super) sheet_modified: Option<i64>,
    pub(super) pressing_fit: Option<i64>,
}

impl RawToTag {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            span_start: row.get(2)?,
            span_frames: row.get(3)?,
            answered: row.get(4)?,
            title: row.get(5)?,
            artist: row.get(6)?,
            artist_mbid: row.get(7)?,
            mbid: row.get(8)?,
            release_track_mbid: row.get(9)?,
            isrc: row.get(10)?,
            track_number: row.get(11)?,
            disc_number: row.get(12)?,
            album: row.get(13)?,
            album_answered: row.get(14)?,
            release_title: row.get(15)?,
            album_artist_answered: row.get(16)?,
            album_artist: row.get(17)?,
            album_artist_mbid: row.get(18)?,
            album_mbid: row.get(19)?,
            release_group: row.get(20)?,
            date: row.get(21)?,
            label: row.get(22)?,
            catalog_number: row.get(23)?,
            barcode: row.get(24)?,
            vaulted: row.get(25)?,
            favourite: row.get(26)?,
            plays: row.get(27)?,
            artist_sort: row.get(28)?,
            album_artist_sort: row.get(29)?,
            pressing_fitted: row.get(30)?,
            sample_rate: row.get(31)?,
            sheet_modified: row.get(32)?,
            pressing_fit: row.get(33)?,
        })
    }

    pub(super) fn into_tagged(
        self,
        tracks: &AHashMap<(i64, i64), u32>,
        discs: &AHashMap<i64, u32>,
    ) -> Result<TrackToTag> {
        let seat = self.disc_number.unwrap_or(FIRST_DISC);
        Ok(TrackToTag {
            id: TrackId::new(self.id as u64)?,
            path: store::path_of(&self.path),
            album_id: self.album.map(|id| AlbumId::new(id as u64)).transpose()?,
            cut: store::span(self.span_start, self.span_frames).is_some(),
            vaulted: self.vaulted,
            answered: self.answered,
            title: self.title,
            artist: self.artist,
            artist_sort: self.artist_sort,
            artist_mbid: self.artist_mbid,
            mbid: self.mbid,
            release_track_mbid: self.release_track_mbid,
            isrc: self.isrc,
            track_number: counting(self.track_number),
            disc_number: counting(self.disc_number),
            album_answered: self.album_answered,
            album: self.release_title,
            album_artist_answered: self.album_artist_answered,
            album_artist: self.album_artist,
            album_artist_sort: self.album_artist_sort,
            album_artist_mbid: self.album_artist_mbid,
            album_mbid: self.album_mbid,
            release_group: self.release_group,
            date: self.date,
            label: self.label,
            catalog_number: self.catalog_number,
            barcode: self.barcode,
            pressing_fitted: self.pressing_fitted,
            pressing_counted: enrich::Fit::counts_agreed_by_every_pressing(self.pressing_fit),
            track_total: self
                .album
                .and_then(|album| tracks.get(&(album, seat)).copied()),
            disc_total: self.album.and_then(|album| discs.get(&album).copied()),
            popularity: Popularity {
                favourite: self.favourite,
                plays: u64::try_from(self.plays).unwrap_or(0),
            },
            span_start: u64::try_from(self.span_start).unwrap_or(0),
            rate: u32::try_from(self.sample_rate).unwrap_or(0),
            sheet_modified: self.sheet_modified,
        })
    }
}

fn counting(value: Option<i64>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

pub(super) struct RawFiled {
    pub(super) id: i64,
    pub(super) path: String,
    pub(super) root: String,
    pub(super) title: String,
    pub(super) artist: Option<String>,
    pub(super) album: Option<i64>,
    pub(super) album_title: Option<String>,
    pub(super) year: Option<i64>,
    pub(super) album_artist: Option<String>,
    pub(super) track: Option<i64>,
    pub(super) disc: Option<i64>,
}

impl RawFiled {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            root: row.get(2)?,
            title: row.get(3)?,
            artist: row.get(4)?,
            album: row.get(5)?,
            album_title: row.get(6)?,
            year: row.get(7)?,
            album_artist: row.get(8)?,
            track: row.get(9)?,
            disc: row.get(10)?,
        })
    }

    pub(super) fn into_filed(self, discs: &AHashMap<i64, u32>) -> Result<TrackToFile> {
        Ok(TrackToFile {
            id: TrackId::new(self.id as u64)?,
            path: store::path_of(&self.path),
            root: store::path_of(&self.root),
            title: self.title,
            artist: self.artist,
            album: self.album_title,
            album_id: self.album.map(|id| AlbumId::new(id as u64)).transpose()?,
            album_artist: self.album_artist,
            year: self.year.and_then(|year| i32::try_from(year).ok()),
            track: self.track.and_then(|number| u32::try_from(number).ok()),
            disc: self
                .disc
                .and_then(|number| u32::try_from(number).ok())
                .and_then(NonZeroU32::new),
            discs: self
                .album
                .and_then(|album| discs.get(&album).copied())
                .unwrap_or_default(),
        })
    }
}

pub(super) fn rows<T>(
    connection: &Connection,
    sql: &str,
    binds: Vec<Value>,
    read: impl Fn(&Row<'_>) -> rusqlite::Result<Result<T>>,
) -> Result<Vec<T>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let found = statement
        .query_map(params_from_iter(binds), read)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found.into_iter().collect()
}

pub(super) fn still_unheld(connection: &Connection, release_track: ReleaseTrackId) -> Result<bool> {
    let held_by: Option<Option<i64>> = connection
        .query_row(
            "SELECT track_id FROM release_tracks WHERE id = ?1",
            params![release_track.get() as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(held_by == Some(None))
}

pub(super) const fn limit(limit: Option<usize>) -> i64 {
    match limit {
        Some(limit) => limit as i64,
        None => -1,
    }
}

pub(super) const fn read_as(direction: Direction, order: Reading) -> &'static str {
    match direction {
        Direction::Ascending => order.up,
        Direction::Descending => order.down,
    }
}

pub(crate) struct RawTrack {
    pub(super) id: i64,
    pub(super) path: String,
    pub(super) title: String,
    pub(super) artist: Option<String>,
    pub(super) album_id: Option<i64>,
    pub(super) track_number: Option<u32>,
    pub(super) disc_number: Option<u32>,
    pub(super) duration: Option<i64>,
    pub(super) sample_rate: u32,
    pub(super) channels: u16,
    pub(super) sample_format: i64,
    pub(super) codec: i64,
    pub(super) rg_track_gain: Option<f64>,
    pub(super) rg_track_peak: Option<f64>,
    pub(super) rg_album_gain: Option<f64>,
    pub(super) rg_album_peak: Option<f64>,
    pub(super) file_size: i64,
    pub(super) modified: i64,
    pub(super) added: i64,
    pub(super) plays: u32,
    pub(super) played: Option<i64>,
    pub(super) span_start: i64,
    pub(super) span_frames: Option<i64>,
    pub(super) artist_id: Option<i64>,
    pub(super) favourite: Option<i64>,
    pub(super) genre: Option<String>,
    pub(super) hidden: bool,
    pub(super) alternatives: u32,
    pub(super) delivered: bool,
    pub(super) explicit: Option<bool>,
}

impl RawTrack {
    pub(crate) fn read_where_joined(row: &Row<'_>) -> rusqlite::Result<Option<Self>> {
        if row.get::<_, Option<i64>>(0)?.is_none() {
            return Ok(None);
        }
        Self::read(row).map(Some)
    }

    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            title: row.get(2)?,
            artist: row.get(3)?,
            album_id: row.get(4)?,
            track_number: row.get(5)?,
            disc_number: row.get(6)?,
            duration: row.get(7)?,
            sample_rate: row.get(8)?,
            channels: row.get(9)?,
            sample_format: row.get(10)?,
            codec: row.get(11)?,
            rg_track_gain: row.get(12)?,
            rg_track_peak: row.get(13)?,
            rg_album_gain: row.get(14)?,
            rg_album_peak: row.get(15)?,
            file_size: row.get(16)?,
            modified: row.get(17)?,
            added: row.get(18)?,
            plays: row.get(19)?,
            played: row.get(20)?,
            span_start: row.get(21)?,
            span_frames: row.get(22)?,
            artist_id: row.get(23)?,
            favourite: row.get(24)?,
            genre: row.get(25)?,
            hidden: row.get(26)?,
            alternatives: row.get(27)?,
            delivered: row.get(28)?,
            explicit: row.get(29)?,
        })
    }

    pub(crate) fn into_track(self) -> Result<Track> {
        let id = TrackId::new(self.id as u64)?;
        let spec = StreamSpec::new(
            SampleRate::new(self.sample_rate)?,
            ChannelLayout::from_count(ChannelCount::new(self.channels)?),
            store::format_from_code(self.sample_format, id)?,
        );

        Ok(Track {
            id,
            location: MediaLocation::local(store::path_of(&self.path)),
            title: self.title,
            artist: self.artist,
            artist_id: self
                .artist_id
                .map(|id| ArtistId::new(id as u64))
                .transpose()?,
            album_id: self
                .album_id
                .map(|id| AlbumId::new(id as u64))
                .transpose()?,
            track_number: self.track_number,
            disc_number: self.disc_number,
            duration: self.duration.map(|frames| Frames(frames as u64)),
            spec,
            codec: store::codec_from_code(self.codec, id)?,
            replay_gain: store::replay_gain(
                self.rg_track_gain,
                self.rg_track_peak,
                self.rg_album_gain,
                self.rg_album_peak,
            ),
            file_size: self.file_size as u64,
            modified: store::from_nanos(self.modified),
            added: store::from_nanos(self.added),
            plays: self.plays,
            played: self.played.map(store::from_nanos),
            span: store::span(self.span_start, self.span_frames),
            favourite: self.favourite.map(store::from_nanos),
            genre: self.genre,
            hidden: self.hidden,
            alternatives: self.alternatives,
            delivered: self.delivered,
            explicit: self.explicit,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TrackToVault {
    pub id: TrackId,
    pub path: PathBuf,
    pub span: Option<FrameSpan>,
    pub file_size: u64,
    pub codec: Codec,
    pub spec: StreamSpec,
    pub album_id: Option<AlbumId>,
    pub renewing: bool,
    pub foretold: Option<VaultKey>,
    pub replacing: Option<Replacing>,
}

impl TrackToVault {
    pub(crate) fn location(&self) -> MediaLocation {
        MediaLocation::local(&self.path)
    }
}

pub(super) struct RawToVault {
    pub(super) id: i64,
    pub(super) path: String,
    pub(super) span_start: i64,
    pub(super) span_frames: Option<i64>,
    pub(super) file_size: i64,
    pub(super) codec: i64,
    pub(super) sample_rate: u32,
    pub(super) channels: u16,
    pub(super) sample_format: i64,
    pub(super) album_id: Option<i64>,
    pub(super) renewing: bool,
    pub(super) foretold: Option<String>,
    pub(super) standing: Option<String>,
    pub(super) standing_bytes: Option<i64>,
    pub(super) standing_encoding: Option<u16>,
}

impl RawToVault {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            span_start: row.get(2)?,
            span_frames: row.get(3)?,
            file_size: row.get(4)?,
            codec: row.get(5)?,
            sample_rate: row.get(6)?,
            channels: row.get(7)?,
            sample_format: row.get(8)?,
            album_id: row.get(9)?,
            renewing: row.get(10)?,
            foretold: row.get(11)?,
            standing: row.get(12)?,
            standing_bytes: row.get(13)?,
            standing_encoding: row.get(14)?,
        })
    }

    pub(super) fn into_vaultable(self) -> Result<TrackToVault> {
        let id = TrackId::new(self.id as u64)?;
        Ok(TrackToVault {
            id,
            path: store::path_of(&self.path),
            span: store::span(self.span_start, self.span_frames),
            file_size: self.file_size as u64,
            codec: store::codec_from_code(self.codec, id)?,
            spec: StreamSpec::new(
                SampleRate::new(self.sample_rate)?,
                ChannelLayout::from_count(ChannelCount::new(self.channels)?),
                store::format_from_code(self.sample_format, id)?,
            ),
            album_id: self
                .album_id
                .map(|held| AlbumId::new(held as u64))
                .transpose()?,
            renewing: self.renewing,
            foretold: self
                .foretold
                .as_deref()
                .and_then(|key| VaultKey::read(key).ok()),
            replacing: self
                .standing
                .as_deref()
                .and_then(|key| VaultKey::read(key).ok())
                .zip(self.standing_bytes)
                .map(|(key, bytes)| Replacing {
                    key,
                    bytes: u64::try_from(bytes).unwrap_or(0),
                    keyed_by_the_pcm_alone: Encoding::of(self.standing_encoding.unwrap_or(0))
                        .keyed_by_the_pcm_alone(),
                }),
        })
    }
}

pub(super) struct RawVaultObject {
    pub(super) key: String,
    pub(super) form: i64,
    pub(super) path: String,
    pub(super) bytes: i64,
    pub(super) sample_rate: u32,
    pub(super) channels: u16,
    pub(super) sample_format: i64,
    pub(super) frames: Option<i64>,
    pub(super) taken_from: String,
    pub(super) took: i64,
    pub(super) was_bytes: i64,
    pub(super) was_codec: i64,
    pub(super) validated: bool,
    pub(super) encoding: u16,
}

impl RawVaultObject {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            key: row.get(0)?,
            form: row.get(1)?,
            path: row.get(2)?,
            bytes: row.get(3)?,
            sample_rate: row.get(4)?,
            channels: row.get(5)?,
            sample_format: row.get(6)?,
            frames: row.get(7)?,
            taken_from: row.get(8)?,
            took: row.get(9)?,
            was_bytes: row.get(10)?,
            was_codec: row.get(11)?,
            validated: row.get(12)?,
            encoding: row.get(13)?,
        })
    }

    pub(super) fn into_object(self, inner: &Inner) -> Result<VaultObject> {
        let key = VaultKey::read(&self.key).map_err(|_| {
            tracing::warn!(key = self.key, "a vault object's key does not read as one");
            Error::NotAVaultKey {
                held_in: KeyColumn::VaultObject,
            }
        })?;

        Ok(VaultObject {
            key,
            form: store::vault_form_of(self.form)?,
            path: inner.in_the_vault(&self.path)?,
            bytes: self.bytes as u64,
            spec: StreamSpec::new(
                SampleRate::new(self.sample_rate)?,
                ChannelLayout::from_count(ChannelCount::new(self.channels)?),
                store::vault_format_of(key, self.sample_format)?,
            ),
            frames: self.frames.map(|held| Frames(held as u64)),
            taken_from: store::path_of(&self.taken_from),
            took: store::from_nanos(self.took),
            was_bytes: self.was_bytes as u64,
            was_codec: store::vault_codec_of(key, self.was_codec)?,
            validated: self.validated,
            encoding: Encoding::of(self.encoding),
        })
    }
}

pub(super) fn read_album(row: &Row<'_>) -> rusqlite::Result<Result<Album>> {
    RawAlbum::read(row).map(RawAlbum::into_album)
}

pub(super) struct RawAlbum {
    pub(super) id: i64,
    pub(super) title: String,
    pub(super) artist_id: Option<i64>,
    pub(super) year: Option<i32>,
    pub(super) track_count: u32,
    pub(super) artist_count: u32,
    pub(super) has_cover_art: bool,
    pub(super) mbid: Option<String>,
    pub(super) missing: u32,
    pub(super) artist: Option<String>,
    pub(super) favourite: Option<i64>,
}

impl RawAlbum {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            artist_id: row.get(2)?,
            year: row.get(3)?,
            track_count: row.get(4)?,
            artist_count: row.get(5)?,
            has_cover_art: row.get(6)?,
            mbid: row.get(7)?,
            missing: row.get(8)?,
            artist: row.get(9)?,
            favourite: row.get(10)?,
        })
    }

    pub(super) fn into_album(self) -> Result<Album> {
        Ok(Album {
            id: AlbumId::new(self.id as u64)?,
            title: self.title,
            artist_id: self
                .artist_id
                .map(|id| ArtistId::new(id as u64))
                .transpose()?,
            artist: self.artist,
            year: self.year,
            track_count: self.track_count,
            artist_count: self.artist_count,
            has_cover_art: self.has_cover_art,
            mbid: mbid_of(self.mbid)?,
            missing: self.missing,
            favourite: self.favourite.map(store::from_nanos),
        })
    }
}

pub(super) fn read_artist(row: &Row<'_>) -> rusqlite::Result<Result<Artist>> {
    let id: i64 = row.get(0)?;
    let name: String = row.get(1)?;
    let album_count: u32 = row.get(2)?;
    let track_count: u32 = row.get(3)?;
    let mbid: Option<String> = row.get(4)?;
    let has_portrait: bool = row.get(5)?;
    let favourite: Option<i64> = row.get(6)?;

    Ok(ArtistId::new(id as u64)
        .map_err(Error::from)
        .and_then(|id| {
            Ok(Artist {
                id,
                name,
                album_count,
                track_count,
                mbid: mbid_of(mbid)?,
                has_portrait,
                favourite: favourite.map(store::from_nanos),
            })
        }))
}

pub(super) const fn unknown(what: Favoured) -> Error {
    match what {
        Favoured::Track(id) => Error::UnknownTrack(id),
        Favoured::Album(id) => Error::UnknownAlbum(id),
        Favoured::Artist(id) => Error::UnknownArtist(id),
    }
}

pub(super) const fn favoured(what: Favoured) -> (&'static str, i64) {
    match what {
        Favoured::Track(id) => ("tracks", id.get() as i64),
        Favoured::Album(id) => ("albums", id.get() as i64),
        Favoured::Artist(id) => ("artists", id.get() as i64),
    }
}

fn mbid_of(text: Option<String>) -> Result<Option<Mbid>> {
    Ok(text.as_deref().map(Mbid::new).transpose()?)
}

pub(super) fn read_link(row: &Row<'_>, from: usize) -> rusqlite::Result<Result<Link>> {
    let relation: i64 = row.get(from)?;
    let provider: i64 = row.get(from + 1)?;
    let url: String = row.get(from + 2)?;

    Ok(store::relation_of(relation).and_then(|relation| {
        Ok(Link {
            relation,
            service: store::service_of(provider)?,
            url,
        })
    }))
}

fn read_owned_link(row: &Row<'_>) -> rusqlite::Result<Result<(i64, Link)>> {
    let owner: i64 = row.get(0)?;
    Ok(read_link(row, 1)?.map(|link| (owner, link)))
}

pub(super) fn grouped_links(
    connection: &Connection,
    sql: &str,
    binds: Vec<Value>,
) -> Result<AHashMap<i64, Vec<Link>>> {
    let mut grouped: AHashMap<i64, Vec<Link>> = AHashMap::new();
    for (owner, link) in rows(connection, sql, binds, read_owned_link)? {
        grouped.entry(owner).or_default().push(link);
    }
    Ok(grouped)
}

pub(super) struct RawRelease {
    pub(super) mbid: Option<String>,
    pub(super) group: Option<String>,
    pub(super) date: Option<String>,
    pub(super) country: Option<String>,
    pub(super) label: Option<String>,
    pub(super) catalog_number: Option<String>,
    pub(super) barcode: Option<String>,
    pub(super) kind: Option<String>,
    pub(super) disambiguation: Option<String>,
    pub(super) cover_source: i64,
    pub(super) asked: Option<i64>,
    pub(super) answered: Option<i64>,
    pub(super) may_have_a_front: bool,
}

impl RawRelease {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            mbid: row.get(0)?,
            group: row.get(1)?,
            date: row.get(2)?,
            country: row.get(3)?,
            label: row.get(4)?,
            catalog_number: row.get(5)?,
            barcode: row.get(6)?,
            kind: row.get(7)?,
            disambiguation: row.get(8)?,
            cover_source: row.get(9)?,
            asked: row.get(10)?,
            answered: row.get(11)?,
            may_have_a_front: row.get(12)?,
        })
    }

    pub(super) fn into_detail(
        self,
        album: AlbumId,
        links: Vec<Link>,
        media: Vec<HeldMedium>,
    ) -> Result<ReleaseDetail> {
        Ok(ReleaseDetail {
            mbid: mbid_of(self.mbid)?,
            group: mbid_of(self.group)?,
            date: self.date,
            country: self.country,
            label: self.label,
            catalog_number: self.catalog_number,
            barcode: self.barcode,
            kind: self.kind,
            disambiguation: self.disambiguation,
            cover_source: store::cover_source_of(album, self.cover_source)?,
            may_have_a_front: self.may_have_a_front,
            asked: self.asked.map(store::from_nanos),
            answered: self.answered.map(store::from_nanos),
            links,
            media,
        })
    }
}

pub(super) struct RawReleaseTrack {
    pub(super) id: i64,
    pub(super) disc: u32,
    pub(super) position: u32,
    pub(super) number: Option<String>,
    pub(super) title: String,
    pub(super) artist: Option<String>,
    pub(super) recording: Option<String>,
    pub(super) track_mbid: Option<String>,
    pub(super) length_ms: Option<i64>,
    pub(super) isrc: Option<String>,
    pub(super) track: Option<i64>,
}

impl RawReleaseTrack {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            disc: row.get(1)?,
            position: row.get(2)?,
            number: row.get(3)?,
            title: row.get(4)?,
            artist: row.get(5)?,
            recording: row.get(6)?,
            track_mbid: row.get(7)?,
            length_ms: row.get(8)?,
            isrc: row.get(9)?,
            track: row.get(10)?,
        })
    }

    pub(super) fn into_held(self, album: AlbumId, links: Vec<Link>) -> Result<HeldReleaseTrack> {
        Ok(HeldReleaseTrack {
            id: ReleaseTrackId::new(self.id as u64)?,
            album,
            disc: self.disc,
            position: self.position,
            number: self.number,
            title: self.title,
            artist: self.artist,
            recording: mbid_of(self.recording)?,
            track_mbid: mbid_of(self.track_mbid)?,
            length: self
                .length_ms
                .map(|millis| Duration::from_millis(millis.max(0) as u64)),
            isrc: self.isrc,
            track: self.track.map(|id| TrackId::new(id as u64)).transpose()?,
            links,
        })
    }
}

pub(super) struct RawArtistDetail {
    pub(super) mbid: Option<String>,
    pub(super) sort_name: Option<String>,
    pub(super) kind: Option<String>,
    pub(super) gender: Option<String>,
    pub(super) country: Option<String>,
    pub(super) area: Option<String>,
    pub(super) began_in: Option<String>,
    pub(super) began: Option<String>,
    pub(super) ended: Option<String>,
    pub(super) has_ended: bool,
    pub(super) disambiguation: Option<String>,
    pub(super) asked: Option<i64>,
    pub(super) answered: Option<i64>,
    pub(super) name: String,
    pub(super) releases_unread: u32,
}

impl RawArtistDetail {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            mbid: row.get(0)?,
            sort_name: row.get(1)?,
            kind: row.get(2)?,
            gender: row.get(3)?,
            country: row.get(4)?,
            area: row.get(5)?,
            began_in: row.get(6)?,
            began: row.get(7)?,
            ended: row.get(8)?,
            has_ended: row.get(9)?,
            disambiguation: row.get(10)?,
            asked: row.get(11)?,
            answered: row.get(12)?,
            name: row.get(13)?,
            releases_unread: row.get(14)?,
        })
    }

    pub(super) fn into_detail(
        self,
        genres: Vec<Genre>,
        links: Vec<Link>,
        releases_unheld: u32,
    ) -> Result<ArtistDetail> {
        Ok(ArtistDetail {
            name: self.name,
            mbid: mbid_of(self.mbid)?,
            sort_name: self.sort_name,
            kind: self.kind,
            gender: self.gender,
            country: self.country,
            area: self.area,
            began_in: self.began_in,
            span: LifeSpan {
                begin: self.began,
                end: self.ended,
                ended: self.has_ended,
            },
            disambiguation: self.disambiguation,
            asked: self.asked.map(store::from_nanos),
            answered: self.answered.map(store::from_nanos),
            genres,
            links,
            releases_unheld,
            releases_unread: self.releases_unread,
        })
    }
}

pub(super) struct RawMissingTrack {
    pub(super) album: i64,
    pub(super) album_title: String,
    pub(super) owner: Option<String>,
    pub(super) release_track: i64,
    pub(super) disc: u32,
    pub(super) position: u32,
    pub(super) number: Option<String>,
    pub(super) title: String,
    pub(super) artist: Option<String>,
    pub(super) length_ms: Option<i64>,
    pub(super) want: Option<i64>,
}

impl RawMissingTrack {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            album: row.get(0)?,
            album_title: row.get(1)?,
            owner: row.get(2)?,
            release_track: row.get(3)?,
            disc: row.get(4)?,
            position: row.get(5)?,
            number: row.get(6)?,
            title: row.get(7)?,
            artist: row.get(8)?,
            length_ms: row.get(9)?,
            want: row.get(10)?,
        })
    }

    pub(super) fn into_missing(self) -> Result<MissingTrack> {
        Ok(MissingTrack {
            album: AlbumId::new(self.album as u64)?,
            album_title: self.album_title,
            owner: self.owner,
            release_track: ReleaseTrackId::new(self.release_track as u64)?,
            disc: self.disc,
            position: self.position,
            number: self.number,
            title: self.title,
            artist: self.artist,
            length: self
                .length_ms
                .map(|millis| Duration::from_millis(millis.max(0) as u64)),
            want: self.want.map(|id| WantId::new(id as u64)).transpose()?,
        })
    }
}

pub(super) struct RawUnheldRelease {
    pub(super) artist: i64,
    pub(super) artist_name: String,
    pub(super) mbid: String,
    pub(super) title: String,
    pub(super) kind: Option<String>,
    pub(super) first_released: Option<String>,
}

impl RawUnheldRelease {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            artist: row.get(0)?,
            artist_name: row.get(1)?,
            mbid: row.get(2)?,
            title: row.get(3)?,
            kind: row.get(4)?,
            first_released: row.get(5)?,
        })
    }

    pub(super) fn into_unheld(self) -> Result<UnheldRelease> {
        Ok(UnheldRelease {
            artist: ArtistId::new(self.artist as u64)?,
            artist_name: self.artist_name,
            mbid: Mbid::new(&self.mbid)?,
            title: self.title,
            kind: self.kind,
            first_released: self.first_released,
        })
    }
}

pub(super) struct RawWant {
    pub(super) id: i64,
    pub(super) release_track: i64,
    pub(super) album: i64,
    pub(super) album_title: String,
    pub(super) title: String,
    pub(super) artist: Option<String>,
    pub(super) wanted: i64,
    pub(super) tried: Option<i64>,
    pub(super) offered: Option<String>,
    pub(super) misses: i64,
    pub(super) recording: Option<String>,
    pub(super) track: Option<String>,
    pub(super) release: Option<String>,
    pub(super) isrcs: String,
    pub(super) length_ms: Option<i64>,
    pub(super) disc: i64,
    pub(super) position: i64,
    pub(super) held: Option<i64>,
}

impl RawWant {
    pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            release_track: row.get(1)?,
            album: row.get(2)?,
            album_title: row.get(3)?,
            title: row.get(4)?,
            artist: row.get(5)?,
            wanted: row.get(6)?,
            tried: row.get(7)?,
            offered: row.get(8)?,
            misses: row.get(9)?,
            recording: row.get(10)?,
            track: row.get(11)?,
            release: row.get(12)?,
            isrcs: row.get(13)?,
            length_ms: row.get(14)?,
            disc: row.get(15)?,
            position: row.get(16)?,
            held: row.get(17)?,
        })
    }

    pub(super) fn into_want(self, links: Vec<Link>, release_links: Vec<Link>) -> Result<Want> {
        Ok(Want {
            id: WantId::new(self.id as u64)?,
            release_track: ReleaseTrackId::new(self.release_track as u64)?,
            album: AlbumId::new(self.album as u64)?,
            album_title: self.album_title,
            title: self.title,
            artist: self.artist,
            recording: store::mbid_in(self.recording.as_deref()),
            track: store::mbid_in(self.track.as_deref()),
            release: store::mbid_in(self.release.as_deref()),
            isrcs: self
                .isrcs
                .split_whitespace()
                .filter_map(|code| store::isrc_in(Some(code)))
                .collect(),
            length: self
                .length_ms
                .map(|millis| Duration::from_millis(millis.max(0) as u64)),
            disc: self.disc.max(0) as u32,
            position: self.position.max(0) as u32,
            wanted: store::from_nanos(self.wanted),
            tried: self.tried.map(store::from_nanos),
            offered: self.offered,
            misses: u32::try_from(self.misses.max(0)).unwrap_or(u32::MAX),
            held: self
                .held
                .map(|held| TrackId::new(held as u64))
                .transpose()?,
            links,
            release_links,
        })
    }
}

pub(crate) struct UnheldCoverDue {
    pub(crate) group: Mbid,
    pub(crate) pressing: Option<Mbid>,
}

pub(super) fn held_cover(id: AlbumId, bytes: Vec<u8>, code: Option<i64>) -> Result<CoverArt> {
    let format = match code {
        Some(code) => store::image_format_from_code(code, id)?,
        None => ImageFormat::sniff(&bytes).ok_or(Error::UntypedCoverArt { album: id })?,
    };
    Ok(CoverArt { format, bytes })
}
