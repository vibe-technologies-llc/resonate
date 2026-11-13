use std::sync::atomic::Ordering;

use gpui::{Context, prelude::*};
use resonate_core::{Switch, Switches};

use crate::{ResonateApp, views::root::RootView};

impl RootView {
    pub(super) fn switches_worn(&self, cx: &Context<Self>) -> Switches {
        Switch::ALL
            .into_iter()
            .fold(Switches::UNSAID, |worn, switch| {
                worn.with(switch, self.wears(switch, cx))
            })
    }

    fn wears(&self, switch: Switch, cx: &Context<Self>) -> bool {
        let library = self.library.read(cx);
        let app = cx.global::<ResonateApp>();

        match switch {
            Switch::Online => library.is_online(),
            Switch::EnrichAfterScan => library.enriches_after_scan(),
            Switch::Study => library.studies(),
            Switch::FetchLyrics => library.fetches_lyrics(),
            Switch::Resume => library.resumes(),
            Switch::EnrichOnMetered => app.online.enrich_on_metered,
            Switch::MusicbrainzMirror => app.online.musicbrainz_mirror,
            Switch::StallDownloadsOnMetered => app.online.stall_downloads_on_metered,
            Switch::HostedServices => app.online.hosted_services,
            Switch::YoutubeMusic => app.online.youtube_music,
            Switch::YoutubeMusicKeepOrder => app.online.youtube_music_keep_order,
            Switch::IdentifyBySound => app.by_sound.load(Ordering::Acquire),
            Switch::LyricsLanguageFromLocale => app.lyrics_by_the_locale.load(Ordering::Acquire),
            Switch::Notify => app.notify.load(Ordering::Acquire),
            Switch::CheckForUpdates => app.check_for_updates,
            Switch::Renderers => app
                .renderers
                .as_ref()
                .is_some_and(|renderers| renderers.looking()),
            Switch::Equaliser => self.player.read(cx).output_settings().equaliser.enabled,
            Switch::Sync => app.synchronised.enabled,
            Switch::SyncNearbyOnly => app.synchronised.nearby_only,
            Switch::ScrollVolume => app.scroll_volume,
            Switch::MouseNavigation => app.mouse_navigation,
            Switch::FileDropped => app.file_dropped,
            Switch::RememberTab => app.remember_tab,
            Switch::RememberWindowSize => app.remember_window_size,
            Switch::RememberSettingsCategory => app.remember_settings_category,
        }
    }

    pub(super) fn put_the_switch_back(&mut self, switch: Switch, cx: &mut Context<Self>) {
        let on = switch.on_unless_said();

        match switch {
            Switch::Online => self.set_online(on, cx),
            Switch::EnrichAfterScan => self.set_after_scan(on, cx),
            Switch::Study => self.set_studies(on, cx),
            Switch::FetchLyrics => self.set_lyrics(on, cx),
            Switch::Resume => self.set_resume(on, cx),
            Switch::EnrichOnMetered => self.set_enrich_on_metered(on, cx),
            Switch::MusicbrainzMirror => self.set_musicbrainz_mirror(on, cx),
            Switch::StallDownloadsOnMetered => self.set_stall_downloads_on_metered(on, cx),
            Switch::HostedServices => self.set_hosted_services(on, cx),
            Switch::YoutubeMusic => self.turn_youtube_music(on, cx),
            Switch::YoutubeMusicKeepOrder => self.keep_youtubes_order(on, cx),
            Switch::IdentifyBySound => self.set_by_sound(on, cx),
            Switch::LyricsLanguageFromLocale => self.set_lyrics_by_the_locale(on, cx),
            Switch::Notify => self.set_notify(on, cx),
            Switch::CheckForUpdates => self.check_for_updates(on, cx),
            Switch::Renderers => self.look_for_renderers(on, cx),
            Switch::Equaliser => self.switch_the_equaliser(on, cx),
            Switch::Sync => self.set_sync(on, cx),
            Switch::SyncNearbyOnly => self.set_sync_nearby_only(on, cx),
            Switch::ScrollVolume => self.wheel_the_volume(on, cx),
            Switch::MouseNavigation => self.navigate_with_mouse_buttons(on, cx),
            Switch::FileDropped => self.file_what_is_dropped(on, cx),
            Switch::RememberTab => {
                self.remembered.tab = on;
                cx.update_global::<ResonateApp, _>(|global, _| global.remember_tab = on);
            }
            Switch::RememberWindowSize => {
                self.remembered.window_size = on;
                cx.update_global::<ResonateApp, _>(|global, _| global.remember_window_size = on);
            }
            Switch::RememberSettingsCategory => {
                self.remembered.settings_category = on;
                cx.update_global::<ResonateApp, _>(|global, _| {
                    global.remember_settings_category = on;
                });
            }
        }
    }
}
