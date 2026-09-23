use gpui::{Context, Div, prelude::*};

use crate::{
    ResonateApp, fonts,
    icons::Icon,
    views::{
        kit,
        root::RootView,
        settings::{Group, action, hugging, note, reading},
    },
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const ASK_AGAIN: &str = "Press it again to take every key out of the settings file. The music \
                         folders and the files themselves are left alone.";

const PUT_BACK: &str = "Everything is back to what this build was made with.";

impl RootView {
    pub(super) fn build_group(&mut self, cx: &mut Context<Self>) -> Div {
        let faces = fonts::drawn_in();
        let sinks = self.player.read(cx).sinks().len();
        let reference = self.library.read(cx).has_reference();

        kit::section_body()
            .child(reading("Version", VERSION))
            .child(reading("Body face", faces.body))
            .child(reading("Figures", faces.monospace))
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
                    "MusicBrainz, the Cover Art Archive, Commons and LRCLIB"
                } else {
                    "none this run — the feature is out of this build, or it started offline"
                },
            ))
    }

    pub(super) fn places_group(&mut self, cx: &mut Context<Self>) -> Div {
        let places = cx.global::<ResonateApp>().places.clone();

        kit::section_body()
            .child(reading("Settings", places.config.display().to_string()))
            .child(reading("Catalog", places.library.display().to_string()))
    }

    pub(super) fn everything_group(&mut self, cx: &mut Context<Self>) -> Div {
        let armed = self.resetting_everything;

        kit::section_body()
            .child(hugging(action(
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
            )))
            .when(armed, |body| body.child(note(ASK_AGAIN)))
            .when(self.reset_everything_landed, |body| {
                body.child(note(PUT_BACK))
            })
    }

    fn arm_the_reset(&mut self, cx: &mut Context<Self>) {
        self.resetting_everything = true;
        self.reset_everything_landed = false;
        cx.notify();
    }

    fn reset_everything(&mut self, cx: &mut Context<Self>) {
        for group in Group::ALL {
            self.put_back(group, cx);
        }
        self.resetting_everything = false;
        self.reset_everything_landed = true;
        cx.notify();
    }
}
