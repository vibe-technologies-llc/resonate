use std::{
    cmp::Reverse,
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ahash::{AHashMap, AHashSet};
use resonate_codec::{Codec, ImageFormat, ReplayGain, Sources, TagSet, probe_cover_art};
use resonate_core::{
    AlbumId, ArtistId, Decibels, FrameSpan, Frames, MediaLocation, PlaylistId, SampleFormat,
    StreamSpec, TrackId,
};
use resonate_vault::{Form, VaultKey};
use rusqlite::{
    Connection, OptionalExtension, Transaction, params, params_from_iter, types::Value,
};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::{
    Column, CoverSource, Direction, EncodedColumn, Error, Isrc, Mbid, OrderedColumn, Relation,
    Result, RowOrder, Service, SortOrder, Spellings, StoreOp, credits,
};

const UNIT_SEPARATOR: char = '\u{1f}';
const SLEEVE_TIER: &str = "sleeve";
const RELEASE_TIER: &str = "release";
const YEAR_DIGITS: usize = 4;
const DATE_DIGITS: [usize; 3] = [YEAR_DIGITS, 6, 8];
const FORGOTTEN_AT_ONCE: usize = 256;

pub const ORPHANS: &str = "
DELETE FROM albums
 WHERE id NOT IN (SELECT album_id FROM tracks WHERE album_id IS NOT NULL)
   AND (found_elsewhere IS NULL
        OR id NOT IN (SELECT rt.album_id FROM wants w
                        JOIN release_tracks rt ON rt.id = w.release_track_id));
DELETE FROM artists
 WHERE id NOT IN (SELECT artist_id FROM tracks WHERE artist_id IS NOT NULL)
   AND id NOT IN (SELECT artist_id FROM albums WHERE artist_id IS NOT NULL)
   AND id NOT IN (SELECT artist_id FROM track_credits);
";

pub struct TrackRecord {
    pub root_id: i64,
    pub path: PathBuf,
    pub sleeve: Option<PathBuf>,
    pub existing: Option<TrackId>,
    pub file_size: u64,
    pub modified: SystemTime,
    pub sheet_modified: Option<SystemTime>,
    pub spec: StreamSpec,
    pub codec: Codec,
    pub duration: Option<Frames>,
    pub span: Option<FrameSpan>,
    pub tags: TagSet,
    pub embeds_a_picture: bool,
}

#[derive(Clone, Copy)]
struct Grouped {
    id: i64,
    owner: Option<i64>,
    year: Option<i64>,
    declared: Declared,
}

#[derive(Clone, Copy)]
struct Declared {
    barcode: bool,
    catalog_number: bool,
    label: bool,
    tagged_tracks: bool,
}

struct Declaration<'a> {
    barcode: Option<&'a str>,
    catalog_number: Option<&'a str>,
    label: Option<&'a str>,
    tagged_tracks: Option<i64>,
}

impl<'a> Declaration<'a> {
    fn of(tags: &'a TagSet) -> Self {
        Self {
            barcode: named(tags.barcode.as_deref()),
            catalog_number: named(tags.catalog_number.as_deref()),
            label: named(tags.label.as_deref()),
            tagged_tracks: tags.track_total.map(i64::from),
        }
    }

    fn fills(&self, held: Declared) -> bool {
        (self.barcode.is_some() && !held.barcode)
            || (self.catalog_number.is_some() && !held.catalog_number)
            || (self.label.is_some() && !held.label)
            || (self.tagged_tracks.is_some() && !held.tagged_tracks)
    }

    fn over(&self, held: Declared) -> Declared {
        Declared {
            barcode: held.barcode || self.barcode.is_some(),
            catalog_number: held.catalog_number || self.catalog_number.is_some(),
            label: held.label || self.label.is_some(),
            tagged_tracks: held.tagged_tracks || self.tagged_tracks.is_some(),
        }
    }
}

#[derive(Clone, Copy)]
struct KnownArtist {
    id: i64,
    identified: bool,
    marks: usize,
}

struct Held {
    id: i64,
    name: String,
    key: String,
    identified: bool,
}

impl Held {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            key: row.get(2)?,
            identified: row.get(3)?,
        })
    }

    fn settled(&self) -> bool {
        self.key == folded_letters(&self.name)
    }
}

#[derive(Clone, Copy)]
struct Billing<'a> {
    name: &'a str,
    mbid: Option<&'a str>,
}

#[derive(Default)]
pub struct Cache {
    artists: HashMap<String, KnownArtist>,
    albums: HashMap<String, Grouped>,
    covered: AHashSet<i64>,
}

pub fn reconcile_artists(connection: &mut Connection) -> Result<usize> {
    let unsettled = artists_folding_together(connection)?;
    if unsettled.is_empty() {
        return Ok(0);
    }

    let tx = connection
        .transaction()
        .map_err(|source| Error::store(StoreOp::Transaction, source))?;
    let mut merged = 0;
    for (key, mut sharing) in unsettled {
        sharing.sort_by_key(|held| (Reverse(marks_in(&held.name)), held.id));
        let spelling = sharing[0].name.clone();
        sharing.sort_by_key(|held| (usize::from(!held.identified), held.id));
        let (keeps, gone) = sharing
            .split_first()
            .expect("a group holds at least one row");

        for held in gone {
            take_over_artist(&tx, held.id, keeps.id)?;
            merged += 1;
        }
        rekey_artist(&tx, keeps.id, &key, &spelling)?;
        if !gone.is_empty() {
            reindex_the_tracks_of(&tx, keeps.id)?;
        }
    }
    tx.commit()
        .map_err(|source| Error::store(StoreOp::Transaction, source))?;
    tracing::info!(merged, "artists were keyed by the fold of their names");

    Ok(merged)
}

fn artists_folding_together(connection: &Connection) -> Result<Vec<(String, Vec<Held>)>> {
    let mut statement = connection
        .prepare("SELECT id, name, key, mbid IS NOT NULL FROM artists")
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let held = statement
        .query_map([], Held::read)
        .and_then(Iterator::collect::<rusqlite::Result<Vec<Held>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut folded: AHashMap<String, Vec<Held>> = AHashMap::new();
    for one in held {
        folded
            .entry(folded_letters(&one.name))
            .or_default()
            .push(one);
    }

    Ok(folded
        .into_iter()
        .filter(|(_, sharing)| sharing.len() > 1 || !sharing[0].settled())
        .collect())
}

fn take_over_artist(tx: &Transaction<'_>, gone: i64, keeps: i64) -> Result<()> {
    for statement in [
        "UPDATE tracks SET artist_id = ?2 WHERE artist_id = ?1",
        "UPDATE albums SET artist_id = ?2 WHERE artist_id = ?1",
        "UPDATE OR IGNORE artist_genres SET artist_id = ?2 WHERE artist_id = ?1",
        "UPDATE OR IGNORE artist_links SET artist_id = ?2 WHERE artist_id = ?1",
        "UPDATE OR IGNORE artist_releases SET artist_id = ?2 WHERE artist_id = ?1",
        "UPDATE OR IGNORE track_credits SET artist_id = ?2 WHERE artist_id = ?1",
    ] {
        tx.execute(statement, params![gone, keeps])
            .map_err(|source| Error::store(StoreOp::Update, source))?;
    }
    tx.execute(
        "UPDATE artists SET
             favourite       = coalesce(min(artists.favourite, o.favourite),
                                        artists.favourite, o.favourite),
             portrait_format = CASE WHEN artists.portrait IS NULL
                                    THEN o.portrait_format ELSE artists.portrait_format END,
             portrait        = coalesce(artists.portrait, o.portrait)
          FROM (SELECT * FROM artists WHERE id = ?1) AS o
         WHERE artists.id = ?2",
        params![gone, keeps],
    )
    .map_err(|source| Error::store(StoreOp::Update, source))?;
    tx.execute("DELETE FROM artists WHERE id = ?1", params![gone])
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Delete, source))
}

