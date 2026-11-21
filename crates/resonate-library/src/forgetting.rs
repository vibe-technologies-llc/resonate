use std::{borrow::Cow, collections::BTreeSet, path::PathBuf};

use resonate_core::MediaLocation;
use rusqlite::{Transaction, params_from_iter, types::Value};

use crate::{Error, Result, StoreOp, alternatives, store};

const PATHS_AT_ONCE: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Forgetting {
    Deleted,
    Folder,
    Delivery,
    Vanished,
    Tidied,
    Unreached,
    Superseded,
    Displaced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Afterwards {
    TheCaller,
    SweepAndSettleWhereRowsWent,
    SweepAndSettleAlways,
}

impl Forgetting {
    fn keeps_the_listens(self) -> bool {
        match self {
            Self::Deleted
            | Self::Folder
            | Self::Delivery
            | Self::Vanished
            | Self::Tidied
            | Self::Unreached
            | Self::Superseded => true,
            Self::Displaced => false,
        }
    }

    fn drops_the_wants(self) -> bool {
        match self {
            Self::Deleted => true,
            Self::Folder
            | Self::Delivery
            | Self::Vanished
            | Self::Tidied
            | Self::Unreached
            | Self::Superseded
            | Self::Displaced => false,
        }
    }

    fn afterwards(self) -> Afterwards {
        match self {
            Self::Deleted | Self::Delivery | Self::Vanished => {
                Afterwards::SweepAndSettleWhereRowsWent
            }
            Self::Folder => Afterwards::SweepAndSettleAlways,
            Self::Tidied | Self::Unreached | Self::Superseded | Self::Displaced => {
                Afterwards::TheCaller
            }
        }
    }
}

pub(crate) enum Rows<'a> {
    AtPath(&'a str),
    RootlessAtPath(&'a str),
    UnvaultedOfRoot(i64),
    UnvaultedAtPaths { root: i64, paths: &'a [PathBuf] },
    Unreached { roots: &'a str, generation: i64 },
    Row(i64),
}

struct Condition {
    clause: Cow<'static, str>,
    bound: Vec<Value>,
    named: Option<String>,
}

impl Rows<'_> {
    fn conditions(&self) -> Vec<Condition> {
        match self {
            Self::AtPath(path) => vec![Condition {
                clause: Cow::Borrowed("t.path = ?1"),
                bound: vec![Value::Text((*path).to_owned())],
                named: Some((*path).to_owned()),
            }],
            Self::RootlessAtPath(path) => vec![Condition {
                clause: Cow::Borrowed("t.root_id IS NULL AND t.path = ?1"),
                bound: vec![Value::Text((*path).to_owned())],
                named: Some((*path).to_owned()),
            }],
            Self::UnvaultedOfRoot(root) => vec![Condition {
                clause: Cow::Borrowed("t.root_id = ?1 AND t.vault_key IS NULL"),
                bound: vec![Value::Integer(*root)],
                named: None,
            }],
            Self::UnvaultedAtPaths { root, paths } => paths
                .chunks(PATHS_AT_ONCE)
                .map(|batch| {
                    let held = vec!["?"; batch.len()].join(",");
                    let mut bound = Vec::with_capacity(batch.len() + 1);

                    bound.push(Value::Integer(*root));
                    bound.extend(
                        batch
                            .iter()
                            .map(|path| Value::Text(store::path_text(path).into_owned())),
                    );
                    Condition {
                        clause: Cow::Owned(format!(
                            "t.root_id = ?1 AND t.vault_key IS NULL AND t.path IN ({held})"
                        )),
                        bound,
                        named: None,
                    }
                })
                .collect(),
            Self::Unreached { roots, generation } => vec![Condition {
                clause: Cow::Owned(format!(
                    "t.seen = -?1 AND t.root_id IN ({roots}) AND t.vault_key IS NULL"
                )),
                bound: vec![Value::Integer(*generation)],
                named: None,
            }],
            Self::Row(id) => vec![Condition {
                clause: Cow::Borrowed("t.id = ?1"),
                bound: vec![Value::Integer(*id)],
                named: None,
            }],
        }
    }
}

pub(crate) fn forget_rows(tx: &Transaction<'_>, why: Forgetting, rows: &[Rows<'_>]) -> Result<u64> {
    let mut forgotten = 0;

    for condition in rows.iter().flat_map(Rows::conditions) {
        if why.keeps_the_listens() {
            keep_the_listens(tx, &condition)?;
        }
        if why.drops_the_wants() {
            drop_the_wants(tx, &condition)?;
        }
        forgotten += store::cached(
            tx,
            &format!("DELETE FROM tracks AS t WHERE {}", condition.clause),
            params_from_iter(condition.bound.iter()),
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))? as u64;
    }

    let rows_went = forgotten > 0;

    match why.afterwards() {
        Afterwards::TheCaller => {}
        Afterwards::SweepAndSettleWhereRowsWent => {
            if rows_went {
                store::sweep_orphans(tx)?;
                alternatives::settle_if_owed(tx)?;
            }
        }
        Afterwards::SweepAndSettleAlways => {
            store::sweep_orphans(tx)?;
            alternatives::settle_if_owed(tx)?;
        }
    }
    Ok(forgotten)
}

fn keep_the_listens(tx: &Transaction<'_>, condition: &Condition) -> Result<()> {
    store::cached(
        tx,
        &format!(
            "INSERT INTO unheld_listens
                (path, span_start, at, began, heard, listen_id, elsewhere, origin_device, origin_id)
         SELECT t.path, t.span_start, l.at, l.began, l.heard, l.id, l.elsewhere, l.origin_device,
                l.origin_id
           FROM listens l JOIN tracks t ON t.id = l.track_id
          WHERE {}",
            condition.clause
        ),
        params_from_iter(condition.bound.iter()),
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;
    Ok(())
}

fn drop_the_wants(tx: &Transaction<'_>, condition: &Condition) -> Result<()> {
    let mut offered: BTreeSet<String> = condition.named.iter().cloned().collect();
    let selected = tx
        .prepare_cached(&format!(
            "SELECT DISTINCT t.path FROM tracks t WHERE {}",
            condition.clause
        ))
        .and_then(|mut statement| {
            statement
                .query_map(params_from_iter(condition.bound.iter()), |row| {
                    row.get::<_, String>(0)
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        })
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    offered.extend(selected);

    for path in offered {
        store::cached(
            tx,
            "DELETE FROM wants WHERE offered = ?1",
            [MediaLocation::local(store::path_of(&path)).to_uri()],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    }

    store::cached(
        tx,
        &format!(
            "DELETE FROM wants
              WHERE release_track_id IN
                    (SELECT id FROM release_tracks
                      WHERE track_id IN (SELECT t.id FROM tracks t WHERE {}))",
            condition.clause
        ),
        params_from_iter(condition.bound.iter()),
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs, iter,
        path::{Path, PathBuf},
    };

    const THE_ONLY_MODULE_THAT_DELETES_TRACKS: &str = "forgetting.rs";

    fn sources_under(folder: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(folder)
            .expect("a source folder is read")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                sources_under(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    fn the_code_apart_from_its_tests(source: &str) -> &str {
        let tests_begin = source
            .match_indices("\n#[cfg(test)]\nmod tests")
            .next()
            .map_or(source.len(), |(at, _)| at);
        &source[..tests_begin]
    }

    fn deletes_tracks(source: &str) -> Vec<usize> {
        const STATEMENT: &str = "delete from tracks";

        let mut squeezed = String::with_capacity(source.len());
        let mut line_of: Vec<usize> = Vec::with_capacity(source.len());
        let mut line = 1;

        for letter in source.chars() {
            let seen_on = line;

            if letter == '\n' {
                line += 1;
            }

            let letter = match letter.is_whitespace() {
                true => ' ',
                false => letter.to_ascii_lowercase(),
            };

            if letter == ' ' && squeezed.ends_with(' ') {
                continue;
            }
            squeezed.push(letter);
            line_of.extend(iter::repeat_n(seen_on, letter.len_utf8()));
        }

        squeezed
            .match_indices(STATEMENT)
            .filter(|(at, _)| {
                !squeezed[at + STATEMENT.len()..]
                    .starts_with(|next: char| next == '_' || next.is_ascii_alphanumeric())
            })
            .map(|(at, _)| line_of[at])
            .collect()
    }

    #[test]
    fn no_source_in_the_library_deletes_tracks_outside_forget_rows() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut sources = Vec::new();

        sources_under(&root, &mut sources);

        let mut raw = Vec::new();

        for source in sources {
            let name = source
                .strip_prefix(&root)
                .expect("a source sits under src")
                .to_string_lossy()
                .replace('\\', "/");

            if name == THE_ONLY_MODULE_THAT_DELETES_TRACKS || name.ends_with("tests.rs") {
                continue;
            }

            let text = fs::read_to_string(&source)
                .expect("a source is read")
                .replace("\r\n", "\n");

            raw.extend(
                deletes_tracks(the_code_apart_from_its_tests(&text))
                    .into_iter()
                    .map(|line| format!("{name}:{line}")),
            );
        }

        assert!(
            raw.is_empty(),
            "rows of tracks deleted outside forgetting::forget_rows:\n{}",
            raw.join("\n")
        );
    }

    #[test]
    fn a_delete_of_tracks_is_told_apart_from_one_of_another_table() {
        assert_eq!(
            deletes_tracks("x\n\"DELETE   FROM\n   tracks WHERE a\""),
            [2]
        );
        assert_eq!(deletes_tracks("delete from Tracks"), [1]);
        assert!(deletes_tracks("DELETE FROM tracks_fts WHERE a").is_empty());
        assert!(deletes_tracks("DELETE FROM tracks2 WHERE a").is_empty());
        assert!(deletes_tracks("DELETE FROM track_credits").is_empty());
        assert!(deletes_tracks("UPDATE tracks SET a = 1").is_empty());
    }
}
