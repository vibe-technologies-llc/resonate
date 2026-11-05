use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use parking_lot::Mutex;
use resonate_library::{ProfileLink, SpotifySignInFailure};
use resonate_online::{SPOTIFY_REDIRECT, SpotifyAuthorizing, SpotifyGrant, SpotifyGrants};
use rusqlite::params;

use crate::{Error, MobilePlayer, Result, StoreOp};

#[derive(Default)]
pub(crate) struct KeptByThePhone {
    held: Mutex<Option<SpotifyGrant>>,
    rotated: Mutex<Option<String>>,
}

impl SpotifyGrants for KeptByThePhone {
    fn grant(&self) -> Option<SpotifyGrant> {
        self.held.lock().clone()
    }

    fn renewed(&self, grant: &SpotifyGrant, token: &str) {
        let mut held = self.held.lock();
        if held.as_ref() == Some(grant) {
            *held = Some(SpotifyGrant {
                client_id: grant.client_id.clone(),
                refresh_token: token.to_owned(),
            });
        }
        *self.rotated.lock() = Some(token.to_owned());
    }
}

impl KeptByThePhone {
    fn given(&self, client_id: &str, refresh_token: &str) {
        let (client_id, refresh_token) = (client_id.trim(), refresh_token.trim());
        *self.held.lock() =
            (!client_id.is_empty() && !refresh_token.is_empty()).then(|| SpotifyGrant {
                client_id: client_id.to_owned(),
                refresh_token: refresh_token.to_owned(),
            });
    }
}

#[derive(Default)]
pub(crate) struct SigningIn {
    waiting: Mutex<Option<SpotifyAuthorizing>>,
    cancelled: Arc<AtomicBool>,
}

#[uniffi::export]
pub fn spotify_redirect() -> String {
    SPOTIFY_REDIRECT.to_owned()
}

#[uniffi::export]
pub fn spotify_account_link() -> String {
    ProfileLink::SpotifyAccount.page()
}

#[uniffi::export]
impl MobilePlayer {
    pub fn configure_spotify(&self, client_id: String, refresh_token: String) {
        self.services
            .spotify_grants
            .given(&client_id, &refresh_token);
    }

    pub fn authorize_spotify(&self, client_id: String) -> Result<String> {
        self.require_online()?;
        let client_id = client_id.trim();
        if client_id.is_empty() {
            return Err(Error::NoSpotifyClient);
        }
        self.services
            .spotify_signing
            .cancelled
            .store(false, Ordering::Relaxed);
        *self.services.spotify_signing.waiting.lock() = None;
        let authorizing = self
            .services
            .spotify_sign_in()
            .authorizing(client_id)
            .map_err(signing_failed)?;
        let address = authorizing.authorize_at.clone();
        *self.services.spotify_signing.waiting.lock() = Some(authorizing);

        Ok(address)
    }

    pub fn finish_spotify_authorization(&self) -> Result<Option<String>> {
        self.require_online()?;
        let Some(authorizing) = self.services.spotify_signing.waiting.lock().take() else {
            return Ok(None);
        };
        let cancelled = Arc::clone(&self.services.spotify_signing.cancelled);
        let reaching = Arc::clone(self.services.online.client());

        self.services
            .spotify_sign_in()
            .authorized(&authorizing, &|| {
                cancelled.load(Ordering::Relaxed) || !reaching.is_reaching()
            })
            .map(|signed_in| signed_in.map(|signed_in| signed_in.refresh_token))
            .map_err(signing_failed)
    }

    pub fn cancel_spotify_authorization(&self) {
        self.services
            .spotify_signing
            .cancelled
            .store(true, Ordering::Relaxed);
        *self.services.spotify_signing.waiting.lock() = None;
    }

    pub fn refreshed_spotify_token(&self) -> Option<String> {
        self.services.spotify_grants.rotated.lock().take()
    }

