use resonate_core::{Frames, Resumable, Resumption, Span, plays_in};

pub(crate) struct Kept {
    pub(crate) rows: Vec<Option<Resumable>>,
    pub(crate) order: Vec<usize>,
    pub(crate) row: usize,
    pub(crate) at: Frames,
    pub(crate) shuffle: bool,
    pub(crate) next: Option<Span>,
}

impl Kept {
    pub(crate) fn resumed(self) -> Option<Resumption> {
        if self.rows.iter().all(Option::is_some) {
            return self.unchanged();
        }

        let drawn = plays_in(&self.order, self.rows.len());
        let loaded_as = renumbered(&self.rows);
        let survives: Vec<bool> = drawn
            .iter()
            .map(|loaded| loaded_as.get(*loaded).copied().flatten().is_some())
            .collect();
        let surviving_before = |position: usize| {
            survives[..position.min(survives.len())]
                .iter()
                .filter(|kept| **kept)
                .count()
        };

        let playing = self.row.min(drawn.len().saturating_sub(1));
        let still_playing = survives.get(playing).copied().unwrap_or(false);
        let next = self
            .next
            .filter(|next| next.last() < drawn.len())
            .and_then(|next| {
                let held = survives[next.range()].iter().filter(|kept| **kept).count();
                let first = surviving_before(next.first());
                held.checked_sub(1)
                    .map(|past_the_first| Span::between(first, first + past_the_first))
            });
        let order = drawn
            .iter()
            .filter_map(|loaded| loaded_as.get(*loaded).copied().flatten())
            .collect();
        let rows: Vec<Resumable> = self.rows.into_iter().flatten().collect();

        (!rows.is_empty()).then(|| Resumption {
            rows,
            order,
            row: surviving_before(playing),
            at: if still_playing { self.at } else { Frames::ZERO },
            shuffle: self.shuffle,
            next,
        })
    }

    fn unchanged(self) -> Option<Resumption> {
        let rows: Vec<Resumable> = self.rows.into_iter().flatten().collect();

        (!rows.is_empty()).then_some(Resumption {
            rows,
            order: self.order,
            row: self.row,
            at: self.at,
            shuffle: self.shuffle,
            next: self.next,
        })
    }
}

fn renumbered(rows: &[Option<Resumable>]) -> Vec<Option<usize>> {
    let mut kept = 0;
    rows.iter()
        .map(|row| {
            row.as_ref().map(|_| {
                kept += 1;
                kept - 1
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use resonate_core::MediaLocation;

    use super::*;

    const PLAYED_INTO: Frames = Frames(44_100);

    fn row(number: usize) -> Resumable {
        Resumable {
            location: MediaLocation::local(format!("/music/{number}.flac")),
            span: None,
            held: None,
        }
    }

    fn kept(rows: &[bool], order: Vec<usize>, row: usize, next: Option<Span>) -> Kept {
        Kept {
            rows: rows
                .iter()
                .enumerate()
                .map(|(number, opens)| opens.then(|| self::row(number)))
                .collect(),
            order,
            row,
            at: PLAYED_INTO,
            shuffle: true,
            next,
        }
    }

    fn named(resumption: &Resumption) -> Vec<usize> {
        resumption
            .in_play_order()
            .into_iter()
            .map(|loaded| {
                resumption.rows[loaded]
                    .location
                    .locator()
                    .to_string()
                    .trim_start_matches("/music/")
                    .trim_end_matches(".flac")
                    .parse()
                    .expect("a fixture row is named by its number")
            })
            .collect()
    }

    #[test]
    fn a_queue_whose_rows_all_open_comes_back_exactly_as_kept() {
        let resumed = kept(&[true, true, true], vec![2, 0, 1], 1, Some(Span::one(2)))
            .resumed()
            .expect("a whole queue resumes");

        assert_eq!(resumed.order, vec![2, 0, 1]);
        assert_eq!(resumed.row, 1);
        assert_eq!(resumed.at, PLAYED_INTO);
        assert_eq!(resumed.next, Some(Span::one(2)));
    }

    #[test]
    fn a_row_that_will_not_open_is_dropped_and_the_place_still_names_the_track_it_named() {
        let resumed = kept(&[true, false, true, true], vec![3, 1, 0, 2], 2, None)
            .resumed()
            .expect("a queue one row short resumes");

        assert_eq!(named(&resumed), vec![3, 0, 2]);
        assert_eq!(resumed.rows.len(), 3);
        assert_eq!(resumed.row, 1);
        assert_eq!(named(&resumed)[resumed.landing()], 0);
        assert_eq!(resumed.at, PLAYED_INTO);
        assert!(resumed.shuffle);
    }

    #[test]
    fn the_playing_row_dropped_resumes_at_the_start_of_the_one_after_it() {
        let resumed = kept(&[true, true, false, true], vec![0, 1, 2, 3], 2, None)
            .resumed()
            .expect("a queue one row short resumes");

        assert_eq!(named(&resumed), vec![0, 1, 3]);
        assert_eq!(named(&resumed)[resumed.landing()], 3);
        assert_eq!(resumed.at, Frames::ZERO);
    }

    #[test]
    fn the_queued_next_span_closes_over_a_dropped_row() {
        let resumed = kept(
            &[true, true, false, true, true],
            vec![0, 1, 2, 3, 4],
            0,
            Some(Span::between(1, 3)),
        )
        .resumed()
        .expect("a queue one row short resumes");

        assert_eq!(resumed.next, Some(Span::between(1, 2)));

        let emptied = kept(&[true, false, true], vec![0, 1, 2], 0, Some(Span::one(1)))
            .resumed()
            .expect("a queue one row short resumes");

        assert_eq!(emptied.next, None);
    }

    #[test]
    fn a_queue_none_of_whose_rows_open_is_not_resumed() {
        assert_eq!(kept(&[false, false], vec![1, 0], 0, None).resumed(), None);
    }
}
