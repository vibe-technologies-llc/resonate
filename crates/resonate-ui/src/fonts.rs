use std::sync::LazyLock;

use gpui::SharedString;
use parking_lot::RwLock;

const BODY_LADDER: [&str; 8] = [
    "Inter",
    "Adwaita Sans",
    "Cantarell",
    "Noto Sans",
    "Open Sans",
    "Source Sans 3",
    "Liberation Sans",
    "DejaVu Sans",
];

const MONOSPACE_LADDER: [&str; 8] = [
    "JetBrains Mono",
    "Adwaita Mono",
    "Fira Mono",
    "Source Code Pro",
    "Hack",
    "Noto Sans Mono",
    "Liberation Mono",
    "DejaVu Sans Mono",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Faces {
    pub body: SharedString,
    pub monospace: SharedString,
}

impl Faces {
    fn wanted() -> Self {
        Self {
            body: SharedString::new_static(BODY_LADDER[0]),
            monospace: SharedString::new_static(MONOSPACE_LADDER[0]),
        }
    }
}

static DRAWN_IN: LazyLock<RwLock<Faces>> = LazyLock::new(|| RwLock::new(Faces::wanted()));

pub fn settle(installed: &[String]) {
    *DRAWN_IN.write() = Faces {
        body: among(&BODY_LADDER, installed),
        monospace: among(&MONOSPACE_LADDER, installed),
    };
}

pub fn drawn_in() -> Faces {
    DRAWN_IN.read().clone()
}

pub fn body() -> SharedString {
    DRAWN_IN.read().body.clone()
}

pub fn monospace() -> SharedString {
    DRAWN_IN.read().monospace.clone()
}

fn among(ladder: &[&'static str], installed: &[String]) -> SharedString {
    ladder
        .iter()
        .find_map(|wanted| held(wanted, installed))
        .unwrap_or_else(|| SharedString::new_static(ladder[0]))
}

fn held(wanted: &str, installed: &[String]) -> Option<SharedString> {
    installed
        .iter()
        .find(|family| family.eq_ignore_ascii_case(wanted))
        .cloned()
        .map(SharedString::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(families: &[&str]) -> Vec<String> {
        families.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_first_installed_family_on_the_ladder_is_the_one_drawn_in() {
        let found = among(&BODY_LADDER, &installed(&["Cantarell", "Noto Sans"]));

        assert_eq!(found.as_ref(), "Cantarell");
    }

    #[test]
    fn a_family_is_answered_with_the_spelling_the_machine_has_it_under() {
        let found = among(&MONOSPACE_LADDER, &installed(&["jetbrains mono"]));

        assert_eq!(found.as_ref(), "jetbrains mono");
    }

    #[test]
    fn a_machine_with_none_of_them_keeps_the_family_that_was_wanted() {
        let found = among(&BODY_LADDER, &installed(&["Comic Sans MS"]));

        assert_eq!(found.as_ref(), BODY_LADDER[0]);
    }

    #[test]
    fn neither_ladder_offers_a_family_the_other_one_does() {
        for body in BODY_LADDER {
            assert!(
                !MONOSPACE_LADDER.contains(&body),
                "{body} stands on both ladders, so a body face could be taken for a mono one"
            );
        }
    }

    #[test]
    fn nothing_is_settled_until_the_window_says_what_is_installed() {
        assert_eq!(Faces::wanted().body.as_ref(), BODY_LADDER[0]);
        assert_eq!(Faces::wanted().monospace.as_ref(), MONOSPACE_LADDER[0]);
    }
}
