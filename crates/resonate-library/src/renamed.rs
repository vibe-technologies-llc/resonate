use std::collections::VecDeque;

use ahash::AHashSet;
use resonate_core::TrackId;

use crate::CatalogStamp;

pub(crate) const RETITLED: &str = "titles_moved";
pub(crate) const ADDED: &str = "titles_added";
pub(crate) const RENAMED_HELD_AT_MOST: usize = 16_384;

pub(crate) const RENAMED_TRIGGERS: &str = "
CREATE TEMP TABLE titles_moved (id INTEGER PRIMARY KEY);
CREATE TEMP TABLE titles_added (id INTEGER PRIMARY KEY);

CREATE TEMP TRIGGER track_titled AFTER INSERT ON main.tracks
BEGIN
    INSERT OR REPLACE INTO temp.titles_added (id) VALUES (NEW.id);
    DELETE FROM temp.titles_added WHERE id = NEW.id;
END;
CREATE TEMP TRIGGER track_untitled AFTER DELETE ON main.tracks
BEGIN
    INSERT OR REPLACE INTO temp.titles_moved (id) VALUES (OLD.id);
    DELETE FROM temp.titles_moved WHERE id = OLD.id;
END;
CREATE TEMP TRIGGER track_retitled AFTER UPDATE OF title, path, span_start, span_frames ON main.tracks
WHEN OLD.title IS NOT NEW.title
    OR OLD.path IS NOT NEW.path
    OR OLD.span_start IS NOT NEW.span_start
    OR OLD.span_frames IS NOT NEW.span_frames
BEGIN
    INSERT OR REPLACE INTO temp.titles_moved (id) VALUES (NEW.id);
    DELETE FROM temp.titles_moved WHERE id = NEW.id;
END;
";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamesMoved {
    Nothing,
    These {
        retitled: AHashSet<TrackId>,
        added: bool,
    },
    Everything,
}

impl NamesMoved {
    pub fn names(&self, track: Option<TrackId>) -> bool {
        match (self, track) {
            (Self::Nothing, _) => false,
            (Self::Everything, _) => true,
            (Self::These { retitled, .. }, Some(track)) => retitled.contains(&track),
            (Self::These { added, .. }, None) => *added,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Retitled {
    at: u64,
    track: TrackId,
    added: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Renamed {
    held: VecDeque<Retitled>,
    lost_through: Option<u64>,
}

impl Renamed {
    pub(crate) fn note(&mut self, at: u64, row: i64, added: bool) {
        let Some(track) = u64::try_from(row)
            .ok()
            .and_then(|row| TrackId::new(row).ok())
        else {
            return;
        };
        if self.held.len() >= RENAMED_HELD_AT_MOST
            && let Some(lost) = self.held.pop_front()
        {
            self.lost_through = Some(self.lost_through.map_or(lost.at, |was| was.max(lost.at)));
        }
        self.held.push_back(Retitled { at, track, added });
    }

    pub(crate) fn since(&self, then: CatalogStamp, now: CatalogStamp) -> NamesMoved {
        if then.still_holds_at(now) {
            return NamesMoved::Nothing;
        }
        if !then.written_here_alone_until(now)
            || self.lost_through.is_some_and(|lost| lost > then.named)
        {
            return NamesMoved::Everything;
        }
        let mut retitled = AHashSet::new();
        let mut added = false;
        for moved in self.held.iter().filter(|moved| moved.at > then.named) {
            retitled.insert(moved.track);
            added |= moved.added;
        }
        NamesMoved::These { retitled, added }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(named: u64) -> CatalogStamp {
        CatalogStamp::at(named, Some(1))
    }

    fn track(raw: u64) -> TrackId {
        TrackId::new(raw).expect("non-zero")
    }

    #[test]
    fn only_the_rows_retitled_since_a_stamp_are_named_as_moved() {
        let mut renamed = Renamed::default();
        renamed.note(1, 7, false);
        renamed.note(3, 8, false);
        renamed.note(4, 9, true);

        let moved = renamed.since(stamp(2), stamp(5));

        assert!(!moved.names(Some(track(7))));
        assert!(moved.names(Some(track(8))));
        assert!(moved.names(Some(track(9))));
        assert!(moved.names(None));
        assert_eq!(renamed.since(stamp(5), stamp(5)), NamesMoved::Nothing);
    }

    #[test]
    fn a_row_unknown_to_the_catalog_is_read_again_only_where_a_row_was_added() {
        let mut renamed = Renamed::default();
        renamed.note(2, 7, false);

        let moved = renamed.since(stamp(1), stamp(2));

        assert!(moved.names(Some(track(7))));
        assert!(!moved.names(None));
    }

    #[test]
    fn a_journal_that_let_go_of_what_moved_since_a_stamp_names_everything() {
        let mut renamed = Renamed::default();
        for at in 1..=RENAMED_HELD_AT_MOST as u64 + 1 {
            renamed.note(at, at as i64, false);
        }

        assert_eq!(
            renamed.since(stamp(0), stamp(u64::MAX)),
            NamesMoved::Everything
        );
        assert_ne!(
            renamed.since(stamp(5), stamp(u64::MAX)),
            NamesMoved::Everything
        );
    }

    #[test]
    fn another_process_writing_the_catalog_names_everything() {
        let renamed = Renamed::default();

        assert_eq!(
            renamed.since(CatalogStamp::at(1, Some(1)), CatalogStamp::at(1, Some(2))),
            NamesMoved::Everything
        );
    }
}
