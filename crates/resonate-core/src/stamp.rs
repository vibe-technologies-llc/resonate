use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QueueStamp(u64);

impl QueueStamp {
    pub fn of<Row: Hash>(rows: impl IntoIterator<Item = Row>) -> Self {
        let mut hasher = DefaultHasher::new();
        for row in rows {
            row.hash(&mut hasher);
        }
        Self(hasher.finish())
    }
}

impl Default for QueueStamp {
    fn default() -> Self {
        Self::of([0u64; 0])
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
    }
}
