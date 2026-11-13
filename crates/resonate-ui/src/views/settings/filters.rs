use gpui::{Context, SharedString, Window, div, prelude::*};
use resonate_core::AUDIO_EXTENSIONS;
use resonate_library::{MinimumLength, MusicExtensions, MusicFilters};

use crate::{
    Notice, ResonateApp, Setting,
    icons::Icon,
    theme,
    views::{
        hint::Names as _,
        kit,
        root::RootView,
        settings::{Rows, action, note},
    },
    words,
};

const EXTENSION_CELL: f32 = 84.0;

const LISTS_THE_HIDDEN: &str = "is:hidden";

impl RootView {
    pub(super) fn music_extensions_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let extensions = cx
            .global::<ResonateApp>()
            .library
            .music_filters()
            .extensions;
        let selected = extensions.selected().count();
        let mut choices = div().flex().flex_wrap().gap_x_1().gap_y_0p5();
        for extension in AUDIO_EXTENSIONS {
            let extension = *extension;
            let included = extensions.selected().any(|selected| selected == extension);
            let id = SharedString::from(format!("music-extension-{extension}"));
            choices = choices.child(
                self.in_the_ring(
                    extension,
                    kit::checkbox(id, format!(".{extension}"), included)
                        .w(theme::width(EXTENSION_CELL))
                        .names(if included {
                            words::settings_filters::exclude_extension(extension.to_string())
                        } else {
                            words::settings_filters::include_extension(extension.to_string())
                        }),
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
        kit::rows()
            .child(kit::stacked_row(
                words::settings_filters::included_of(AUDIO_EXTENSIONS.len(), selected),
                words::settings_filters::choose_the_formats_to_include_in(),
                choices,
            ))
            .child(
                kit::action_row()
                    .child(action(
                        "music-extensions-all",
                        words::settings_filters::select_all(),
                        Icon::Check,
                        extensions == MusicExtensions::default(),
                        |this, _, cx| this.set_music_extensions(MusicExtensions::default(), cx),
                        self,
                        cx,
                    ))
                    .child(action(
                        "music-extensions-clear",
                        words::settings_filters::clear_selection(),
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
            .note(words::settings_filters::changes_apply_to_library_listings_immediately())
    }

    pub(super) fn hidden_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let hidden = self.library.read(cx).hidden();
        if hidden.counted == 0 {
            return kit::rows().child(note(words::settings_filters::nothing_hidden()));
        }

        let shown = hidden.tracks.len() as u64;
        let counted = hidden.counted;
        let mut listed = kit::rows().child(note(if shown < counted {
            words::settings_filters::songs_hidden_and_listed(counted, shown)
        } else {
            words::settings_filters::songs_hidden(counted)
        }));
        for track in &hidden.tracks {
            let id = track.id;
            let under = track.artist.clone().unwrap_or_default();
            let control = kit::button(
                SharedString::from(format!("show-hidden-{}", id.get())),
                Some(Icon::Undo),
                words::settings_filters::show_again(),
                words::settings_filters::show_in_the_library_again(&track.title),
                kit::Tone::Ghost,
            );
            listed = listed.child(kit::row(
                track.title.clone(),
                under,
                self.in_the_ring_at(
                    format!("show-hidden-{}", id.get()),
                    control,
                    move |this, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.hide_track(id, false, cx));
                    },
                    cx,
                ),
            ));
        }

        listed.child(
            kit::action_row()
                .child(action(
                    "show-every-hidden",
                    words::settings_filters::show_them_all_again(),
                    Icon::Undo,
                    false,
                    |this, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.show_every_hidden_track(cx));
                    },
                    self,
                    cx,
                ))
                .child(action(
                    "list-the-hidden",
                    words::settings_filters::list_them_in_tracks(),
                    Icon::Search,
                    false,
                    |this, window, cx| {
                        this.search_instead(LISTS_THE_HIDDEN.to_owned(), window, cx);
                        this.choose_pane(crate::views::root::Pane::Tracks, cx);
                    },
                    self,
                    cx,
                )),
        )
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

    pub(super) fn minimum_length_group(&mut self, cx: &mut Context<Self>) -> Rows {
        kit::rows()
            .child(kit::field(
                words::settings_filters::minimum_length_in_seconds(),
                self.text_field(
                    "minimum-length",
                    &self.settings_fields.read(cx).minimum_length.clone(),
                    |this, window, cx| this.leave_minimum_length(window, cx),
                    cx,
                ),
            ))
            .note(words::settings_filters::enter_a_value_from_0_to())
    }

    fn leave_minimum_length(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .settings_fields
            .read(cx)
            .minimum_length
            .read(cx)
            .is_focused(window)
        {
            return;
        }
        let held = cx
            .global::<ResonateApp>()
            .library
            .music_filters()
            .minimum_length;
        if self
            .settings_fields
            .read(cx)
            .minimum_length
            .read(cx)
            .text()
            .trim()
            != held.seconds().to_string()
        {
            self.minimum_length_given(window, cx);
        } else {
            window.focus(&self.focus, cx);
        }
    }

    pub(crate) fn minimum_length_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parsed = self
            .settings_fields
            .read(cx)
            .minimum_length
            .read(cx)
            .text()
            .trim()
            .parse::<u16>()
            .ok()
            .and_then(MinimumLength::new);
        let Some(length) = parsed else {
            self.report(
                Notice::Trouble(
                    words::settings_filters::enter_a_minimum_length_between_0().to_owned(),
                ),
                cx,
            );
            return;
        };
        self.set_minimum_length(length, cx);
        window.focus(&self.focus, cx);
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
        self.settings_fields
            .read(cx)
            .minimum_length
            .clone()
            .update(cx, |field, cx| {
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
    use crate::{
        driven::{Driven, Folder},
        views::settings::Group,
    };

    #[gpui::test]
    fn a_hidden_song_is_listed_in_the_filters_and_shown_again_by_one_press(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        for title in ["Echoes", "Time", "Money"] {
            folder.tone(&format!("{title}.wav"), 1);
        }
        let library = Arc::new(Library::open_in_memory().expect("a catalog"));
        Driven::scanned(&library, &folder);
        let tracks = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks");
        for track in &tracks[..2] {
            library.hide_track(track.id, true).expect("a hide");
        }
        let first = library.hidden_tracks(10).expect("the hidden").tracks[0].id;
        let mut driven = Driven::open(cx, Arc::clone(&library));
        driven.click("tab-settings");
        driven.click("category-Filters");
        driven.until(|root, cx| root.library.read(cx).hidden().counted == 2);

        driven.click(Box::leak(
            format!("show-hidden-{}", first.get()).into_boxed_str(),
        ));
        driven.until(|root, cx| root.library.read(cx).hidden().counted == 1);
        assert!(
            !library
                .track(first)
                .expect("a read")
                .expect("the row")
                .hidden
        );

        driven.click("show-every-hidden");
        driven.until(|root, cx| root.library.read(cx).hidden().counted == 0);
        assert_eq!(library.hidden_tracks(10).expect("the hidden").counted, 0);
    }

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
        driven.focus_on(|root, cx| root.settings_fields.read(cx).minimum_length.clone());
        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            let field = root
                .read(cx)
                .settings_fields
                .read(cx)
                .minimum_length
                .clone()
                .clone();
            field.update(cx, |field, cx| field.hold("600".to_owned(), cx));
        });
        driven.cx.simulate_keystrokes("enter");
        assert_eq!(library.music_filters().minimum_length.seconds(), 600);
        driven.focus_on(|root, cx| root.settings_fields.read(cx).minimum_length.clone());
        driven.cx.update(|_, cx| {
            let field = root
                .read(cx)
                .settings_fields
                .read(cx)
                .minimum_length
                .clone()
                .clone();
            field.update(cx, |field, cx| field.hold("601".to_owned(), cx));
        });
        driven.cx.simulate_keystrokes("enter");
        assert_eq!(library.music_filters().minimum_length.seconds(), 600);
        driven.cx.simulate_keystrokes("escape");
        assert_eq!(
            driven.read(|root, cx| root
                .settings_fields
                .read(cx)
                .minimum_length
                .read(cx)
                .text()
                .to_owned()),
            "600"
        );
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.put_back(Group::MinimumLength, cx)));
        assert_eq!(library.music_filters(), MusicFilters::default());
    }
}
