use crate::MusicFilters;

pub(super) const INDEX_JOIN: &str = " JOIN tracks_fts ON tracks_fts.rowid = tracks.id";

pub(super) const INDEX_LOOKUP: &str =
    "tracks.id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?)";

pub(super) const LISTENS_SINCE: &str =
    "(SELECT count(*) FROM listens WHERE listens.track_id = tracks.id AND listens.at >= ?)";

pub(super) const NOTHING_MATCHES: &str = "0";

pub(super) const A_FAVOURITE_TRACK: &str = "tracks.favourite IS NOT NULL";

pub(super) fn studied_as(column: &str, value: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM track_studies s WHERE s.track_id = tracks.id AND s.{column} = '{value}')"
    )
}

pub(super) const A_FAVOURITE_ALBUM: &str = "a.favourite IS NOT NULL";

pub(super) const A_FAVOURITE_ARTIST: &str = "r.favourite IS NOT NULL";

pub(super) const THE_FAVOURITES_SEARCH: &str = "is:favourite";

pub(super) const THE_BEST_COPY: &str = "+tracks.alternative_of IS NULL";

pub(super) const A_SHOWN_TRACK: &str = "tracks.hidden = 0";

pub(super) const A_HIDDEN_TRACK: &str = "tracks.hidden = 1";

pub(super) const CD_SAMPLE_RATE: u32 = 44_100;

pub(super) const CD_SAMPLE_DEPTH: u8 = 16;

pub(super) const IDS_READ_AT_ONCE: usize = 500;

pub(super) const RELEASES_TRIED_AFTER_THE_FIRST: usize = 3;

pub(crate) const TRACK_COLUMNS: &str =
    "tracks.id, tracks.path, tracks.title, tracks.artist, tracks.album_id,
     tracks.track_number, tracks.disc_number, tracks.duration, tracks.sample_rate, tracks.channels,
     tracks.sample_format, tracks.codec, tracks.rg_track_gain, tracks.rg_track_peak,
     tracks.rg_album_gain, tracks.rg_album_peak, tracks.file_size, tracks.modified, tracks.added,
     tracks.plays, tracks.played, tracks.span_start, tracks.span_frames, tracks.artist_id,
     tracks.favourite, tracks.genre, tracks.hidden,
     (SELECT count(*) FROM tracks x WHERE x.alternative_of = tracks.id), tracks.root_id IS NULL, tracks.explicit";

pub(crate) const BESIDE_A_TRACK: usize = listed(TRACK_COLUMNS);

pub(super) const fn listed(columns: &str) -> usize {
    let bytes = columns.as_bytes();
    let mut counted = 1;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b',' {
            counted += 1;
        }
        at += 1;
    }
    counted
}

macro_rules! album_title {
    () => {
        "coalesce(a.release_title, a.title)"
    };
}

macro_rules! album_filed_as {
    () => {
        "coalesce(a.tagged_sort, a.release_title, a.title)"
    };
}

macro_rules! album_owner {
    () => {
        "(SELECT r.name FROM artists r WHERE r.id = a.artist_id)"
    };
}

macro_rules! filed_as {
    ($artist:literal) => {
        concat!(
            "coalesce(",
            $artist,
            ".tagged_sort, ",
            $artist,
            ".sort_name, ",
            $artist,
            ".name) COLLATE NOCASE"
        )
    };
}

macro_rules! album_owner_filed_as {
    () => {
        concat!(
            "(SELECT ",
            filed_as!("r"),
            " FROM artists r WHERE r.id = a.artist_id)"
        )
    };
}

macro_rules! album_added {
    () => {
        "(SELECT max(t.added) FROM tracks t WHERE t.album_id = a.id)"
    };
}

macro_rules! heard_this_month {
    () => {
        "(SELECT count(*) FROM listens l
           WHERE l.track_id = tracks.id
             AND l.at >= (unixepoch() - 2592000) * 1000000000)"
    };
}

macro_rules! unheld_by_any_album {
    () => {
        "NOT EXISTS (SELECT 1 FROM albums a WHERE a.release_group = r.mbid)
         AND NOT EXISTS (SELECT 1 FROM tracks t
                          WHERE r.song IS NOT NULL AND t.artist_id = r.artist_id
                            AND t.title_words = r.song)
         AND NOT EXISTS (SELECT 1 FROM dismissed_releases d
                          WHERE d.artist_id = r.artist_id AND d.mbid = r.mbid)"
    };
}

