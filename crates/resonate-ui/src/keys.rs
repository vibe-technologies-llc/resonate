use std::{borrow::Cow, sync::OnceLock};

use gpui::OperatingSystem;

use crate::words;

macro_rules! secondary {
    ($keys:literal) => {
        crate::keys::Chord::apart(concat!("cmd-", $keys), concat!("ctrl-", $keys))
    };
}

macro_rules! alike {
    ($keys:literal) => {
        crate::keys::Chord::alike($keys)
    };
}

macro_rules! chord {
    (play_pause) => {
        alike!("space")
    };
    (previous) => {
        secondary!("left")
    };
    (next) => {
        secondary!("right")
    };
    (louder) => {
        secondary!("up")
    };
    (quieter) => {
        secondary!("down")
    };
    (mute) => {
        alike!("ctrl-m")
    };
    (seek) => {
        alike!("right")
    };
    (seek_back) => {
        alike!("left")
    };
    (seek_further) => {
        alike!("shift-right")
    };
    (seek_further_back) => {
        alike!("shift-left")
    };
    (queue) => {
        secondary!("u")
    };
    (stop) => {
        secondary!("s")
    };
    (shuffle) => {
        alike!("ctrl-h")
    };
    (repeat) => {
        secondary!("r")
    };
    (listen) => {
        secondary!("l")
    };
    (hold_the_picture) => {
        secondary!("p")
    };
    (quit) => {
        secondary!("q")
    };
    (leave) => {
        alike!("escape")
    };
    (undo) => {
        secondary!("z")
    };
    (paste) => {
        secondary!("v")
    };
    (redo) => {
        secondary!("shift-z")
    };
    (reach_above) => {
        alike!("up")
    };
    (reach_below) => {
        alike!("down")
    };
    (widen_above) => {
        alike!("shift-up")
    };
    (widen_below) => {
        alike!("shift-down")
    };
    (reach_first) => {
        alike!("home")
    };
    (reach_last) => {
        alike!("end")
    };
    (reach_page_above) => {
        alike!("pageup")
    };
    (reach_page_below) => {
        alike!("pagedown")
    };
    (reach_everything) => {
        secondary!("a")
    };
    (choose_reached) => {
        alike!("ctrl-space")
    };
    (hop_above) => {
        secondary!("shift-up")
    };
    (hop_below) => {
        secondary!("shift-down")
    };
    (raise_row) => {
        alike!("alt-up")
    };
    (lower_row) => {
        alike!("alt-down")
    };
    (play_reached) => {
        alike!("enter")
    };
    (drop_reached) => {
        alike!("delete")
    };
    (open_menu) => {
        alike!("shift-f10")
    };
    (band_higher) => {
        alike!("right")
    };
    (band_lower) => {
        alike!("left")
    };
    (band_louder) => {
        alike!("up")
    };
    (band_quieter) => {
        alike!("down")
    };
    (band_narrower) => {
        alike!("shift-up")
    };
    (band_wider) => {
        alike!("shift-down")
    };
    (rail_on) => {
        alike!("right")
    };
    (rail_back) => {
        alike!("left")
    };
    (rail_up) => {
        alike!("up")
    };
    (rail_down) => {
        alike!("down")
    };
    (order_above) => {
        alike!("up")
    };
    (order_below) => {
        alike!("down")
    };
    (order_first) => {
        alike!("home")
    };
    (order_last) => {
        alike!("end")
    };
    (read_the_first_way) => {
        alike!("left")
    };
    (read_the_other_way) => {
        alike!("right")
    };
    (take_the_order) => {
        alike!("enter")
    };
    (next_anywhere) => {
        secondary!("shift-right")
    };
    (previous_anywhere) => {
        secondary!("shift-left")
    };
    (find) => {
        secondary!("f")
    };
    (find_a_setting) => {
        secondary!(",")
    };
    (next_pane) => {
        alike!("ctrl-tab")
    };
    (previous_pane) => {
        alike!("ctrl-shift-tab")
    };
    (redo_also) => {
        secondary!("y")
    };
    (cut) => {
        secondary!("x")
    };
    (copy) => {
        secondary!("c")
    };
    (select_everything) => {
        secondary!("a")
    };
    (word_left) => {
        crate::keys::Chord::apart("alt-left", "ctrl-left")
    };
    (word_right) => {
        crate::keys::Chord::apart("alt-right", "ctrl-right")
    };
    (select_word_left) => {
        crate::keys::Chord::apart("alt-shift-left", "ctrl-shift-left")
    };
    (select_word_right) => {
        crate::keys::Chord::apart("alt-shift-right", "ctrl-shift-right")
    };
    (delete_word_back) => {
        crate::keys::Chord::apart("alt-backspace", "ctrl-backspace")
    };
    (delete_word_forward) => {
        crate::keys::Chord::apart("alt-delete", "ctrl-delete")
    };
}

