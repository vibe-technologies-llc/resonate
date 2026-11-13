use gpui::{Context, Window};

use super::RootView;
use crate::views::settings::{Entered, Left};

impl RootView {
    pub(super) fn entered(
        &mut self,
        entered: Entered,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match entered {
            Entered::Contact => self.contact_given(window, cx),
            Entered::AcoustidKey => self.acoustid_key_given(window, cx),
            Entered::ReplicationToken => self.replication_token_given(window, cx),
            Entered::AuddToken => self.audd_token_given(window, cx),
            Entered::ListenbrainzToken => self.listenbrainz_token_given(window, cx),
            Entered::Account(service, account) => {
                self.account_given(service, account, window, cx);
            }
            Entered::Tidal(account) => self.tidal_given(account, window, cx),
            Entered::Lastfm(account) => self.lastfm_given(account, window, cx),
            Entered::SpotifyClient => self.spotify_client_given(window, cx),
            Entered::YoutubeMusic(named) => self.youtube_music_given(named, window, cx),
            Entered::DiscordApp => self.discord_app_given(window, cx),
            Entered::DiscordIcon => self.discord_icon_given(window, cx),
            Entered::SyncServer => self.sync_server_given(window, cx),
            Entered::SyncDevice => self.sync_device_given(window, cx),
            Entered::SharedName => self.shared_name_given(window, cx),
            Entered::SyncCode => self.sync_code_given(window, cx),
            Entered::MinimumLength => self.minimum_length_given(window, cx),
            Entered::Layout => self.layout_given(window, cx),
            Entered::MusicFolder => self.root_typed(window, cx),
            Entered::Filter => self.leave_filter(window, cx),
            Entered::Figure => self.figure_given(window, cx),
            Entered::Looking => self.look_for_a_device(cx),
        }
    }

    pub(super) fn left(&mut self, left: Left, window: &mut Window, cx: &mut Context<Self>) {
        match left {
            Left::Contact => self.contact_left(cx),
            Left::AcoustidKey => self.acoustid_key_left(cx),
            Left::ReplicationToken => self.replication_token_left(cx),
            Left::AuddToken => self.audd_token_left(cx),
            Left::ListenbrainzToken => self.listenbrainz_token_left(cx),
            Left::Layout => self.layout_left(cx),
            Left::MusicFolder => self.root_typed(window, cx),
            Left::Figure => self.figure_left(cx),
        }
    }
}