macro_rules! shown_tracks {
    () => {
        "shown_tracks"
    };
}

macro_rules! shown_albums {
    () => {
        "shown_albums"
    };
}

fn a_shown_copy(filters: MusicFilters) -> String {
    match filters.predicate("t") {
        Some(predicate) => format!("t.alternative_of IS NULL AND t.hidden = 0 AND {predicate}"),
        None => "t.alternative_of IS NULL AND t.hidden = 0".to_owned(),
    }
}

pub(super) fn holds_a_best_copy(filters: MusicFilters) -> String {
    format!(
        "EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = a.id AND {})",
        a_shown_copy(filters)
    )
}

pub(super) fn album_columns(filters: MusicFilters) -> String {
    format!(
        "a.id, {title}, a.artist_id, a.year,
     (SELECT count(*) FROM tracks t WHERE t.album_id = a.id AND {shown}) AS {counted},
     (SELECT count(DISTINCT t.artist_id) FROM tracks t WHERE t.album_id = a.id),
     (a.cover_art IS NOT NULL OR a.cover_path IS NOT NULL), a.mbid,
     (SELECT count(*) FROM release_tracks rt WHERE rt.album_id = a.id AND rt.track_id IS NULL), {owner},
     a.favourite",
        title = album_title!(),
        shown = a_shown_copy(filters),
        counted = shown_tracks!(),
        owner = album_owner!(),
    )
}

fn held_by_an_artist(select: &str, filters: MusicFilters) -> String {
    format!(
        "(SELECT {select} FROM tracks t
           WHERE (t.artist_id = r.id
                  OR t.id IN (SELECT c.track_id FROM track_credits c WHERE c.artist_id = r.id))
             AND {})",
        a_shown_copy(filters)
    )
}

pub(super) fn artist_tracks(filters: MusicFilters) -> String {
    held_by_an_artist("count(*)", filters)
}

pub(super) fn artist_columns(filters: MusicFilters) -> String {
    format!(
        "r.id, r.name, {albums} AS {shown_albums}, {tracks} AS {shown_tracks},
     r.mbid, r.portrait IS NOT NULL, r.favourite",
        albums = held_by_an_artist("count(DISTINCT t.album_id)", filters),
        shown_albums = shown_albums!(),
        tracks = artist_tracks(filters),
        shown_tracks = shown_tracks!(),
    )
}

pub(super) const LINKS_OF_UNPICTURED_ARTISTS: &str =
    "SELECT l.artist_id, l.relation, l.provider, l.url
       FROM artist_links l JOIN artists r ON r.id = l.artist_id
      WHERE r.portrait IS NULL AND (r.portrait_asked IS NULL OR r.portrait_asked < ?1)
      ORDER BY l.artist_id, l.url";

pub(super) const WHAT_AN_ARTIST_HOLDS: &str = "SELECT count(DISTINCT album_id), count(*),
            sum(duration * 1.0 / sample_rate), sum(plays), max(played)
       FROM tracks
      WHERE (artist_id = ?1 OR id IN (SELECT track_id FROM track_credits WHERE artist_id = ?1))
        AND alternative_of IS NULL AND hidden = 0";

pub(super) const RELEASE_TRACK_COLUMNS: &str =
    "id, disc, position, number, title, artist, recording_mbid,
     track_mbid, length_ms, isrc, track_id";

pub(super) const LINKS_OF_RELEASE_TRACKS: &str = "SELECT release_track_id, relation, provider, url
       FROM release_track_links
      WHERE release_track_id IN (SELECT id FROM release_tracks WHERE album_id = ?1)
      ORDER BY release_track_id, url";

pub(super) const LINKS_OF_WANTS: &str = "SELECT release_track_id, relation, provider, url
       FROM release_track_links
      WHERE release_track_id IN (SELECT release_track_id FROM wants)
      ORDER BY release_track_id, url";

pub(super) const LINKS_OF_WANTED_RELEASES: &str = "SELECT album_id, relation, provider, url
       FROM album_links
      WHERE album_id IN (SELECT rt.album_id
                           FROM wants w
                           JOIN release_tracks rt ON rt.id = w.release_track_id)
      ORDER BY album_id, url";

