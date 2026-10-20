use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct QueueStamp(u64);

impl QueueStamp {
    pub fn of<Row: Hash>(rows: impl IntoIterator<Item = Row>) -> Self {
        rows.into_iter()
            .enumerate()
            .fold(Self::default(), |stamp, (place, row)| {
                stamp.with(RowStamp::of(row, place))
            })
    }

    #[must_use]
    pub const fn with(self, row: RowStamp) -> Self {
        Self(self.0.wrapping_add(row.0))
    }

    #[must_use]
    pub const fn without(self, row: RowStamp) -> Self {
        Self(self.0.wrapping_sub(row.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowStamp(u64);

impl RowStamp {
    pub fn of<Row: Hash>(row: Row, place: usize) -> Self {
        let mut hasher = DefaultHasher::new();
        place.hash(&mut hasher);
        row.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_of_nothing_stamps_the_way_an_unstamped_queue_does() {
        assert_eq!(QueueStamp::of([0u64; 0]), QueueStamp::default());
    }

    #[test]
    fn the_same_rows_in_the_same_order_stamp_the_same() {
        assert_eq!(QueueStamp::of([1, 2, 3]), QueueStamp::of([1, 2, 3]));
    }

    #[test]
    fn a_row_that_arrived_left_or_changed_places_stamps_otherwise() {
        let held = QueueStamp::of([1, 2, 3]);

        assert_ne!(QueueStamp::of([1, 2, 3, 4]), held);
        assert_ne!(QueueStamp::of([1, 3]), held);
        assert_ne!(QueueStamp::of([3, 2, 1]), held);
        assert_ne!(QueueStamp::of([1, 1, 3]), held);
    }

    #[test]
    fn a_row_taken_or_given_back_moves_the_stamp_by_that_row_alone() {
        let held = QueueStamp::of([1, 2, 3]);
        let without_the_middle = held.without(RowStamp::of(2, 1));

        assert_eq!(held.without(RowStamp::of(3, 2)), QueueStamp::of([1, 2]));
        assert_eq!(
            QueueStamp::of([1, 2]).with(RowStamp::of(3, 2)),
            QueueStamp::of([1, 2, 3])
        );
        assert_ne!(without_the_middle, held);
        assert_ne!(without_the_middle, QueueStamp::of([1, 3]));
        assert_eq!(without_the_middle.with(RowStamp::of(2, 1)), held);
    }
}
