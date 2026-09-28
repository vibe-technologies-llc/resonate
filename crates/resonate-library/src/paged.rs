use std::path::{Path, PathBuf};

use crate::error::Result;

pub(crate) const ROWS_A_PAGE: usize = 2_048;

pub(crate) struct Paging {
    after: Option<PathBuf>,
    at_most: usize,
    rows_a_page: usize,
    done: bool,
}

impl Paging {
    pub(crate) const fn by(rows_a_page: usize) -> Self {
        Self {
            after: None,
            at_most: rows_a_page,
            rows_a_page,
            done: false,
        }
    }

    pub(crate) fn next<R>(
        &mut self,
        fetch: impl Fn(Option<&Path>, usize) -> Result<Vec<R>>,
        path_of: impl Fn(&R) -> &Path,
    ) -> Result<Option<Vec<R>>> {
        while !self.done {
            let mut page = fetch(self.after.as_deref(), self.at_most)?;
            let more = page.len() >= self.at_most;
            if more {
                let Some(last) = page.last().map(|row| path_of(row).to_path_buf()) else {
                    self.done = true;
                    break;
                };
                let whole = page.partition_point(|row| path_of(row) != last);
                if whole == 0 {
                    self.at_most = self.at_most.saturating_mul(2);
                    continue;
                }
                page.truncate(whole);
            }

            self.done = !more;
            self.at_most = self.rows_a_page;
            let Some(last) = page.last().map(|row| path_of(row).to_path_buf()) else {
                self.done = true;
                break;
            };
            self.after = Some(last);
            return Ok(Some(page));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Vec<PathBuf> {
        ["a", "b", "b", "b", "b", "b", "c", "d", "d", "e"]
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }

    fn fetched(held: &[PathBuf], after: Option<&Path>, at_most: usize) -> Vec<PathBuf> {
        held.iter()
            .filter(|path| after.is_none_or(|after| path.as_path() > after))
            .take(at_most)
            .cloned()
            .collect()
    }

    #[test]
    fn every_row_is_handed_out_once_and_no_file_is_split_across_two_pages() {
        let held = catalog();
        for rows_a_page in 1..=held.len() + 2 {
            let mut paging = Paging::by(rows_a_page);
            let mut seen = Vec::new();
            while let Some(page) = paging
                .next(
                    |after, at_most| Ok(fetched(&held, after, at_most)),
                    |row| row,
                )
                .expect("an answer")
            {
                assert!(!page.is_empty());
                if let (Some(before), Some(first)) = (seen.last(), page.first()) {
                    assert_ne!(before, first, "a file was split across two pages");
                }
                seen.extend(page);
            }
            assert_eq!(seen, held, "pages of {rows_a_page} lost or repeated a row");
        }
    }

    #[test]
    fn an_empty_catalog_hands_out_nothing() {
        let mut paging = Paging::by(4);
        let page = paging
            .next(|_, _| Ok(Vec::<PathBuf>::new()), |row| row)
            .expect("an answer");
        assert!(page.is_none());
    }
}