fn rekey_artist(tx: &Transaction<'_>, artist: i64, key: &str, name: &str) -> Result<()> {
    tx.execute(
        "UPDATE artists SET key = ?2, name = ?3 WHERE id = ?1",
        params![artist, key, name],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub const fn format_code(format: SampleFormat) -> i64 {
    match format {
        SampleFormat::S16 => 0,
        SampleFormat::S24 => 1,
        SampleFormat::S32 => 2,
        SampleFormat::F32 => 3,
    }
}

pub(crate) const fn format_named_by(code: i64) -> Option<SampleFormat> {
    match code {
        0 => Some(SampleFormat::S16),
        1 => Some(SampleFormat::S24),
        2 => Some(SampleFormat::S32),
        3 => Some(SampleFormat::F32),
        _ => None,
    }
}

pub fn format_from_code(code: i64, track: TrackId) -> Result<SampleFormat> {
    format_named_by(code).ok_or(Error::UnknownEncoding {
        track,
        column: EncodedColumn::SampleFormat,
        code,
    })
}

pub fn vault_format_of(key: VaultKey, code: i64) -> Result<SampleFormat> {
    format_named_by(code).ok_or(Error::UnknownVaultEncoding {
        key,
        column: EncodedColumn::SampleFormat,
        code,
    })
}

pub const fn codec_code(codec: Codec) -> i64 {
    match codec {
        Codec::Unknown => 0,
        Codec::Flac => 1,
        Codec::Alac => 2,
        Codec::Pcm => 5,
        Codec::Aac => 6,
        Codec::Mp3 => 7,
        Codec::Vorbis => 8,
        Codec::Opus => 9,
        Codec::Dsd => 10,
    }
}

const READ_BACKWARDS: i64 = 16;

pub const fn sort_code(sort: SortOrder, reading: Direction) -> i64 {
    let order = match sort {
        SortOrder::Relevance => 0,
        SortOrder::AlbumThenTrack => 1,
        SortOrder::Title => 2,
        SortOrder::Artist => 3,
        SortOrder::DateAdded => 4,
        SortOrder::Duration => 5,
        SortOrder::Plays => 6,
        SortOrder::Played => 7,
        SortOrder::Favourited => 8,
    };

    match reading {
        Direction::Ascending => order,
        Direction::Descending => order + READ_BACKWARDS,
    }
}

pub const fn sort_of(playlist: PlaylistId, code: i64) -> Result<(SortOrder, Direction)> {
    let reading = if code >= READ_BACKWARDS {
        Direction::Descending
    } else {
        Direction::Ascending
    };
    let sort = match code % READ_BACKWARDS {
        0 => SortOrder::Relevance,
        1 => SortOrder::AlbumThenTrack,
        2 => SortOrder::Title,
        3 => SortOrder::Artist,
        4 => SortOrder::DateAdded,
        5 => SortOrder::Duration,
        6 => SortOrder::Plays,
        7 => SortOrder::Played,
        8 => SortOrder::Favourited,
        _ => {
            return Err(Error::UnknownOrder {
                playlist,
                column: OrderedColumn::Sort,
                code,
            });
        }
    };

    if code < 0 || code >= READ_BACKWARDS * 2 {
        return Err(Error::UnknownOrder {
            playlist,
            column: OrderedColumn::Sort,
            code,
        });
    }

    Ok((sort, reading))
}

pub const fn row_order_code(order: RowOrder) -> i64 {
    match order {
        RowOrder::Album => 0,
        RowOrder::Artist => 1,
        RowOrder::Title => 2,
        RowOrder::Length => 3,
        RowOrder::File => 4,
    }
}

pub const fn row_order_of(playlist: PlaylistId, code: i64) -> Result<RowOrder> {
    match code {
        0 => Ok(RowOrder::Album),
        1 => Ok(RowOrder::Artist),
        2 => Ok(RowOrder::Title),
        3 => Ok(RowOrder::Length),
        4 => Ok(RowOrder::File),
        code => Err(Error::UnknownOrder {
            playlist,
            column: OrderedColumn::KeptOrder,
            code,
        }),
    }
}

pub const fn direction_code(direction: Direction) -> i64 {
    match direction {
        Direction::Ascending => 0,
        Direction::Descending => 1,
    }
}

pub const fn direction_of(playlist: PlaylistId, code: i64) -> Result<Direction> {
    match code {
        0 => Ok(Direction::Ascending),
        1 => Ok(Direction::Descending),
        code => Err(Error::UnknownOrder {
            playlist,
            column: OrderedColumn::KeptReading,
            code,
        }),
    }
}

pub const fn image_format_code(format: ImageFormat) -> i64 {
    match format {
        ImageFormat::Jpeg => 0,
        ImageFormat::Png => 1,
        ImageFormat::Webp => 2,
        ImageFormat::Gif => 3,
        ImageFormat::Bmp => 4,
    }
}

pub fn image_format_from_code(code: i64, album: AlbumId) -> Result<ImageFormat> {
    match code {
        0 => Ok(ImageFormat::Jpeg),
        1 => Ok(ImageFormat::Png),
        2 => Ok(ImageFormat::Webp),
        3 => Ok(ImageFormat::Gif),
        4 => Ok(ImageFormat::Bmp),
        code => Err(Error::UnknownImageFormat { album, code }),
    }
}

pub const fn cover_source_code(source: CoverSource) -> i64 {
    match source {
        CoverSource::File => 0,
        CoverSource::Archive => 1,
        CoverSource::Vault => 2,
    }
}

pub const fn cover_source_of(album: AlbumId, code: i64) -> Result<CoverSource> {
    match code {
        0 => Ok(CoverSource::File),
        1 => Ok(CoverSource::Archive),
        2 => Ok(CoverSource::Vault),
        code => Err(Error::UnknownCoverSource { album, code }),
    }
}

pub const fn vault_form_code(form: Form) -> i64 {
    match form {
        Form::Flac => 0,
        Form::Wave => 1,
        Form::Kept => 2,
    }
}

pub const fn vault_form_of(code: i64) -> Result<Form> {
    match code {
        0 => Ok(Form::Flac),
        1 => Ok(Form::Wave),
        2 => Ok(Form::Kept),
        code => Err(Error::UnknownVaultForm { code }),
    }
}

pub const fn relation_code(relation: Relation) -> i64 {
    match relation {
        Relation::Streaming => 0,
        Relation::FreeStreaming => 1,
        Relation::PurchaseForDownload => 2,
        Relation::PurchaseForMailOrder => 3,
        Relation::Homepage => 4,
        Relation::Wikipedia => 5,
        Relation::Wikidata => 6,
        Relation::Discogs => 7,
        Relation::AllMusic => 8,
        Relation::Image => 9,
        Relation::SocialNetwork => 10,
        Relation::Lyrics => 11,
        Relation::Youtube => 12,
        Relation::YoutubeMusic => 13,
        Relation::Soundcloud => 14,
        Relation::Bandcamp => 15,
        Relation::LastFm => 16,
        Relation::Other => 17,
    }
}

pub const fn relation_of(code: i64) -> Result<Relation> {
    match code {
        0 => Ok(Relation::Streaming),
        1 => Ok(Relation::FreeStreaming),
        2 => Ok(Relation::PurchaseForDownload),
        3 => Ok(Relation::PurchaseForMailOrder),
        4 => Ok(Relation::Homepage),
        5 => Ok(Relation::Wikipedia),
        6 => Ok(Relation::Wikidata),
        7 => Ok(Relation::Discogs),
        8 => Ok(Relation::AllMusic),
        9 => Ok(Relation::Image),
        10 => Ok(Relation::SocialNetwork),
        11 => Ok(Relation::Lyrics),
        12 => Ok(Relation::Youtube),
        13 => Ok(Relation::YoutubeMusic),
        14 => Ok(Relation::Soundcloud),
        15 => Ok(Relation::Bandcamp),
        16 => Ok(Relation::LastFm),
        17 => Ok(Relation::Other),
        code => Err(Error::UnknownLinkCode {
            column: EncodedColumn::Relation,
            code,
        }),
    }
}

pub const fn service_code(service: Service) -> i64 {
    match service {
        Service::Spotify => 0,
        Service::Tidal => 1,
        Service::AppleMusic => 2,
        Service::Deezer => 3,
        Service::Qobuz => 4,
        Service::AmazonMusic => 5,
        Service::Youtube => 6,
        Service::YoutubeMusic => 7,
        Service::Bandcamp => 8,
        Service::Soundcloud => 9,
        Service::Beatport => 10,
        Service::SevenDigital => 11,
        Service::Discogs => 12,
        Service::Wikipedia => 13,
        Service::Wikidata => 14,
        Service::AllMusic => 15,
        Service::LastFm => 16,
        Service::WikimediaCommons => 17,
        Service::Facebook => 18,
        Service::Instagram => 19,
        Service::Twitter => 20,
        Service::Other => 21,
    }
}

pub const fn service_of(code: i64) -> Result<Service> {
    match code {
        0 => Ok(Service::Spotify),
        1 => Ok(Service::Tidal),
        2 => Ok(Service::AppleMusic),
        3 => Ok(Service::Deezer),
        4 => Ok(Service::Qobuz),
        5 => Ok(Service::AmazonMusic),
        6 => Ok(Service::Youtube),
        7 => Ok(Service::YoutubeMusic),
        8 => Ok(Service::Bandcamp),
        9 => Ok(Service::Soundcloud),
        10 => Ok(Service::Beatport),
        11 => Ok(Service::SevenDigital),
        12 => Ok(Service::Discogs),
        13 => Ok(Service::Wikipedia),
        14 => Ok(Service::Wikidata),
        15 => Ok(Service::AllMusic),
        16 => Ok(Service::LastFm),
        17 => Ok(Service::WikimediaCommons),
        18 => Ok(Service::Facebook),
        19 => Ok(Service::Instagram),
        20 => Ok(Service::Twitter),
        21 => Ok(Service::Other),
        code => Err(Error::UnknownLinkCode {
            column: EncodedColumn::Service,
            code,
        }),
    }
}

pub fn portrait_format_of(code: i64, artist: ArtistId) -> Result<ImageFormat> {
    match code {
        0 => Ok(ImageFormat::Jpeg),
        1 => Ok(ImageFormat::Png),
        2 => Ok(ImageFormat::Webp),
        3 => Ok(ImageFormat::Gif),
        4 => Ok(ImageFormat::Bmp),
        code => Err(Error::UnknownPortraitFormat { artist, code }),
    }
}

pub(crate) const fn codec_named_by(code: i64) -> Option<Codec> {
    match code {
        0 => Some(Codec::Unknown),
        1 => Some(Codec::Flac),
        2 => Some(Codec::Alac),
        5 => Some(Codec::Pcm),
        6 => Some(Codec::Aac),
        7 => Some(Codec::Mp3),
        8 => Some(Codec::Vorbis),
        9 => Some(Codec::Opus),
        10 => Some(Codec::Dsd),
        _ => None,
    }
}

pub fn codec_from_code(code: i64, track: TrackId) -> Result<Codec> {
    codec_named_by(code).ok_or(Error::UnknownEncoding {
        track,
        column: EncodedColumn::Codec,
        code,
    })
}

pub fn vault_codec_of(key: VaultKey, code: i64) -> Result<Codec> {
    codec_named_by(code).ok_or(Error::UnknownVaultEncoding {
        key,
        column: EncodedColumn::Codec,
        code,
    })
}

pub fn span(start: i64, frames: Option<i64>) -> Option<FrameSpan> {
    let start = Frames(start.max(0) as u64);
    match frames {
        Some(frames) => Some(FrameSpan::between(
            start,
            start.saturating_add(Frames(frames.max(0) as u64)),
        )),
        None if start == Frames::ZERO => None,
        None => Some(FrameSpan::starting(start)),
    }
}

pub const fn span_columns(span: Option<FrameSpan>) -> (i64, Option<i64>) {
    match span {
        Some(span) => (
            span.start().get() as i64,
            match span.frames() {
                Some(frames) => Some(frames.get() as i64),
                None => None,
            },
        ),
        None => (0, None),
    }
}

pub fn to_nanos(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_nanos()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_nanos())
            .map_or(i64::MIN, |nanos| nanos.saturating_neg()),
    }
}

