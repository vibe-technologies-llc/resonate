use ahash::AHashSet;
use resonate_core::ArtistId;
use rusqlite::{OptionalExtension as _, params, types::Value};

use super::{
    Library,
    rows::{RawArtistDetail, RawMissingTrack, limit, read_link, rows},
    searched::{Infix, scoped_missing},
    statements::{
        MISSING_TRACKS, MISSING_TRACKS_IN_ORDER, SHORT_OF_WHAT_IS_HELD_OR_WANTED,
        UNHELD_MATCHING_IN_ORDER, UNHELD_OF_ARTIST,
    },
};
use crate::{
    ALBUMS_FOUND_ELSEWHERE_AT_MOST, ARTISTS_FOUND_ELSEWHERE_AT_MOST, AlbumFound, AlbumMatch,
    ArtistDetail, ArtistFound, ArtistRelease, Error, Found, Genre, MissingTrack, RecordingMatch,
    Reference, Result, Search, SortOrder, StoreOp, Sung, TrackQuery, elsewhere, enrich, store,
};

impl Library {
    pub fn artist_detail(&self, id: ArtistId) -> Result<Option<ArtistDetail>> {
        let artist = id.get() as i64;
        self.inner.read(|connection| {
            let Some(raw) = connection
                .query_row(
                    "SELECT mbid, sort_name, kind, gender, country, area, began_in, began, ended,
                            has_ended, disambiguation, asked, answered, name, releases_unread
                       FROM artists WHERE id = ?1",
                    params![artist],
                    RawArtistDetail::read,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(None);
            };
            let genres = rows(
                connection,
                "SELECT name, weight FROM artist_genres WHERE artist_id = ?1
                  ORDER BY weight DESC, name",
                vec![Value::Integer(artist)],
                |row| {
                    Ok(Ok(Genre {
                        name: row.get(0)?,
                        weight: row.get(1)?,
                    }))
                },
            )?;
            let links = rows(
                connection,
                "SELECT relation, provider, url FROM artist_links WHERE artist_id = ?1
                  ORDER BY url",
                vec![Value::Integer(artist)],
                |row| read_link(row, 0),
            )?;
            let releases_unheld = connection
                .query_row(UNHELD_OF_ARTIST, params![artist], |row| {
                    row.get::<_, u32>(0)
                })
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            raw.into_detail(genres, links, releases_unheld).map(Some)
        })
    }

    pub fn missing_tracks(
        &self,
        narrowing: Option<&str>,
        at_most: Option<usize>,
    ) -> Result<Vec<MissingTrack>> {
        let Some(scoped) = scoped_missing(narrowing) else {
            return Ok(Vec::new());
        };
        let sql = format!("{MISSING_TRACKS}{}{MISSING_TRACKS_IN_ORDER}", scoped.from);
        let mut binds = scoped.binds;
        binds.push(Value::Integer(limit(at_most)));

        self.inner.read(|connection| {
            rows(connection, &sql, binds, |row| {
                RawMissingTrack::read(row).map(RawMissingTrack::into_missing)
            })
        })
    }

    pub fn unheld_matching(&self, text: &str, at_most: Option<usize>) -> Result<Vec<MissingTrack>> {
        let words = elsewhere::words_asked(text);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = format!("{MISSING_TRACKS}{SHORT_OF_WHAT_IS_HELD_OR_WANTED}");
        let mut binds = Vec::with_capacity(words.len() + 1);
        for word in &words {
            for piece in word.split_whitespace() {
                sql.push_str(" AND ");
                sql.push_str(
                    &Infix::OF_A_MISSING_TRACK.holding(&store::folded_letters(piece), &mut binds),
                );
            }
        }
        sql.push_str(UNHELD_MATCHING_IN_ORDER);
        binds.push(Value::Integer(limit(at_most)));

        self.inner.read(|connection| {
            rows(connection, &sql, binds, |row| {
                RawMissingTrack::read(row).map(RawMissingTrack::into_missing)
            })
        })
    }

    pub fn sung(&self, text: &str) -> Result<Option<Sung>> {
        let Some(sung) = Search::read(text).as_sung() else {
            return Ok(None);
        };
        let query = sung.to_string();
        let measured = self.measured(&TrackQuery {
            album: None,
            artist: None,
            text: Some(query.clone()),
            sort: SortOrder::default(),
            reading: SortOrder::default().reads(),
            limit: None,
            offset: 0,
        })?;

        Ok((measured.rows > 0).then_some(Sung {
            query,
            tracks: measured.rows,
        }))
    }