    pub fn sign_out_of_spotify(&self) -> Result<u32> {
        self.services.spotify_grants.given("", "");
        let link = ProfileLink::SpotifyAccount;
        let connection = self.catalog.0.lock();
        let let_go = connection
            .execute(
                "DELETE FROM playlist_links WHERE link IN
                 (SELECT page FROM profile_pages WHERE profile = ?1)",
                params![link.page()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        connection
            .execute(
                "DELETE FROM profile_links WHERE link = ?1",
                params![link.page()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;

        Ok(let_go as u32)
    }
}

fn signing_failed(error: resonate_online::SpotifySignInError) -> Error {
    Error::SpotifySignIn {
        failure: SpotifySignInFailure::from(error),
    }
}

#[cfg(test)]
mod tests {
    use resonate_library::{PlaylistLink, ProfileLink};

    use super::*;
    use crate::playlists::tests::player;

    #[test]
    fn a_grant_needs_both_halves_and_a_rotated_token_is_held_for_the_phone_once() {
        let kept = KeptByThePhone::default();

        kept.given("registered", " ");
        assert_eq!(kept.grant(), None);

        kept.given(" registered ", "first");
        let given = kept.grant().expect("a whole grant");
        assert_eq!(given.client_id, "registered");
        assert_eq!(given.refresh_token, "first");

        kept.renewed(&given, "second");
        assert_eq!(
            kept.grant().map(|grant| grant.refresh_token),
            Some("second".to_owned()),
            "the rotated token is the one used from now on"
        );
        assert_eq!(kept.rotated.lock().take().as_deref(), Some("second"));

        kept.given("", "");
        assert_eq!(kept.grant(), None);
    }

    #[test]
    fn a_rotation_landing_after_a_sign_out_does_not_sign_back_in() {
        let kept = KeptByThePhone::default();
        kept.given("registered", "first");
        let given = kept.grant().expect("a whole grant");

        kept.given("", "");
        kept.renewed(&given, "second");

        assert_eq!(kept.grant(), None);
    }

    #[test]
    fn a_sign_in_with_no_client_or_online_off_asks_nothing() {
        let player = player();

        assert!(matches!(
            player.authorize_spotify("registered".to_owned()),
            Err(Error::OnlineOff)
        ));
        player.configure_online(true);
        assert!(matches!(
            player.authorize_spotify("  ".to_owned()),
            Err(Error::NoSpotifyClient)
        ));
        assert_eq!(
            player
                .finish_spotify_authorization()
                .expect("nothing waits"),
            None
        );
        player.configure_online(false);
    }

    #[test]
    fn an_account_nobody_signed_in_to_says_so_and_asks_nothing() {
        let player = player();
        player.configure_online(true);

        let told = player.follow_profile(spotify_account_link(), true);

        player.configure_online(false);
        assert!(matches!(told, Err(Error::NotSignedInToSpotify)));
    }

    #[test]
    fn the_accounts_link_is_a_profile_a_search_never_offers() {
        assert_eq!(
            ProfileLink::read(&spotify_account_link()),
            Some(ProfileLink::SpotifyAccount)
        );
        assert_eq!(
            crate::profile_links::profile_link_service(spotify_account_link()),
            None
        );
        assert_eq!(spotify_redirect(), "http://127.0.0.1:43879/spotify");
    }

    #[test]
    fn signing_out_lets_the_accounts_playlists_go_and_keeps_their_rows() {
        let player = player();
        let liked = PlaylistLink::SpotifyLiked.page();
        let imported = player
            .import_linked_playlist(liked.clone(), "Liked Songs".to_owned(), Vec::new(), true)
            .expect("a playlist kept in step");
        {
            let connection = player.catalog.0.lock();
            connection
                .execute(
                    "INSERT INTO profile_links (link, name, refreshed) VALUES (?1, 'A Listener', 0)",
                    [spotify_account_link()],
                )
                .expect("followed");
            connection
                .execute(
                    "INSERT INTO profile_pages (profile, page) VALUES (?1, ?2)",
                    [spotify_account_link(), liked],
                )
                .expect("a page");
        }
        player.configure_spotify("registered".to_owned(), "first".to_owned());

        let let_go = player.sign_out_of_spotify().expect("signed out");

        assert_eq!(let_go, 1);
        assert_eq!(player.services.spotify_grants.grant(), None);
        assert!(player.profiles_followed().expect("the list").is_empty());
        assert_eq!(player.playlist_link(imported.id).expect("the link"), None);
        assert!(
            player
                .playlists()
                .expect("the playlists")
                .iter()
                .any(|playlist| playlist.id == imported.id),
            "the playlist stays"
        );
    }
}
