use std::{path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params_from_iter, types::Value};

use super::copy::Table;
use crate::{Error, Result, Store, StoreOp};

pub const TABLES_FILE: &str = "musicbrainz.tables.sqlite";
pub const TABLES_FORMAT: u32 = 1;
const WAITED_FOR_A_READER: Duration = Duration::from_secs(30);

const KEPT_BESIDE_THE_BUILD: &str = "
DROP TABLE placed_tracks;
CREATE INDEX kept_track_id ON track(id);
CREATE INDEX kept_track_medium ON track(medium, position);
CREATE INDEX kept_track_credit ON track(artist_credit);
CREATE INDEX kept_recording_credit ON recording(artist_credit);
CREATE INDEX kept_release_credit ON release(artist_credit);
CREATE INDEX kept_release_country ON release_country(release);
CREATE INDEX kept_release_unknown_country ON release_unknown_country(release);
CREATE INDEX kept_release_group_meta ON release_group_meta(id);
CREATE INDEX kept_secondary_types ON release_group_secondary_type_join(release_group);
CREATE INDEX kept_release_label ON release_label(label);
CREATE INDEX kept_artist_urls ON l_artist_url(entity1);
CREATE INDEX kept_recording_urls ON l_recording_url(entity1);
CREATE INDEX kept_release_group_urls ON l_release_group_url(entity1);
CREATE INDEX kept_release_urls ON l_release_url(entity1);
CREATE VIEW placed_tracks AS
    SELECT t.medium AS medium, t.position AS position, t.number AS number, t.name AS name,
           t.artist_credit AS artist_credit, t.gid AS gid, t.length AS length,
           t.recording AS recording, r.gid AS recording_gid, r.length AS recording_length,
           (SELECT group_concat(isrc, ' ') FROM
               (SELECT isrc FROM isrc WHERE recording = t.recording ORDER BY isrc)) AS isrcs
    FROM track t LEFT JOIN recording r ON r.id = t.recording
    WHERE t.medium IS NOT NULL AND t.gid IS NOT NULL;
CREATE TABLE kept (
    format INTEGER NOT NULL,
    dump TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    schema INTEGER NOT NULL
);
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sequenced {
    pub replication: u64,
    pub schema: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptTables {
    pub dump: String,
    pub sequence: u64,
    pub schema: u32,
}

pub struct Change {
    pub table: &'static Table,
    pub old: Option<Vec<Value>>,
    pub new: Option<Vec<Value>>,
}

fn written(source: rusqlite::Error) -> Error {
    Error::store(Store::Tables, StoreOp::Write)(source)
}

pub(crate) fn kept_beside_the_build(
    staging: &Connection,
    dump: &str,
    sequenced: Sequenced,
) -> Result<()> {
    staging
        .execute_batch(KEPT_BESIDE_THE_BUILD)
        .map_err(Error::store(Store::Tables, StoreOp::LayOut))?;
    staging
        .execute(
            "INSERT INTO kept (format, dump, sequence, schema) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                TABLES_FORMAT,
                dump,
                i64::try_from(sequenced.replication).unwrap_or(i64::MAX),
                sequenced.schema
            ],
        )
        .map_err(written)?;
    staging
        .execute_batch("ANALYZE; PRAGMA locking_mode = NORMAL; PRAGMA journal_mode = DELETE;")
        .map_err(Error::store(Store::Tables, StoreOp::Configure))
}

