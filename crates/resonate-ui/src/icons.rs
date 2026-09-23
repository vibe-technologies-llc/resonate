use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, rgb, svg};

use crate::theme;

macro_rules! icons {
    ($($variant:ident => $drawn:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum Icon {
            $($variant,)+
        }

        impl Icon {
            const ALL: &'static [Self] = &[$(Self::$variant,)+];

            const fn path(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("icons/", $drawn, ".svg"),)+
                }
            }

            const fn drawing(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => {
                        include_bytes!(concat!("../assets/icons/", $drawn, ".svg"))
                    })+
                }
            }
        }
    };
}

icons! {
    Resonate => "resonate",
    Play => "play",
    Pause => "pause",
    Previous => "previous",
    Next => "next",
    Shuffle => "shuffle",
    Repeat => "repeat",
    RepeatTrack => "repeat-track",
    Sleep => "sleep",
    Equaliser => "equaliser",
    Albums => "albums",
    Artists => "artists",
    Tracks => "tracks",
    Statistics => "statistics",
    Suggestions => "suggestions",
    Queue => "queue",
    QueueNext => "queue-next",
    QueueLast => "queue-last",
    Playlists => "playlists",
    Lyrics => "lyrics",
    Inspector => "inspector",
    Visualiser => "visualiser",
    Analysis => "analysis",
    Settings => "settings",
    Search => "search",
    Rename => "rename",
    Info => "info",
    Check => "check",
    Folder => "folder",
    Plus => "plus",
    Discard => "discard",
    Stop => "stop",
    Import => "import",
    Export => "export",
    Tidy => "tidy",
    Fold => "fold",
    Sort => "sort",
    Undo => "undo",
    Redo => "redo",
    Volume => "volume",
    Back => "back",
    Close => "close",
    ChevronUp => "chevron-up",
    ChevronDown => "chevron-down",
    Disc => "disc",
    Missing => "missing",
    Link => "link",
    Appearance => "appearance",
    Globe => "globe",
    Want => "want",
    Wanted => "wanted",
    Favourite => "favourite",
    Favourited => "favourited",
    Pin => "pin",
    Pinned => "pinned",
    Share => "share",
    WindowClose => "window-close",
    Listen => "listen",
}

pub(crate) struct Embedded;

impl AssetSource for Embedded {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Icon::ALL
            .iter()
            .copied()
            .find(|icon| icon.path() == path)
            .map(|icon| Cow::Borrowed(icon.drawing())))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Icon::ALL
            .iter()
            .copied()
            .map(|icon| SharedString::new_static(icon.path()))
            .collect())
    }
}

pub(crate) fn icon(icon: Icon, size: f32, colour: u32) -> Svg {
    svg()
        .flex_none()
        .size(theme::width(size))
        .text_color(rgb(colour))
        .path(icon.path())
}

pub(crate) fn lit_on_hover(icon: Svg, group: &'static str) -> Svg {
    icon.group_hover(group, |icon| icon.text_color(rgb(theme::text())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_embedded_under_the_path_it_is_drawn_from() {
        for icon in Icon::ALL.iter().copied() {
            let loaded = Embedded
                .load(icon.path())
                .expect("the embedded source cannot fail")
                .unwrap_or_else(|| panic!("{icon:?} is not embedded under {}", icon.path()));

            assert!(loaded.starts_with(b"<svg"), "{icon:?} is not an SVG");
        }
    }

    #[test]
    fn the_app_mark_the_window_draws_is_the_one_the_desktop_entry_installs() {
        let entry = include_str!("../../../packaging/resonate.desktop");
        let named = entry
            .lines()
            .find_map(|line| line.strip_prefix("Icon="))
            .expect("the desktop entry names an icon");

        assert_eq!(Icon::Resonate.path(), format!("icons/{named}.svg"));
        assert_eq!(
            marks(Icon::Resonate.drawing()),
            marks(include_bytes!("../../../packaging/resonate.svg")),
            "the mark the window draws and the mark the launcher shows have drifted apart"
        );
    }

    fn marks(drawing: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(drawing)
            .split("<path d=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next().map(str::to_owned))
            .collect()
    }

    #[test]
    fn a_path_no_icon_claims_loads_nothing() {
        let loaded = Embedded
            .load("icons/nothing.svg")
            .expect("the embedded source cannot fail");

        assert!(loaded.is_none());
    }
}
