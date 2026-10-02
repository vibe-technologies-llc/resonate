use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use resonate_analysis::{Watch, excerpt};
use resonate_codec::Sources;
use resonate_core::SourceId;
use resonate_library::{
    Fingerprints, Printed, RecordingAsked, RecordingMatch, Reference, Sounded, Wording,
};

use crate::{Client, Online, Shazam};

const BY_EAR: &str = "by-ear";
const HEARD_FROM_AT_MOST: Duration = Duration::from_secs(30);
const HEARD_FOR: Duration = Duration::from_secs(12);
const LENGTHS_AGREE_WITHIN: Duration = Duration::from_secs(10);

pub struct ByEar {
    source: SourceId,
    shazam: Shazam,
    reference: Online,
    sources: Arc<Sources>,
    asked: Arc<AtomicBool>,
}

impl ByEar {
    pub fn new(client: Arc<Client>, sources: Arc<Sources>, asked: Arc<AtomicBool>) -> Self {
        Self {
            source: SourceId::new(BY_EAR).unwrap_or_else(|_| SourceId::local()),
            shazam: Shazam::new(Arc::clone(&client)),
            reference: Online::with_client(client),
            sources,
            asked,
        }
    }
}

impl Fingerprints for ByEar {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn answers(&self) -> bool {
        self.asked.load(Ordering::Acquire)
    }

    fn recognise(&self, sounded: &Sounded) -> resonate_library::Result<Printed> {
        let from = sounded.length.map_or(HEARD_FROM_AT_MOST, |length| {
            (length / 3).min(HEARD_FROM_AT_MOST)
        });
        let heard = match excerpt(
            &self.sources,
            &sounded.location,
            sounded.span,
            from,
            HEARD_FOR,
            &Watch::default(),
        ) {
            Ok(Some(heard)) => heard,
            Ok(None) => return Ok(Printed::Nothing),
            Err(error) => {
                tracing::debug!(%error, location = %sounded.location, "nothing could be heard of a track");
                return Ok(Printed::Nothing);
            }
        };
        let Some((named, _)) = self.shazam.signed(&heard.mono, heard.rate)? else {
            return Ok(Printed::Nothing);
        };

        if let Some(isrc) = named.isrc.as_ref() {
            let coded: Vec<RecordingMatch> = self
                .reference
                .recordings_of_isrc(isrc)?
                .into_iter()
                .filter(|recording| lengths_agree(recording.length, sounded.length))
                .map(RecordingMatch::from)
                .collect();
            if !coded.is_empty() {
                return Ok(Printed::Recognised(coded));
            }
        }

        Ok(Printed::Recognised(self.reference.find_recording(
            &RecordingAsked {
                title: named.title,
                artist: named.artist,
                artist_mbid: None,
                release: named.album,
                length: sounded.length,
                wording: Wording::Phrase,
            },
        )?))
    }
}

fn lengths_agree(found: Option<Duration>, held: Option<Duration>) -> bool {
    match (found, held) {
        (Some(found), Some(held)) => found.abs_diff(held) <= LENGTHS_AGREE_WITHIN,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_agrees_within_its_allowance_and_one_nobody_knows_agrees_with_anything() {
        let minutes = |seconds: u64| Some(Duration::from_secs(seconds));

        assert!(lengths_agree(minutes(240), minutes(248)));
        assert!(!lengths_agree(minutes(240), minutes(251)));
        assert!(lengths_agree(None, minutes(240)));
        assert!(lengths_agree(minutes(240), None));
    }
}
