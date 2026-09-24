use std::sync::Arc;

use resonate_codec::CoverArt;
use resonate_core::SourceId;
use resonate_library::{
    ArtistMatch, ArtistProfile, ArtistRelease, GroupAsked, GroupMatch, Isrc, Link, Mbid, Recording,
    RecordingAsked, RecordingMatch, Reference, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch,
};

use crate::{Client, Identity, apple, commons, coverart, deezer, musicbrainz, wikidata};

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

    fn find_songs(&self, words: &str) -> resonate_library::Result<Vec<RecordingMatch>> {
        Ok(musicbrainz::find_songs(&self.client, words)?)
    }

    fn release_group(&self, id: &Mbid) -> resonate_library::Result<Option<ReleaseGroup>> {
        Ok(musicbrainz::release_group(&self.client, id)?)
    }

    fn find_release_group(&self, asked: &GroupAsked) -> resonate_library::Result<Vec<GroupMatch>> {
        Ok(musicbrainz::find_release_group(&self.client, asked)?)
    }

    fn artist(&self, id: &Mbid) -> resonate_library::Result<Option<ArtistProfile>> {
        Ok(musicbrainz::artist(&self.client, id)?)
    }

    fn find_artist(&self, name: &str) -> resonate_library::Result<Vec<ArtistMatch>> {
        Ok(musicbrainz::find_artist(&self.client, name)?)
    }

    fn release_groups_of(&self, artist: &Mbid) -> resonate_library::Result<Vec<ArtistRelease>> {
        Ok(musicbrainz::release_groups_of(&self.client, artist)?)
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

    fn portrait(&self, links: &[Link]) -> resonate_library::Result<Option<CoverArt>> {
        for url in resonate_library::portrait_urls(links) {
            if let Some(held) = commons::portrait(&self.client, url)? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::wikidata_urls(links) {
            let Some(scaled) = wikidata::pictured(&self.client, url)? else {
                continue;
            };
            if let Some(held) = commons::fetch(&self.client, &scaled)? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::apple_music_urls(links) {
            if let Some(held) = apple::portrait(&self.client, url)? {
                return Ok(Some(held));
            }
        }

        for url in resonate_library::deezer_urls(links) {
            if let Some(held) = deezer::portrait(&self.client, url)? {
                return Ok(Some(held));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
