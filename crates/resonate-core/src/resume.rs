use crate::{FrameSpan, Frames, MediaLocation, Span};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Resumable {
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reordered {
    pub order: Vec<usize>,
    pub row: usize,
    pub at: Frames,
    pub shuffle: bool,
    pub next: Option<Span>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resumption {
    pub rows: Vec<Resumable>,
    pub order: Vec<usize>,
    pub row: usize,
    pub at: Frames,
    pub shuffle: bool,
    pub next: Option<Span>,
}

impl Resumption {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn landing(&self) -> usize {
        self.row.min(self.rows.len().saturating_sub(1))
    }

    pub fn in_play_order(&self) -> Vec<usize> {
        plays_in(&self.order, self.rows.len())
    }
}

pub fn plays_in(order: &[usize], rows: usize) -> Vec<usize> {
    let names_each_row_once = order.len() == rows && {
        let mut named = vec![false; rows];
        order.iter().all(|loaded| {
            named
                .get_mut(*loaded)
                .is_some_and(|seen| !std::mem::replace(seen, true))
        })
    };

    if names_each_row_once {
        return order.to_vec();
    }

    (0..rows).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(number: usize) -> Resumable {
        Resumable {
            location: MediaLocation::local(format!("/music/{number}.flac")),
            span: None,
        }
    }

    fn rows(count: usize) -> Vec<Resumable> {
        (0..count).map(row).collect()
    }

    fn kept(rows: Vec<Resumable>, order: Vec<usize>) -> Resumption {
        Resumption {
            rows,
            order,
            row: 0,
            at: Frames::ZERO,
            shuffle: false,
            next: None,
        }
    }

    #[test]
    fn a_resumption_holding_no_rows_has_nothing_to_resume() {
        let nothing = kept(Vec::new(), Vec::new());

        assert!(nothing.is_empty());
        assert_eq!(nothing.landing(), 0);
        assert_eq!(nothing.in_play_order(), Vec::<usize>::new());
    }

    #[test]
    fn a_row_past_the_end_lands_on_the_last_one() {
        let kept = Resumption {
            row: 9,
            at: Frames(1_000),
            ..kept(rows(3), vec![0, 1, 2])
        };

        assert_eq!(kept.landing(), 2);
    }

    #[test]
    fn a_row_inside_the_queue_lands_where_it_says() {
        let kept = Resumption {
            row: 1,
            at: Frames(1_000),
            ..kept(rows(3), vec![0, 1, 2])
        };

        assert_eq!(kept.landing(), 1);
    }

    #[test]
    fn an_order_naming_each_row_once_is_the_order_it_plays_in() {
        let kept = kept(rows(4), vec![2, 0, 3, 1]);

        assert_eq!(kept.in_play_order(), vec![2, 0, 3, 1]);
    }

    #[test]
    fn an_order_that_is_not_a_permutation_gives_back_the_order_it_was_loaded_in() {
        for nonsense in [
            vec![0, 1, 2, 3],
            vec![0, 0, 1],
            vec![0, 1, 9],
            vec![0, 1],
            Vec::new(),
        ] {
            let plays = kept(rows(3), nonsense.clone()).in_play_order();

            assert_eq!(
                plays.len(),
                3,
                "an order of {nonsense:?} left a row of the queue unplayable"
            );
            assert!(
                plays.iter().all(|loaded| *loaded < 3),
                "an order of {nonsense:?} named a row the queue does not hold"
            );
        }
    }

    #[test]
    fn a_queue_kept_in_the_order_it_was_loaded_comes_back_as_it_was() {
        let kept = kept(rows(4), vec![0, 1, 2, 3]);

        assert_eq!(kept.in_play_order(), vec![0, 1, 2, 3]);
        assert_eq!(kept.landing(), 0);
    }
}
