use gpui::{Context, Div, SharedString, Window, div, prelude::*};
use resonate_core::AUDIO_EXTENSIONS;
use resonate_library::{MinimumLength, MusicExtensions, MusicFilters};

use crate::{
    Notice, ResonateApp, Setting,
    icons::{self, Icon},
    theme,
    views::{
        hint::Names as _,
        kit,
        root::RootView,
        settings::{action, note},
    },
};

impl RootView {
    pub(super) fn music_extensions_group(&mut self, cx: &mut Context<Self>) -> Div {
        let extensions = cx
            .global::<ResonateApp>()
            .library
            .music_filters()
            .extensions;
        let selected = extensions.selected().count();
        let mut choices = div().flex().flex_wrap().gap_2();
        for extension in AUDIO_EXTENSIONS {
            let extension = *extension;
            let included = extensions.selected().any(|selected| selected == extension);
            let id = SharedString::from(format!("music-extension-{extension}"));
            choices = choices.child(
                self.in_the_ring(
                    extension,
                    kit::chip(id, format!(".{extension}"), included)
                        .names(format!(
                            "{} .{extension} {} the library",
                            if included { "Exclude" } else { "Include" },
                            if included { "from" } else { "in" },
                        ))
                        .child(
                            div()
                                .flex_none()
                                .size(theme::width(theme::row_control_icon()))
                                .when(included, |mark| {
                                    mark.child(icons::icon(
                                        Icon::Check,
                                        theme::row_control_icon(),
                                        theme::accent(),
                                    ))
                                }),
                        ),
                    move |this, _, cx| {
                        let extensions = cx
                            .global::<ResonateApp>()
                            .library
                            .music_filters()
                            .extensions;
                        this.set_music_extensions(extensions.toggled(extension), cx);
                    },
                    cx,
                ),
            );
        }
        kit::section_body()
            .child(note(format!(
                "{selected} of {} selected. Choose the formats to include in your library.",
                AUDIO_EXTENSIONS.len(),
            )))
            .child(choices)
            .child(
                kit::actions()
                    .child(action(
                        "music-extensions-all",
                        "Select all",
                        Icon::Check,
                        extensions == MusicExtensions::default(),
                        |this, _, cx| this.set_music_extensions(MusicExtensions::default(), cx),
                        self,
                        cx,
                    ))
                    .child(action(
                        "music-extensions-clear",
                        "Clear selection",
                        Icon::Close,
                        selected == 0,
                        |this, _, cx| {
                            this.set_music_extensions(
                                MusicExtensions::parse(&[])
                                    .expect("an empty extension selection is valid"),
                                cx,
                            );
                        },
                        self,
                        cx,
                    )),
            )
            .child(note("Changes apply to library listings immediately. Excluded files stay on disk and in the catalog."))
    }

    pub(crate) fn set_music_extensions(
        &mut self,
        extensions: MusicExtensions,
        cx: &mut Context<Self>,
    ) {
        let library = &cx.global::<ResonateApp>().library;
        library.filter_music(MusicFilters {
            extensions,
            ..library.music_filters()
        });
        self.library.update(cx, |library, cx| library.reload(cx));
        self.store(&Setting::MusicExtensions(extensions), cx);
        cx.notify();
    }

    pub(super) fn minimum_length_group(&mut self, cx: &mut Context<Self>) -> Div {
        kit::section_body().child(kit::field("Minimum length in seconds", self.key_field("minimum-length", &self.minimum_length,
            |this, window, cx| this.leave_minimum_length(window, cx), cx)))
            .child(note("Enter a value from 0 to 600 seconds (10 minutes), then press Enter to apply. Zero includes every length; a positive minimum excludes tracks whose length is unknown."))
    }

    fn leave_minimum_length(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.minimum_length.read(cx).is_focused(window) {
            return;
        }
        let held = cx
            .global::<ResonateApp>()
            .library
            .music_filters()
            .minimum_length;
        if self.minimum_length.read(cx).text().trim() != held.seconds().to_string() {
            self.minimum_length_given(window, cx);
        } else {
            window.focus(&self.focus);
        }
    }

    pub(crate) fn minimum_length_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parsed = self
            .minimum_length
            .read(cx)
            .text()
            .trim()
            .parse::<u16>()
            .ok()
            .and_then(MinimumLength::new);
        let Some(length) = parsed else {
            self.report(
                Notice::Trouble("Enter a minimum length between 0 and 600 seconds".to_owned()),
                cx,
            );
            return;
        };
        self.set_minimum_length(length, cx);
        window.focus(&self.focus);
    }

    pub(crate) fn set_minimum_length(
        &mut self,
        minimum_length: MinimumLength,
        cx: &mut Context<Self>,
    ) {
        let library = &cx.global::<ResonateApp>().library;
        library.filter_music(MusicFilters {
            minimum_length,
            ..library.music_filters()
        });
        self.minimum_length.update(cx, |field, cx| {
            field.hold(minimum_length.seconds().to_string(), cx)
        });
        self.library.update(cx, |library, cx| library.reload(cx));
        self.store(&Setting::MinimumLength(minimum_length), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::TestAppContext;
    use resonate_library::Library;

    use super::*;
    use crate::{driven::Driven, views::settings::Group};

    #[gpui::test]
    fn filters_can_be_changed_reset_and_cancelled_from_settings(cx: &mut TestAppContext) {
        let library = Arc::new(Library::open_in_memory().expect("a catalog"));
        let mut driven = Driven::open(cx, Arc::clone(&library));
        driven.click("tab-settings");
        driven.click("category-Filters");
        driven.click("music-extension-aac");
        assert!(
            !library
                .music_filters()
                .extensions
                .selected()
                .any(|extension| extension == "aac")
        );
        driven.click("music-extensions-clear");
        assert_eq!(library.music_filters().extensions.selected().count(), 0);
        driven.click("music-extension-mp3");
        driven.click("music-extension-flac");
        driven.click("music-extension-m4a");
        assert_eq!(
            library
                .music_filters()
                .extensions
                .selected()
                .collect::<Vec<_>>(),
            ["flac", "m4a", "mp3"],
        );
        driven.click("music-extensions-all");
        assert_eq!(
            library.music_filters().extensions,
            MusicExtensions::default()
        );
        driven.click("music-extension-aac");
        let root = driven.root.clone();
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.put_back(Group::MusicExtensions, cx)));
        assert_eq!(
            library.music_filters().extensions,
            MusicExtensions::default()
        );
        driven.focus(|root| &root.minimum_length);
        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            let field = root.read(cx).minimum_length.clone();
            field.update(cx, |field, cx| field.hold("600".to_owned(), cx));
        });
        driven.cx.simulate_keystrokes("enter");
        assert_eq!(library.music_filters().minimum_length.seconds(), 600);
        driven.focus(|root| &root.minimum_length);
        driven.cx.update(|_, cx| {
            let field = root.read(cx).minimum_length.clone();
            field.update(cx, |field, cx| field.hold("601".to_owned(), cx));
        });
        driven.cx.simulate_keystrokes("enter");
        assert_eq!(library.music_filters().minimum_length.seconds(), 600);
        driven.cx.simulate_keystrokes("escape");
        assert_eq!(
            driven.read(|root, cx| root.minimum_length.read(cx).text().to_owned()),
            "600"
        );
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.put_back(Group::MinimumLength, cx)));
        assert_eq!(library.music_filters(), MusicFilters::default());
    }
}
