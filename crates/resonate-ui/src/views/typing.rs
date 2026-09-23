use std::time::{Duration, Instant};

use resonate_library::folded_letters;

pub(crate) const HELD_FOR: Duration = Duration::from_millis(1_100);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TypeAhead {
    letters: String,
    added_at: Option<Instant>,
    found: bool,
}

impl TypeAhead {
    pub(crate) fn is_live(&self) -> bool {
        !self.letters.is_empty()
    }

    pub(crate) fn took(&mut self, letter: &str, at: Instant) {
        self.letters.push_str(letter);
        self.added_at = Some(at);
    }

    pub(crate) fn dropped_a_letter(&mut self, at: Instant) -> bool {
        if self.letters.pop().is_none() {
            return false;
        }
        self.added_at = Some(at);
        true
    }

    pub(crate) fn cleared(&mut self) -> bool {
        let live = self.is_live();
        self.letters.clear();
        self.added_at = None;
        self.found = false;
        live
    }

    pub(crate) fn ran_out_by(&self, now: Instant) -> bool {
        self.added_at
            .is_none_or(|at| now.saturating_duration_since(at) >= HELD_FOR)
    }

    pub(crate) fn typed(&self) -> Option<&str> {
        (!self.letters.is_empty()).then_some(self.letters.as_str())
    }

    pub(crate) fn landed(&mut self, found: bool) {
        self.found = found;
    }

    pub(crate) const fn found(&self) -> bool {
        self.found
    }
}

pub(crate) fn jumped(rows: &[&str], from: usize, typed: &str) -> Option<usize> {
    let wanted = folded_letters(typed);
    if wanted.is_empty() || rows.is_empty() {
        return None;
    }

    let start = from % rows.len();
    let mut holding = None;
    for step in 0..rows.len() {
        let at = (start + step) % rows.len();
        let row = folded_letters(rows[at]);
        if row.starts_with(&wanted) {
            return Some(at);
        }
        if holding.is_none() && row.contains(&wanted) {
            holding = Some(at);
        }
    }

    holding
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: [&str; 5] = [
        "Astronomy Domine",
        "Careful with That Axe, Eugene",
        "Echoes",
        "A Saucerful of Secrets",
        "Careful Preparation",
    ];

    #[test]
    fn a_name_is_reached_however_its_marks_are_typed() {
        let rows = ["Echoes", "Marcin Przybyłowicz", "Kıskanç"];

        assert_eq!(jumped(&rows, 0, "przybylowicz"), Some(1));
        assert_eq!(jumped(&rows, 0, "marcin przybył"), Some(1));
        assert_eq!(jumped(&rows, 0, "KISKANC"), Some(2));
    }

    #[test]
    fn nothing_typed_jumps_nowhere() {
        assert_eq!(jumped(&ROWS, 0, ""), None);
    }

    #[test]
    fn a_list_holding_the_letters_nowhere_jumps_nowhere() {
        assert_eq!(jumped(&ROWS, 0, "zz"), None);
    }

    #[test]
    fn an_empty_list_jumps_nowhere() {
        assert_eq!(jumped(&[], 0, "a"), None);
    }

    #[test]
    fn the_row_the_cursor_is_on_is_the_first_one_weighed() {
        assert_eq!(jumped(&ROWS, 2, "e"), Some(2));
        assert_eq!(jumped(&ROWS, 1, "careful"), Some(1));
    }

    #[test]
    fn the_search_carries_on_past_the_end_of_the_list_and_round_to_the_start() {
        assert_eq!(jumped(&ROWS, 3, "astronomy"), Some(0));
        assert_eq!(jumped(&ROWS, 4, "echoes"), Some(2));
    }

    #[test]
    fn a_row_that_starts_with_the_letters_is_taken_over_one_that_merely_holds_them() {
        let rows = ["A Saucerful of Secrets", "Saucerful"];

        assert_eq!(jumped(&rows, 0, "sau"), Some(1));
    }

    #[test]
    fn a_row_that_holds_the_letters_is_taken_where_none_starts_with_them() {
        let rows = ["A Saucerful of Secrets", "Echoes"];

        assert_eq!(jumped(&rows, 0, "sau"), Some(0));
    }

    #[test]
    fn the_letters_are_weighed_without_regard_to_case() {
        assert_eq!(jumped(&ROWS, 0, "ECHOES"), Some(2));
        assert_eq!(jumped(&["ÉCHOES"], 0, "é"), Some(0));
    }

    #[test]
    fn a_list_of_one_row_answers_with_it_or_with_nothing() {
        assert_eq!(jumped(&["Echoes"], 0, "ech"), Some(0));
        assert_eq!(jumped(&["Echoes"], 0, "sau"), None);
    }

    #[test]
    fn a_cursor_past_the_end_of_the_list_is_read_round_rather_than_refused() {
        assert_eq!(jumped(&ROWS, 97, "echoes"), Some(2));
    }

    #[test]
    fn what_was_typed_runs_out_once_nothing_has_been_added_for_a_moment() {
        let typed_at = Instant::now();
        let mut ahead = TypeAhead::default();

        assert!(ahead.ran_out_by(typed_at));

        ahead.took("e", typed_at);

        assert!(!ahead.ran_out_by(typed_at + HELD_FOR - Duration::from_millis(1)));
        assert!(ahead.ran_out_by(typed_at + HELD_FOR));
    }

    #[test]
    fn a_letter_added_puts_the_wait_back_to_its_whole_length() {
        let first = Instant::now();
        let mut ahead = TypeAhead::default();
        ahead.took("e", first);
        ahead.took("c", first + HELD_FOR);

        assert!(!ahead.ran_out_by(first + HELD_FOR));
        assert_eq!(ahead.typed(), Some("ec"));
    }

    #[test]
    fn a_backspace_drops_the_last_letter_and_says_whether_there_was_one() {
        let at = Instant::now();
        let mut ahead = TypeAhead::default();
        ahead.took("e", at);
        ahead.took("c", at);

        assert!(ahead.dropped_a_letter(at));
        assert_eq!(ahead.typed(), Some("e"));
        assert!(ahead.dropped_a_letter(at));
        assert_eq!(ahead.typed(), None);
        assert!(!ahead.dropped_a_letter(at));
    }

    #[test]
    fn clearing_says_whether_anything_was_live() {
        let mut ahead = TypeAhead::default();

        assert!(!ahead.cleared());

        ahead.took("e", Instant::now());
        ahead.landed(true);

        assert!(ahead.cleared());
        assert_eq!(ahead.typed(), None);
        assert!(!ahead.found());
    }
}
