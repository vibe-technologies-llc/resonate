use std::sync::Arc;

use resonate_codec::CoverArt;
use resonate_core::SourceId;
use resonate_library::{
    AlbumLink, AlbumMatch, AlbumNames, ArtistLink, ArtistMatch, ArtistProfile, Barcode,
    BarcodeMatch, Discography, GroupAsked, GroupMatch, Isrc, Link, LinkNames, LyricText,
    LyricsAsked, Mbid, Recording, RecordingAsked, RecordingMatch, Reference, Release, ReleaseAsked,
    ReleaseGroup, ReleaseMatch, SongLink, SongsAsked, StreamAsked,
};

use crate::{
    Client, Error, Identity, Result, apple, client::passed_over_when_refused, commons, coverart,
    deezer, linked, lrclib, musicbrainz, soundcloud, spotify, wikidata, wikipedia,
};

const MUSICBRAINZ: &str = "musicbrainz";

pub struct Online {
    client: Arc<Client>,
    source: SourceId,
}

impl Online {
    pub fn new(identity: Identity) -> Self {
        Self::with_client(Arc::new(Client::new(identity)))
    }

    pub fn with_client(client: Arc<Client>) -> Self {
        Self {
            client,
            source: SourceId::new(MUSICBRAINZ).unwrap_or_else(|_| SourceId::local()),
        }
    }

    pub fn client(&self) -> &Arc<Client> {
        &self.client
    }
}

impl Reference for Online {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn release(&self, id: &Mbid) -> resonate_library::Result<Option<Release>> {
        Ok(musicbrainz::release(&self.client, id)?)
    }

    fn find_release(&self, asked: &ReleaseAsked) -> resonate_library::Result<Vec<ReleaseMatch>> {
        Ok(musicbrainz::find_release(&self.client, asked)?)
    }

    fn releases_by_barcode(
        &self,
        barcode: &Barcode,
    ) -> resonate_library::Result<Vec<BarcodeMatch>> {
        Ok(musicbrainz::releases_by_barcode(&self.client, barcode)?)
    }

    fn recording(&self, id: &Mbid) -> resonate_library::Result<Option<Recording>> {
        Ok(musicbrainz::recording(&self.client, id)?)
    }

    fn recordings_of_isrc(&self, isrc: &Isrc) -> resonate_library::Result<Vec<Recording>> {
        Ok(musicbrainz::recordings_of_isrc(&self.client, isrc)?)
    }

    fn find_recording(
        &self,
        asked: &RecordingAsked,
    ) -> resonate_library::Result<Vec<RecordingMatch>> {
        Ok(musicbrainz::find_recording(&self.client, asked)?)
    }

    fn find_songs(&self, asked: &SongsAsked) -> resonate_library::Result<Vec<RecordingMatch>> {
        Ok(musicbrainz::find_songs(&self.client, asked)?)
    }

    fn release_group(&self, id: &Mbid) -> resonate_library::Result<Option<ReleaseGroup>> {
        Ok(musicbrainz::release_group(&self.client, id)?)
    }

    fn find_release_group(&self, asked: &GroupAsked) -> resonate_library::Result<Vec<GroupMatch>> {
        Ok(musicbrainz::find_release_group(&self.client, asked)?)
    }

    fn find_albums(&self, words: &str) -> resonate_library::Result<Vec<AlbumMatch>> {
        Ok(musicbrainz::find_albums(&self.client, words)?)
    }

    fn artist(&self, id: &Mbid) -> resonate_library::Result<Option<ArtistProfile>> {
        Ok(musicbrainz::artist(&self.client, id)?)
    }

    fn find_artist(&self, name: &str) -> resonate_library::Result<Vec<ArtistMatch>> {
        Ok(musicbrainz::find_artist(&self.client, name)?)
    }

    fn release_groups_of(&self, artist: &Mbid, from: u32) -> resonate_library::Result<Discography> {
        Ok(musicbrainz::release_groups_of(&self.client, artist, from)?)
    }

    fn releases_of_group(&self, group: &Mbid) -> resonate_library::Result<Vec<Release>> {
        Ok(musicbrainz::releases_of_group(&self.client, group)?)
    }

    fn cover(
        &self,
        release: &Mbid,
        group: Option<&Mbid>,
    ) -> resonate_library::Result<Option<CoverArt>> {
        Ok(coverart::cover(&self.client, release, group)?)
    }

    fn group_cover(&self, group: &Mbid) -> resonate_library::Result<Option<CoverArt>> {
        Ok(coverart::group_cover(&self.client, group)?)
    }

    fn streamed_at(&self, asked: &StreamAsked) -> resonate_library::Result<Option<Link>> {
        Ok(deezer::streamed(&self.client, asked)?)
    }

    fn lyrics(&self, asked: &LyricsAsked) -> resonate_library::Result<Option<LyricText>> {
        Ok(lrclib::told(&self.client, asked)?)
    }

    fn song_linked(&self, link: &SongLink) -> resonate_library::Result<Option<LinkNames>> {
        Ok(linked::named_at(&self.client, link)?)
    }

    fn album_linked(&self, link: &AlbumLink) -> resonate_library::Result<Option<AlbumNames>> {
        Ok(linked::album_named_at(&self.client, link)?)
    }