    pub fn found_elsewhere(&self, reference: &dyn Reference, text: &str) -> Result<Vec<Found>> {
        let Some(words) = elsewhere::songs_asked(text) else {
            return Ok(Vec::new());
        };

        self.unheld_among(
            elsewhere::weighed_for(&words, reference.find_songs(&words)?),
            Some(&words),
        )
    }

    pub fn unheld_artists_among(
        &self,
        matches: &[RecordingMatch],
        text: &str,
    ) -> Result<Vec<ArtistFound>> {
        let mut unheld = Vec::new();
        for named in elsewhere::artists_named_by(matches, text) {
            if unheld.len() == ARTISTS_FOUND_ELSEWHERE_AT_MOST {
                break;
            }
            let held: bool = self.inner.read(|connection| {
                connection
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM artists
                                         WHERE found_elsewhere IS NULL
                                           AND (mbid = ?1 OR key = ?2))",
                        params![named.mbid.as_str(), store::folded_letters(&named.name)],
                        |row| row.get(0),
                    )
                    .map_err(|source| Error::store(StoreOp::Query, source))
            })?;
            if !held {
                unheld.push(named);
            }
        }

        Ok(unheld)
    }

    pub fn unheld_albums_among(
        &self,
        matches: &[AlbumMatch],
        text: &str,
    ) -> Result<Vec<AlbumFound>> {
        let mut unheld = Vec::new();
        for named in elsewhere::albums_named_by(matches, text) {
            if unheld.len() == ALBUMS_FOUND_ELSEWHERE_AT_MOST {
                break;
            }
            let held: bool = self.inner.read(|connection| {
                connection
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM albums a
                                         WHERE a.found_elsewhere IS NULL
                                           AND (a.release_group = ?1
                                                OR (words_of(a.title) = ?2
                                                    AND a.artist_id IN
                                                        (SELECT id FROM artists WHERE key = ?3))))",
                        params![
                            named.group.as_str(),
                            store::words_of(&named.title),
                            store::folded_letters(&named.artist)
                        ],
                        |row| row.get(0),
                    )
                    .map_err(|source| Error::store(StoreOp::Query, source))
            })?;
            if !held {
                unheld.push(named);
            }
        }

        Ok(unheld)
    }

    pub fn open_artist_found(
        &self,
        reference: &dyn Reference,
        found: &ArtistFound,
    ) -> Result<ArtistId> {
        let artist = self
            .inner
            .write(|transaction| elsewhere::artist_of_found(transaction, found))?;
        if let Some(profile) = reference.artist(&found.mbid)? {
            self.land_artist(artist, &profile)?;
        }
        let discography = reference.release_groups_of(&found.mbid, 0)?;
        let kept: Vec<ArtistRelease> = discography
            .releases
            .into_iter()
            .filter(enrich::worth_keeping)
            .collect();
        self.land_artist_releases(artist, &kept, (discography.unread, discography.read_to))?;

        Ok(artist)
    }

    pub fn unheld_among(
        &self,
        matches: Vec<RecordingMatch>,
        asked: Option<&elsewhere::SongsAsked>,
    ) -> Result<Vec<Found>> {
        let held = self.recordings_held_of(&matches)?;

        Ok(elsewhere::found_among(matches, asked, |recording| {
            held.contains(recording.as_str())
        }))
    }

    fn recordings_held_of(&self, matches: &[RecordingMatch]) -> Result<AHashSet<String>> {
        if matches.is_empty() {
            return Ok(AHashSet::new());
        }
        let asked = vec!["?"; matches.len()].join(", ");
        let sql = format!(
            "SELECT mbid FROM tracks WHERE mbid IN ({asked})
             UNION SELECT recording_mbid FROM release_tracks
                    WHERE recording_mbid IN ({asked}) AND track_id IS NOT NULL"
        );
        let named: Vec<Value> = matches
            .iter()
            .map(|matched| Value::Text(matched.recording.as_str().to_owned()))
            .collect();
        let binds = [named.clone(), named].concat();

        self.inner
            .read(|connection| {
                rows(connection, &sql, binds, |row| {
                    row.get::<_, String>(0).map(Ok)
                })
            })
            .map(|held| held.into_iter().collect())
    }
}