pub(super) const TRACKS_TO_FILE: &str = concat!(
    "SELECT tracks.id, tracks.path, roots.path, tracks.title,
            tracks.artist, tracks.album_id, ",
    album_title!(),
    ", a.year, artists.name,
            tracks.track_number, tracks.disc_number
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id
       LEFT JOIN albums a ON a.id = tracks.album_id
       LEFT JOIN artists ON artists.id = a.artist_id"
);

pub(super) const FILES_TO_FILE: &str =
    "SELECT tracks.path, roots.path, tracks.album_id, tracks.disc_number
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id";

pub(super) const FILED_IN_ORDER: &str = " ORDER BY tracks.path, tracks.span_start";

pub(super) const TRACKS_TO_VAULT: &str = "SELECT tracks.id, tracks.path, tracks.span_start,
            tracks.span_frames, tracks.file_size, tracks.codec, tracks.sample_rate,
            tracks.channels, tracks.sample_format, tracks.album_id,
            tracks.vault_key IS NOT NULL,
            (SELECT min(o.key) FROM vault_objects o
              WHERE tracks.span_frames IS NULL AND o.frames = tracks.duration
                AND o.sample_rate = tracks.sample_rate AND o.channels = tracks.channels
                AND o.encoding >= ?
             HAVING count(*) = 1),
            tracks.vault_key, vault_objects.bytes, vault_objects.encoding
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id
       LEFT JOIN vault_objects ON vault_objects.key = tracks.vault_key
      WHERE (tracks.vault_key IS NULL OR coalesce(vault_objects.encoding, 0) < ?)
        AND NOT EXISTS (SELECT 1 FROM vault_refused
                         WHERE vault_refused.track_id = tracks.id AND vault_refused.under >= ?)";

pub(super) const TRACKS_IN_THE_VAULT: &str = "SELECT tracks.id, tracks.path
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id
      WHERE tracks.vault_key IS NOT NULL";

pub(super) const VAULT_OBJECTS: &str = "SELECT key, form, path, bytes, sample_rate, channels,
            sample_format, frames, taken_from, took, was_bytes, was_codec, validated, encoding
       FROM vault_objects ORDER BY path";

pub(super) const VAULT_OBJECTS_UNHELD: &str =
    "SELECT key, form, path, bytes, sample_rate, channels,
            sample_format, frames, taken_from, took, was_bytes, was_codec, validated, encoding
       FROM vault_objects
      WHERE key NOT IN (SELECT vault_key FROM tracks WHERE vault_key IS NOT NULL)
      ORDER BY path";

pub(super) const TRACKS_TO_TAG: &str =
    "SELECT tracks.id, tracks.path, tracks.span_start, tracks.span_frames,
            tracks.answered IS NOT NULL, tracks.title, tracks.artist, tracks.artist_mbid,
            tracks.mbid, tracks.release_track_mbid, tracks.isrc,
            CASE WHEN tracks.numbered_by_its_stem THEN NULL ELSE tracks.track_number END,
            tracks.disc_number, tracks.album_id,
            a.answered IS NOT NULL, a.release_title,
            artists.answered IS NOT NULL, artists.name, artists.mbid,
            a.mbid, a.release_group, a.date, a.label, a.catalog_number, a.barcode,
            tracks.vault_key IS NOT NULL, tracks.favourite IS NOT NULL, tracks.plays,
            CASE WHEN performer.answered IS NOT NULL AND performer.name = tracks.artist
                 THEN performer.sort_name END,
            artists.sort_name, a.pressing_fit IS NOT NULL,
            tracks.sample_rate, tracks.sheet_modified, a.pressing_fit
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id
       LEFT JOIN albums a ON a.id = tracks.album_id
       LEFT JOIN artists ON artists.id = a.artist_id
       LEFT JOIN artists performer ON performer.id = tracks.artist_id";

pub(super) const RELEASE_DISC_TRACKS: &str = "SELECT album_id, disc, count(*) FROM release_tracks
      GROUP BY album_id, disc";

pub(super) const RELEASE_MEDIA: &str =
    "SELECT album_id, count(*) FROM release_media GROUP BY album_id";

pub(super) const FIRST_DISC: i64 = 1;

pub(super) const ALBUM_DISCS: &str = "SELECT album_id, max(disc_number) FROM tracks
      WHERE album_id IS NOT NULL AND disc_number IS NOT NULL
      GROUP BY album_id";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Linking {
    WithTheirLinks,
    Without,
}

