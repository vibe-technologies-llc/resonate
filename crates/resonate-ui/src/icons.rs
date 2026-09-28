use std::{borrow::Cow, sync::LazyLock};

use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, rgb, svg};

use crate::theme;

macro_rules! icons {
    ($mark:ident => packaged $named:literal, $($variant:ident => $drawn:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum Icon {
            $mark,
            $($variant,)+
        }

        impl Icon {
            const ALL: &'static [Self] = &[Self::$mark, $(Self::$variant,)+];

            const fn path(self) -> &'static str {
                match self {
                    Self::$mark => concat!("icons/", $named, ".svg"),
                    $(Self::$variant => concat!("icons/", $drawn, ".svg"),)+
                }
            }

            fn drawing(self) -> &'static [u8] {
                match self {
                    Self::$mark => THE_MARK.as_bytes(),
                    $(Self::$variant => {
                        include_bytes!(concat!("../assets/icons/", $drawn, ".svg"))
                    })+
                }
            }
        }
    };
}

const PACKAGED: &str = include_str!("../../../packaging/resonate.svg");
const DRAWN_ON: &str = "0 0 24 24";
const WORN_BY_THE_LAUNCHER: &str = "url(#wave)";
const WORN_AS_A_MASK: &str = "#000";

static THE_MARK: LazyLock<String> = LazyLock::new(|| masked(PACKAGED));

fn masked(packaged: &str) -> String {
    let Some(group) = packaged.find("<g ").and_then(|from| {
        let to = from + packaged[from..].find("</g>")? + "</g>".len();
        packaged.get(from..to)
    }) else {
        return String::new();
    };
    let placed = group.find(" transform=\"").and_then(|at| {
        let opened = at + " transform=\"".len();
        Some(at..opened + group[opened..].find('"')? + 1)
    });
    let unplaced = match placed {
        Some(placed) => format!("{}{}", &group[..placed.start], &group[placed.end..]),
        None => group.to_owned(),
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{DRAWN_ON}\">{}</svg>",
        unplaced.replace(WORN_BY_THE_LAUNCHER, WORN_AS_A_MASK)
    )
}

icons! {
    Resonate => packaged "resonate",
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
    Alert => "alert",
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
    Muted => "muted",
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
    WindowMinimise => "window-minimise",
    WindowMaximise => "window-maximise",
    WindowRestore => "window-restore",
    Listen => "listen",
    More => "more",
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
    fn the_app_mark_the_window_draws_is_drawn_from_the_one_the_desktop_entry_installs() {
        let entry = include_str!("../../../packaging/resonate.desktop");
        let named = entry
            .lines()
            .find_map(|line| line.strip_prefix("Icon="))
            .expect("the desktop entry names an icon");
        assert_eq!(Icon::Resonate.path(), format!("icons/{named}.svg"));

        let mark = String::from_utf8_lossy(Icon::Resonate.drawing()).into_owned();
        assert!(mark.starts_with("<svg"), "the mark is not an SVG: {mark}");
        assert!(mark.contains("viewBox=\"0 0 24 24\""));
        assert!(
            !mark.contains("transform"),
            "the launcher's placing rode along"
        );
        assert!(!mark.contains("<rect"), "the launcher's ground rode along");
        assert!(!mark.contains("url("), "the launcher's gradient rode along");
        assert_eq!(
            marks(mark.as_bytes()),
            marks(PACKAGED.as_bytes()),
            "the mark lost a stroke on its way out of the packaged icon"
        );
        assert_eq!(marks(mark.as_bytes()).len(), 5);
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
