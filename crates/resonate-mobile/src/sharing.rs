use std::time::Duration;

use resonate_core::{Isrc, Mbid};
use resonate_library::{Reference, Shared};
use rusqlite::OptionalExtension;

use crate::{Error, MobilePlayer, Result, StoreOp, Track, catalog::Catalog};

const THE_ISRC_OF_A_SONG: &str = "SELECT isrc FROM documents WHERE id = ?1";

const DIGITS_IN_A_YEAR: usize = 4;

#[uniffi::export]
impl MobilePlayer {
    pub fn share_link(&self, id: u64) -> Result<Option<String>> {
        let shared = self.catalog.shared(id)?;
        let shared = if self.services.online.client().is_reaching() {
            shared.streamed_where_asked(&self.services.online as &dyn Reference)
        } else {
            shared
        };
        Ok(shared.written())
    }
}

impl Catalog {
    pub(crate) fn shared(&self, id: u64) -> Result<Shared> {
        let track = self.track(id)?;
        let isrc = self
            .0
            .lock()
            .query_row(THE_ISRC_OF_A_SONG, [id as i64], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .flatten();
        Ok(shared_from(&track, isrc.as_deref()))
    }
}

fn shared_from(track: &Track, isrc: Option<&str>) -> Shared {
    Shared {
        title: track.title.clone(),
        artist: named(&track.artist),
        album: named(&track.album),
        year: year_of(&track.date),
        recording: track
            .recording
            .as_deref()
            .and_then(|text| Mbid::new(text).ok()),
        isrc: isrc.and_then(|text| Isrc::new(text).ok()),
        length: (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
        links: Vec::new(),
    }
}

fn named(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_owned())
}

fn year_of(date: &str) -> Option<i32> {
    date.get(..DIGITS_IN_A_YEAR)
        .and_then(|digits| digits.parse().ok())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rusqlite::params;

    use super::*;

    const RECORDING: &str = "b1a9c0e9-d987-4042-ae91-78d6a3267d69";
    const ISRC: &str = "GBAYE0601498";

    fn song(catalog: &Catalog, recording: Option<&str>, isrc: Option<&str>) -> u64 {
        let track = catalog
            .upsert(Track {
                uri: "content://test/one.flac".into(),
                name: "one.flac".into(),
                title: "One".into(),
                artist: "Artist".into(),
                album: "Début".into(),
                duration_ms: 30000,
                ..Track::default()
            })
            .expect("a song");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET recording = ?1, isrc = ?2, date = '1999-05-01' WHERE id = ?3",
                params![recording, isrc, track.id as i64],
            )
            .expect("an identity");
        track.id
    }

    #[test]
    fn a_song_is_shared_as_the_names_and_codes_its_row_holds() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let id = song(&catalog, Some(RECORDING), Some(ISRC));

        let shared = catalog.shared(id).expect("a song to share");

        assert_eq!(shared.title, "One");
        assert_eq!(shared.artist.as_deref(), Some("Artist"));
        assert_eq!(shared.album.as_deref(), Some("Début"));
        assert_eq!(shared.year, Some(1999));
        assert_eq!(shared.length, Some(Duration::from_secs(30)));
        assert_eq!(shared.isrc.as_ref().map(Isrc::as_str), Some(ISRC));
        assert_eq!(
            shared.written(),
            Some(format!("https://musicbrainz.org/recording/{RECORDING}"))
        );
    }

    #[test]
    fn a_song_with_no_recording_and_no_service_has_no_link_to_share() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let id = song(&catalog, None, None);

        let shared = catalog.shared(id).expect("a song to share");

        assert_eq!(shared.written(), None);
    }

    #[test]
    fn a_stored_code_that_is_not_one_is_left_out_rather_than_refused() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let id = song(&catalog, Some("not-a-recording"), Some("not an isrc"));

        let shared = catalog.shared(id).expect("a song to share");

        assert_eq!(shared.recording, None);
        assert_eq!(shared.isrc, None);
    }

    #[test]
    fn a_song_that_is_not_held_is_unknown() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");

        let refused = catalog.shared(404);

        assert!(matches!(refused, Err(Error::UnknownTrack)));
    }
}