macro_rules! key {
    ($name:ident) => {{
        static SHOWN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        crate::keys::shown(chord!($name), &SHOWN)
    }};
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chord {
    on_a_mac: &'static str,
    elsewhere: &'static str,
}

impl Chord {
    pub(crate) const fn alike(keys: &'static str) -> Self {
        Self {
            on_a_mac: keys,
            elsewhere: keys,
        }
    }

    pub(crate) const fn apart(on_a_mac: &'static str, elsewhere: &'static str) -> Self {
        Self {
            on_a_mac,
            elsewhere,
        }
    }

    pub(crate) fn on(self, system: OperatingSystem) -> &'static str {
        if system == OperatingSystem::Mac {
            self.on_a_mac
        } else {
            self.elsewhere
        }
    }
}

pub(crate) fn shown(chord: Chord, kept: &'static OnceLock<String>) -> &'static str {
    if cfg!(target_os = "macos") {
        kept.get_or_init(|| in_apples_marks(chord.on_a_mac))
    } else {
        chord.elsewhere
    }
}

const HELD_IN_APPLES_ORDER: [(&str, char); 4] =
    [("ctrl", '⌃'), ("alt", '⌥'), ("shift", '⇧'), ("cmd", '⌘')];

fn in_apples_marks(keys: &str) -> String {
    let (held, key) = keys.rsplit_once('-').unwrap_or(("", keys));

    let mut marks: String = HELD_IN_APPLES_ORDER
        .iter()
        .filter(|(named, _)| held.split('-').any(|part| part == *named))
        .map(|(_, mark)| *mark)
        .collect();
    marks.push_str(&key_as_apple_marks_it(key));
    marks
}

fn key_as_apple_marks_it(key: &str) -> Cow<'static, str> {
    let marked = match key {
        "left" => "←",
        "right" => "→",
        "up" => "↑",
        "down" => "↓",
        "escape" => "⎋",
        "tab" => "⇥",
        "enter" => "↩",
        "backspace" => "⌫",
        "delete" => "⌦",
        "home" => "↖",
        "end" => "↘",
        "pageup" => "⇞",
        "pagedown" => "⇟",
        "space" => words::keys::space(),
        lettered => return Cow::Owned(lettered.to_uppercase()),
    };
    Cow::Borrowed(marked)
}

pub(crate) fn keyed(what: &str, ways: &[&str]) -> String {
    words::keys::keyed(ways_said(ways), what)
}

fn ways_said(ways: &[&str]) -> String {
    match ways {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [one, other] => words::keys::either(*one, *other),
        [first, rest @ ..] => words::keys::among(*first, ways_said(rest)),
    }
}

#[cfg(test)]
mod tests {
    use gpui::{KeyBinding, Keystroke, OperatingSystem};

    use super::{Chord, in_apples_marks, keyed};
    use crate::app;

    const NAMED: [Chord; 54] = [
        chord!(play_pause),
        chord!(previous),
        chord!(next),
        chord!(louder),
        chord!(quieter),
        chord!(mute),
        chord!(seek_further),
        chord!(seek_further_back),
        chord!(queue),
        chord!(stop),
        chord!(shuffle),
        chord!(repeat),
        chord!(listen),
        chord!(hold_the_picture),
        chord!(quit),
        chord!(leave),
        chord!(undo),
        chord!(paste),
        chord!(redo),
        chord!(reach_above),
        chord!(reach_below),
        chord!(widen_above),
        chord!(widen_below),
        chord!(reach_first),
        chord!(reach_last),
        chord!(reach_page_above),
        chord!(reach_page_below),
        chord!(reach_everything),
        chord!(raise_row),
        chord!(lower_row),
        chord!(play_reached),
        chord!(drop_reached),
        chord!(open_menu),
        chord!(band_higher),
        chord!(band_lower),
        chord!(band_louder),
        chord!(band_quieter),
        chord!(band_narrower),
        chord!(band_wider),
        chord!(rail_on),
        chord!(rail_back),
        chord!(rail_up),
        chord!(rail_down),
        chord!(order_above),
        chord!(order_below),
        chord!(order_first),
        chord!(order_last),
        chord!(read_the_first_way),
        chord!(read_the_other_way),
        chord!(take_the_order),
        chord!(find),
        chord!(find_a_setting),
        chord!(cut),
        chord!(copy),
    ];

