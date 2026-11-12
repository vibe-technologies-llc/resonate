use gpui::{App, Context, Entity, ScrollStrategy, prelude::*};
use resonate_core::{ArtistsDrawn, ZoneId};
use resonate_engine::{Command, Counting};
use resonate_library::Track;

use super::{Pane, PlayingRow, RootView, somewhere_in, unsaved};
use crate::{
    ConfigKey, Notice, PlayerModel, ResonateApp, Setting, SettingChange,
    toast::{self},
    views::zones::{self},
};

impl RootView {
    pub(crate) fn count_a_play(
        &mut self,
        id: ZoneId,
        player: &Entity<PlayerModel>,
        cx: &mut Context<Self>,
    ) {
        let (state, queue) = {
            let model = player.read(cx);
            (model.state().clone(), model.queue())
        };
        let Some(zone) = self.zones.iter_mut().find(|zone| zone.id == id) else {
            return;
        };
        let Some(counting) = zone.listening.heard(&state, &queue) else {
            return;
        };

        self.library.update(cx, |library, cx| match counting {
            Counting::Counts(played) => library.track_heard(played, cx),
            Counting::Hears(played) => library.track_hearing(played.heard, cx),
            Counting::Settles(played) => library.track_settled(played.heard, cx),
            Counting::Passes(heard) => library.track_passed(heard, cx),
        });
    }

    pub(crate) fn keep_the_queue(
        &mut self,
        id: ZoneId,
        player: &Entity<PlayerModel>,
        cx: &mut Context<Self>,
    ) {
        if !self.library.read(cx).resumes() {
            return;
        }
        let (state, queued) = {
            let model = player.read(cx);
            (model.state().clone(), model.queued())
        };
        let Some(zone) = self.zones.iter_mut().find(|zone| zone.id == id) else {
            return;
        };
        let Some(keep) = zone.resuming.kept(&state, &queued) else {
            return;
        };

        self.library
            .update(cx, |library, cx| library.keep_the_queue(id, keep, cx));
    }

    pub(crate) fn follow_the_playing_row(&mut self, player: &Entity<PlayerModel>, cx: &App) {
        let playing = PlayingRow::of(player.read(cx).state());
        if self
            .following
            .follows(playing, self.pane == Pane::Queue)
            .is_none()
        {
            return;
        }

        self.lift_the_playing_row(cx);
    }

    pub(super) fn lift_the_playing_row(&self, cx: &App) {
        if let Some(top) = self.queue_parts(cx).opens_at() {
            self.queue_rows
                .scroll_to_item_strict(top, ScrollStrategy::Top);
        }
    }

    pub(super) fn noticed(cx: &App) -> bool {
        toast::is_showing(cx)
    }

    pub(crate) fn send(&self, command: Command, cx: &mut Context<Self>) {
        let command = match command {
            Command::Load {
                items,
                start_at,
                autoplay,
            } => Command::Load {
                items: self.with_the_versions_chosen(items, false, cx),
                start_at,
                autoplay,
            },
            Command::Insert { items, at, play } => Command::Insert {
                items: self.with_the_versions_chosen(items, true, cx),
                at,
                play,
            },
            other => other,
        };
        let kind = command.kind();
        if let Err(error) = Self::zones(cx).send(self.switcher.selected(), command) {
            tracing::warn!(%error, command = ?kind, "the engine refused a command");
            toast::tell(Notice::Trouble(zones::refusal(&error, &self.switcher)), cx);
        }
    }

    pub(crate) fn send_by_position(&self, command: Command, cx: &mut Context<Self>) {
        self.player.read(cx).send_by_position(command);
    }

    pub(crate) fn report(&self, notice: Notice, cx: &mut Context<Self>) {
        toast::tell(notice, cx);
    }

    pub(crate) fn plays_in_order(&self, cx: &mut Context<Self>) {
        self.send(Command::SetShuffle(false), cx);
    }

    pub(crate) fn play_shuffled(&mut self, tracks: &[Track], cx: &mut Context<Self>) {
        self.play(tracks, somewhere_in(tracks.len()), cx);
        self.send(Command::SetShuffle(true), cx);
    }

    pub(crate) fn draw_the_artists_as(&mut self, drawn: ArtistsDrawn, cx: &mut Context<Self>) {
        if self.artists_drawn == drawn {
            return;
        }
        self.artists_drawn = drawn;
        cx.update_global::<ResonateApp, _>(|global, _| global.artists_drawn = drawn);
        self.store(&Setting::ArtistsDrawn(drawn), cx);
        cx.notify();
    }

    pub(crate) fn store(&mut self, setting: &Setting, cx: &mut Context<Self>) {
        self.store_for(self.switcher.selected(), setting, cx);
    }

    pub(super) fn store_for(&mut self, zone: ZoneId, setting: &Setting, cx: &mut Context<Self>) {
        if crate::settings::is_the_zones_own(setting.key()) && !zone.is_first() {
            return;
        }
        self.settings_written
            .change(SettingChange::Store(setting.clone()));
        self.write_the_settings(cx);
    }

    pub(crate) fn forget(&mut self, key: ConfigKey, cx: &mut Context<Self>) {
        if crate::settings::is_the_zones_own(key) && !self.switcher.selected().is_first() {
            return;
        }
        self.settings_written.change(SettingChange::Forget(key));
        self.write_the_settings(cx);
    }

    pub(super) fn write_the_settings(&mut self, cx: &mut Context<Self>) {
        let Some(landed) = self.settings_written.next_batch() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let written = landed.await;
            let landed = this.update(cx, |this, cx| {
                this.settings_written.landed();
                if let Some(notice) = unsaved(written) {
                    toast::tell(notice, cx);
                }
                this.write_the_settings(cx);
            });
            let _ = landed;
        })
        .detach();
    }
}