macro_rules! wants_selected {
    () => {
        concat!(
            "SELECT w.id, w.release_track_id, rt.album_id, ",
            album_title!(),
            ", rt.title, coalesce(rt.artist, ar.name),
                    w.wanted, w.tried, w.offered, w.misses,
                    rt.recording_mbid, rt.track_mbid, a.mbid,
                    coalesce(rt.isrc || ' ', '') ||
                        coalesce((SELECT group_concat(c.isrc, ' ') FROM release_track_isrcs c
                                   WHERE c.release_track_id = rt.id
                                     AND c.isrc IS NOT rt.isrc), ''),
                    rt.length_ms, rt.disc,
                    rt.position, rt.track_id
               FROM wants w
               JOIN release_tracks rt ON rt.id = w.release_track_id
               JOIN albums a ON a.id = rt.album_id
               LEFT JOIN artists ar ON ar.id = a.artist_id"
        )
    };
}

pub(super) const WANTS: &str = concat!(wants_selected!(), " ORDER BY w.wanted DESC, w.id DESC");

pub(super) const WANTS_UNHELD: &str = concat!(
    wants_selected!(),
    " JOIN (SELECT wanted, count(*) AS songs FROM wants GROUP BY wanted) asked_together
        ON asked_together.wanted = w.wanted
      WHERE rt.track_id IS NULL
      ORDER BY asked_together.songs > 1,
               CASE WHEN asked_together.songs > 1 THEN w.wanted ELSE -w.wanted END,
               rt.disc, rt.position, w.id"
);

pub(super) const REMEMBER_THE_DELIVERY_FORGOTTEN: &str =
    "INSERT INTO forgotten_deliveries (want_id, taken_from, forgotten)
     SELECT w.id, d.taken_from, ?2
       FROM tracks t
       JOIN (SELECT key, taken_from FROM vault_objects
             UNION
             SELECT key, taken_from FROM object_deliveries) d ON d.key = t.vault_key
       JOIN release_tracks rt ON rt.track_id = t.id
       JOIN wants w ON w.release_track_id = rt.id
      WHERE t.root_id IS NULL AND t.path = ?1
     ON CONFLICT(want_id, taken_from) DO UPDATE SET forgotten = excluded.forgotten";

pub(super) const NOTE_WHERE_AN_OBJECT_WAS_DELIVERED_FROM: &str =
    "INSERT INTO object_deliveries (key, taken_from, took) VALUES (?1, ?2, ?3)
     ON CONFLICT(key, taken_from) DO UPDATE SET took = excluded.took";

pub(super) const ASK_AGAIN_FOR_WHAT_A_FORGOTTEN_DELIVERY_HELD: &str =
    "UPDATE wants SET offered = NULL, tried = NULL, misses = 0
      WHERE release_track_id IN (SELECT rt.id FROM release_tracks rt
                                   JOIN tracks t ON t.id = rt.track_id
                                  WHERE t.root_id IS NULL AND t.path = ?1)";

pub(super) const MISSING_TRACKS: &str = concat!(
    "SELECT rt.album_id, ",
    album_title!(),
    ", ar.name, rt.id, rt.disc, rt.position, rt.number, rt.title, rt.artist,
            rt.length_ms, w.id
       FROM release_tracks rt
       JOIN albums a ON a.id = rt.album_id
       LEFT JOIN artists ar ON ar.id = a.artist_id
       LEFT JOIN wants w ON w.release_track_id = rt.id"
);

macro_rules! held_or_wanted {
    () => {
        "(EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = rt.album_id)
          OR EXISTS (SELECT 1 FROM wants wn WHERE wn.release_track_id = rt.id))
         AND NOT EXISTS (SELECT 1 FROM dismissed_missing d
                          WHERE d.album_id = rt.album_id AND d.disc = rt.disc
                            AND d.position = rt.position AND d.folded = rt.folded)"
    };
}

pub(super) const SHORT_OF_WHAT_IS_HELD_OR_WANTED: &str =
    concat!(" WHERE rt.track_id IS NULL AND ", held_or_wanted!());

pub(super) const UNHELD_MATCHING_IN_ORDER: &str = concat!(
    " ORDER BY w.id IS NULL, ",
    album_title!(),
    " COLLATE NOCASE, a.id, rt.disc, rt.position
      LIMIT ?"
);

pub(super) const MISSING_TRACKS_IN_ORDER: &str = concat!(
    " WHERE rt.track_id IS NULL AND ",
    held_or_wanted!(),
    "
      ORDER BY ",
    album_title!(),
    " COLLATE NOCASE, a.id, rt.disc, rt.position
      LIMIT ?"
);

