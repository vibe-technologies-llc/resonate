use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    AnyView, App, BoxShadow, Context, Div, IntoElement, MouseButton, Render, SharedString,
    Stateful, StatefulInteractiveElement, Window, div, hsla, point, prelude::*, px, rgb,
};

use crate::{
    icons::{self, Icon},
    theme,
};

const HINT_GROUP: &str = "hint";

static ASKED: AtomicBool = AtomicBool::new(true);

pub(crate) fn asking(can: bool) {
    ASKED.store(can, Ordering::Relaxed);
}

pub(crate) fn asked() -> bool {
    ASKED.load(Ordering::Relaxed)
}

pub(crate) trait Names: StatefulInteractiveElement {
    fn names(self, itself: impl Into<SharedString>) -> Self {
        if !asked() {
            return self;
        }

        self.tooltip(saying(itself.into()))
    }

    fn names_when_raised(self, itself: impl Fn() -> SharedString + 'static) -> Self {
        if !asked() {
            return self;
        }

        self.tooltip(written(itself))
    }
}

impl<E: StatefulInteractiveElement> Names for E {}

pub(crate) struct Hint {
    text: SharedString,
}

impl Render for Hint {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(theme::hint_width()))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(theme::raised()))
            .border_1()
            .border_color(rgb(theme::outline()))
            .shadow(vec![BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.45),
                offset: point(px(0.0), px(4.0)),
                blur_radius: px(16.0),
                spread_radius: px(0.0),
            }])
            .font_family(theme::ui_face())
            .text_size(px(theme::text_sm()))
            .line_height(px(theme::text_sm() * 1.45))
            .text_color(rgb(theme::text()))
            .child(self.text.clone())
    }
}

pub(crate) fn explains(about: &'static str, text: &'static str) -> Stateful<Div> {
    div()
        .id(SharedString::new_static(about))
        .group(HINT_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(theme::hint_control()))
        .rounded_full()
        .cursor_default()
        .hover(|mark| mark.bg(rgb(theme::hover())))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(icons::lit_on_hover(
            icons::icon(Icon::Info, theme::hint_icon(), theme::faint()),
            HINT_GROUP,
        ))
        .names(text)
}

fn saying(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| cx.new(|_| Hint { text: text.clone() }).into()
}

fn written(
    text: impl Fn() -> SharedString + 'static,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| cx.new(|_| Hint { text: text() }).into()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    const SEAM: &str = "hint.rs";
    const ATTACHED: &str = concat!("tooltip", "(");
    const THE_HOVERABLE_ONE: &str = concat!("hoverable_", "tooltip");

    fn sources() -> Vec<(String, String)> {
        let mut read = Vec::new();
        walk(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path(),
            &mut read,
        );
        assert!(
            read.len() > 10,
            "the walk found {} source files, so it is not reading the crate at all",
            read.len()
        );
        read
    }

    fn walk(folder: &Path, read: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(folder).expect("the crate's own source folder is readable") {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                walk(&path, read);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let name = path
                    .file_name()
                    .expect("a file has a name")
                    .to_string_lossy()
                    .into_owned();
                read.push((
                    name,
                    fs::read_to_string(&path).expect("a readable source file"),
                ));
            }
        }
    }

    #[test]
    fn no_control_raises_the_hint_gpui_takes_down_neither_on_a_press_nor_on_a_scroll() {
        for (name, source) in sources() {
            assert!(
                !source.contains(THE_HOVERABLE_ONE),
                "{name} raises a hoverable hint, which gpui takes down neither on a press nor on a \
                 scroll, so it is the one that stays painted after the pointer has gone"
            );
        }
    }

    #[test]
    fn a_control_names_itself_through_this_seam_and_nowhere_else() {
        for (name, source) in sources() {
            if name == SEAM {
                continue;
            }
            assert!(
                !source.contains(ATTACHED),
                "{name} attaches a hint of its own rather than through hint::names, so it is not \
                 held back while an overlay is up or the pointer has left the window"
            );
        }
    }
}
