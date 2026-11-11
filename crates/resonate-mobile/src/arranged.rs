use resonate_core::{Frames, Resumable, Resumption, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Laid<T> {
    pub(crate) row: T,
    pub(crate) loaded: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Arranged<T> {
    pub(crate) rows: Vec<Laid<T>>,
    pub(crate) row: usize,
    pub(crate) shuffle: bool,
    pub(crate) next: Option<Span>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PassedOver {
    pub(crate) any: bool,
    pub(crate) the_place: bool,
}

impl<T> Default for Arranged<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            row: 0,
            shuffle: false,
            next: None,
        }
    }
}

impl<T> Arranged<T> {
    #[cfg(test)]
    pub(crate) fn in_play_order(rows: impl IntoIterator<Item = T>) -> Self {
        Self {
            rows: rows
                .into_iter()
                .enumerate()
                .map(|(loaded, row)| Laid { row, loaded })
                .collect(),
            ..Self::default()
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(crate) fn retained<U>(
        self,
        mut keep: impl FnMut(usize, T) -> Option<U>,
    ) -> (Arranged<U>, PassedOver) {
        let mut rows = Vec::with_capacity(self.rows.len());
        let mut passed = PassedOver::default();
        let mut passed_before_the_place = 0;
        let mut next_kept: Option<Span> = None;

        for (at, laid) in self.rows.into_iter().enumerate() {
            let Some(row) = keep(at, laid.row) else {
                passed.any = true;
                match at.cmp(&self.row) {
                    std::cmp::Ordering::Less => passed_before_the_place += 1,
                    std::cmp::Ordering::Equal => passed.the_place = true,
                    std::cmp::Ordering::Greater => {}
                }
                continue;
            };
            if self.next.is_some_and(|next| next.holds(at)) {
                let landed = rows.len();
                next_kept = Some(next_kept.map_or(Span::one(landed), |kept| {
                    Span::between(kept.first(), landed)
                }));
            }
            rows.push(Laid {
                row,
                loaded: laid.loaded,
            });
        }

        (
            Arranged {
                rows,
                row: self.row.saturating_sub(passed_before_the_place),
                shuffle: self.shuffle,
                next: next_kept,
            },
            passed,
        )
    }
}

impl Arranged<Resumable> {
    pub(crate) fn resumption(self, at: Frames) -> Resumption {
        let mut by_load: Vec<usize> = (0..self.rows.len()).collect();
        by_load.sort_by_key(|play| (self.rows[*play].loaded, *play));

        let mut order = vec![0; self.rows.len()];
        for (loaded, play) in by_load.iter().enumerate() {
            order[*play] = loaded;
        }
        let mut in_play_order: Vec<Option<Resumable>> =
            self.rows.into_iter().map(|laid| Some(laid.row)).collect();
        let rows = by_load
            .iter()
            .filter_map(|play| in_play_order[*play].take())
            .collect();

        Resumption {
            rows,
            order,
            row: self.row,
            at,
            shuffle: self.shuffle,
            next: self.next,
        }
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::MediaLocation;

    use super::*;

    fn resumable(name: &str) -> Resumable {
        Resumable {
            location: MediaLocation::local(format!("/music/{name}.flac")),
            span: None,
            held: None,
        }
    }

    fn laid(rows: &[(&'static str, usize)]) -> Vec<Laid<&'static str>> {
        rows.iter()
            .map(|(row, loaded)| Laid {
                row: *row,
                loaded: *loaded,
            })
            .collect()
    }

    #[test]
    fn rows_passed_over_move_the_place_and_the_rows_played_next_down_with_them() {
        let arranged = Arranged {
            rows: laid(&[("a", 3), ("b", 0), ("c", 4), ("d", 1), ("e", 2)]),
            row: 3,
            shuffle: true,
            next: Some(Span::between(1, 2)),
        };

        let (kept, passed) = arranged.retained(|_, row| (row != "b").then_some(row));

        assert_eq!(kept.rows, laid(&[("a", 3), ("c", 4), ("d", 1), ("e", 2)]));
        assert_eq!(kept.row, 2);
        assert!(kept.shuffle);
        assert_eq!(kept.next, Some(Span::one(1)));
        assert_eq!(
            passed,
            PassedOver {
                any: true,
                the_place: false
            }
        );
    }

    #[test]
    fn rows_played_next_all_passed_over_leave_none_and_the_place_passed_over_is_told() {
        let arranged = Arranged {
            rows: laid(&[("a", 0), ("b", 1), ("c", 2)]),
            row: 1,
            shuffle: false,
            next: Some(Span::one(1)),
        };

        let (kept, passed) = arranged.retained(|_, row| (row != "b").then_some(row));

        assert_eq!(kept.next, None);
        assert_eq!(kept.row, 1);
        assert!(passed.the_place);
    }

    #[test]
    fn a_resumption_lists_the_rows_in_the_order_they_were_loaded_and_plays_them_as_they_were() {
        let arranged = Arranged {
            rows: vec![
                Laid {
                    row: resumable("c"),
                    loaded: 7,
                },
                Laid {
                    row: resumable("a"),
                    loaded: 2,
                },
                Laid {
                    row: resumable("b"),
                    loaded: 5,
                },
            ],
            row: 1,
            shuffle: true,
            next: Some(Span::one(2)),
        };

        let resumption = arranged.resumption(Frames(441));

        assert_eq!(
            resumption.rows,
            vec![resumable("a"), resumable("b"), resumable("c")]
        );
        assert_eq!(resumption.order, vec![2, 0, 1]);
        assert_eq!(
            resumption
                .in_play_order()
                .into_iter()
                .map(|loaded| resumption.rows[loaded].clone())
                .collect::<Vec<_>>(),
            vec![resumable("c"), resumable("a"), resumable("b")]
        );
        assert_eq!(resumption.row, 1);
        assert_eq!(resumption.at, Frames(441));
        assert!(resumption.shuffle);
        assert_eq!(resumption.next, Some(Span::one(2)));
    }
}