pub fn from_nanos(nanos: i64) -> SystemTime {
    match u64::try_from(nanos) {
        Ok(nanos) => UNIX_EPOCH + Duration::from_nanos(nanos),
        Err(_) => UNIX_EPOCH - Duration::from_nanos(nanos.unsigned_abs()),
    }
}

pub fn decibels(value: Option<f64>) -> Option<Decibels> {
    Decibels::new(value? as f32).ok()
}

pub fn album_key(title: &str, owner: Option<&str>) -> String {
    format!(
        "{}{UNIT_SEPARATOR}{}",
        title.to_lowercase(),
        owner.unwrap_or_default().to_lowercase()
    )
}

pub fn sleeve_key(title: &str, folder: &Path) -> String {
    format!(
        "{UNIT_SEPARATOR}{SLEEVE_TIER}{UNIT_SEPARATOR}{}{UNIT_SEPARATOR}{}",
        title.to_lowercase(),
        folder.display()
    )
}

pub fn release_key(release: &str) -> String {
    format!(
        "{UNIT_SEPARATOR}{RELEASE_TIER}{UNIT_SEPARATOR}{}",
        release.to_lowercase()
    )
}

pub fn is_keyed_by_its_folder(key: &str) -> bool {
    tier_of(key) == Some(SLEEVE_TIER)
}

fn tier_of(key: &str) -> Option<&str> {
    key.strip_prefix(UNIT_SEPARATOR)?
        .split(UNIT_SEPARATOR)
        .next()
}

fn grouping_keys(record: &TrackRecord, title: &str, owner: Option<&str>) -> Vec<String> {
    let tags = &record.tags;
    if let Some(release) = named(tags.musicbrainz_album_id.as_deref()) {
        return vec![release_key(release)];
    }

    let mut naming = Vec::new();
    if !tags.compilation
        && let Some(owner) = named(tags.album_artist.as_deref())
    {
        naming.push(album_key(title, Some(owner)));
    }
    if let Some(folder) = record.sleeve.as_deref() {
        naming.push(sleeve_key(title, folder));
    }
    if naming.is_empty() {
        naming.push(album_key(title, owner));
    }

    naming
}

fn named(value: Option<&str>) -> Option<&str> {
    let named = value?.trim();

    (!named.is_empty()).then_some(named)
}

pub fn mbid_in(text: Option<&str>) -> Option<Mbid> {
    let text = named(text)?;
    match Mbid::new(text) {
        Ok(mbid) => Some(mbid),
        Err(error) => {
            tracing::debug!(%error, text, "a MusicBrainz id tag holds no id");
            None
        }
    }
}

pub fn isrc_in(text: Option<&str>) -> Option<Isrc> {
    let text = named(text)?;
    match Isrc::new(text) {
        Ok(isrc) => Some(isrc),
        Err(error) => {
            tracing::debug!(%error, text, "an ISRC tag holds no recording code");
            None
        }
    }
}

fn attribution(tags: &TagSet) -> (Option<Billing<'_>>, Option<Billing<'_>>) {
    let album_artist = tags.album_artist.as_deref().map(|name| Billing {
        name,
        mbid: tags.musicbrainz_album_artist_id.as_deref(),
    });
    let artist = tags.artist.as_deref().map(|name| Billing {
        name,
        mbid: tags.musicbrainz_artist_id.as_deref(),
    });

    if tags.compilation {
        return (None, artist.or(album_artist));
    }
    let owner = album_artist.or(artist);
    (owner, owner)
}

pub fn apply(
    tx: &Transaction<'_>,
    cache: &mut Cache,
    record: &TrackRecord,
    generation: i64,
    extract_cover_art: bool,
) -> Result<()> {
    let (owner, performer) = attribution(&record.tags);
    let owner_id = match owner {
        Some(billed) => Some(artist(tx, cache, billed)?),
        None => None,
    };
    let performer_id = match performer {
        Some(billed) if owner.is_none_or(|owner| owner.name != billed.name) => {
            Some(artist(tx, cache, billed)?)
        }
        _ => owner_id,
    };

    let album_id = match record.tags.album.as_deref() {
        Some(title) => Some(album(
            tx,
            cache,
            title,
            owner.map(|billed| billed.name),
            owner_id,
            record,
            extract_cover_art,
        )?),
        None => None,
    };

    let stored = track(tx, record, performer_id, album_id, generation)?;
    index(tx, &stored, record)
}

