macro_rules! key {
    (play_pause) => {
        "space"
    };
    (previous) => {
        "ctrl-left"
    };
    (next) => {
        "ctrl-right"
    };
    (louder) => {
        "ctrl-up"
    };
    (quieter) => {
        "ctrl-down"
    };
    (mute) => {
        "ctrl-m"
    };
    (seek_further) => {
        "shift-right"
    };
    (seek_further_back) => {
        "shift-left"
    };
    (queue) => {
        "ctrl-u"
    };
    (stop) => {
        "ctrl-s"
    };
    (shuffle) => {
        "ctrl-h"
    };
    (repeat) => {
        "ctrl-r"
    };
    (listen) => {
        "ctrl-l"
    };
    (quit) => {
        "ctrl-q"
    };
    (leave) => {
        "escape"
    };
    (undo) => {
        "ctrl-z"
    };
    (paste) => {
        "ctrl-v"
    };
    (redo) => {
        "ctrl-shift-z"
    };
    (reach_above) => {
        "up"
    };
    (reach_below) => {
        "down"
    };
    (widen_above) => {
        "shift-up"
    };
    (widen_below) => {
        "shift-down"
    };
    (reach_first) => {
        "home"
    };
    (reach_last) => {
        "end"
    };
    (reach_page_above) => {
        "pageup"
    };
    (reach_page_below) => {
        "pagedown"
    };
    (reach_everything) => {
        "ctrl-a"
    };
    (raise_row) => {
        "alt-up"
    };
    (lower_row) => {
        "alt-down"
    };
    (play_reached) => {
        "enter"
    };
    (drop_reached) => {
        "delete"
    };
    (band_higher) => {
        "right"
    };
    (band_lower) => {
        "left"
    };
    (band_louder) => {
        "up"
    };
    (band_quieter) => {
        "down"
    };
    (band_narrower) => {
        "shift-up"
    };
    (band_wider) => {
        "shift-down"
    };
}

macro_rules! ways {
    ($only:expr) => {
        $only
    };
    ($one:expr, $other:expr) => {
        concat!($one, " or ", $other)
    };
    ($first:expr, $($rest:expr),+) => {
        concat!($first, ", ", ways!($($rest),+))
    };
}

macro_rules! keyed {
    ($what:literal, $($way:expr),+) => {
        concat!($what, " — ", ways!($($way),+))
    };
}

#[cfg(test)]
mod tests {
    use gpui::KeyBinding;

    use crate::app;

    const NAMED: [&str; 37] = [
        key!(play_pause),
        key!(previous),
        key!(next),
        key!(louder),
        key!(quieter),
        key!(mute),
        key!(seek_further),
        key!(seek_further_back),
        key!(queue),
        key!(stop),
        key!(shuffle),
        key!(repeat),
        key!(listen),
        key!(quit),
        key!(leave),
        key!(undo),
        key!(paste),
        key!(redo),
        key!(reach_above),
        key!(reach_below),
        key!(widen_above),
        key!(widen_below),
        key!(reach_first),
        key!(reach_last),
        key!(reach_page_above),
        key!(reach_page_below),
        key!(reach_everything),
        key!(raise_row),
        key!(lower_row),
        key!(play_reached),
        key!(drop_reached),
        key!(band_higher),
        key!(band_lower),
        key!(band_louder),
        key!(band_quieter),
        key!(band_narrower),
        key!(band_wider),
    ];

    #[test]
    fn every_key_a_hint_names_is_a_key_something_is_bound_to() {
        let bound: Vec<String> = app::bindings()
            .iter()
            .map(|binding: &KeyBinding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(|stroke| stroke.unparse())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        for named in NAMED {
            assert!(
                bound.iter().any(|stroke| stroke == named),
                "a hint names {named}, which nothing is bound to"
            );
        }
    }

    #[test]
    fn a_hint_names_what_it_does_and_then_its_keys() {
        assert_eq!(keyed!("Play", key!(play_pause)), "Play — space");
        assert_eq!(
            keyed!("Louder", "the wheel", key!(louder)),
            "Louder — the wheel or ctrl-up"
        );
        assert_eq!(
            keyed!("Reach further", "home", "end", "pageup"),
            "Reach further — home, end or pageup"
        );
    }
}
