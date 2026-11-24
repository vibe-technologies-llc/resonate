use rusqlite::{OptionalExtension, params};

use crate::{
    Error, Result, StoreOp, Track,
    catalog::{Catalog, read},
};

pub(crate) const INDEXING_REVISION: i64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DocumentStamp {
    size: u64,
    modified_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Restamped {
    Kept,
    Moved,
}

impl DocumentStamp {
    pub(crate) fn of(size: u64, modified_ms: i64) -> Option<Self> {
        (size != 0 || modified_ms != 0).then_some(Self { size, modified_ms })
    }

    pub(crate) fn size_column(self) -> i64 {
        self.size.min(i64::MAX as u64) as i64
    }

    pub(crate) const fn modified_column(self) -> i64 {
        self.modified_ms
    }
}

impl Catalog {
    pub(crate) fn unchanged(&self, uri: &str, stamp: DocumentStamp) -> Result<Option<Track>> {
        self.0
            .lock()
            .query_row(
                "SELECT * FROM documents WHERE uri = ?1 AND missing = 0 AND size = ?2 AND modified = ?3 AND indexed_by = ?4",
                params![
                    uri,
                    stamp.size_column(),
                    stamp.modified_column(),
                    INDEXING_REVISION
                ],
                read,
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::*;

    fn study(harness: &Harness, id: u64) {
        harness
            .player
            .catalog
            .0
            .lock()
            .execute(
                "INSERT INTO studies (track, verdict, peak, rms, true_peak, clipped, band_levels, band_peaks)
                 VALUES (?1, 'genuine', 0.5, 0.1, 0.5, 0, zeroblob(29), zeroblob(472))",
                [id as i64],
            )
            .expect("a study");
    }

    fn studied(harness: &Harness, id: u64) -> bool {
        harness
            .player
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT band_levels IS NOT NULL AND band_peaks IS NOT NULL FROM studies WHERE track = ?1",
                [id as i64],
                |row| row.get(0),
            )
            .expect("a study")
    }

    fn stamp_held(harness: &Harness, id: u64) -> (Option<i64>, Option<i64>, Option<i64>) {
        harness
            .player
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT size, modified, indexed_by FROM documents WHERE id = ?1",
                [id as i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("a document")
    }

    #[test]
    fn an_open_with_that_knows_no_stamp_keeps_the_stamp_and_the_analysis_of_a_known_document() {
        let harness = Harness::new();
        let uri = format!("{FOLDER}a.wav");

        let song = harness
            .player
            .scan_document(uri.clone(), "a.wav".into(), 100, 5)
            .expect("a first scan");

        study(&harness, song.id);

        let opened = harness
            .player
            .index_document(uri, "a.wav".into())
            .expect("an open-with");

        assert_eq!(opened.id, song.id);
        assert!(studied(&harness, song.id));
        assert_eq!(stamp_held(&harness, song.id), (Some(100), Some(5), None));
    }

    #[test]
    fn an_unconfirmed_open_with_is_confirmed_by_the_next_scan_which_keeps_the_analysis_while_the_stamp_holds()
     {
        let harness = Harness::new();
        let uri = format!("{FOLDER}a.wav");

        let song = harness
            .player
            .scan_document(uri.clone(), "a.wav".into(), 100, 5)
            .expect("a first scan");

        study(&harness, song.id);
        harness
            .player
            .index_document(uri.clone(), "a.wav".into())
            .expect("an open-with");
        harness
            .player
            .scan_document(uri, "a.wav".into(), 100, 5)
            .expect("a rescan");

        assert!(studied(&harness, song.id));
        assert_eq!(
            stamp_held(&harness, song.id),
            (Some(100), Some(5), Some(INDEXING_REVISION))
        );
    }

    #[test]
    fn a_scan_that_finds_a_changed_stamp_forgets_the_analysis_and_takes_the_new_stamp() {
        let harness = Harness::new();
        let uri = format!("{FOLDER}a.wav");

        let song = harness
            .player
            .scan_document(uri.clone(), "a.wav".into(), 100, 5)
            .expect("a first scan");

        study(&harness, song.id);
        harness
            .player
            .scan_document(uri, "a.wav".into(), 100, 6)
            .expect("a changed scan");

        assert!(!studied(&harness, song.id));
        assert_eq!(
            stamp_held(&harness, song.id),
            (Some(100), Some(6), Some(INDEXING_REVISION))
        );
    }

    #[test]
    fn a_document_never_stamped_stays_unstamped_when_opened_with_no_stamp() {
        let harness = Harness::new();

        let song = harness
            .player
            .index_document(format!("{FOLDER}a.wav"), "a.wav".into())
            .expect("an open-with");

        assert_eq!(stamp_held(&harness, song.id), (None, None, None));
    }
}
