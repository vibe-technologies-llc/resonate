use std::time::{SystemTime, UNIX_EPOCH};

use ahash::AHashMap;
use resonate_core::{AlbumId, ArtistId, MediaLocation, ReleaseTrackId, WantId};
use rusqlite::{OptionalExtension as _, params, types::Value};

use super::{
    Library,
    rows::{RawWant, grouped_links, rows},
    statements::{
        DISMISS_A_MISSING_ROW, DISMISS_AN_UNHELD_RELEASE, DISMISSED_MISSING_ROWS,
        DISMISSED_UNHELD_RELEASES, LINKS_OF_WANTED_RELEASES, LINKS_OF_WANTS, Linking, WANTS,
        WANTS_UNHELD,
    },
};
use crate::{Error, Mbid, Missing, Result, StoreOp, Want, elsewhere, store, supply};

impl Library {
    pub fn want(&self, release_track: ReleaseTrackId) -> Result<WantId> {
        self.inner
            .write(|transaction| elsewhere::want_in(transaction, release_track, SystemTime::now()))
    }

    pub fn want_release_tracks(
        &self,
        release_tracks: &[ReleaseTrackId],
    ) -> Result<Vec<(ReleaseTrackId, WantId)>> {
        self.inner.write(|transaction| {
            let now = SystemTime::now();
            release_tracks
                .iter()
                .map(|release_track| {
                    elsewhere::want_in(transaction, *release_track, now)
                        .map(|want| (*release_track, want))
                })
                .collect()
        })
    }

    pub fn want_missing_tracks(&self, album: AlbumId) -> Result<Vec<(ReleaseTrackId, WantId)>> {
        self.inner.write(|transaction| {
            let release_tracks = rows(
                transaction,
                "SELECT id FROM release_tracks
                  WHERE album_id = ?1 AND track_id IS NULL
                  ORDER BY disc, position",
                vec![Value::Integer(album.get() as i64)],
                |row| {
                    row.get::<_, i64>(0)
                        .map(|id| ReleaseTrackId::new(id as u64).map_err(Error::from))
                },
            )?;
            let now = SystemTime::now();
            release_tracks
                .into_iter()
                .map(|release_track| {
                    elsewhere::want_in(transaction, release_track, now)
                        .map(|want| (release_track, want))
                })
                .collect()
        })
    }

    pub fn unwant(&self, id: WantId) -> Result<bool> {
        self.inner.write(|transaction| {
            transaction
                .execute("DELETE FROM wants WHERE id = ?1", params![id.get() as i64])
                .map(|removed| removed > 0)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })
    }

    pub fn dismiss_missing(&self, release_track: ReleaseTrackId) -> Result<bool> {
        let row = release_track.get() as i64;

        self.inner.write(|transaction| {
            let dismissed = transaction
                .execute(DISMISS_A_MISSING_ROW, params![row])
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            let known = transaction
                .query_row(
                    "SELECT 1 FROM release_tracks WHERE id = ?1",
                    params![row],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            if known.is_none() {
                return Err(Error::UnknownReleaseTrack(release_track));
            }
            transaction
                .execute(
                    "DELETE FROM wants WHERE release_track_id = ?1",
                    params![row],
                )
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
            Ok(dismissed > 0)
        })
    }

    pub fn dismiss_release(&self, artist: ArtistId, release: &Mbid) -> Result<bool> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    DISMISS_AN_UNHELD_RELEASE,
                    params![artist.get() as i64, release.as_str()],
                )
                .map(|dismissed| dismissed > 0)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }

    pub fn dismissed(&self) -> Result<Missing> {
        Ok(Missing {
            tracks: u64::from(self.counted(DISMISSED_MISSING_ROWS, Vec::new())?),
            releases: u64::from(self.counted(DISMISSED_UNHELD_RELEASES, Vec::new())?),
        })
    }

    pub fn bring_back_dismissed(&self) -> Result<Missing> {
        let before = self.dismissed()?;

        self.inner.write(|transaction| {
            transaction
                .execute_batch("DELETE FROM dismissed_missing; DELETE FROM dismissed_releases;")
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })?;
        Ok(before)
    }

    pub fn wants(&self) -> Result<Vec<Want>> {
        self.wants_read(WANTS, Linking::WithTheirLinks)
    }

    pub fn wants_as_they_stand(&self) -> Result<Vec<Want>> {
        self.wants_read(WANTS, Linking::Without)
    }

    pub(crate) fn wants_unheld(&self) -> Result<Vec<Want>> {
        self.wants_read(WANTS_UNHELD, Linking::WithTheirLinks)
    }

    fn wants_read(&self, sql: &str, linking: Linking) -> Result<Vec<Want>> {
        self.inner.read(|connection| {
            let held = rows(connection, sql, Vec::new(), |row| {
                RawWant::read(row).map(Ok)
            })?;
            let (mut links, release_links) = match linking {
                Linking::WithTheirLinks => (
                    grouped_links(connection, LINKS_OF_WANTS, Vec::new())?,
                    grouped_links(connection, LINKS_OF_WANTED_RELEASES, Vec::new())?,
                ),
                Linking::Without => (AHashMap::new(), AHashMap::new()),
            };

            held.into_iter()
                .map(|raw| {
                    let links = links.remove(&raw.release_track).unwrap_or_default();
                    let release_links = release_links.get(&raw.album).cloned().unwrap_or_default();
                    raw.into_want(links, release_links)
                })
                .collect()
        })
    }

    pub(crate) fn note_refused_offer(
        &self,
        want: WantId,
        taken_from: &MediaLocation,
        now: SystemTime,
    ) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO refused_offers (want_id, taken_from, refused) VALUES (?1, ?2, ?3)
                     ON CONFLICT(want_id, taken_from) DO UPDATE SET refused = excluded.refused",
                    params![want.get() as i64, taken_from.to_uri(), store::to_nanos(now)],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))
                .map(|_| ())
        })
    }

    pub(crate) fn declined_offer_rows(
        &self,
        now: SystemTime,
    ) -> Result<Vec<(WantId, supply::ForgottenDelivery)>> {
        let refused_since = now
            .checked_sub(supply::REFUSALS_REMEMBERED_FOR)
            .unwrap_or(UNIX_EPOCH);
        self.inner.read(|connection| {
            rows(
                connection,
                "SELECT want_id, taken_from, forgotten FROM forgotten_deliveries
                 UNION ALL
                 SELECT want_id, taken_from, refused FROM refused_offers WHERE refused > ?1",
                vec![Value::Integer(store::to_nanos(refused_since))],
                |row| {
                    let want = row.get::<_, i64>(0)?;
                    let delivery = supply::ForgottenDelivery {
                        taken_from: row.get(1)?,
                        forgotten: store::from_nanos(row.get(2)?),
                    };
                    Ok(WantId::new(want as u64)
                        .map(|want| (want, delivery))
                        .map_err(Error::from))
                },
            )
        })
    }

    pub fn note_tried(&self, id: WantId, offered: Option<&MediaLocation>) -> Result<()> {
        self.inner.write(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE wants
                        SET tried = ?1,
                            misses = CASE WHEN ?2 IS NULL AND offered IS NULL
                                          THEN misses + 1 ELSE 0 END,
                            offered = coalesce(?2, offered)
                      WHERE id = ?3",
                    params![
                        store::to_nanos(SystemTime::now()),
                        offered.map(MediaLocation::to_uri),
                        id.get() as i64
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            if changed == 0 {
                return Err(Error::UnknownWant(id));
            }
            Ok(())
        })
    }
}
