use resonate_core::Appearance;

use crate::theme;

const PACKAGED: &str = include_str!("../../../packaging/resonate.svg");
const PACKAGED_STOPS: [&str; 2] = ["stop-color=\"#6ea8fe\"", "stop-color=\"#63d29b\""];
const OPENS: &str = "<svg ";
const OPENS_RECOLOURED: &str = "<svg id=\"resonate-in-the-accent\" ";
const LIFTED_TOWARD_WHITE: f32 = 0.3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppIcon {
    Packaged,
    Recoloured(String),
}

impl AppIcon {
    pub fn of(appearance: Appearance) -> Self {
        let accent = theme::sample(appearance).accent;
        if accent == theme::sample(Appearance::DEFAULT).accent {
            return Self::Packaged;
        }

        let [lit, deep] = PACKAGED_STOPS;
        Self::Recoloured(
            PACKAGED
                .replacen(OPENS, OPENS_RECOLOURED, 1)
                .replacen(lit, &stop(theme::lifted(accent, LIFTED_TOWARD_WHITE)), 1)
                .replacen(deep, &stop(accent), 1),
        )
    }

    pub fn drawn_here(held: &[u8]) -> bool {
        held.starts_with(OPENS_RECOLOURED.as_bytes())
    }
}

fn stop(colour: u32) -> String {
    format!("stop-color=\"#{colour:06x}\"")
}

pub trait Launcher: Send + Sync {
    fn show(&self, icon: AppIcon);
}

#[cfg(test)]
mod tests {
    use resonate_core::{Accent, Theme};

    use super::*;

    fn every_appearance() -> impl Iterator<Item = Appearance> {
        Theme::ALL.into_iter().flat_map(|theme| {
            [None]
                .into_iter()
                .chain(Accent::ALL.map(Some))
                .map(move |accent| Appearance {
                    theme,
                    accent,
                    ..Appearance::DEFAULT
                })
        })
    }

    fn marks(drawing: &str) -> Vec<&str> {
        drawing
            .split("<path d=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect()
    }

    #[test]
    fn the_packaged_icon_opens_and_is_graded_the_way_the_recolouring_expects() {
        assert!(PACKAGED.starts_with(OPENS));
        for packaged in PACKAGED_STOPS {
            assert_eq!(PACKAGED.matches(packaged).count(), 1, "{packaged}");
        }
    }

    #[test]
    fn the_default_appearance_keeps_the_packaged_icon() {
        assert_eq!(AppIcon::of(Appearance::DEFAULT), AppIcon::Packaged);
    }

    #[test]
    fn only_an_appearance_wearing_the_default_accent_keeps_the_packaged_icon() {
        let default = theme::sample(Appearance::DEFAULT).accent;

        for appearance in every_appearance() {
            let worn = theme::sample(appearance).accent;
            match AppIcon::of(appearance) {
                AppIcon::Packaged => assert_eq!(worn, default, "{appearance:?}"),
                AppIcon::Recoloured(drawn) => {
                    assert_ne!(worn, default, "{appearance:?}");
                    assert!(drawn.contains(&stop(worn)), "{appearance:?}");
                }
            }
        }
    }

    #[test]
    fn a_recoloured_icon_is_the_packaged_mark_graded_in_the_accent_alone() {
        let recoloured =
            every_appearance().filter_map(|appearance| match AppIcon::of(appearance) {
                AppIcon::Recoloured(drawn) => Some(drawn),
                AppIcon::Packaged => None,
            });

        for drawn in recoloured {
            assert!(AppIcon::drawn_here(drawn.as_bytes()));
            assert_eq!(marks(&drawn), marks(PACKAGED));
            for packaged in PACKAGED_STOPS {
                assert!(!drawn.contains(packaged), "{drawn}");
            }
        }
    }

    #[test]
    fn the_packaged_icon_is_not_taken_for_one_drawn_here() {
        assert!(!AppIcon::drawn_here(PACKAGED.as_bytes()));
        assert!(!AppIcon::drawn_here(b""));
    }
}
