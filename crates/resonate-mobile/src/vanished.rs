use crate::{Error, MobilePlayer, Result, StoreOp, catalog::Catalog, documents::location};

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct MissingSongs {
    pub hidden: u32,
    pub restored: u32,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn hide_missing_songs(&self, prefixes: Vec<String>) -> Result<MissingSongs> {
        let documents = self.catalog.documents_under(&prefixes)?;
        let mut hide = Vec::new();
        let mut restore = Vec::new();
        for (id, uri, missing) in documents {
            let Ok(at) = location(&uri) else { continue };
            let gone = self.sources.open(&at).is_err();
            if gone && !missing {
                hide.push(id);
            } else if !gone && missing {
                restore.push(id);
            }
        }
        self.catalog.mark_missing(&hide, &restore)?;
        Ok(MissingSongs {
            hidden: u32::try_from(hide.len()).unwrap_or(u32::MAX),
            restored: u32::try_from(restore.len()).unwrap_or(u32::MAX),
        })
    }
}

impl Catalog {
    pub(crate) fn documents_under(&self, prefixes: &[String]) -> Result<Vec<(i64, String, bool)>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare("SELECT id, uri, missing FROM documents ORDER BY id")
            .map_err(read_error)?;
        let documents = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        Ok(documents
            .into_iter()
            .filter(|(_, uri, _)| prefixes.iter().any(|prefix| uri.starts_with(prefix)))
            .collect())
    }

    pub(crate) fn mark_missing(&self, hide: &[i64], restore: &[i64]) -> Result<()> {
        if hide.is_empty() && restore.is_empty() {
            return Ok(());
        }
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        for (ids, flag) in [(hide, 1), (restore, 0)] {
            for id in ids {
                transaction
                    .execute(
                        "UPDATE documents SET missing = ?2 WHERE id = ?1",
                        [*id, flag],
                    )
                    .map_err(write_error)?;
            }
        }
        transaction.commit().map_err(write_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(catalog: &Catalog, name: &str) -> crate::Track {
        catalog
            .upsert(crate::Track {
                uri: format!("resonate-folder://local/{name}"),
                name: name.into(),
                title: name.into(),
                ..crate::Track::default()
            })
            .expect("upserted")
    }

    #[test]
    fn a_missing_song_leaves_the_lists_and_returns_when_its_file_does() {
        let folder = std::env::temp_dir().join(format!("resonate-vanished-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        let catalog = Catalog::open(&folder.join("catalog.sqlite")).expect("opened");
        let kept = track(&catalog, "kept.flac");
        let lost = track(&catalog, "lost.flac");

        catalog
            .mark_missing(&[lost.id as i64], &[])
            .expect("hidden");

        let shown = catalog.tracks("", 0, 10).expect("listed");
        assert_eq!(
            shown.iter().map(|song| song.id).collect::<Vec<_>>(),
            [kept.id]
        );

        track(&catalog, "lost.flac");
        let shown = catalog.tracks("", 0, 10).expect("listed");
        assert_eq!(shown.len(), 2);

        let _ = std::fs::remove_dir_all(&folder);
    }
}