pub fn register_root(tx: &Transaction<'_>, canonical: &Path) -> Result<i64> {
    let held = roots_held(tx)?;
    if let Some((_, outer)) = held
        .iter()
        .find(|(_, held)| canonical != held && canonical.starts_with(held))
    {
        return Err(Error::RootInsideRoot {
            path: canonical.to_path_buf(),
            inside: outer.clone(),
        });
    }

    let text = path_text(canonical)?;
    tx.execute(
        "INSERT INTO roots (path) VALUES (?1) ON CONFLICT(path) DO NOTHING",
        params![text],
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;

    let id = tx
        .query_row(
            "SELECT id FROM roots WHERE path = ?1",
            params![text],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let taken_in: Vec<i64> = held
        .iter()
        .filter(|(_, held)| canonical != held && held.starts_with(canonical))
        .map(|(inner, _)| *inner)
        .collect();
    take_in(tx, id, &taken_in)?;

    Ok(id)
}

fn roots_held(tx: &Transaction<'_>) -> Result<Vec<(i64, PathBuf)>> {
    let mut statement = tx
        .prepare("SELECT id, path FROM roots ORDER BY path")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                PathBuf::from(row.get::<_, String>(1)?),
            ))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn take_in(tx: &Transaction<'_>, root: i64, taken: &[i64]) -> Result<()> {
    if taken.is_empty() {
        return Ok(());
    }
    let scoped = taken
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");

    tx.execute(
        &format!("UPDATE tracks SET root_id = ?1 WHERE root_id IN ({scoped})"),
        params![root],
    )
    .map_err(|source| Error::store(StoreOp::Update, source))?;
    tx.execute_batch(&format!("DELETE FROM roots WHERE id IN ({scoped})"))
        .map_err(|source| Error::store(StoreOp::Delete, source))
}

pub fn touch(tx: &Transaction<'_>, id: TrackId, generation: i64) -> Result<()> {
    tx.execute(
        "UPDATE tracks SET seen = ?1 WHERE id = ?2",
        params![generation, id.get() as i64],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub fn prune(tx: &Transaction<'_>, roots: &[i64], generation: i64) -> Result<u64> {
    if roots.is_empty() {
        return Ok(0);
    }
    let scoped = roots
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");

    let removed = tx
        .execute(
            &format!(
                "DELETE FROM tracks WHERE seen != ?1 AND root_id IN ({scoped})
                    AND vault_key IS NULL"
            ),
            params![generation],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    let superseded = superseded_in_the_vault(tx, &scoped, generation)?;

    sweep_orphans(tx)?;

    Ok(removed as u64 + superseded)
}

fn superseded_in_the_vault(tx: &Transaction<'_>, scoped: &str, generation: i64) -> Result<u64> {
    let unseen: Vec<(i64, String)> = tx
        .prepare(&format!(
            "SELECT id, path FROM tracks WHERE seen != ?1 AND root_id IN ({scoped})
                AND vault_key IS NOT NULL"
        ))
        .and_then(|mut statement| {
            statement
                .query_map(params![generation], |row| Ok((row.get(0)?, row.get(1)?)))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        })
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut removed = 0;
    for (id, path) in unseen {
        if !Path::new(&path).exists() {
            continue;
        }
        removed += tx
            .execute("DELETE FROM tracks WHERE id = ?1", params![id])
            .map_err(|source| Error::store(StoreOp::Delete, source))?;
    }
    Ok(removed as u64)
}

pub fn settle_the_credits(connection: &mut Connection) -> Result<()> {
    let tx = connection
        .transaction()
        .map_err(|source| Error::store(StoreOp::Transaction, source))?;
    sweep_orphans(&tx)?;
    tx.commit()
        .map_err(|source| Error::store(StoreOp::Transaction, source))
}

pub fn sweep_orphans(tx: &Transaction<'_>) -> Result<()> {
    credits::credit_the_members(tx)?;
    tx.execute_batch(ORPHANS)
        .map_err(|source| Error::store(StoreOp::Delete, source))
}

pub fn paths_under(connection: &Connection, root: i64) -> Result<Vec<PathBuf>> {
    let mut statement = connection
        .prepare("SELECT DISTINCT path FROM tracks WHERE root_id = ?1 AND vault_key IS NULL")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let found = statement
        .query_map(params![root], |row| row.get::<_, String>(0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(found.into_iter().map(PathBuf::from).collect())
}

pub fn rooted_paths_at(
    connection: &Connection,
    path: &str,
    from: &str,
    past: &str,
) -> Result<Vec<(i64, PathBuf)>> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT root_id, path FROM tracks
             WHERE root_id IS NOT NULL AND vault_key IS NULL
               AND (path = ?1 OR (path >= ?2 AND path < ?3))",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let found = statement
        .query_map(params![path, from, past], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(found
        .into_iter()
        .map(|(root, path)| (root, PathBuf::from(path)))
        .collect())
}

pub fn forget_paths(tx: &Transaction<'_>, root: i64, paths: &[PathBuf]) -> Result<u64> {
    let mut forgotten = 0;
    for batch in paths.chunks(FORGOTTEN_AT_ONCE) {
        let held = vec!["?"; batch.len()].join(",");
        let mut statement = tx
            .prepare(&format!(
                "DELETE FROM tracks WHERE root_id = ?1 AND vault_key IS NULL AND path IN ({held})"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let mut bound: Vec<Value> = Vec::with_capacity(batch.len() + 1);
        bound.push(Value::Integer(root));
        for path in batch {
            bound.push(Value::Text(path_text(path)?.to_owned()));
        }
        forgotten += statement
            .execute(params_from_iter(bound))
            .map_err(|source| Error::store(StoreOp::Delete, source))?;
    }

    Ok(forgotten as u64)
}

pub fn spellings(connection: &Connection) -> Result<Spellings> {
    let mut spellings = Spellings::default();

    let mut rows = connection
        .prepare("SELECT title, artist, genre FROM tracks")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let mut read = rows
        .query([])
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    while let Some(row) = read
        .next()
        .map_err(|source| Error::store(StoreOp::Query, source))?
    {
        let title: String = row
            .get(0)
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        let artist: Option<String> = row
            .get(1)
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        let genre: Option<String> = row
            .get(2)
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        spellings.taking(Column::Title, &title);
        if let Some(artist) = artist {
            spellings.taking(Column::Artist, &artist);
        }
        if let Some(genre) = genre {
            spellings.taking(Column::Genre, &genre);
        }
    }

    for (column, named) in [
        (Column::Artist, "SELECT name FROM artists"),
        (Column::Album, "SELECT title FROM albums"),
        (Column::Genre, "SELECT name FROM artist_genres"),
    ] {
        let mut statement = connection
            .prepare(named)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let found = statement
            .query_map([], |row| row.get::<_, String>(0))
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        for name in found {
            spellings.taking(column, &name);
        }
    }

    Ok(spellings)
}

pub fn folded_letters(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for letter in text.to_lowercase().nfd() {
        if is_combining_mark(letter) {
            continue;
        }
        match spelled_out(letter) {
            Some(plainly) => folded.push_str(plainly),
            None => folded.push(letter),
        }
    }

    folded
}

const fn spelled_out(letter: char) -> Option<&'static str> {
    Some(match letter {
        'ł' => "l",
        'ø' => "o",
        'đ' | 'ð' => "d",
        'þ' => "th",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        'ı' => "i",
        'ħ' => "h",
        'ŋ' => "n",
        'ŧ' => "t",
        'ĸ' => "k",
        'ſ' => "s",
        _ => return None,
    })
}

pub(crate) fn marks_in(name: &str) -> usize {
    name.chars().filter(|letter| !letter.is_ascii()).count()
}

fn artist(tx: &Transaction<'_>, cache: &mut Cache, billed: Billing<'_>) -> Result<i64> {
    let key = folded_letters(billed.name);
    let mbid = mbid_in(billed.mbid);
    let marks = marks_in(billed.name);
    if let Some(known) = cache.artists.get_mut(&key) {
        if !known.identified
            && let Some(mbid) = &mbid
        {
            fill_artist_mbid(tx, known.id, mbid)?;
            known.identified = true;
        }
        if marks > known.marks {
            respell_artist(tx, known.id, billed.name)?;
            known.marks = marks;
        }
        return Ok(known.id);
    }

    let known = match held_artist(tx, &key)? {
        Some(held) => {
            let held_marks = marks_in(&held.name);
            if marks > held_marks {
                respell_artist(tx, held.id, billed.name)?;
            }
            if !held.identified
                && let Some(mbid) = &mbid
            {
                fill_artist_mbid(tx, held.id, mbid)?;
            }
            KnownArtist {
                id: held.id,
                identified: held.identified || mbid.is_some(),
                marks: marks.max(held_marks),
            }
        }
        None => tx
            .query_row(
                "INSERT INTO artists (key, name, mbid) VALUES (?1, ?2, ?3)
                 RETURNING id, mbid IS NOT NULL",
                params![key, billed.name, mbid.as_ref().map(Mbid::as_str)],
                |row| {
                    Ok(KnownArtist {
                        id: row.get(0)?,
                        identified: row.get(1)?,
                        marks,
                    })
                },
            )
            .map_err(|source| Error::store(StoreOp::Insert, source))?,
    };

    cache.artists.insert(key, known);
    Ok(known.id)
}

pub(crate) fn artist_named_in(tx: &Transaction<'_>, name: &str, mbid: Option<&str>) -> Result<i64> {
    artist(tx, &mut Cache::default(), Billing { name, mbid })
}

fn held_artist(tx: &Transaction<'_>, key: &str) -> Result<Option<Held>> {
    tx.query_row(
        "SELECT id, name, key, mbid IS NOT NULL FROM artists WHERE key = ?1",
        params![key],
        Held::read,
    )
    .optional()
    .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn artist_named(tx: &Transaction<'_>, name: &str, mbid: Option<&Mbid>) -> Result<i64> {
    let key = folded_letters(name);
    if let Some(held) = held_artist(tx, &key)? {
        if !held.identified
            && let Some(mbid) = mbid
        {
            fill_artist_mbid(tx, held.id, mbid)?;
        }
        return Ok(held.id);
    }

    tx.query_row(
        "INSERT INTO artists (key, name, mbid) VALUES (?1, ?2, ?3) RETURNING id",
        params![key, name, mbid.map(Mbid::as_str)],
        |row| row.get(0),
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))
}

fn respell_artist(tx: &Transaction<'_>, artist: i64, name: &str) -> Result<()> {
    tx.execute(
        "UPDATE artists SET name = ?1 WHERE id = ?2",
        params![name, artist],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

fn fill_artist_mbid(tx: &Transaction<'_>, artist: i64, mbid: &Mbid) -> Result<()> {
    tx.execute(
        "UPDATE artists SET mbid = ?1 WHERE id = ?2 AND mbid IS NULL",
        params![mbid.as_str(), artist],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

fn album(
    tx: &Transaction<'_>,
    cache: &mut Cache,
    title: &str,
    owner: Option<&str>,
    owner_id: Option<i64>,
    record: &TrackRecord,
    extract_cover_art: bool,
) -> Result<i64> {
    let naming = grouping_keys(record, title, owner);
    let year = record.tags.date.as_deref().and_then(year);
    let mbid = mbid_in(record.tags.musicbrainz_album_id.as_deref());
    let release_group = mbid_in(record.tags.musicbrainz_release_group_id.as_deref());
    let declaration = Declaration::of(&record.tags);

    let held = naming.iter().find_map(|key| cache.albums.get(key).copied());

    let id = match held {
        Some(mut held) => {
            if held.owner.is_some() && held.owner != owner_id {
                disown(tx, held.id)?;
                held.owner = None;
            }
            if let Some(year) = year
                && held.year.is_none()
            {
                fill_year(tx, held.id, year)?;
                held.year = Some(year);
            }
            if declaration.fills(held.declared) {
                fill_declared(tx, held.id, &declaration)?;
                held.declared = declaration.over(held.declared);
            }
            name_the_album(tx, cache, &naming, held)?;
            held.id
        }
        None => {
            let declared = Tagged {
                title,
                owner_id,
                year,
                mbid: mbid.as_ref().map(Mbid::as_str),
                release_group: release_group.as_ref().map(Mbid::as_str),
                declaration: &declaration,
            };
            let grouped = match album_already_named(tx, &naming)? {
                Some(held) => fill_album(tx, held, &declared)?,
                None => make_album(tx, &declared)?,
            };
            name_the_album(tx, cache, &naming, grouped)?;
            grouped.id
        }
    };

    if extract_cover_art && !cache.covered.contains(&id) && cover(tx, id, record)? {
        cache.covered.insert(id);
    }
    Ok(id)
}

struct Tagged<'a> {
    title: &'a str,
    owner_id: Option<i64>,
    year: Option<i64>,
    mbid: Option<&'a str>,
    release_group: Option<&'a str>,
    declaration: &'a Declaration<'a>,
}

pub(crate) fn album_keyed(tx: &Transaction<'_>, key: &str) -> Result<Option<i64>> {
    tx.query_row(
        "SELECT album_id FROM album_keys WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn keys_of(tx: &Transaction<'_>, album: i64) -> Result<Vec<String>> {
    let mut statement = tx
        .prepare_cached("SELECT key FROM album_keys WHERE album_id = ?1 ORDER BY key")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![album], |row| row.get(0))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn key_album(tx: &Transaction<'_>, key: &str, album: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO album_keys (key, album_id) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET album_id = excluded.album_id",
        params![key, album],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Insert, source))
}

pub(crate) fn re_key_album(tx: &Transaction<'_>, was: &str, now: &str) -> Result<()> {
    tx.execute(
        "UPDATE album_keys SET key = ?2 WHERE key = ?1",
        params![was, now],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

fn fill_album(tx: &Transaction<'_>, album: i64, tagged: &Tagged<'_>) -> Result<Grouped> {
    tx.query_row(
        "UPDATE albums SET
             title          = ?2,
             artist_id      = CASE WHEN artist_id IS ?3 THEN artist_id END,
             year           = coalesce(year, ?4),
             mbid           = coalesce(?5, mbid),
             release_group  = coalesce(?6, release_group),
             barcode        = coalesce(?7, barcode),
             catalog_number = coalesce(?8, catalog_number),
             label          = coalesce(?9, label),
             tagged_tracks  = coalesce(?10, tagged_tracks)
         WHERE id = ?1
         RETURNING id, artist_id, year, barcode IS NOT NULL,
                   catalog_number IS NOT NULL, label IS NOT NULL,
                   tagged_tracks IS NOT NULL",
        params![
            album,
            tagged.title,
            tagged.owner_id,
            tagged.year,
            tagged.mbid,
            tagged.release_group,
            tagged.declaration.barcode,
            tagged.declaration.catalog_number,
            tagged.declaration.label,
            tagged.declaration.tagged_tracks,
        ],
        grouped_row,
    )
    .map_err(|source| Error::store(StoreOp::Update, source))
}

fn album_already_named(tx: &Transaction<'_>, naming: &[String]) -> Result<Option<i64>> {
    for key in naming {
        if let Some(held) = album_keyed(tx, key)? {
            return Ok(Some(held));
        }
    }

    Ok(None)
}

fn name_the_album(
    tx: &Transaction<'_>,
    cache: &mut Cache,
    naming: &[String],
    grouped: Grouped,
) -> Result<()> {
    for key in naming {
        if cache.albums.insert(key.clone(), grouped).is_some() {
            continue;
        }
        tx.execute(
            "INSERT INTO album_keys (key, album_id) VALUES (?1, ?2) ON CONFLICT(key) DO NOTHING",
            params![key, grouped.id],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    Ok(())
}

fn make_album(tx: &Transaction<'_>, tagged: &Tagged<'_>) -> Result<Grouped> {
    let grouped = tx
        .query_row(
            "INSERT INTO albums (title, artist_id, year, mbid, release_group,
                                 barcode, catalog_number, label, tagged_tracks)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             RETURNING id, artist_id, year, barcode IS NOT NULL,
                       catalog_number IS NOT NULL, label IS NOT NULL,
                       tagged_tracks IS NOT NULL",
            params![
                tagged.title,
                tagged.owner_id,
                tagged.year,
                tagged.mbid,
                tagged.release_group,
                tagged.declaration.barcode,
                tagged.declaration.catalog_number,
                tagged.declaration.label,
                tagged.declaration.tagged_tracks,
            ],
            grouped_row,
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;

    Ok(grouped)
}

fn grouped_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Grouped> {
    Ok(Grouped {
        id: row.get(0)?,
        owner: row.get(1)?,
        year: row.get(2)?,
        declared: Declared {
            barcode: row.get(3)?,
            catalog_number: row.get(4)?,
            label: row.get(5)?,
            tagged_tracks: row.get(6)?,
        },
    })
}

fn disown(tx: &Transaction<'_>, album: i64) -> Result<()> {
    tx.execute(
        "UPDATE albums SET artist_id = NULL WHERE id = ?1",
        params![album],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

fn fill_year(tx: &Transaction<'_>, album: i64, year: i64) -> Result<()> {
    tx.execute(
        "UPDATE albums SET year = ?1 WHERE id = ?2 AND year IS NULL",
        params![year, album],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

fn fill_declared(tx: &Transaction<'_>, album: i64, declared: &Declaration<'_>) -> Result<()> {
    tx.execute(
        "UPDATE albums SET
             barcode        = coalesce(barcode, ?1),
             catalog_number = coalesce(catalog_number, ?2),
             label          = coalesce(label, ?3),
             tagged_tracks  = coalesce(tagged_tracks, ?4)
         WHERE id = ?5",
        params![
            declared.barcode,
            declared.catalog_number,
            declared.label,
            declared.tagged_tracks,
            album
        ],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

fn cover(tx: &Transaction<'_>, album: i64, record: &TrackRecord) -> Result<bool> {
    let present = tx
        .query_row(
            "SELECT (cover_art IS NOT NULL AND cover_source = ?2) OR cover_path IS NOT NULL
               FROM albums WHERE id = ?1",
            params![album, cover_source_code(CoverSource::File)],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    if present {
        return Ok(true);
    }

    if !record.embeds_a_picture {
        return Ok(false);
    }

    let art = match probe_cover_art(&Sources::local(), &MediaLocation::local(&record.path)) {
        Ok(Some(art)) => art,
        Ok(None) => return Ok(false),
        Err(error) => {
            tracing::debug!(%error, path = %record.path.display(), "no cover art could be read");
            return Ok(false);
        }
    };

    tx.execute(
        "UPDATE albums SET cover_art = ?1, cover_format = ?2, cover_source = ?3
          WHERE id = ?4 AND cover_path IS NULL",
        params![
            art.bytes,
            image_format_code(art.format),
            cover_source_code(CoverSource::File),
            album
        ],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))?;

    Ok(true)
}

fn track(
    tx: &Transaction<'_>,
    record: &TrackRecord,
    artist_id: Option<i64>,
    album_id: Option<i64>,
    generation: i64,
) -> Result<Stored> {
    let ReplayGain {
        track_gain,
        track_peak,
        album_gain,
        album_peak,
    } = record.tags.replay_gain;
    let now = to_nanos(SystemTime::now());
    let path = path_text(&record.path)?;
    let (span_start, span_frames) = span_columns(record.span);

    tx.query_row(
        "INSERT INTO tracks (
             root_id, path, title, artist, artist_id, album_id, track_number, disc_number,
             duration, sample_rate, channels, sample_format, codec,
             rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak,
             file_size, modified, sheet_modified, added, seen, span_start, span_frames,
             mbid, artist_mbid, release_track_mbid, isrc, tagged_title, tagged_artist,
             genre, lyrics, release_title, asked, answered
         ) VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
             ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26,
             ?27, ?28, ?29, ?30, ?31, ?32, NULL, NULL, NULL
         )
         ON CONFLICT(path, span_start) DO UPDATE SET
             root_id            = excluded.root_id,
             title              = CASE
                 WHEN tracks.answered IS NULL                            THEN excluded.title
                 WHEN tracks.tagged_title  IS NOT excluded.tagged_title  THEN excluded.title
                 WHEN tracks.tagged_artist IS NOT excluded.tagged_artist THEN excluded.title
                 ELSE tracks.title END,
             artist             = CASE
                 WHEN tracks.answered IS NULL                            THEN excluded.artist
                 WHEN tracks.tagged_title  IS NOT excluded.tagged_title  THEN excluded.artist
                 WHEN tracks.tagged_artist IS NOT excluded.tagged_artist THEN excluded.artist
                 ELSE tracks.artist END,
             artist_id          = CASE
                 WHEN tracks.answered IS NULL                            THEN excluded.artist_id
                 WHEN tracks.tagged_title  IS NOT excluded.tagged_title  THEN excluded.artist_id
                 WHEN tracks.tagged_artist IS NOT excluded.tagged_artist THEN excluded.artist_id
                 ELSE tracks.artist_id END,
             album_id           = excluded.album_id,
             track_number       = excluded.track_number,
             disc_number        = excluded.disc_number,
             duration           = excluded.duration,
             sample_rate        = excluded.sample_rate,
             channels           = excluded.channels,
             sample_format      = excluded.sample_format,
             codec              = excluded.codec,
             rg_track_gain      = excluded.rg_track_gain,
             rg_track_peak      = excluded.rg_track_peak,
             rg_album_gain      = excluded.rg_album_gain,
             rg_album_peak      = excluded.rg_album_peak,
             file_size          = excluded.file_size,
             modified           = excluded.modified,
             sheet_modified     = excluded.sheet_modified,
             seen               = excluded.seen,
             span_frames        = excluded.span_frames,
             mbid               = excluded.mbid,
             artist_mbid        = excluded.artist_mbid,
             release_track_mbid = excluded.release_track_mbid,
             isrc               = excluded.isrc,
             tagged_title       = excluded.tagged_title,
             tagged_artist      = excluded.tagged_artist,
             genre              = excluded.genre,
             lyrics             = excluded.lyrics,
             vault_key          = CASE
                 WHEN tracks.file_size   = excluded.file_size
                  AND tracks.modified    = excluded.modified
                  AND tracks.span_frames IS excluded.span_frames THEN tracks.vault_key
                 ELSE NULL END,
             vault_path         = CASE
                 WHEN tracks.file_size   = excluded.file_size
                  AND tracks.modified    = excluded.modified
                  AND tracks.span_frames IS excluded.span_frames THEN tracks.vault_path
                 ELSE NULL END,
             asks               = CASE
                 WHEN tracks.tagged_title  IS NOT excluded.tagged_title
                   OR tracks.tagged_artist IS NOT excluded.tagged_artist THEN 0
                 ELSE tracks.asks END,
             refusals           = CASE
                 WHEN tracks.tagged_title  IS NOT excluded.tagged_title
                   OR tracks.tagged_artist IS NOT excluded.tagged_artist THEN 0
                 ELSE tracks.refusals END,
             answered           = CASE
                 WHEN tracks.tagged_title  IS NOT excluded.tagged_title
                   OR tracks.tagged_artist IS NOT excluded.tagged_artist THEN NULL
                 ELSE tracks.answered END
         RETURNING id, title, artist, artist_id",
        params![
            record.root_id,
            path,
            title(record),
            record.tags.artist,
            artist_id,
            album_id,
            record.tags.track_number,
            record.tags.disc_number,
            record.duration.map(|frames| frames.get() as i64),
            i64::from(record.spec.rate.hz()),
            i64::from(record.spec.channel_count().get()),
            format_code(record.spec.format),
            codec_code(record.codec),
            track_gain.map(|gain| f64::from(gain.get())),
            track_peak.map(f64::from),
            album_gain.map(|gain| f64::from(gain.get())),
            album_peak.map(f64::from),
            i64::try_from(record.file_size).unwrap_or(i64::MAX),
            to_nanos(record.modified),
            record.sheet_modified.map(to_nanos),
            now,
            generation,
            span_start,
            span_frames,
            mbid_in(record.tags.musicbrainz_track_id.as_deref()).map(|mbid| mbid.to_string()),
            mbid_in(record.tags.musicbrainz_artist_id.as_deref()).map(|mbid| mbid.to_string()),
            mbid_in(record.tags.musicbrainz_release_track_id.as_deref())
                .map(|mbid| mbid.to_string()),
            isrc_in(record.tags.isrc.as_deref())
                .as_ref()
                .map(Isrc::as_str),
            record.tags.title,
            record.tags.artist,
            record.tags.genre,
            record.tags.lyrics,
        ],
        |row| {
            Ok(Stored {
                id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                artist_id: row.get(3)?,
            })
        },
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))
}

struct Stored {
    id: i64,
    title: String,
    artist: Option<String>,
    artist_id: Option<i64>,
}

fn index(tx: &Transaction<'_>, stored: &Stored, record: &TrackRecord) -> Result<()> {
    index_row(
        tx,
        stored.id,
        &stored.title,
        &indexed_artist(
            stored.artist.as_deref(),
            record.tags.album_artist.as_deref(),
        ),
        record.tags.album.as_deref().unwrap_or_default(),
        &indexed_genre_of(tx, record.tags.genre.as_deref(), stored.artist_id)?,
    )
}

pub(crate) fn index_row(
    tx: &Transaction<'_>,
    id: i64,
    title: &str,
    artist: &str,
    album: &str,
    genre: &str,
) -> Result<()> {
    tx.execute("DELETE FROM tracks_fts WHERE rowid = ?1", params![id])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    let sung = sung_by(tx, id)?;
    tx.execute(
        "INSERT INTO tracks_fts (rowid, title, artist, album, genre, lyrics)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            folded_letters(title),
            folded_letters(artist),
            folded_letters(album),
            folded_letters(genre),
            sung,
        ],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Insert, source))
}

const THE_WORDS_A_TRACK_SINGS: &str = "SELECT coalesce(t.lyrics, k.text)
       FROM tracks t
       LEFT JOIN lyrics_kept k ON k.path = t.path AND k.span_start = t.span_start
      WHERE t.id = ?1";

fn sung_by(tx: &Transaction<'_>, id: i64) -> Result<String> {
    let held: Option<String> = tx
        .query_row(THE_WORDS_A_TRACK_SINGS, params![id], |row| row.get(0))
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .flatten();

    Ok(held.as_deref().map(sung_words).unwrap_or_default())
}

pub(crate) fn index_what_is_sung(
    tx: &Transaction<'_>,
    path: &str,
    span_start: i64,
    text: Option<&str>,
) -> Result<()> {
    tx.execute(
        "UPDATE tracks_fts SET lyrics = ?1
          WHERE rowid IN (SELECT id FROM tracks
                           WHERE path = ?2 AND span_start = ?3 AND lyrics IS NULL)",
        params![text.map(sung_words).unwrap_or_default(), path, span_start],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn sung_words(text: &str) -> String {
    let mut words = String::with_capacity(text.len());
    let mut closing = None;
    for letter in text.chars() {
        match (closing, letter) {
            (Some(close), _) if letter == close => closing = None,
            (Some(_), _) => {}
            (None, '[') => closing = Some(']'),
            (None, '<') => closing = Some('>'),
            (None, _) => words.push(letter),
        }
    }

    folded_letters(&words)
}

struct Indexable {
    id: i64,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    genre: Option<String>,
}

pub(crate) fn reindex_the_tracks_of(tx: &Transaction<'_>, artist: i64) -> Result<()> {
    let given = genres_of_the_artist(tx, Some(artist))?;
    let mut statement = tx
        .prepare(
            "SELECT t.id, t.title, t.artist, a.title, t.genre
               FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
              WHERE t.artist_id = ?1",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let held: Vec<Indexable> = statement
        .query_map(params![artist], |row| {
            Ok(Indexable {
                id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                genre: row.get(4)?,
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    drop(statement);

    for track in held {
        index_row(
            tx,
            track.id,
            &track.title,
            track.artist.as_deref().unwrap_or_default(),
            track.album.as_deref().unwrap_or_default(),
            &indexed_genre(track.genre.as_deref(), &given),
        )?;
    }

    Ok(())
}

pub(crate) fn indexed_genre_of(
    tx: &Transaction<'_>,
    tagged: Option<&str>,
    artist: Option<i64>,
) -> Result<String> {
    Ok(indexed_genre(tagged, &genres_of_the_artist(tx, artist)?))
}

fn genres_of_the_artist(tx: &Transaction<'_>, artist: Option<i64>) -> Result<Vec<String>> {
    let Some(artist) = artist else {
        return Ok(Vec::new());
    };

    let mut statement = tx
        .prepare("SELECT name FROM artist_genres WHERE artist_id = ?1 ORDER BY weight DESC, name")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let named = statement
        .query_map(params![artist], |row| row.get::<_, String>(0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(named)
}

fn indexed_genre(tagged: Option<&str>, given: &[String]) -> String {
    let mut named: Vec<&str> = Vec::with_capacity(given.len() + 1);
    named.extend(tagged.map(str::trim).filter(|genre| !genre.is_empty()));
    for genre in given {
        let genre = genre.trim();
        if !genre.is_empty() && !named.iter().any(|held| held.eq_ignore_ascii_case(genre)) {
            named.push(genre);
        }
    }

    named.join(" ")
}

fn indexed_artist(artist: Option<&str>, album_artist: Option<&str>) -> String {
    let artist = artist.unwrap_or_default();
    let album_artist = album_artist.unwrap_or_default();

    match (artist, album_artist) {
        (artist, "") => artist.to_owned(),
        ("", album_artist) => album_artist.to_owned(),
        (artist, album_artist) if artist == album_artist => artist.to_owned(),
        (artist, album_artist) => format!("{artist} {album_artist}"),
    }
}

fn title(record: &TrackRecord) -> String {
    record.tags.title.clone().unwrap_or_else(|| {
        record
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

pub fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| Error::NonUtf8Path {
        path: path.to_path_buf(),
    })
}

pub fn year(date: &str) -> Option<i64> {
    let digits: String = date.chars().take_while(char::is_ascii_digit).collect();
    if !DATE_DIGITS.contains(&digits.len()) {
        return None;
    }
    digits[..YEAR_DIGITS].parse().ok()
}

pub fn replay_gain(
    track_gain: Option<f64>,
    track_peak: Option<f64>,
    album_gain: Option<f64>,
    album_peak: Option<f64>,
) -> ReplayGain {
    ReplayGain {
        track_gain: decibels(track_gain),
        track_peak: track_peak.map(|peak| peak as f32),
        album_gain: decibels(album_gain),
        album_peak: album_peak.map(|peak| peak as f32),
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, SampleRate};
    use rusqlite::Connection;

    use super::*;
    use crate::schema;

    const ALBUM: &str = "Meddle";

    fn dated(date: Option<&str>) -> TrackRecord {
        TrackRecord {
            root_id: 1,
            path: PathBuf::from("/music/Meddle/Echoes.wav"),
            sleeve: Some(PathBuf::from("/music/Meddle")),
            existing: None,
            file_size: 0,
            modified: UNIX_EPOCH,
            sheet_modified: None,
            spec: StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                SampleFormat::S16,
            ),
            codec: Codec::Pcm,
            duration: None,
            span: None,
            tags: TagSet {
                album: Some(ALBUM.to_owned()),
                date: date.map(str::to_owned),
                ..TagSet::default()
            },
            embeds_a_picture: false,
        }
    }

    fn year_of(scanned: [Option<&str>; 2]) -> Option<i64> {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        let tx = connection.transaction().expect("a transaction");
        let mut cache = Cache::default();

        for date in scanned {
            album(&tx, &mut cache, ALBUM, None, None, &dated(date), false)
                .expect("the album is stored");
        }

        tx.query_row("SELECT year FROM albums", [], |row| row.get(0))
            .expect("one album was grouped")
    }

    #[test]
    fn an_album_takes_the_year_a_track_declares_whichever_of_them_was_scanned_first() {
        assert_eq!(year_of([Some("1971-10-30"), None]), Some(1971));
        assert_eq!(year_of([None, Some("1971-10-30")]), Some(1971));
        assert_eq!(year_of([None, None]), None);
    }

    fn billed_to(names: [&str; 2]) -> (Connection, Vec<(String, String)>) {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        {
            let tx = connection.transaction().expect("a transaction");
            let mut cache = Cache::default();
            for name in names {
                artist(&tx, &mut cache, Billing { name, mbid: None })
                    .expect("the artist is stored");
            }
            tx.commit().expect("the transaction commits");
        }

        let held = artists_in(&connection);
        (connection, held)
    }

    fn artists_in(connection: &Connection) -> Vec<(String, String)> {
        let mut statement = connection
            .prepare("SELECT key, name FROM artists ORDER BY id")
            .expect("the artists read back");
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .and_then(Iterator::collect::<rusqlite::Result<Vec<(String, String)>>>)
            .expect("the artists read back")
    }

    #[test]
    fn a_name_is_folded_to_its_letters_with_the_marks_taken_off_and_the_rest_spelled_out() {
        assert_eq!(folded_letters("Marcin Przybyłowicz"), "marcin przybylowicz");
        assert_eq!(folded_letters("Marcin Przybylowicz"), "marcin przybylowicz");
        assert_eq!(folded_letters("Björk"), "bjork");
        assert_eq!(folded_letters("Mötley Crüe"), "motley crue");
        assert_eq!(folded_letters("Sigur Rós"), "sigur ros");
        assert_eq!(folded_letters("Blue Öyster Cult"), "blue oyster cult");
        assert_eq!(folded_letters("Æther"), "aether");
        assert_eq!(folded_letters("Straße"), "strasse");
    }

    #[test]
    fn turkish_dotless_and_dotted_i_both_fold_onto_the_plain_one() {
        assert_eq!(folded_letters("Kıskanç"), "kiskanc");
        assert_eq!(folded_letters("KISKANÇ"), "kiskanc");
        assert_eq!(folded_letters("kiskanc"), "kiskanc");
        assert_eq!(folded_letters("İstanbul"), "istanbul");
        assert_eq!(folded_letters("ISTANBUL"), "istanbul");
        assert_eq!(folded_letters("ıslak"), "islak");
    }

    #[test]
    fn folding_a_name_twice_says_what_folding_it_once_said() {
        for name in ["Marcin Przybyłowicz", "Björk", "Straße", "細野晴臣", "ΑΒΓ"] {
            let once = folded_letters(name);
            assert_eq!(
                folded_letters(&once),
                once,
                "{name} folds differently twice"
            );
        }
    }

    #[test]
    fn two_spellings_of_one_name_are_one_artist_billed_under_the_marked_spelling() {
        for order in [
            ["Marcin Przybyłowicz", "Marcin Przybylowicz"],
            ["Marcin Przybylowicz", "Marcin Przybyłowicz"],
        ] {
            let (_, held) = billed_to(order);

            assert_eq!(
                held,
                vec![(
                    "marcin przybylowicz".to_owned(),
                    "Marcin Przybyłowicz".to_owned()
                )],
                "{order:?} was billed to two artists"
            );
        }
    }

    #[test]
    fn a_catalog_keyed_before_the_fold_is_folded_back_into_one_artist_when_it_is_opened() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute_batch(
                "INSERT INTO artists (id, key, name) VALUES
                     (1, 'marcin przybylowicz', 'Marcin Przybylowicz'),
                     (2, 'marcin przybyłowicz', 'Marcin Przybyłowicz');
                 INSERT INTO roots (id, path) VALUES (1, '/music');
                 INSERT INTO albums (id, title, artist_id) VALUES (1, 'Blood', 2);
                 INSERT INTO album_keys (key, album_id) VALUES ('a', 1);
                 INSERT INTO artist_genres (artist_id, name, weight) VALUES
                     (1, 'soundtrack', 1), (2, 'soundtrack', 1), (2, 'folk', 1);
                 INSERT INTO artist_releases (artist_id, mbid, title, folded) VALUES
                     (1, 'f5093c06-23e3-404f-aeaa-40f72885ee3a', 'Blood', 'blood'),
                     (2, 'f5093c06-23e3-404f-aeaa-40f72885ee3a', 'Blood', 'blood'),
                     (2, '9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d', 'Hearts of Stone',
                      'hearts of stone');",
            )
            .expect("a catalog keyed the old way");

        let merged = reconcile_artists(&mut connection).expect("the artists reconcile");

        assert_eq!(merged, 1);
        assert_eq!(
            artists_in(&connection),
            vec![(
                "marcin przybylowicz".to_owned(),
                "Marcin Przybyłowicz".to_owned()
            )]
        );
        assert_eq!(
            connection
                .query_row("SELECT artist_id FROM albums WHERE id = 1", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .expect("the album kept an owner"),
            1
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM artist_genres", [], |row| row
                    .get::<_, i64>(0))
                .expect("the genres read back"),
            2
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*), min(artist_id), max(artist_id) FROM artist_releases",
                    [],
                    |row| Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?
                    ))
                )
                .expect("the releases read back"),
            (2, 1, 1)
        );
        assert_eq!(
            reconcile_artists(&mut connection).expect("a settled catalog reconciles"),
            0
        );
    }

    #[test]
    fn an_artist_folded_into_another_hands_on_its_favourite_and_its_portrait() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute_batch(
                "INSERT INTO artists (id, key, name, mbid, favourite, portrait, portrait_format)
                 VALUES
                     (1, 'marcin przybylowicz', 'Marcin Przybylowicz',
                      '8d8e1d53-2a0a-4b53-9f0b-d1d4e7c1b3a8', 900, NULL, NULL),
                     (2, 'marcin przybyłowicz', 'Marcin Przybyłowicz', NULL, 500, x'89', 1),
                     (3, 'adam skorupa', 'Adam Skorupa', NULL, NULL, x'01', 0),
                     (4, 'adam skórupa', 'Adam Skórupa', NULL, 700, x'02', 1);",
            )
            .expect("a catalog keyed the old way");

        assert_eq!(
            reconcile_artists(&mut connection).expect("the artists reconcile"),
            2
        );

        let held = |id: i64| {
            connection
                .query_row(
                    "SELECT favourite, portrait, portrait_format FROM artists WHERE id = ?1",
                    params![id],
                    |row| {
                        Ok((
                            row.get::<_, Option<i64>>(0)?,
                            row.get::<_, Option<Vec<u8>>>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    },
                )
                .expect("the artist that was kept")
        };
        assert_eq!(held(1), (Some(500), Some(vec![0x89]), Some(1)));
        assert_eq!(held(3), (Some(700), Some(vec![0x01]), Some(0)));
    }

    #[test]
    fn the_tracks_of_a_folded_artist_are_found_by_every_genre_it_now_holds() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute_batch(
                "INSERT INTO artists (id, key, name, mbid) VALUES
                     (1, 'marcin przybylowicz', 'Marcin Przybylowicz',
                      '8d8e1d53-2a0a-4b53-9f0b-d1d4e7c1b3a8'),
                     (2, 'marcin przybyłowicz', 'Marcin Przybyłowicz', NULL);
                 INSERT INTO artist_genres (artist_id, name, weight) VALUES
                     (1, 'soundtrack', 1), (2, 'folk', 1);
                 INSERT INTO roots (id, path) VALUES (1, '/music');
                 INSERT INTO tracks (id, root_id, path, title, artist, artist_id, sample_rate,
                                     channels, sample_format, codec, file_size, modified,
                                     added, seen)
                 VALUES
                     (10, 1, '/music/a.flac', 'Blood', 'Marcin Przybylowicz', 1,
                      44100, 2, 0, 0, 1, 0, 0, 0),
                     (20, 1, '/music/b.flac', 'Wine', 'Marcin Przybyłowicz', 2,
                      44100, 2, 0, 0, 1, 0, 0, 0);",
            )
            .expect("a catalog keyed the old way");
        let tx = connection.transaction().expect("a transaction");
        reindex_the_tracks_of(&tx, 1).expect("the first artist's tracks index");
        reindex_the_tracks_of(&tx, 2).expect("the second artist's tracks index");
        tx.commit().expect("the index commits");

        reconcile_artists(&mut connection).expect("the artists reconcile");

        let found = |genre: &str| {
            connection
                .prepare("SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1 ORDER BY rowid")
                .and_then(|mut statement| {
                    statement
                        .query_map(params![format!("genre:{genre}")], |row| {
                            row.get::<_, i64>(0)
                        })
                        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
                })
                .expect("the index answers")
        };
        assert_eq!(found("folk"), [10, 20]);
        assert_eq!(found("soundtrack"), [10, 20]);
    }

    #[test]
    fn a_date_written_with_no_separators_names_the_year_it_starts_with() {
        assert_eq!(year("1975"), Some(1975));
        assert_eq!(year("1975-06-01"), Some(1975));
        assert_eq!(year("197506"), Some(1975));
        assert_eq!(year("19750601"), Some(1975));
        assert_eq!(year("197"), None);
        assert_eq!(year("19750"), None);
        assert_eq!(year("banana"), None);
    }
}
