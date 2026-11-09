use crate::words;

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
    (seek) => {
        "right"
    };
    (seek_back) => {
        "left"
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
    (hold_the_picture) => {
        "ctrl-p"
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
    (choose_reached) => {
        "ctrl-space"
    };
    (hop_above) => {
        "ctrl-shift-up"
    };
    (hop_below) => {
        "ctrl-shift-down"
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
    (open_menu) => {
        "shift-f10"
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
    (rail_on) => {
        "right"
    };
    (rail_back) => {
        "left"
    };
    (rail_up) => {
        "up"
    };
    (rail_down) => {
        "down"
    };
    (order_above) => {
        "up"
    };
    (order_below) => {
        "down"
    };
    (order_first) => {
        "home"
    };
    (order_last) => {
        "end"
    };
    (read_the_first_way) => {
        "left"
    };
    (read_the_other_way) => {
        "right"
    };
    (take_the_order) => {
        "enter"
    };
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
    use gpui::KeyBinding;

    use super::keyed;
    use crate::app;

    const NAMED: [&str; 50] = [
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
        key!(hold_the_picture),
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
        key!(open_menu),
        key!(band_higher),
        key!(band_lower),
        key!(band_louder),
        key!(band_quieter),
        key!(band_narrower),
        key!(band_wider),
        key!(rail_on),
        key!(rail_back),
        key!(rail_up),
        key!(rail_down),
        key!(order_above),
        key!(order_below),
        key!(order_first),
        key!(order_last),
        key!(read_the_first_way),
        key!(read_the_other_way),
        key!(take_the_order),
    ];

    #[test]
    fn every_key_a_hint_names_is_a_key_something_is_bound_to() {
        let bound: Vec<String> = app::bindings()
            .iter()
            .map(|binding: &KeyBinding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(|stroke| stroke.unparse(gpui::OperatingSystem::Linux))
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
    fn a_hint_said_through_the_catalog_names_what_it_does_and_then_its_keys() {
        assert_eq!(keyed("Play", &[key!(play_pause)]), "Play — space");
        assert_eq!(
            keyed("Louder", &["the wheel", key!(louder)]),
            "Louder — the wheel or ctrl-up"
        );
        assert_eq!(
            keyed("Reach further", &["home", "end", "pageup"]),
            "Reach further — home, end or pageup"
        );
    }
}
