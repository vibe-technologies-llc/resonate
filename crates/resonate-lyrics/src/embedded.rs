use resonate_core::SourceId;

use crate::{LyricProvider, Lyrics, Result, Wanted, read_lyrics};

const EMBEDDED: &str = "embedded";

pub struct Embedded {
    source: SourceId,
}

impl Default for Embedded {
    fn default() -> Self {
        Self {
            source: SourceId::new(EMBEDDED).unwrap_or_else(|_| SourceId::local()),
        }
    }
}

impl LyricProvider for Embedded {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn lyrics(&self, wanted: &Wanted) -> Result<Option<Lyrics>> {
        let Some(carried) = wanted.carried.as_deref() else {
            return Ok(None);
        };

        Ok(read_lyrics(self.source.clone(), carried)?
            .and_then(|whole| wanted.cut_of_the_file(whole)))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::MediaLocation;

    use super::*;
    use crate::Timing;

    fn carrying(text: Option<&str>) -> Wanted {
        Wanted {
            carried: text.map(str::to_owned),
            ..Wanted::for_media(MediaLocation::local("/music/Echoes.flac"))
        }
    }

    fn found(wanted: &Wanted) -> Option<Lyrics> {
        Embedded::default().lyrics(wanted).expect("nothing failed")
    }

    #[test]
    fn the_text_a_track_carries_itself_is_what_this_provider_answers_with() {
        let lyrics = found(&carrying(Some("all that you touch\nall that you see")))
            .expect("the track carries them");

        assert_eq!(lyrics.timing(), Timing::Unsynced);
        assert_eq!(lyrics.source().as_str(), EMBEDDED);
        assert_eq!(lyrics.lines().len(), 2);
    }

    #[test]
    fn a_carried_tag_written_as_an_lrc_is_read_as_a_synced_set() {
        let lyrics =
            found(&carrying(Some("[00:01.00]all that you touch"))).expect("the track carries them");

        assert_eq!(lyrics.timing(), Timing::Synced);
    }

    #[test]
    fn a_carried_tag_past_what_a_sheet_holds_is_refused_rather_than_repeated() {
        let repeated = format!("{}all that you touch", "[00:00.00]".repeat(100_000));

        assert!(
            Embedded::default()
                .lyrics(&carrying(Some(&repeated)))
                .is_err()
        );
    }

    #[test]
    fn a_carried_tag_that_credits_its_own_making_keeps_those_credits() {
        let lyrics = found(&carrying(Some(
            "[by:a stranger]\n[re:LRCGET]\n[ve:0.5]\n[00:01.00]all that you touch",
        )))
        .expect("the track carries them");

        assert_eq!(lyrics.credits().sheet_by.as_deref(), Some("a stranger"));
        assert_eq!(lyrics.credits().editor.as_deref(), Some("LRCGET"));
        assert_eq!(lyrics.credits().version.as_deref(), Some("0.5"));
    }

    #[test]
    fn a_carried_tag_naming_another_track_is_still_the_track_it_came_out_of() {
        let wanted = Wanted {
            title: Some("Echoes".to_owned()),
            duration: Some(Duration::from_secs(23 * 60 + 31)),
            ..carrying(Some(
                "[ti:Time]\n[length:03:20]\n[00:01.00]all that you touch",
            ))
        };

        assert!(found(&wanted).is_some());
    }

    #[test]
    fn a_track_carrying_nothing_answers_with_nothing() {
        assert!(found(&carrying(None)).is_none());
        assert!(found(&carrying(Some("   \n\n"))).is_none());
    }
}
