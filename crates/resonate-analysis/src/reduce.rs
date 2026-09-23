use std::mem;

pub(crate) trait Absorbs: Clone {
    fn absorb(&mut self, later: &Self);
}

#[derive(Clone, Debug)]
pub(crate) struct Doubling<T> {
    held: Vec<T>,
    cap: usize,
    per_column: u64,
    filling: Option<T>,
    filled: u64,
}

impl<T: Absorbs> Doubling<T> {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            held: Vec::with_capacity(cap),
            cap: cap.max(2),
            per_column: 1,
            filling: None,
            filled: 0,
        }
    }

    pub(crate) fn push(&mut self, unit: T) {
        match &mut self.filling {
            Some(column) => column.absorb(&unit),
            None => self.filling = Some(unit),
        }
        self.filled += 1;
        if self.filled < self.per_column {
            return;
        }

        self.held.extend(self.filling.take());
        self.filled = 0;
        if self.held.len() >= self.cap {
            self.halve();
        }
    }

    fn halve(&mut self) {
        let held = mem::take(&mut self.held);
        self.held = held
            .chunks(2)
            .map(|pair| {
                let mut first = pair[0].clone();
                if let [_, second] = pair {
                    first.absorb(second);
                }
                first
            })
            .collect();
        self.per_column *= 2;
    }

    pub(crate) const fn per_column(&self) -> u64 {
        self.per_column
    }

    pub(crate) fn finished(mut self) -> Vec<T> {
        self.held.extend(self.filling.take());
        self.held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Counted(u64);

    impl Absorbs for Counted {
        fn absorb(&mut self, later: &Self) {
            self.0 += later.0;
        }
    }

    #[test]
    fn a_stream_of_any_length_is_held_in_no_more_than_the_cap_and_nothing_is_lost() {
        for units in [0_u64, 1, 7, 64, 65, 1_000, 10_007, 250_000] {
            let mut columns = Doubling::new(64);
            for _ in 0..units {
                columns.push(Counted(1));
            }
            let per_column = columns.per_column();
            let held = columns.finished();

            assert!(held.len() <= 64, "{units} units held in {}", held.len());
            assert!(per_column.is_power_of_two());
            assert_eq!(held.iter().map(|column| column.0).sum::<u64>(), units);
            if let Some((last, whole)) = held.split_last() {
                assert!(whole.iter().all(|column| column.0 == per_column));
                assert!(last.0 <= per_column);
            }
        }
    }
}
