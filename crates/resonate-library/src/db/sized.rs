use std::path::PathBuf;

use resonate_codec::Codec;
use resonate_core::{ChannelCount, ChannelLayout, SampleRate, StreamSpec, TrackId};
use rusqlite::{Connection, types::Value};

use super::{Library, rows::rows};
use crate::{Result, store};

const A_FILES_FIRST_ROW: &str = "
SELECT t.id, t.path, t.file_size, t.codec, t.sample_rate, t.channels, t.sample_format,
       t.vault_key IS NOT NULL,
       (SELECT group_concat(id) FROM (SELECT o.id FROM tracks o
                                       WHERE o.path = t.path ORDER BY o.span_start))
  FROM tracks t";

const FIRST_OF_ITS_FILE: &str =
    "NOT EXISTS (SELECT 1 FROM tracks e WHERE e.path = t.path AND e.span_start < t.span_start)";

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SizedAfter {
    pub bytes: u64,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SizedFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub codec: Codec,
    pub spec: StreamSpec,
    pub vaulted: bool,
    pub tracks: Vec<TrackId>,
}

impl SizedFile {
    pub fn key(&self) -> SizedAfter {
        SizedAfter {
            bytes: self.bytes,
            path: self.path.clone(),
        }
    }
}

impl Library {
    pub fn largest_files(
        &self,
        after: Option<&SizedAfter>,
        limit: usize,
    ) -> Result<Vec<SizedFile>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);

        self.inner.read(|connection| {
            let Some(after) = after else {
                return sized(
                    connection,
                    &format!("{A_FILES_FIRST_ROW} WHERE {FIRST_OF_ITS_FILE} ORDER BY t.file_size DESC, t.path LIMIT ?1"),
                    vec![Value::Integer(limit)],
                );
            };

            let bytes = i64::try_from(after.bytes).unwrap_or(i64::MAX);
            let path = store::path_text(&after.path).into_owned();

            let mut page = sized(
                connection,
                &format!("{A_FILES_FIRST_ROW} WHERE t.file_size = ?1 AND t.path > ?2 AND {FIRST_OF_ITS_FILE} ORDER BY t.file_size DESC, t.path LIMIT ?3"),
                vec![Value::Integer(bytes), Value::Text(path), Value::Integer(limit)],
            )?;

            let left = limit - page.len() as i64;

            if left > 0 {
                page.extend(sized(
                    connection,
                    &format!("{A_FILES_FIRST_ROW} WHERE t.file_size < ?1 AND {FIRST_OF_ITS_FILE} ORDER BY t.file_size DESC, t.path LIMIT ?2"),
                    vec![Value::Integer(bytes), Value::Integer(left)],
                )?);
            }
            Ok(page)
        })
    }
}

fn sized(connection: &Connection, sql: &str, binds: Vec<Value>) -> Result<Vec<SizedFile>> {
    rows(connection, sql, binds, |row| {
        let id: i64 = row.get(0)?;
        let path: String = row.get(1)?;
        let bytes: i64 = row.get(2)?;
        let codec: i64 = row.get(3)?;
        let rate: u32 = row.get(4)?;
        let channels: u16 = row.get(5)?;
        let format: i64 = row.get(6)?;
        let vaulted: bool = row.get(7)?;
        let ids: Option<String> = row.get(8)?;

        Ok(sized_file(
            Read {
                id,
                path,
                bytes,
                codec,
                rate,
                channels,
                format,
            },
            vaulted,
            ids.as_deref(),
        ))
    })
}

struct Read {
    id: i64,
    path: String,
    bytes: i64,
    codec: i64,
    rate: u32,
    channels: u16,
    format: i64,
}

fn sized_file(read: Read, vaulted: bool, ids: Option<&str>) -> Result<SizedFile> {
    let first = TrackId::new(read.id as u64)?;
    let tracks = ids
        .unwrap_or_default()
        .split(',')
        .filter_map(|id| id.parse::<u64>().ok())
        .map(TrackId::new)
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(SizedFile {
        path: store::path_of(&read.path),
        bytes: read.bytes.max(0) as u64,
        codec: store::codec_from_code(read.codec, first)?,
        spec: StreamSpec::new(
            SampleRate::new(read.rate)?,
            ChannelLayout::from_count(ChannelCount::new(read.channels)?),
            store::format_from_code(read.format, first)?,
        ),
        vaulted,
        tracks,
    })
}