pub(super) const MISSING_TRACKS_COUNTED: &str =
    "SELECT count(*) FROM release_tracks rt JOIN albums a ON a.id = rt.album_id";

pub(super) const UNHELD_RELEASES: &str = concat!(
    "SELECT r.artist_id, ar.name, r.mbid, r.title, r.kind, r.first_released
       FROM artist_releases r
       JOIN artists ar ON ar.id = r.artist_id
      WHERE ",
    unheld_by_any_album!()
);

pub(super) const UNHELD_IN_ORDER: &str =
    " ORDER BY ar.name COLLATE NOCASE, r.artist_id, r.first_released IS NULL, r.first_released,
               r.title COLLATE NOCASE
      LIMIT ?";

pub(super) const UNHELD_ALBUMS_KEPT: &str = concat!(
    "SELECT r.mbid, r.title, ar.name, r.kind, r.first_released
       FROM artist_releases r
       JOIN artists ar ON ar.id = r.artist_id
      WHERE r.kind IN (?, ?) AND ",
    unheld_by_any_album!(),
    " AND "
);

pub(super) const UNHELD_ALBUMS_KEPT_IN_ORDER: &str =
    " ORDER BY r.first_released IS NULL, r.first_released, r.title COLLATE NOCASE, r.mbid";

pub(super) const UNHELD_COUNTED: &str = concat!(
    "SELECT count(*) FROM artist_releases r JOIN artists ar ON ar.id = r.artist_id WHERE ",
    unheld_by_any_album!()
);

macro_rules! songs_read_since {
    () => {
        "NOT EXISTS (SELECT 1 FROM discography_songs_read k
                      WHERE k.release_group = r.mbid
                        AND k.read > CASE WHEN k.refusals > 0 THEN ?1
                                          ELSE ?2 + spread_of(r.artist_id) END)"
    };
}

pub(super) const ARTISTS_WITH_SONGS_DUE: &str = concat!(
    "SELECT r.artist_id FROM artist_releases r
      WHERE ",
    unheld_by_any_album!(),
    " AND ",
    songs_read_since!(),
    "
      GROUP BY r.artist_id
      ORDER BY (SELECT coalesce(sum(t.plays), 0) FROM tracks t
                 WHERE t.artist_id = r.artist_id) DESC,
               r.artist_id"
);

pub(super) const SONGS_DUE_FOR_AN_ARTIST: &str = concat!(
    "SELECT r.mbid FROM artist_releases r
      WHERE r.artist_id = ?3 AND ",
    unheld_by_any_album!(),
    " AND ",
    songs_read_since!(),
    "
      ORDER BY r.first_released IS NULL, r.first_released, r.mbid"
);

pub(super) const SONG_GROUP_STILL_DUE: &str = concat!(
    "SELECT EXISTS (SELECT 1 FROM artist_releases r
                     WHERE r.mbid = ?3 AND ",
    unheld_by_any_album!(),
    " AND ",
    songs_read_since!(),
    ")"
);

pub(super) const ARTIST_MBID: &str = "SELECT mbid FROM artists WHERE id = ?1";

pub(super) const UNHELD_COVERS_DUE: &str = concat!(
    "SELECT r.mbid,
            (SELECT s.release_mbid FROM discography_songs s WHERE s.release_group = r.mbid LIMIT 1)
       FROM artist_releases r
      WHERE ",
    unheld_by_any_album!(),
    "
        AND NOT EXISTS (SELECT 1 FROM unheld_covers c
                         WHERE c.release_group = r.mbid
                           AND (c.cover IS NOT NULL OR c.asked > ?1))
      GROUP BY r.mbid
      ORDER BY max((SELECT coalesce(sum(t.plays), 0) FROM tracks t
                     WHERE t.artist_id = r.artist_id)) DESC,
               min(r.first_released IS NULL), min(r.first_released)
      LIMIT ?2"
);

pub(super) const UNHELD_COVER_DUE: &str = concat!(
    "SELECT EXISTS (SELECT 1 FROM artist_releases r
                     WHERE r.mbid = ?2 AND ",
    unheld_by_any_album!(),
    "
                       AND NOT EXISTS (SELECT 1 FROM unheld_covers c
                                        WHERE c.release_group = r.mbid
                                          AND (c.cover IS NOT NULL OR c.asked > ?1)))"
);

