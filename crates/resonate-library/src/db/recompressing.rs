use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use resonate_codec::Codec;
use resonate_core::{ChannelCount, ChannelLayout, SampleRate, StreamSpec, TrackId};
use resonate_vault::Halt;
use rusqlite::{OptionalExtension as _, params};

use super::{Library, Walk, rows::rows};
use crate::{
    Ceiling, Error, KeptOriginal, RecompressCandidate, RecompressHandle, Recompressed, Result,
    StoreOp,
    recompress::{InPlace, recompress_in_place},
    recompressing,
    scan::{self, LandedRead},
    store,
};

const THE_FLAC_CANDIDATES: &str = "
SELECT t.id, t.path, r.path, t.file_size, t.sample_rate, t.channels, t.sample_format
  FROM tracks t
  JOIN roots r ON r.id = t.root_id
 WHERE t.codec = ?1
   AND t.vault_key IS NULL
   AND t.span_start = 0
   AND t.span_frames IS NULL
   AND NOT EXISTS (SELECT 1 FROM tracks o WHERE o.path = t.path AND o.id <> t.id)
 ORDER BY t.path";

const THE_ROW_TO_RECOMPRESS: &str = "
SELECT t.path, r.path, t.codec,
       t.vault_key IS NOT NULL,
       t.span_frames IS NOT NULL OR t.span_start <> 0
           OR EXISTS (SELECT 1 FROM tracks o WHERE o.path = t.path AND o.id <> t.id)
  FROM tracks t
  LEFT JOIN roots r ON r.id = t.root_id
 WHERE t.id = ?1";

struct RowToRecompress {
    path: PathBuf,
    root: Option<PathBuf>,
    codec: i64,
    vaulted: bool,
    cut: bool,
}

impl RowToRecompress {
    fn refused(&self) -> Option<KeptOriginal> {
        if self.cut {
            return Some(KeptOriginal::CutBySheet);
        }
        if self.vaulted {
            return Some(KeptOriginal::Vaulted);
        }
        if self.codec != store::codec_code(Codec::Flac) {
            return Some(KeptOriginal::NotAFlac);
        }
        self.root
            .is_none()
            .then_some(KeptOriginal::OutsideTheLibrary)
    }
}

impl Library {
    pub fn recompress(
        &self,
        track: TrackId,
        ceiling: Ceiling,
        halt: &AtomicBool,
    ) -> Result<Recompressed> {
        let row = self
            .row_to_recompress(track)?
            .ok_or(Error::UnknownTrack(track))?;

        if let Some(kept) = row.refused() {
            return Ok(Recompressed::Kept(kept));
        }

        let walking = self.walk_the_tree()?;
        let outcome = recompress_in_place(InPlace {
            path: &row.path,
            ceiling,
            halt: Halt::on(halt),
        })?;

        if let (Recompressed::Smaller(_), Some(root)) = (outcome, &row.root) {
            self.landed_read(&walking, root, std::slice::from_ref(&row.path));
        }
        Ok(outcome)
    }

    pub fn recompress_candidates(&self) -> Result<Vec<RecompressCandidate>> {
        self.inner.read(|connection| {
            rows(
                connection,
                THE_FLAC_CANDIDATES,
                vec![store::codec_code(Codec::Flac).into()],
                |row| {
                    let id: i64 = row.get(0)?;
                    let path: String = row.get(1)?;
                    let root: String = row.get(2)?;
                    let bytes: i64 = row.get(3)?;
                    let rate: u32 = row.get(4)?;
                    let channels: u16 = row.get(5)?;
                    let format: i64 = row.get(6)?;

                    Ok(candidate_of(
                        id, &path, &root, bytes, rate, channels, format,
                    ))
                },
            )
        })
    }

    pub fn recompress_all(&self, ceiling: Ceiling) -> Result<RecompressHandle> {
        recompressing::start(self.shared(), self.recompress_candidates()?, ceiling)
    }

    pub(crate) fn landed_after_recompressing(
        &self,
        walking: &Walk,
        landed: &[RecompressCandidate],
    ) {
        let mut by_root: Vec<(&Path, Vec<PathBuf>)> = Vec::new();

        for candidate in landed {
            match by_root
                .iter_mut()
                .find(|(root, _)| *root == candidate.root.as_path())
            {
                Some((_, paths)) => paths.push(candidate.path.clone()),
                None => by_root.push((&candidate.root, vec![candidate.path.clone()])),
            }
        }
        for (root, paths) in by_root {
            self.landed_read(walking, root, &paths);
        }
    }

    fn landed_read(&self, walking: &Walk, root: &Path, paths: &[PathBuf]) {
        match scan::read_the_landed_while_walking(&self.inner, walking, root, paths) {
            Ok(LandedRead::Read) => {}
            Ok(LandedRead::OwesTheRootAWalk) => {
                tracing::debug!(root = %root.display(), "a re-compressed file is read by the next walk of its root");
            }
            Err(error) => {
                tracing::warn!(%error, root = %root.display(), "a re-compressed file is read by the next walk of its root");
            }
        }
    }

    fn row_to_recompress(&self, track: TrackId) -> Result<Option<RowToRecompress>> {
        self.inner.read(|connection| {
            connection
                .query_row(THE_ROW_TO_RECOMPRESS, params![track.get() as i64], |row| {
                    Ok(RowToRecompress {
                        path: store::path_of(&row.get::<_, String>(0)?),
                        root: row
                            .get::<_, Option<String>>(1)?
                            .map(|root| store::path_of(&root)),
                        codec: row.get(2)?,
                        vaulted: row.get(3)?,
                        cut: row.get(4)?,
                    })
                })
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }
}

fn candidate_of(
    id: i64,
    path: &str,
    root: &str,
    bytes: i64,
    rate: u32,
    channels: u16,
    format: i64,
) -> Result<RecompressCandidate> {
    let track = TrackId::new(id as u64)?;

    Ok(RecompressCandidate {
        track,
        path: store::path_of(path),
        root: store::path_of(root),
        bytes: bytes.max(0) as u64,
        spec: StreamSpec::new(
            SampleRate::new(rate)?,
            ChannelLayout::from_count(ChannelCount::new(channels)?),
            store::format_from_code(format, track)?,
        ),
    })
}
