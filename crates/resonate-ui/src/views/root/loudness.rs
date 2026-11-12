use gpui::{App, Context};
use resonate_core::{Volume, ZoneId};
use resonate_engine::Command;

use super::{RootView, VOLUME_SETTLE};
use crate::{Setting, SettingChange};

impl RootView {
    pub(crate) fn volume_by(&mut self, delta: f32, cx: &mut Context<Self>) {
        let current = self
            .muted_at(cx)
            .map_or_else(|| self.volume_now(cx), |muted_from| muted_from.get());
        self.set_volume(current + delta, cx);
    }

    pub(super) fn volume_now(&self, cx: &App) -> f32 {
        self.volume_aimed
            .unwrap_or_else(|| self.player.read(cx).state().volume.get())
    }

    pub(crate) fn muted_at(&self, cx: &App) -> Option<Volume> {
        self.muted_from
            .filter(|_| self.player.read(cx).state().volume == Volume::MUTE)
    }

    pub(crate) fn is_muted(&self, cx: &App) -> bool {
        self.muted_at(cx).is_some() || self.device_is_muted(cx)
    }

    pub(super) fn device_is_muted(&self, cx: &App) -> bool {
        self.player
            .read(cx)
            .state()
            .output
            .is_some_and(|output| output.device_muted)
    }

    pub(super) fn device_takes_the_volume(&self, cx: &App) -> bool {
        self.player
            .read(cx)
            .state()
            .output
            .is_some_and(|output| output.device_turned)
    }

    pub(crate) fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        if let Some(muted_from) = self.muted_at(cx) {
            self.set_volume(muted_from.get(), cx);
            return;
        }
        if self.device_takes_the_volume(cx) {
            let muted = self.device_is_muted(cx);
            self.send(Command::SetDeviceMute(!muted), cx);
            cx.notify();
            return;
        }
        let Ok(heard_at) = Volume::new(self.volume_now(cx).clamp(0.0, 1.0)) else {
            return;
        };
        if heard_at == Volume::MUTE {
            return;
        }
        self.volume_aimed = None;
        self.muted_from = Some(heard_at);
        self.send(Command::SetVolume(Volume::MUTE), cx);
        cx.notify();
    }

    pub(crate) fn set_volume(&mut self, position: f32, cx: &mut Context<Self>) {
        let Ok(volume) = Volume::new(position.clamp(0.0, 1.0)) else {
            return;
        };
        self.muted_from = None;
        self.send(Command::SetVolume(volume), cx);
        self.volume_aimed = Some(volume.get());
        self.volume_unkept = Some((self.switcher.selected(), volume));

        self.volume_settled = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VOLUME_SETTLE).await;
            let stored = this.update(cx, |this, cx| this.keep_the_volume(cx));
            let _ = stored;
        });
    }

    pub(crate) fn keep_the_volume(&mut self, cx: &mut Context<Self>) {
        self.volume_aimed = None;
        if let Some(volume) = self.volume_to_keep() {
            self.store_for(ZoneId::FIRST, &Setting::Volume(volume), cx);
        }
    }

    pub(super) fn volume_to_keep(&mut self) -> Option<Volume> {
        let (zone, volume) = self.volume_unkept.take()?;
        zone.is_first().then_some(volume)
    }

    pub(crate) fn keep_the_volume_as_the_window_leaves(&mut self) {
        if let Some(volume) = self.volume_to_keep() {
            self.settings_written
                .change(SettingChange::Store(Setting::Volume(volume)));
        }
    }
}