pub(super) const SONG_HELD_NOWHERE: &str = concat!(
    "NOT EXISTS (SELECT 1 FROM tracks t WHERE t.mbid = s.recording_mbid)
     AND NOT EXISTS (SELECT 1 FROM release_tracks rt WHERE rt.recording_mbid = s.recording_mbid)
     AND EXISTS (SELECT 1 FROM artist_releases r
                  WHERE r.mbid = s.release_group AND ",
    unheld_by_any_album!(),
    "
                    AND NOT EXISTS (SELECT 1 FROM tracks t
                                     WHERE t.artist_id = r.artist_id
                                       AND t.title_words = s.words)"
);

pub(super) const SONGS_IN_ORDER: &str =
    " ORDER BY s.released IS NULL, s.released, s.release_group, s.disc,
                                         s.position";

pub(super) const SONGS_SOUGHT_AT_MOST: i64 = 200;

pub(super) const MISSING_FROM_AN_ARTISTS_ALBUMS: &str =
    "SELECT rt.recording_mbid, rt.title, coalesce(rt.artist, ar.name), rt.length_ms, a.mbid,
            a.title, NULL, NULL, rt.disc, rt.position, a.artist_id
       FROM release_tracks rt
       JOIN albums a ON a.id = rt.album_id
       JOIN artists ar ON ar.id = a.artist_id
      WHERE a.artist_id = ?1 AND rt.track_id IS NULL AND rt.recording_mbid IS NOT NULL
        AND a.mbid IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.mbid = rt.recording_mbid)
        AND NOT EXISTS (SELECT 1 FROM dismissed_missing d
                         WHERE d.album_id = rt.album_id AND d.disc = rt.disc
                           AND d.position = rt.position AND d.folded = rt.folded)
        AND NOT EXISTS (SELECT 1 FROM tracks t
                         WHERE t.artist_id = a.artist_id AND t.title_words = words_of(rt.title))
      ORDER BY a.id, rt.disc, rt.position";

pub(super) const ALBUMS_NOT_HELD_BY_AN_ARTIST: &str =
    "SELECT r.artist_id, ar.name, r.mbid, r.title, r.kind, r.first_released,
            (SELECT s.release_mbid FROM discography_songs s WHERE s.release_group = r.mbid LIMIT 1)
       FROM artist_releases r
       JOIN artists ar ON ar.id = r.artist_id
      WHERE r.artist_id = ?1
        AND NOT EXISTS (SELECT 1 FROM albums a
                         WHERE a.release_group = r.mbid
                           AND EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = a.id))
        AND NOT EXISTS (SELECT 1 FROM tracks t
                         WHERE r.song IS NOT NULL AND t.artist_id = r.artist_id
                           AND t.title_words = r.song)
        AND NOT EXISTS (SELECT 1 FROM dismissed_releases d
                         WHERE d.artist_id = r.artist_id AND d.mbid = r.mbid)
      ORDER BY r.first_released IS NULL, r.first_released, r.title COLLATE NOCASE";

pub(super) const DISMISS_A_MISSING_ROW: &str =
    "INSERT INTO dismissed_missing (album_id, disc, position, folded)
     SELECT album_id, disc, position, folded FROM release_tracks WHERE id = ?1
     ON CONFLICT DO NOTHING";

pub(super) const DISMISS_AN_UNHELD_RELEASE: &str =
    "INSERT INTO dismissed_releases (artist_id, mbid)
     SELECT artist_id, mbid FROM artist_releases WHERE artist_id = ?1 AND mbid = ?2
     ON CONFLICT DO NOTHING";

pub(super) const DISMISSED_MISSING_ROWS: &str = "SELECT count(*) FROM release_tracks rt
       JOIN dismissed_missing d
         ON d.album_id = rt.album_id AND d.disc = rt.disc AND d.position = rt.position
        AND d.folded = rt.folded
      WHERE rt.track_id IS NULL";

pub(super) const DISMISSED_UNHELD_RELEASES: &str = "SELECT count(*) FROM artist_releases r
       JOIN dismissed_releases d ON d.artist_id = r.artist_id AND d.mbid = r.mbid
      WHERE NOT EXISTS (SELECT 1 FROM albums a WHERE a.release_group = r.mbid)";

pub(super) const UNHELD_OF_ARTIST: &str = concat!(
    "SELECT count(*) FROM artist_releases r WHERE r.artist_id = ?1 AND ",
    unheld_by_any_album!()
);
