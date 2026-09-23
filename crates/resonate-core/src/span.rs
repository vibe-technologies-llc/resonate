use std::{fmt, ops::RangeInclusive};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    first: usize,
    last: usize,
}

impl Span {
    pub const fn one(row: usize) -> Self {
        Self {
            first: row,
            last: row,
        }
    }

    pub const fn between(one: usize, other: usize) -> Self {
        if one <= other {
            Self {
                first: one,
                last: other,
            }
        } else {
            Self {
                first: other,
                last: one,
            }
        }
    }

    pub const fn first(self) -> usize {
        self.first
    }

    pub const fn last(self) -> usize {
        self.last
    }

    pub const fn rows(self) -> usize {
        self.last - self.first + 1
    }

    pub const fn is_one_row(self) -> bool {
        self.first == self.last
    }

    pub const fn holds(self, row: usize) -> bool {
        self.first <= row && row <= self.last
    }

    pub const fn range(self) -> RangeInclusive<usize> {
        self.first..=self.last
    }

    pub const fn within(self, held: usize) -> Option<Self> {
        let Some(last) = held.checked_sub(1) else {
            return None;
        };

        Some(Self {
            first: if self.first < last { self.first } else { last },
            last: if self.last < last { self.last } else { last },
        })
    }

    pub const fn landing(self, to: usize) -> usize {
        if to <= self.first {
            to
        } else if to >= self.last {
            self.first + (to - self.last)
        } else {
            self.first
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_one_row() {
            return write!(f, "{}", self.first);
        }
        write!(f, "{}-{}", self.first, self.last)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::Span;

    fn a_list_and_two_rows() -> impl Strategy<Value = (usize, usize, usize)> {
        (1usize..12).prop_flat_map(|held| (Just(held), 0..held, 0..held))
    }

    fn a_list_a_span_and_a_row() -> impl Strategy<Value = (usize, Span, usize)> {
        (1usize..12)
            .prop_flat_map(|held| (Just(held), 0..held, 0..held, 0..held))
            .prop_map(|(held, one, other, to)| (held, Span::between(one, other), to))
    }

    fn a_row_the_span_holds() -> impl Strategy<Value = (usize, Span, usize)> {
        a_list_a_span_and_a_row()
            .prop_filter("a row the span holds", |(_, rows, to)| rows.holds(*to))
    }

    fn a_row_above_the_span() -> impl Strategy<Value = (usize, Span, usize)> {
        a_list_a_span_and_a_row()
            .prop_filter("a row above the span", |(_, rows, to)| *to < rows.first())
    }

    fn a_row_below_the_span() -> impl Strategy<Value = (usize, Span, usize)> {
        a_list_a_span_and_a_row()
            .prop_filter("a row below the span", |(_, rows, to)| *to > rows.last())
    }

    fn after_moving(held: usize, rows: Span, to: usize) -> Vec<usize> {
        let mut list = (0..held).collect::<Vec<_>>();
        let taken = list.drain(rows.range()).collect::<Vec<_>>();
        let at = rows.landing(to);
        list.splice(at..at, taken);
        list
    }

    #[test]
    fn a_span_of_one_row_is_that_row() {
        let one = Span::one(3);

        assert_eq!(one.first(), 3);
        assert_eq!(one.last(), 3);
        assert_eq!(one.rows(), 1);
        assert!(one.is_one_row());
        assert_eq!(one.to_string(), "3");
    }

    #[test]
    fn a_span_reads_the_same_either_way_round() {
        assert_eq!(Span::between(5, 2), Span::between(2, 5));
        assert_eq!(Span::between(5, 2).first(), 2);
        assert_eq!(Span::between(5, 2).last(), 5);
        assert_eq!(Span::between(5, 2).rows(), 4);
        assert_eq!(Span::between(5, 2).to_string(), "2-5");
    }

    #[test]
    fn a_span_holds_every_row_between_its_ends() {
        let rows = Span::between(2, 4);

        assert!(!rows.holds(1));
        assert!(rows.holds(2));
        assert!(rows.holds(3));
        assert!(rows.holds(4));
        assert!(!rows.holds(5));
        assert_eq!(rows.range().collect::<Vec<_>>(), vec![2, 3, 4]);
    }

    #[test]
    fn a_span_past_the_end_is_pulled_back_to_it() {
        assert_eq!(Span::between(7, 9).within(5), Some(Span::one(4)));
        assert_eq!(Span::between(2, 9).within(5), Some(Span::between(2, 4)));
        assert_eq!(Span::between(0, 3).within(9), Some(Span::between(0, 3)));
        assert_eq!(Span::one(0).within(0), None);
    }

    #[test]
    fn a_span_dropped_above_takes_the_row_it_landed_on() {
        assert_eq!(Span::between(4, 6).landing(1), 1);
        assert_eq!(Span::one(4).landing(1), 1);
    }

    #[test]
    fn a_span_dropped_below_ends_on_the_row_it_landed_on() {
        assert_eq!(Span::between(1, 3).landing(6), 4);
        assert_eq!(Span::one(1).landing(6), 6);
    }

    #[test]
    fn a_span_dropped_on_itself_stays_where_it_is() {
        assert_eq!(Span::between(2, 5).landing(2), 2);
        assert_eq!(Span::between(2, 5).landing(3), 2);
        assert_eq!(Span::between(2, 5).landing(5), 2);
        assert_eq!(Span::between(0, 3).landing(3), 0);
    }

    proptest! {
        #[test]
        fn the_ends_of_a_span_are_sorted_however_they_are_given(
            one in 0usize..1_000,
            other in 0usize..1_000,
        ) {
            let span = Span::between(one, other);

            prop_assert_eq!(span, Span::between(other, one));
            prop_assert!(span.first() <= span.last());
            prop_assert_eq!(span.first(), one.min(other));
            prop_assert_eq!(span.last(), one.max(other));
        }

        #[test]
        fn a_span_holds_every_row_it_steps_through_and_none_beside_them(
            one in 0usize..1_000,
            other in 0usize..1_000,
        ) {
            let span = Span::between(one, other);

            prop_assert_eq!(span.range().count(), span.rows());
            prop_assert!(span.range().all(|row| span.holds(row)));
            prop_assert!(!span.holds(span.last() + 1));
            prop_assert_eq!(span.is_one_row(), span.rows() == 1);

            if let Some(above) = span.first().checked_sub(1) {
                prop_assert!(!span.holds(above));
            }
        }

        #[test]
        fn a_span_pulled_back_to_a_list_holds_only_rows_that_list_has(
            held in 0usize..12,
            one in 0usize..20,
            other in 0usize..20,
        ) {
            let span = Span::between(one, other);

            match span.within(held) {
                None => prop_assert_eq!(held, 0),
                Some(fitted) => {
                    prop_assert!(fitted.last() < held);
                    prop_assert!(fitted.rows() <= span.rows());
                    prop_assert!(fitted.first() <= span.first());

                    if span.last() < held {
                        prop_assert_eq!(fitted, span);
                    }
                }
            }
        }

        #[test]
        fn a_span_dropped_on_a_row_it_already_holds_moves_nothing(
            (held, rows, to) in a_row_the_span_holds(),
        ) {
            prop_assert_eq!(after_moving(held, rows, to), (0..held).collect::<Vec<_>>());
        }

        #[test]
        fn a_span_dropped_above_itself_starts_on_the_row_it_landed_on(
            (held, rows, to) in a_row_above_the_span(),
        ) {
            let moved = after_moving(held, rows, to);

            prop_assert_eq!(moved[to], rows.first());
            prop_assert_eq!(moved[to + rows.rows()], to);
        }

        #[test]
        fn a_span_dropped_below_itself_ends_on_the_row_it_landed_on(
            (held, rows, to) in a_row_below_the_span(),
        ) {
            let moved = after_moving(held, rows, to);

            prop_assert_eq!(moved[to], rows.last());
            prop_assert_eq!(moved[to - rows.rows()], to);
        }

        #[test]
        fn a_span_never_lands_where_it_would_run_past_the_end_of_its_list(
            (held, rows, to) in a_list_a_span_and_a_row(),
        ) {
            prop_assert!(rows.landing(to) + rows.rows() <= held);
        }

        #[test]
        fn a_span_of_one_row_lands_exactly_where_it_is_dropped(
            (held, row, to) in a_list_and_two_rows(),
        ) {
            let one = Span::one(row);

            prop_assert_eq!(one.landing(to), to);
            prop_assert_eq!(after_moving(held, one, to)[to], row);
        }
    }
}
