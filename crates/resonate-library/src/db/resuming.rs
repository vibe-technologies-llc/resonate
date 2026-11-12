use resonate_core::{
    Frames, KeptZone, MediaLocation, Reordered, Resumable, Resumption, TrackId, Volume, ZoneId,
    ZoneName,
};
use rusqlite::{OptionalExtension as _, params};

use super::{KeptPlace, Library, keep_order, keep_place, kept_span};
use crate::{Error, Result, StoreOp, resumed, store};

impl Library {
    pub fn resumption(&self) -> Result<Option<Resumption>> {
        self.resumption_of(ZoneId::FIRST)
    }

    pub fn resumption_of(&self, zone: ZoneId) -> Result<Option<Resumption>> {
        let zone = i64::from(zone.get());
        self.inner.read(|reading| {
            let connection = reading
                .unchecked_transaction()
                .map_err(|source| Error::store(StoreOp::Transaction, source))?;
            let Some((row, at, shuffle, next)) = connection
                .query_row(
                    "SELECT row, at, shuffle, next_first, next_last FROM resume WHERE zone = ?1",
                    [zone],
                    |read| {
                        Ok((
                            read.get::<_, i64>(0)?,
                            read.get::<_, i64>(1)?,
                            read.get::<_, bool>(2)?,
                            kept_span(read.get(3)?, read.get(4)?),
                        ))
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(None);
            };

            let mut statement = connection
                .prepare(
                    "SELECT uri, span_start, span_frames
                     FROM resume_rows WHERE zone = ?1 ORDER BY position",
                )
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let kept = statement
                .query_map([zone], |read| {
                    Ok((
                        read.get::<_, String>(0)?,
                        store::span(read.get(1)?, read.get(2)?),
                    ))
                })
                .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            let mut held = connection
                .prepare("SELECT id FROM tracks WHERE path = ?1 AND span_start = ?2")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let mut rows = Vec::with_capacity(kept.len());
            for (uri, span) in kept {
                let Some(location) = MediaLocation::from_uri(&uri) else {
                    tracing::warn!(
                        uri,
                        "a kept queue row names nothing openable; the rest are resumed"
                    );
                    rows.push(None);
                    continue;
                };
                let held = match location.as_path().map(store::path_text) {
                    Some(path) => held
                        .query_row(params![path, store::span_columns(span).0], |read| {
                            read.get::<_, i64>(0)
                        })
                        .optional()
                        .map_err(|source| Error::store(StoreOp::Query, source))?
                        .and_then(|id| TrackId::new(id as u64).ok()),
                    _ => None,
                };
                rows.push(Some(Resumable {
                    location,
                    span,
                    held,
                }));
            }

            let mut statement = connection
                .prepare("SELECT loaded_at FROM resume_order WHERE zone = ?1 ORDER BY position")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let order = statement
                .query_map([zone], |read| Ok(read.get::<_, i64>(0)?.max(0) as usize))
                .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            Ok(resumed::Kept {
                rows,
                order,
                row: row.max(0) as usize,
                at: Frames(at.max(0) as u64),
                shuffle,
                next,
            }
            .resumed())
        })
    }

    pub fn keep_resumption(&self, zone: ZoneId, resumption: &Resumption) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "DELETE FROM resume_rows WHERE zone = ?1",
                    [i64::from(zone.get())],
                )
                .map_err(|source| Error::store(StoreOp::Delete, source))?;

            for (position, row) in resumption.rows.iter().enumerate() {
                let (start, frames) = store::span_columns(row.span);
                store::cached(
                    transaction,
                    "INSERT INTO resume_rows (zone, position, uri, span_start, span_frames)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        i64::from(zone.get()),
                        position as i64,
                        row.location.to_uri(),
                        start,
                        frames
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            }

            keep_order(
                transaction,
                zone,
                &resumption.order,
                KeptPlace {
                    row: resumption.row,
                    at: resumption.at,
                    shuffle: resumption.shuffle,
                    next: resumption.next,
                },
            )
        })
    }

    pub fn keep_order(&self, zone: ZoneId, reordered: &Reordered) -> Result<()> {
        self.inner.write(|transaction| {
            keep_order(
                transaction,
                zone,
                &reordered.order,
                KeptPlace {
                    row: reordered.row,
                    at: reordered.at,
                    shuffle: reordered.shuffle,
                    next: reordered.next,
                },
            )
        })
    }

    pub fn keep_place(&self, zone: ZoneId, row: usize, at: Frames) -> Result<()> {
        self.inner
            .write(|transaction| keep_place(transaction, zone, row, at))
    }

    pub fn forget_resumption(&self) -> Result<()> {
        self.inner.write(|transaction| {
            for emptied in ["resume_rows", "resume_order", "resume", "zones"] {
                transaction
                    .execute(&format!("DELETE FROM {emptied}"), [])
                    .map_err(|source| Error::store(StoreOp::Delete, source))?;
            }

            Ok(())
        })
    }

    pub fn zones(&self) -> Result<Vec<KeptZone>> {
        self.inner.read(|connection| {
            let mut statement = connection
                .prepare("SELECT id, name, device, volume FROM zones ORDER BY id")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let rows = statement
                .query_map([], |read| {
                    Ok((
                        read.get::<_, i64>(0)?,
                        read.get::<_, String>(1)?,
                        read.get::<_, Option<String>>(2)?,
                        read.get::<_, Option<f64>>(3)?,
                    ))
                })
                .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            Ok(rows
                .into_iter()
                .filter_map(|(id, name, device, volume)| {
                    let id = ZoneId::new(u32::try_from(id).ok()?)?;
                    Some(KeptZone {
                        id,
                        name: ZoneName::given(&name)
                            .unwrap_or_else(|| ZoneName::first_given_to(id)),
                        device: device.map(String::into_boxed_str),
                        volume: volume.and_then(|volume| Volume::new(volume as f32).ok()),
                    })
                })
                .collect())
        })
    }

    pub fn keep_zone(&self, zone: &KeptZone) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO zones (id, name, device, volume) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(id) DO UPDATE SET
                         name   = excluded.name,
                         device = excluded.device,
                         volume = excluded.volume",
                    params![
                        i64::from(zone.id.get()),
                        zone.name.as_str(),
                        zone.device.as_deref(),
                        zone.volume.map(|volume| f64::from(volume.get()))
                    ],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }

    pub fn forget_zone(&self, zone: ZoneId) -> Result<()> {
        self.inner.write(|transaction| {
            for emptied in ["resume_rows", "resume_order", "resume"] {
                transaction
                    .execute(
                        &format!("DELETE FROM {emptied} WHERE zone = ?1"),
                        [i64::from(zone.get())],
                    )
                    .map_err(|source| Error::store(StoreOp::Delete, source))?;
            }
            transaction
                .execute("DELETE FROM zones WHERE id = ?1", [i64::from(zone.get())])
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })
    }
}