    fn artist_linked(&self, link: &ArtistLink) -> resonate_library::Result<Option<String>> {
        match link {
            ArtistLink::MusicBrainz(_) => Ok(None),
            ArtistLink::Deezer(artist) => Ok(deezer::artist_named(&self.client, *artist)?),
        }
    }

    fn portrait(&self, links: &[Link]) -> resonate_library::Result<Option<CoverArt>> {
        let mut walk = Walk::default();

        for url in resonate_library::portrait_urls(links) {
            if let Some(held) = walk.tried(commons::portrait(&self.client, url))? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::wikidata_urls(links) {
            let Some(scaled) = walk.tried(wikidata::pictured(&self.client, url))? else {
                continue;
            };
            if let Some(held) = walk.tried(commons::fetch(&self.client, &scaled))? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::wikipedia_urls(links) {
            let Some(scaled) = walk.tried(wikipedia::pictured(&self.client, url))? else {
                continue;
            };
            if let Some(held) = walk.tried(commons::fetch(&self.client, &scaled))? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::apple_music_urls(links) {
            if let Some(held) = walk.tried(apple::portrait(&self.client, url))? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::spotify_urls(links) {
            if let Some(held) = walk.tried(spotify::portrait(&self.client, url))? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::deezer_urls(links) {
            if let Some(held) = walk.tried(deezer::portrait(&self.client, url))? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::soundcloud_urls(links) {
            if let Some(held) = walk.tried(soundcloud::portrait(&self.client, url))? {
                return Ok(Some(held));
            }
        }

        Ok(walk.ended()?)
    }
}

#[derive(Default)]
struct Walk {
    first_failure: Option<Error>,
}

impl Walk {
    fn tried<T>(&mut self, answered: Result<Option<T>>) -> Result<Option<T>> {
        match passed_over_when_refused(answered) {
            Ok(held) => Ok(held),
            Err(unreachable @ Error::Unreachable { .. }) => Err(unreachable),
            Err(failed) => {
                tracing::debug!(%failed, "a portrait source failed; the next is tried");
                self.first_failure.get_or_insert(failed);
                Ok(None)
            }
        }
    }

    fn ended<T>(self) -> Result<Option<T>> {
        self.first_failure.map_or(Ok(None), Err)
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use resonate_library::LookupOp;

    use super::*;
    use crate::Host;

    fn refused(host: Host, status: u16) -> Result<Option<&'static str>> {
        Err(Error::Refused {
            host,
            op: LookupOp::Portrait,
            status,
        })
    }

    #[test]
    fn a_source_that_fails_is_passed_over_and_the_next_is_asked() {
        let mut walk = Walk::default();

        assert_eq!(walk.tried(refused(Host::Commons, 403)).ok(), Some(None));
        assert_eq!(walk.tried(refused(Host::Wikidata, 500)).ok(), Some(None));
        assert_eq!(
            walk.tried::<&str>(Err(Error::Unreadable {
                host: Host::Wikidata,
                op: LookupOp::Portrait,
            }))
            .ok(),
            Some(None)
        );
        assert_eq!(
            walk.tried(Ok(Some("apple's picture"))).ok(),
            Some(Some("apple's picture"))
        );
    }

    #[test]
    fn a_walk_that_found_nothing_answers_its_first_refusal_and_a_miss_under_five_hundred_is_none() {
        let mut walk = Walk::default();
        let _ = walk.tried(refused(Host::Commons, 404));
        let _ = walk.tried(refused(Host::Spotify, 403));
        assert!(matches!(walk.ended::<()>(), Ok(None)));

        let mut walk = Walk::default();
        let _ = walk.tried(refused(Host::Wikidata, 502));
        let _ = walk.tried(refused(Host::Commons, 500));
        let _ = walk.tried::<()>(Ok(None));
        assert!(matches!(
            walk.ended::<()>(),
            Err(Error::Refused {
                host: Host::Wikidata,
                status: 502,
                ..
            })
        ));
    }

    #[test]
    fn a_throttled_host_is_a_failure_to_ask_again_and_not_a_picture_the_artist_lacks() {
        let mut walk = Walk::default();

        assert_eq!(walk.tried(refused(Host::Spotify, 429)).ok(), Some(None));
        assert!(matches!(
            walk.ended::<()>(),
            Err(Error::Refused {
                host: Host::Spotify,
                status: 429,
                ..
            })
        ));
    }

    #[test]
    fn a_source_that_cannot_be_reached_ends_the_walk() {
        let mut walk = Walk::default();

        let answered = walk.tried::<()>(Err(Error::Unreachable {
            host: Host::Wikidata,
            op: LookupOp::Portrait,
            source: io::Error::from(io::ErrorKind::TimedOut),
        }));
        assert!(matches!(answered, Err(Error::Unreachable { .. })));
    }

    #[test]
    fn the_reference_is_named_musicbrainz_and_shares_one_client() {
        let client = Arc::new(Client::new(Identity::of_this_build()));
        let online = Online::with_client(client.clone());

        assert_eq!(online.source().as_str(), MUSICBRAINZ);
        assert!(Arc::ptr_eq(online.client(), &client));
        assert_eq!(
            Online::new(Identity::of_this_build()).source().as_str(),
            MUSICBRAINZ
        );
    }
}