pub(crate) fn opened(path: &Path) -> Result<(Connection, KeptTables)> {
    if !path.is_file() {
        return Err(Error::NoTables {
            path: path.to_path_buf(),
        });
    }
    let tables = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(Error::store(Store::Tables, StoreOp::Open))?;
    tables
        .busy_timeout(WAITED_FOR_A_READER)
        .map_err(Error::store(Store::Tables, StoreOp::Configure))?;
    let kept: Option<(u32, String, i64, u32)> = tables
        .query_row(
            "SELECT format, dump, sequence, schema FROM kept",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(Error::store(Store::Tables, StoreOp::Read))?;
    let Some((format, dump, sequence, schema)) = kept else {
        return Err(Error::TablesFormat {
            found: 0,
            reads: TABLES_FORMAT,
        });
    };
    if format != TABLES_FORMAT {
        return Err(Error::TablesFormat {
            found: format,
            reads: TABLES_FORMAT,
        });
    }
    Ok((
        tables,
        KeptTables {
            dump,
            sequence: u64::try_from(sequence).unwrap_or(0),
            schema,
        },
    ))
}

pub(crate) fn sequenced_at(tables: &Connection, sequence: u64) -> Result<()> {
    tables
        .execute(
            "UPDATE kept SET sequence = ?1",
            [i64::try_from(sequence).unwrap_or(i64::MAX)],
        )
        .map(drop)
        .map_err(written)
}

fn matching(table: &Table) -> String {
    if table.identified() {
        return "id IS ?1".to_owned();
    }
    table
        .laid_names()
        .enumerate()
        .map(|(at, column)| format!("{column} IS ?{}", at + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn keyed(table: &Table, row: &[Value]) -> Vec<Value> {
    if table.identified() {
        row.first().cloned().into_iter().collect()
    } else {
        row.to_vec()
    }
}

fn removed(tables: &Connection, table: &Table, row: &[Value]) -> Result<()> {
    let sql = format!(
        "DELETE FROM {name} WHERE rowid IN (SELECT rowid FROM {name} WHERE {matching} LIMIT 1)",
        name = table.name,
        matching = matching(table)
    );
    tables
        .prepare_cached(&sql)
        .and_then(|mut statement| statement.execute(params_from_iter(keyed(table, row))))
        .map(drop)
        .map_err(written)
}

fn inserted(tables: &Connection, table: &Table, row: &[Value]) -> Result<()> {
    let columns: Vec<&str> = table.laid_names().collect();
    let placeholders: Vec<String> = (1..=columns.len()).map(|at| format!("?{at}")).collect();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        table.name,
        columns.join(", "),
        placeholders.join(", ")
    );
    tables
        .prepare_cached(&sql)
        .and_then(|mut statement| statement.execute(params_from_iter(row.iter())))
        .map(drop)
        .map_err(written)
}

pub(crate) fn applied(tables: &Connection, change: &Change) -> Result<()> {
    let table = change.table;
    if let Some(old) = &change.old {
        removed(tables, table, old)?;
    }
    if let Some(new) = &change.new {
        if table.identified() && change.old.is_none() {
            removed(tables, table, new)?;
        }
        inserted(tables, table, new)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mirror::copy::table_called;

    fn laid_out(names: &[&str]) -> Connection {
        let tables = Connection::open_in_memory().expect("a store");
        for name in names {
            let table = table_called(name).expect("a table");
            tables
                .execute_batch(&format!("CREATE TABLE {} ({});", table.name, table.laid))
                .expect("laid out");
        }
        tables
    }

    fn text(text: &str) -> Value {
        Value::Text(text.to_owned())
    }

    fn rows(tables: &Connection, sql: &str) -> Vec<(i64, Option<String>)> {
        tables
            .prepare(sql)
            .expect("a statement")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("rows")
            .collect::<rusqlite::Result<_>>()
            .expect("every row")
    }

    #[test]
    fn a_row_with_an_id_is_inserted_updated_and_deleted_by_that_id() {
        let tables = laid_out(&["label"]);
        let label = table_called("label").expect("label");
        let change = |old: Option<Vec<Value>>, new: Option<Vec<Value>>| Change {
            table: label,
            old,
            new,
        };

        applied(
            &tables,
            &change(None, Some(vec![Value::Integer(7), text("EMI")])),
        )
        .expect("inserted");
        applied(
            &tables,
            &change(None, Some(vec![Value::Integer(8), text("Warp")])),
        )
        .expect("inserted");
        applied(
            &tables,
            &change(
                Some(vec![Value::Integer(7), text("EMI")]),
                Some(vec![Value::Integer(7), text("EMI Records")]),
            ),
        )
        .expect("updated");
        applied(
            &tables,
            &change(Some(vec![Value::Integer(8), text("Warp")]), None),
        )
        .expect("deleted");

        assert_eq!(
            rows(&tables, "SELECT id, name FROM label ORDER BY id"),
            [(7, Some("EMI Records".to_owned()))]
        );
    }

    #[test]
    fn a_row_with_no_id_is_found_by_every_value_it_held_nulls_included() {
        let tables = laid_out(&["release_label"]);
        let release_label = table_called("release_label").expect("release_label");
        let row = |label: i64, number: Option<&str>| {
            vec![
                Value::Integer(1),
                Value::Integer(label),
                number.map_or(Value::Null, text),
            ]
        };

        for (label, number) in [(10, None), (11, Some("CAT-1")), (11, Some("CAT-2"))] {
            applied(
                &tables,
                &Change {
                    table: release_label,
                    old: None,
                    new: Some(row(label, number)),
                },
            )
            .expect("inserted");
        }
        applied(
            &tables,
            &Change {
                table: release_label,
                old: Some(row(10, None)),
                new: None,
            },
        )
        .expect("deleted");
        applied(
            &tables,
            &Change {
                table: release_label,
                old: Some(row(11, Some("CAT-2"))),
                new: Some(row(12, Some("CAT-2"))),
            },
        )
        .expect("updated");

        assert_eq!(
            rows(
                &tables,
                "SELECT label, catalog_number FROM release_label ORDER BY label"
            ),
            [
                (11, Some("CAT-1".to_owned())),
                (12, Some("CAT-2".to_owned()))
            ]
        );
    }
}
