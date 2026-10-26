use std::cell::Cell;

use gpui::{
    Role, SharedString, StatefulInteractiveElement, Toggled,
    accesskit::{ActionData, Live},
};

const THE_KEYS_FOLLOW: &str = " — ";

const PERCENT: f64 = 100.0;

thread_local! {
    static LISTENED_TO: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn listened_to(heard: bool) {
    LISTENED_TO.set(heard);
}

pub(crate) fn listening() -> bool {
    LISTENED_TO.get()
}

pub(crate) fn named_by(saying: &str) -> &str {
    saying
        .split_once(THE_KEYS_FOLLOW)
        .map_or(saying, |(named, _)| named)
}

pub(crate) fn song(title: &str, artist: Option<&str>) -> SharedString {
    match artist.filter(|artist| !artist.is_empty()) {
        Some(artist) => SharedString::from(format!("{title}, {artist}")),
        None => SharedString::from(title.to_owned()),
    }
}

pub(crate) fn share_asked(asked: Option<&ActionData>) -> Option<f32> {
    match asked? {
        ActionData::NumericValue(value) => Some((value / PERCENT).clamp(0.0, 1.0) as f32),
        _ => None,
    }
}

const fn toggled(on: bool) -> Toggled {
    if on { Toggled::True } else { Toggled::False }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Placed {
    pub(crate) at: usize,
    pub(crate) of: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Urgency {
    Told,
    Warned,
}

impl Urgency {
    const fn role(self) -> Role {
        match self {
            Self::Told => Role::Status,
            Self::Warned => Role::Alert,
        }
    }

    const fn live(self) -> Live {
        match self {
            Self::Told => Live::Polite,
            Self::Warned => Live::Assertive,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reading {
    pub(crate) share: f32,
    pub(crate) said: SharedString,
}

pub(crate) trait Spoken: StatefulInteractiveElement + Sized {
    fn spoken_as(self, role: Role) -> Self {
        if !listening() {
            return self;
        }
        self.role(role)
    }

    fn keyed(self, keys: Option<SharedString>) -> Self {
        match keys {
            Some(keys) if listening() => self.aria_keyshortcuts(keys),
            Some(_) | None => self,
        }
    }

    fn said_as(self, role: Role, saying: &str) -> Self {
        if !listening() {
            return self;
        }
        let named = named_by(saying);
        let spoken = self
            .role(role)
            .aria_label(SharedString::from(named.to_owned()));
        match named.len() == saying.len() {
            true => spoken,
            false => spoken.aria_description(SharedString::from(saying.to_owned())),
        }
    }

    fn named_as(self, role: Role, label: &str, saying: &str) -> Self {
        if !listening() {
            return self;
        }
        if label.is_empty() {
            return self.said_as(role, saying);
        }
        let spoken = self
            .role(role)
            .aria_label(SharedString::from(label.to_owned()));
        match saying.is_empty() || saying == label {
            true => spoken,
            false => spoken.aria_description(SharedString::from(saying.to_owned())),
        }
    }

    fn thrown(self, on: bool) -> Self {
        if !listening() {
            return self;
        }
        self.aria_toggled(toggled(on))
    }

    fn chosen(self, chosen: bool) -> Self {
        if !listening() {
            return self;
        }
        self.aria_selected(chosen)
    }

    fn greyed_for_a_reader(self) -> Self {
        if !listening() {
            return self;
        }
        self.a11y_synthetic_children(|built| built.parent_node().set_disabled())
    }

    fn listed_as(self, label: impl FnOnce() -> SharedString) -> Self {
        if !listening() {
            return self;
        }
        self.role(Role::ListItem).aria_label(label())
    }

    fn placed(self, placed: Placed) -> Self {
        if !listening() {
            return self;
        }
        self.aria_position_in_set(placed.at + 1)
            .aria_size_of_set(placed.of)
    }

    fn told(self, said: impl Into<SharedString>, urgency: Urgency) -> Self {
        if !listening() {
            return self;
        }
        let live = urgency.live();
        self.role(urgency.role())
            .aria_label(said)
            .a11y_synthetic_children(move |built| built.parent_node().set_live(live))
    }

    fn read_as(self, label: &'static str, reading: Reading) -> Self {
        if !listening() {
            return self;
        }
        self.role(Role::Slider)
            .aria_label(label)
            .aria_min_numeric_value(0.0)
            .aria_max_numeric_value(PERCENT)
            .aria_numeric_value(f64::from(reading.share.clamp(0.0, 1.0)) * PERCENT)
            .aria_value(reading.said)
    }
}

impl<E: StatefulInteractiveElement> Spoken for E {}

#[cfg(test)]
pub(crate) fn node_of(element: &impl gpui::Element) -> Option<gpui::accesskit::Node> {
    let mut node = gpui::accesskit::Node::new(element.a11y_role()?);
    element.write_a11y_info(&mut node);
    Some(node)
}

#[cfg(test)]
mod tests {
    use gpui::{div, prelude::*};

    use super::*;

    #[test]
    fn nothing_is_spoken_while_no_reader_listens() {
        listened_to(false);

        let unheard = div().id("unheard").said_as(Role::Button, "Play — space");

        assert!(node_of(&unheard).is_none());
    }

    #[test]
    fn a_control_is_named_by_its_hint_and_described_by_the_rest() {
        listened_to(true);

        let play = node_of(&div().id("play").said_as(Role::Button, "Play — space"))
            .expect("a heard control is a node");
        let cover = node_of(
            &div()
                .id("cover")
                .said_as(Role::Button, "See the cover full size"),
        )
        .expect("a heard control is a node");

        assert_eq!(play.role(), Role::Button);
        assert_eq!(play.label(), Some("Play"));
        assert_eq!(play.description(), Some("Play — space"));
        assert_eq!(cover.label(), Some("See the cover full size"));
        assert_eq!(
            cover.description(),
            None,
            "a hint saying no more describes nothing"
        );
    }

    #[test]
    fn a_rail_reads_as_a_slider_with_what_it_says() {
        listened_to(true);

        let rail = node_of(&div().id("seek").read_as(
            "Position",
            Reading {
                share: 0.25,
                said: SharedString::new_static("1:00 of 4:00"),
            },
        ))
        .expect("a heard rail is a node");

        assert_eq!(rail.role(), Role::Slider);
        assert_eq!(rail.label(), Some("Position"));
        assert_eq!(rail.numeric_value(), Some(25.0));
        assert_eq!(rail.max_numeric_value(), Some(PERCENT));
        assert_eq!(rail.value(), Some("1:00 of 4:00"));
    }

    #[test]
    fn a_listed_row_says_its_song_where_it_stands_and_whether_it_is_chosen() {
        listened_to(true);

        let row = node_of(
            &div()
                .id("row")
                .listed_as(|| song("Echoes", Some("Pink Floyd")))
                .placed(Placed { at: 2, of: 9 })
                .chosen(true),
        )
        .expect("a heard row is a node");

        assert_eq!(row.role(), Role::ListItem);
        assert_eq!(row.label(), Some("Echoes, Pink Floyd"));
        assert_eq!(row.position_in_set(), Some(3));
        assert_eq!(row.size_of_set(), Some(9));
        assert_eq!(row.is_selected(), Some(true));
    }

    #[test]
    fn a_notice_is_spoken_by_how_urgent_it_is() {
        listened_to(true);

        let trouble = node_of(
            &div()
                .id("trouble")
                .told("Couldn't play that song", Urgency::Warned),
        )
        .expect("a heard notice is a node");
        let done = node_of(&div().id("done").told("Queued", Urgency::Told))
            .expect("a heard notice is a node");

        assert_eq!(trouble.role(), Role::Alert);
        assert_eq!(trouble.label(), Some("Couldn't play that song"));
        assert_eq!(done.role(), Role::Status);
    }

    #[test]
    fn a_hint_names_its_control_by_what_comes_before_the_keys() {
        assert_eq!(named_by("Play — space"), "Play");
        assert_eq!(
            named_by("Mute — click or ctrl-m. Louder — ctrl-up"),
            "Mute",
            "a hint of several sentences is named by its first"
        );
        assert_eq!(
            named_by("See the cover full size"),
            "See the cover full size"
        );
    }

    #[test]
    fn a_reader_sets_a_rail_by_the_percentage_it_reads() {
        assert_eq!(
            share_asked(Some(&ActionData::NumericValue(40.0))),
            Some(0.4)
        );
        assert_eq!(
            share_asked(Some(&ActionData::NumericValue(140.0))),
            Some(1.0)
        );
        assert_eq!(share_asked(None), None);
    }

    #[test]
    fn a_song_is_spoken_as_its_title_then_its_artist() {
        assert_eq!(song("Echoes", Some("Pink Floyd")), "Echoes, Pink Floyd");
        assert_eq!(song("Echoes", Some("")), "Echoes");
        assert_eq!(song("Echoes", None), "Echoes");
    }
}
