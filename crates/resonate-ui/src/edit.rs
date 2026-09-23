use std::{collections::VecDeque, ops::Range};

use unicode_segmentation::UnicodeSegmentation as _;

const UNDO_DEPTH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    Collapse,
    Extend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Removal {
    Backward,
    Forward,
    WordBackward,
    WordForward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Symbol,
}

impl Class {
    fn of(character: char) -> Self {
        if character.is_whitespace() {
            Self::Space
        } else if character.is_alphanumeric() || character == '_' {
            Self::Word
        } else {
            Self::Symbol
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Span {
    Typing,
    Removing,
    Composing,
    Discrete,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Snapshot {
    content: String,
    selection: Range<usize>,
    reversed: bool,
}

#[derive(Clone, Debug, Default)]
struct History {
    done: VecDeque<Snapshot>,
    undone: Vec<Snapshot>,
    open: Option<(Span, Range<usize>)>,
}

impl History {
    fn resumes(&self, span: Span, selection: &Range<usize>) -> bool {
        match &self.open {
            Some((Span::Composing, _)) => span == Span::Composing,
            Some((open, left)) => *open == span && left == selection,
            None => false,
        }
    }

    fn keep(&mut self, state: Snapshot) {
        if self.done.len() == UNDO_DEPTH {
            self.done.pop_front();
        }
        self.done.push_back(state);
        self.undone.clear();
    }

    fn settle(&mut self, span: Span, selection: Range<usize>) {
        self.open = (span != Span::Discrete).then_some((span, selection));
    }

    fn close(&mut self) {
        self.open = None;
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Edit {
    content: String,
    selection: Range<usize>,
    reversed: bool,
    history: History,
}

impl Edit {
    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    pub(crate) const fn is_reversed(&self) -> bool {
        self.reversed
    }

    pub(crate) const fn cursor(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    pub(crate) fn selected(&self) -> &str {
        self.slice(self.selection.clone())
    }

    pub(crate) fn set_content(&mut self, content: String) {
        self.history.keep(self.snapshot());
        self.content = content;
        let end = self.content.len();
        self.selection = end..end;
        self.reversed = false;
        self.history.close();
    }

    pub(crate) fn replace(&mut self, range: Range<usize>, text: &str) {
        self.edited(Span::Composing, range, text);
    }

    pub(crate) fn insert(&mut self, text: &str) {
        self.edited(Span::Typing, self.selection.clone(), text);
        if text.contains(char::is_whitespace) {
            self.history.close();
        }
    }

    pub(crate) fn paste(&mut self, text: &str) {
        self.edited(Span::Discrete, self.selection.clone(), text);
    }

    pub(crate) fn delete_selection(&mut self) {
        self.edited(Span::Discrete, self.selection.clone(), "");
    }

    pub(crate) fn undo(&mut self) -> bool {
        let Some(previous) = self.history.done.pop_back() else {
            return false;
        };
        let current = self.snapshot();
        self.history.undone.push(current);
        self.history.close();
        self.restore(previous);
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        let Some(next) = self.history.undone.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.history.done.push_back(current);
        self.history.close();
        self.restore(next);
        true
    }

    fn edited(&mut self, span: Span, range: Range<usize>, text: &str) {
        if !self.history.resumes(span, &self.selection) {
            self.history.keep(self.snapshot());
        }

        let range = self.clamped(range);
        let mut next = String::with_capacity(self.content.len() + text.len());
        next.push_str(self.slice(0..range.start));
        next.push_str(text);
        next.push_str(self.slice(range.end..self.content.len()));

        self.content = next;
        let at = range.start.saturating_add(text.len());
        self.selection = at..at;
        self.reversed = false;
        self.history.settle(span, self.selection.clone());
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            content: self.content.clone(),
            selection: self.selection.clone(),
            reversed: self.reversed,
        }
    }

    fn restore(&mut self, state: Snapshot) {
        self.content = state.content;
        self.selection = state.selection;
        self.reversed = state.reversed;
    }

    pub(crate) fn go(&mut self, motion: Motion, anchor: Anchor) {
        let to = match (motion, anchor, self.selection.is_empty()) {
            (Motion::Left, Anchor::Collapse, false) => self.selection.start,
            (Motion::Right, Anchor::Collapse, false) => self.selection.end,
            _ => self.landing(motion),
        };

        match anchor {
            Anchor::Collapse => self.move_to(to),
            Anchor::Extend => self.select_to(to),
        }
    }

    pub(crate) fn remove(&mut self, removal: Removal) {
        if !self.selection.is_empty() {
            self.edited(Span::Discrete, self.selection.clone(), "");
            return;
        }

        let at = self.cursor();
        let range = match removal {
            Removal::Backward => self.previous_grapheme(at)..at,
            Removal::Forward => at..self.next_grapheme(at),
            Removal::WordBackward => self.previous_word(at)..at,
            Removal::WordForward => at..self.next_word(at),
        };
        self.edited(Span::Removing, range, "");
    }

    pub(crate) fn select_all(&mut self) {
        self.selection = 0..self.content.len();
        self.reversed = false;
    }

    pub(crate) fn select(&mut self, range: Range<usize>) {
        self.selection = self.clamped(range);
        self.reversed = false;
    }

    pub(crate) fn move_to(&mut self, offset: usize) {
        let at = self.boundary(offset);
        self.selection = at..at;
        self.reversed = false;
    }

    pub(crate) fn select_to(&mut self, offset: usize) {
        let to = self.boundary(offset);
        if self.reversed {
            self.selection.start = to;
        } else {
            self.selection.end = to;
        }

        if self.selection.end < self.selection.start {
            self.reversed = !self.reversed;
            self.selection = self.selection.end..self.selection.start;
        }
    }

    pub(crate) fn word_at(&self, offset: usize) -> Range<usize> {
        let at = self.boundary(offset);
        let after = self.slice(at..self.content.len());
        let before = self.slice(0..at);

        let Some(class) = after
            .chars()
            .next()
            .or_else(|| before.chars().next_back())
            .map(Class::of)
        else {
            return 0..0;
        };

        let start = before
            .char_indices()
            .rev()
            .take_while(|(_, character)| Class::of(*character) == class)
            .map(|(index, _)| index)
            .last()
            .unwrap_or(at);
        let end = after
            .char_indices()
            .take_while(|(_, character)| Class::of(*character) == class)
            .map(|(index, character)| at + index + character.len_utf8())
            .last()
            .unwrap_or(at);

        start..end
    }

    fn landing(&self, motion: Motion) -> usize {
        match motion {
            Motion::Left => self.previous_grapheme(self.cursor()),
            Motion::Right => self.next_grapheme(self.cursor()),
            Motion::WordLeft => self.previous_word(self.cursor()),
            Motion::WordRight => self.next_word(self.cursor()),
            Motion::Start => 0,
            Motion::End => self.content.len(),
        }
    }

    fn previous_grapheme(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(at, _)| (at < offset).then_some(at))
            .unwrap_or(0)
    }

    fn next_grapheme(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(at, _)| (at > offset).then_some(at))
            .unwrap_or(self.content.len())
    }

    fn previous_word(&self, offset: usize) -> usize {
        let mut walk = self
            .slice(0..offset)
            .char_indices()
            .rev()
            .skip_while(|(_, character)| Class::of(*character) == Class::Space)
            .peekable();

        let Some(class) = walk.peek().map(|(_, character)| Class::of(*character)) else {
            return 0;
        };

        walk.take_while(|(_, character)| Class::of(*character) == class)
            .map(|(index, _)| index)
            .last()
            .unwrap_or(0)
    }

    fn next_word(&self, offset: usize) -> usize {
        let after = self.slice(offset..self.content.len());
        let mut walk = after
            .char_indices()
            .skip_while(|(_, character)| Class::of(*character) == Class::Space)
            .peekable();

        let Some(class) = walk.peek().map(|(_, character)| Class::of(*character)) else {
            return self.content.len();
        };

        walk.take_while(|(_, character)| Class::of(*character) == class)
            .map(|(index, character)| offset + index + character.len_utf8())
            .last()
            .unwrap_or(self.content.len())
    }

    fn clamped(&self, range: Range<usize>) -> Range<usize> {
        let start = self.boundary(range.start);
        let end = self.boundary(range.end).max(start);
        start..end
    }

    fn boundary(&self, offset: usize) -> usize {
        let mut at = offset.min(self.content.len());
        while !self.content.is_char_boundary(at) {
            at = at.saturating_sub(1);
        }
        at
    }

    fn slice(&self, range: Range<usize>) -> &str {
        self.content.get(range).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(content: &str) -> Edit {
        let mut edit = Edit::default();
        edit.set_content(content.to_owned());
        edit
    }

    fn at(content: &str, cursor: usize) -> Edit {
        let mut edit = edit(content);
        edit.move_to(cursor);
        edit
    }

    fn typed(text: &str) -> Edit {
        let mut edit = Edit::default();
        for character in text.chars() {
            edit.insert(character.encode_utf8(&mut [0; 4]));
        }
        edit
    }

    #[test]
    fn fresh_content_leaves_the_caret_at_the_end() {
        let edit = edit("dark side");

        assert_eq!(edit.cursor(), 9);
        assert!(edit.selected().is_empty());
    }

    #[test]
    fn typing_lands_where_the_caret_is_rather_than_at_the_end() {
        let mut edit = at("dark side", 4);
        edit.insert("er");

        assert_eq!(edit.content(), "darker side");
        assert_eq!(edit.cursor(), 6);
    }

    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut edit = edit("dark side");
        edit.select(0..4);
        edit.insert("bright");

        assert_eq!(edit.content(), "bright side");
        assert_eq!(edit.cursor(), 6);
        assert!(edit.selected().is_empty());
    }

    #[test]
    fn the_caret_steps_over_a_grapheme_rather_than_a_byte() {
        let mut edit = at("e\u{301}cho", 0);

        edit.go(Motion::Right, Anchor::Collapse);
        assert_eq!(edit.cursor(), 3);

        edit.go(Motion::Left, Anchor::Collapse);
        assert_eq!(edit.cursor(), 0);
    }

    #[test]
    fn backspace_takes_a_whole_grapheme_with_it() {
        let mut edit = edit("e\u{301}cho");
        edit.remove(Removal::Backward);
        edit.remove(Removal::Backward);
        edit.remove(Removal::Backward);
        edit.remove(Removal::Backward);

        assert_eq!(edit.content(), "");
        assert_eq!(edit.cursor(), 0);
    }

    #[test]
    fn a_collapsing_motion_lands_on_the_edge_of_a_selection_rather_than_past_it() {
        let mut edit = edit("dark side");
        edit.select(2..6);

        edit.go(Motion::Left, Anchor::Collapse);
        assert_eq!(edit.cursor(), 2);

        edit.select(2..6);
        edit.go(Motion::Right, Anchor::Collapse);
        assert_eq!(edit.cursor(), 6);
    }

    #[test]
    fn extending_left_then_right_collapses_the_selection_again() {
        let mut edit = at("dark side", 4);

        edit.go(Motion::Left, Anchor::Extend);
        edit.go(Motion::Left, Anchor::Extend);
        assert_eq!(edit.selected(), "rk");
        assert!(edit.is_reversed());

        edit.go(Motion::Right, Anchor::Extend);
        edit.go(Motion::Right, Anchor::Extend);
        assert!(edit.selected().is_empty());
        assert_eq!(edit.cursor(), 4);
    }

    #[test]
    fn a_word_motion_skips_the_space_and_stops_on_the_word() {
        let mut edit = at("dark side of the moon", 0);

        edit.go(Motion::WordRight, Anchor::Collapse);
        assert_eq!(edit.cursor(), 4);

        edit.go(Motion::WordRight, Anchor::Collapse);
        assert_eq!(edit.cursor(), 9);

        edit.go(Motion::WordLeft, Anchor::Collapse);
        assert_eq!(edit.cursor(), 5);

        edit.go(Motion::WordLeft, Anchor::Collapse);
        assert_eq!(edit.cursor(), 0);
    }

    #[test]
    fn a_word_motion_treats_punctuation_as_its_own_run() {
        let mut edit = at("wish.you-were", 0);

        edit.go(Motion::WordRight, Anchor::Collapse);
        assert_eq!(edit.cursor(), 4);

        edit.go(Motion::WordRight, Anchor::Collapse);
        assert_eq!(edit.cursor(), 5);
    }

    #[test]
    fn a_word_motion_at_the_edges_stops_rather_than_wrapping() {
        let mut edit = at("dark side", 0);
        edit.go(Motion::WordLeft, Anchor::Collapse);
        assert_eq!(edit.cursor(), 0);

        edit.go(Motion::End, Anchor::Collapse);
        edit.go(Motion::WordRight, Anchor::Collapse);
        assert_eq!(edit.cursor(), 9);
    }

    #[test]
    fn deleting_a_word_backward_takes_the_space_before_it_too() {
        let mut edit = edit("dark side of");
        edit.remove(Removal::WordBackward);

        assert_eq!(edit.content(), "dark side ");

        edit.remove(Removal::WordBackward);
        assert_eq!(edit.content(), "dark ");
    }

    #[test]
    fn deleting_a_word_forward_leaves_what_is_behind_the_caret() {
        let mut edit = at("dark side of", 5);
        edit.remove(Removal::WordForward);

        assert_eq!(edit.content(), "dark  of");
        assert_eq!(edit.cursor(), 5);
    }

    #[test]
    fn a_removal_with_a_selection_takes_the_selection_whichever_direction_it_names() {
        for removal in [
            Removal::Backward,
            Removal::Forward,
            Removal::WordBackward,
            Removal::WordForward,
        ] {
            let mut edit = edit("dark side");
            edit.select(0..5);
            edit.remove(removal);

            assert_eq!(
                edit.content(),
                "side",
                "{removal:?} did not take the selection"
            );
        }
    }

    #[test]
    fn a_removal_at_the_edge_of_the_content_changes_nothing() {
        let mut edit = at("dark", 0);
        edit.remove(Removal::Backward);
        assert_eq!(edit.content(), "dark");

        edit.go(Motion::End, Anchor::Collapse);
        edit.remove(Removal::Forward);
        assert_eq!(edit.content(), "dark");
    }

    #[test]
    fn selecting_everything_covers_the_content_whatever_the_caret_was_doing() {
        let mut edit = at("dark side", 3);
        edit.select_all();

        assert_eq!(edit.selected(), "dark side");
        assert_eq!(edit.cursor(), 9);
    }

    #[test]
    fn a_word_under_a_click_covers_the_run_it_sits_in() {
        let edit = edit("dark side of");

        assert_eq!(edit.word_at(0), 0..4);
        assert_eq!(edit.word_at(2), 0..4);
        assert_eq!(edit.word_at(4), 4..5);
        assert_eq!(edit.word_at(7), 5..9);
        assert_eq!(edit.word_at(12), 10..12);
    }

    #[test]
    fn a_word_under_a_click_in_empty_content_is_empty() {
        assert_eq!(Edit::default().word_at(0), 0..0);
    }

    #[test]
    fn an_offset_past_the_content_is_pulled_back_to_its_end() {
        let mut edit = edit("dark");
        edit.move_to(99);
        assert_eq!(edit.cursor(), 4);

        edit.select(2..99);
        assert_eq!(edit.selected(), "rk");
    }

    #[test]
    fn an_offset_inside_a_character_is_pulled_back_to_its_start() {
        let mut edit = edit("\u{e9}cho");
        edit.move_to(1);

        assert_eq!(edit.cursor(), 0);
    }

    #[test]
    fn a_run_of_typing_comes_back_in_one_undo() {
        let mut edit = typed("dark");

        assert!(edit.undo());
        assert_eq!(edit.content(), "");
        assert!(!edit.undo());
    }

    #[test]
    fn a_space_ends_the_run_so_undo_takes_back_a_word() {
        let mut edit = typed("dark side");

        assert!(edit.undo());
        assert_eq!(edit.content(), "dark ");

        assert!(edit.undo());
        assert_eq!(edit.content(), "");
    }

    #[test]
    fn a_run_of_deleting_comes_back_in_one_undo() {
        let mut edit = typed("dark");
        for _ in 0..4 {
            edit.remove(Removal::Backward);
        }
        assert_eq!(edit.content(), "");

        assert!(edit.undo());
        assert_eq!(edit.content(), "dark");
    }

    #[test]
    fn moving_the_caret_between_keystrokes_starts_a_new_step() {
        let mut edit = typed("dark");
        edit.move_to(0);
        edit.insert("x");
        assert_eq!(edit.content(), "xdark");

        assert!(edit.undo());
        assert_eq!(edit.content(), "dark");
    }

    #[test]
    fn undo_restores_what_typing_over_everything_replaced() {
        let mut edit = typed("darkside");
        edit.select_all();
        edit.insert("x");
        assert_eq!(edit.content(), "x");

        assert!(edit.undo());
        assert_eq!(edit.content(), "darkside");
        assert_eq!(edit.selection(), 0..8);
    }

    #[test]
    fn undo_restores_what_clearing_the_field_threw_away() {
        let mut edit = typed("dark side");
        edit.set_content(String::new());
        assert_eq!(edit.content(), "");

        assert!(edit.undo());
        assert_eq!(edit.content(), "dark side");
    }

    #[test]
    fn a_paste_is_its_own_step_rather_than_joining_the_typing() {
        let mut edit = typed("dark");
        edit.paste("side");
        assert_eq!(edit.content(), "darkside");

        assert!(edit.undo());
        assert_eq!(edit.content(), "dark");
    }

    #[test]
    fn redo_puts_back_what_undo_took() {
        let mut edit = typed("dark");
        assert!(edit.undo());

        assert!(edit.redo());
        assert_eq!(edit.content(), "dark");
        assert!(!edit.redo());
    }

    #[test]
    fn an_edit_after_an_undo_forgets_what_was_undone() {
        let mut edit = typed("dark");
        assert!(edit.undo());
        edit.insert("x");

        assert!(!edit.redo());
        assert_eq!(edit.content(), "x");
    }

    #[test]
    fn undo_on_an_untouched_edit_reports_there_was_nothing_to_take_back() {
        assert!(!Edit::default().undo());
        assert!(!Edit::default().redo());
    }

    #[test]
    fn the_history_stops_growing_at_the_depth_it_bounds_itself_to() {
        let mut edit = Edit::default();
        for _ in 0..UNDO_DEPTH * 2 {
            edit.paste("x");
        }

        for step in 0..UNDO_DEPTH {
            assert!(
                edit.undo(),
                "step {step} of the bounded history was missing"
            );
        }
        assert!(!edit.undo());
    }

    #[test]
    fn a_preedit_the_input_method_keeps_revising_is_one_undo_step() {
        let mut edit = typed("dark ");
        for (range, text) in [(5..5, "k"), (5..6, "ka"), (5..7, "\u{304b}")] {
            edit.replace(range, text);
            edit.select(edit.selection());
        }
        assert_eq!(edit.content(), "dark \u{304b}");

        assert!(edit.undo());
        assert_eq!(edit.content(), "dark ");
    }

    #[test]
    fn a_replacement_the_input_method_asks_for_lands_where_it_named() {
        let mut edit = edit("kana");
        edit.replace(0..4, "\u{304b}\u{306a}");

        assert_eq!(edit.content(), "\u{304b}\u{306a}");
        assert_eq!(edit.cursor(), 6);
    }
}
