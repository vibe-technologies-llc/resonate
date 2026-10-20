use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QueueStamp(u64);

impl QueueStamp {
    pub fn of<Row: Hash>(rows: impl IntoIterator<Item = Row>) -> Self {
        let mut stamping = Stamping::default();
        stamping.extend(rows);
        stamping.stamp()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stamping(DefaultHasher);

impl Stamping {
    pub fn row<Row: Hash>(&mut self, row: Row) {
        row.hash(&mut self.0);
    }

    pub fn extend<Row: Hash>(&mut self, rows: impl IntoIterator<Item = Row>) {
        for row in rows {
            self.row(row);
        }
    }

    pub fn stamp(&self) -> QueueStamp {
        QueueStamp(self.0.finish())
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
    fn rows_stamped_as_they_arrive_stamp_as_the_whole_run_does() {
        let mut stamping = Stamping::default();
        stamping.extend([1, 2]);
        let before = stamping.clone();
        stamping.row(3);

        assert_eq!(before.stamp(), QueueStamp::of([1, 2]));
        assert_eq!(stamping.stamp(), QueueStamp::of([1, 2, 3]));
    }

    #[test]
    fn a_row_that_arrived_left_or_changed_places_stamps_otherwise() {
        let held = QueueStamp::of([1, 2, 3]);

        assert_ne!(QueueStamp::of([1, 2, 3, 4]), held);
        assert_ne!(QueueStamp::of([1, 3]), held);
        assert_ne!(QueueStamp::of([3, 2, 1]), held);
    }
}
