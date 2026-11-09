use gpui::{Context, prelude::*};

use crate::{
    ResonateApp, fonts,
    icons::Icon,
    views::{
        kit,
        root::RootView,
        settings::{Armed, Group, Rows, action, reading},
    },
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const ASK_AGAIN: &str = "Press it again to take every key out of the settings file. The music \
                         folders and the files themselves are left alone.";

const PUT_BACK: &str = "Everything is back to what this build was made with.";

const THIS_BUILD: &str = "The faces are the ones found on this machine when the window opened, \
                          and the reference is what the lookups reach this run.";

const PLACES: &str = "The settings file is the one --config named, or the usual one where it \
                      named none; the catalog is the library every pane reads. Neither is \
                      moved from here.";

const START_AGAIN: &str = "Puts every setting on this page and the others back to what this \
                           build starts with. The first press arms it and the second resets; \
                           the music folders, the catalog and the files are left alone.";

impl RootView {
    pub(super) fn build_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let faces = fonts::drawn_in();
        let sinks = self.player.read(cx).sinks().len();
        let reference = self.library.read(cx).has_reference();

        kit::rows()
            .child(reading("Version", VERSION))
            .child(reading("Body face", faces.body))
            .child(reading("Codes", faces.monospace))
            .child(reading(
                "Sinks",
                match sinks {
                    0 => "none in the graph".to_owned(),
                    1 => "1 advertised".to_owned(),
                    many => format!("{many} advertised"),
                },
            ))
            .child(reading(
                "Reference",
                if reference {
                    "MusicBrainz, the Cover Art Archive, Commons, Apple Music, Deezer, LRCLIB and Better Lyrics"
                } else {
                    "none this run — the feature is out of this build, or it started offline"
                },
            ))
            .note(THIS_BUILD)
    }

    pub(super) fn places_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let places = cx.global::<ResonateApp>().places.clone();

        kit::rows()
            .child(reading("Settings", places.config.display().to_string()))
            .child(reading("Catalog", places.library.display().to_string()))
            .note(PLACES)
    }

    pub(super) fn everything_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let armed = self.is_armed(&Armed::ResetEverything);

        kit::rows().child(kit::row(
            "Every setting",
            if armed {
                ASK_AGAIN
            } else if self.reset_everything_landed {
                PUT_BACK
            } else {
                START_AGAIN
            },
            action(
                "reset-everything",
                if armed {
                    "Press again to reset everything"
                } else {
                    "Reset everything"
                },
                Icon::Undo,
                false,
                move |this, _, cx| {
                    if armed {
                        this.reset_everything(cx);
                    } else {
                        this.arm_the_reset(cx);
                    }
                },
                self,
                cx,
            ),
        ))
    }

    fn arm_the_reset(&mut self, cx: &mut Context<Self>) {
        self.arm(Armed::ResetEverything);
        self.reset_everything_landed = false;
        cx.notify();
    }

    fn reset_everything(&mut self, cx: &mut Context<Self>) {
        for group in Group::ALL {
            self.put_back(group, cx);
        }
        self.lower(&Armed::ResetEverything);
        self.reset_everything_landed = true;
        cx.notify();
    }
}
