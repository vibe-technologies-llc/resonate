pub const SONGS_FOUND_AHEAD: usize = 3;

pub fn wanted_ahead(host_next: &[u64]) -> Vec<u64> {
    host_next.iter().take(SONGS_FOUND_AHEAD).copied().collect()
}

#[derive(Clone, Debug, Default)]
pub struct Ahead {
    asked_for: Option<Vec<u64>>,
    finding: bool,
}

impl Ahead {
    pub fn step(&mut self, host_next: &[u64], standing: &[u64]) -> Option<Vec<u64>> {
        let wanted = wanted_ahead(host_next);
        if wanted == standing {
            self.asked_for = None;
            return None;
        }
        if self.finding || self.asked_for.as_ref() == Some(&wanted) {
            return None;
        }

        self.asked_for = Some(wanted.clone());
        self.finding = true;

        Some(wanted)
    }

    pub fn found(&mut self, wanted: &[u64], host_next: &[u64]) -> bool {
        self.finding = false;

        wanted_ahead(host_next) == wanted
    }

    pub fn not_kept(&mut self) {
        self.asked_for = None;
    }

    pub fn not_found(&mut self) {
        self.finding = false;
    }

    pub fn forget(&mut self) {
        self.asked_for = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_the_host_has_not_edited_past_what_stands_asks_for_nothing() {
        let mut ahead = Ahead::default();

        assert_eq!(ahead.step(&[2, 3, 4, 5, 6], &[2, 3, 4]), None);
        assert_eq!(ahead.step(&[2], &[2]), None);
        assert_eq!(ahead.step(&[], &[]), None);
    }

    #[test]
    fn the_host_reaching_past_what_stands_asks_for_the_three_it_now_has_ahead_once() {
        let mut ahead = Ahead::default();

        assert_eq!(ahead.step(&[3, 4, 5, 6], &[3, 4]), Some(vec![3, 4, 5]));
        assert_eq!(
            ahead.step(&[3, 4, 5, 6], &[3, 4]),
            None,
            "a search in flight is not begun again"
        );
    }

    #[test]
    fn a_run_found_for_a_queue_the_host_has_edited_since_is_not_taken() {
        let mut ahead = Ahead::default();
        let wanted = ahead.step(&[3, 4, 5], &[3]).expect("something to find");

        assert!(!ahead.found(&wanted, &[3, 9, 5]));
        assert_eq!(ahead.step(&[3, 9, 5], &[3]), Some(vec![3, 9, 5]));
    }

    #[test]
    fn what_the_guest_does_not_hold_is_not_asked_for_again_until_the_host_moves_on() {
        let mut ahead = Ahead::default();
        let wanted = ahead.step(&[3, 4, 5], &[]).expect("something to find");

        assert!(ahead.found(&wanted, &[3, 4, 5]));
        assert_eq!(ahead.step(&[3, 4, 5], &[3]), None);
        assert_eq!(ahead.step(&[4, 5, 6], &[3]), Some(vec![4, 5, 6]));
    }

    #[test]
    fn a_run_the_seat_could_not_keep_is_asked_for_again() {
        let mut ahead = Ahead::default();
        let wanted = ahead.step(&[3, 4, 5], &[]).expect("something to find");

        assert!(ahead.found(&wanted, &[3, 4, 5]));
        ahead.not_kept();

        assert_eq!(ahead.step(&[3, 4, 5], &[]), Some(vec![3, 4, 5]));
    }

    #[test]
    fn a_search_that_could_not_begin_leaves_the_next_look_free_to_ask() {
        let mut ahead = Ahead::default();
        ahead.step(&[3, 4, 5], &[]).expect("something to find");

        ahead.not_found();
        ahead.forget();

        assert_eq!(ahead.step(&[3, 4, 5], &[]), Some(vec![3, 4, 5]));
    }
}
