use std::fmt::Write;

use resonate_core::SourceId;
use resonate_library::{LookupOp, LyricText, LyricsAsked};
use resonate_lyrics::{LyricOp, Lyrics, Timing, Voice, read_ttml};
use serde::Deserialize;

use crate::{Client, Error, Host, lyrics::Failed, query::Params};

pub(crate) const BETTER_LYRICS: &str = "betterlyrics";
const MATCHED_AT_LEAST: f64 = 0.5;
const UNCACHED_WITHOUT_A_KEY: u16 = 401;

#[derive(Deserialize)]
struct Answer {
    ttml: String,
    #[serde(default)]
    score: Option<f64>,
}

impl Answer {
    fn told(self, asked: &LyricsAsked) -> Option<LyricText> {
        if self.score.is_some_and(|score| score < MATCHED_AT_LEAST) {
            tracing::debug!(score = self.score, "better lyrics matched another song");
            return None;
        }
        let read = read_ttml(source(), &self.ttml)
            .map_err(|unread| {
                tracing::debug!(
                    ?unread,
                    "better lyrics answered a document this build cannot read"
                )
            })
            .ok()?;
        if read.declares_another_length_than(asked.length) {
            tracing::debug!("better lyrics answered for a song of another length");
            return None;
        }
        let lyrics = read.lyrics?;

        Some(LyricText {
            text: lrc_of(&lyrics),
            synced: lyrics.timing() == Timing::Synced,
            lyricsfile: read.says_more_than_its_lines.then_some(self.ttml),
        })
    }
}

pub(crate) fn source() -> SourceId {
    SourceId::new(BETTER_LYRICS).unwrap_or_else(|_| SourceId::local())
}

fn lrc_of(lyrics: &Lyrics) -> String {
    let mut sheet = String::new();
    for line in lyrics.lines() {
        if let Some(at) = line.at {
            let hundredths = at.as_millis() / 10;
            let _ = write!(
                sheet,
                "[{:02}:{:02}.{:02}]",
                hundredths / 6_000,
                hundredths / 100 % 60,
                hundredths % 100
            );
        }
        match line.voice {
            Voice::Two if line.at.is_some() => {
                let _ = write!(sheet, "[v2: {}]", line.text);
            }
            Voice::One | Voice::Two => sheet.push_str(&line.text),
        }
        sheet.push('\n');
    }
    sheet
}

pub(crate) fn told(client: &Client, asked: &LyricsAsked) -> Result<Option<LyricText>, Failed> {
    let seconds = asked.length.map(|length| length.as_secs().to_string());
    let get = Params::new()
        .with("s", &asked.title)
        .with("a", &asked.artist)
        .maybe("al", asked.album.as_deref())
        .maybe("d", seconds.as_deref())
        .finish();
    let answered: Option<Answer> = match client.json(
        Host::BetterLyrics,
        LookupOp::Lyrics,
        &format!("/getLyrics{get}"),
    ) {
        Err(Error::Refused {
            status: UNCACHED_WITHOUT_A_KEY,
            ..
        }) => None,
        answered => answered.map_err(|error| Failed::new(source(), LyricOp::Fetch, error))?,
    };
    let told = answered.and_then(|answer| answer.told(asked));
    tracing::debug!(
        title = asked.title,
        artist = asked.artist,
        synced = told.as_ref().map(|told| told.synced),
        document = told.as_ref().is_some_and(|told| told.lyricsfile.is_some()),
        "better lyrics answered"
    );

    Ok(told)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_lyrics::{Detail, read_document};

    use super::*;

    const WORDED: &str = include_str!("../tests/fixtures/betterlyrics_worded.json");
    const DUET: &str = include_str!("../tests/fixtures/betterlyrics_duet.json");

    fn asked(length: Option<u64>) -> LyricsAsked {
        LyricsAsked {
            title: "Viva la Vida".to_owned(),
            artist: "Coldplay".to_owned(),
            album: None,
            length: length.map(Duration::from_secs),
        }
    }

    fn answer(document: &str) -> Answer {
        serde_json::from_str(document).expect("the fixture parses")
    }

    #[test]
    fn a_word_timed_answer_keeps_its_document_beside_a_sheet_of_its_lines() {
        let told = answer(WORDED)
            .told(&asked(Some(242)))
            .expect("the answer carries words");

        assert!(told.synced);
        assert!(
            told.text
                .starts_with("[00:12.74]I used to rule the world\n")
        );
        let document = told
            .lyricsfile
            .expect("the document says more than its lines");
        let lyrics = read_document(source(), &document)
            .expect("the document reads back")
            .lyrics
            .expect("its lines are there");
        assert_eq!(lyrics.detail(), Detail::Words);
    }

    #[test]
    fn the_second_singer_is_written_into_the_sheet_as_the_second_voice() {
        let told = answer(DUET)
            .told(&asked(None))
            .expect("the answer carries words");

        assert!(told.text.starts_with("[00:03.43][v2: Ooh, ooh]\n"));
        let sheet = resonate_lyrics::read_lyrics(source(), &told.text)
            .expect("the sheet reads")
            .expect("the sheet holds lines");
        assert_eq!(sheet.lines()[0].voice, Voice::Two);
        assert_eq!(sheet.lines()[0].text, "Ooh, ooh");
    }

    #[test]
    fn an_answer_for_a_song_of_another_length_or_a_poor_match_is_not_taken() {
        assert!(answer(WORDED).told(&asked(Some(300))).is_none());
        assert!(answer(WORDED).told(&asked(Some(260))).is_some());

        let guessed = Answer {
            score: Some(0.2),
            ..answer(WORDED)
        };
        assert!(guessed.told(&asked(None)).is_none());
        let matched = Answer {
            score: Some(0.95),
            ..answer(WORDED)
        };
        assert!(matched.told(&asked(None)).is_some());
    }
}