    const SYSTEMS: [OperatingSystem; 3] = [
        OperatingSystem::Linux,
        OperatingSystem::Windows,
        OperatingSystem::Mac,
    ];

    fn spelt(keys: &'static str, system: OperatingSystem) -> String {
        Keystroke::parse(keys)
            .unwrap_or_else(|_| panic!("{keys} is not a keystroke gpui can read"))
            .unparse(system)
    }

    #[test]
    fn every_key_a_hint_names_is_a_key_something_is_bound_to_on_every_system() {
        for system in SYSTEMS {
            let bound: Vec<String> = app::bindings(system)
                .iter()
                .map(|binding: &KeyBinding| {
                    binding
                        .keystrokes()
                        .iter()
                        .map(|stroke| stroke.unparse(system))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();

            for named in NAMED {
                let named = spelt(named.on(system), system);

                assert!(
                    bound.contains(&named),
                    "a hint names {named} on {system:?}, which nothing is bound to there"
                );
            }
        }
    }

    #[test]
    fn a_key_is_named_away_from_a_mac_as_it_was_before_a_mac_was_built_for() {
        for (chord, named) in [
            (chord!(play_pause), "space"),
            (chord!(previous), "ctrl-left"),
            (chord!(louder), "ctrl-up"),
            (chord!(mute), "ctrl-m"),
            (chord!(shuffle), "ctrl-h"),
            (chord!(queue), "ctrl-u"),
            (chord!(redo), "ctrl-shift-z"),
            (chord!(hop_above), "ctrl-shift-up"),
            (chord!(choose_reached), "ctrl-space"),
            (chord!(open_menu), "shift-f10"),
            (chord!(find), "ctrl-f"),
            (chord!(find_a_setting), "ctrl-,"),
            (chord!(cut), "ctrl-x"),
        ] {
            assert_eq!(chord.on(OperatingSystem::Linux), named);
            assert_eq!(chord.on(OperatingSystem::Windows), named);
        }
    }

    #[test]
    fn a_mac_takes_command_where_the_rest_take_control_and_keeps_the_chords_the_system_owns() {
        assert_eq!(chord!(queue).on(OperatingSystem::Mac), "cmd-u");
        assert_eq!(chord!(redo).on(OperatingSystem::Mac), "cmd-shift-z");
        assert_eq!(chord!(find_a_setting).on(OperatingSystem::Mac), "cmd-,");

        assert_eq!(chord!(mute).on(OperatingSystem::Mac), "ctrl-m");
        assert_eq!(chord!(shuffle).on(OperatingSystem::Mac), "ctrl-h");
        assert_eq!(
            chord!(choose_reached).on(OperatingSystem::Mac),
            "ctrl-space"
        );
        assert_eq!(chord!(next_pane).on(OperatingSystem::Mac), "ctrl-tab");
    }

    #[test]
    fn a_mac_is_shown_its_keys_in_apples_marks_and_apples_order() {
        assert_eq!(in_apples_marks("cmd-u"), "⌘U");
        assert_eq!(in_apples_marks("cmd-shift-z"), "⇧⌘Z");
        assert_eq!(in_apples_marks("ctrl-cmd-f"), "⌃⌘F");
        assert_eq!(in_apples_marks("alt-cmd-h"), "⌥⌘H");
        assert_eq!(in_apples_marks("ctrl-m"), "⌃M");
        assert_eq!(in_apples_marks("alt-up"), "⌥↑");
        assert_eq!(in_apples_marks("cmd-left"), "⌘←");
        assert_eq!(in_apples_marks("cmd-,"), "⌘,");
        assert_eq!(in_apples_marks("shift-f10"), "⇧F10");
        assert_eq!(in_apples_marks("escape"), "⎋");
        assert_eq!(in_apples_marks("space"), "Space");
        assert_eq!(in_apples_marks("ctrl-space"), "⌃Space");
    }

    #[test]
    fn a_hint_said_through_the_catalog_names_what_it_does_and_then_its_keys() {
        assert_eq!(keyed("Play", &["space"]), "Play — space");
        assert_eq!(
            keyed("Louder", &["the wheel", "ctrl-up"]),
            "Louder — the wheel or ctrl-up"
        );
        assert_eq!(
            keyed("Reach further", &["home", "end", "pageup"]),
            "Reach further — home, end or pageup"
        );
    }
}
