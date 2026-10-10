use std::iter;

use resonate_core::words_of_a_name;

use crate::{Column, Reach, RecordingMatch, Word, spelt_alike};

const BY: &str = "by";
const DASHES: [&str; 3] = ["-", "–", "—"];
const MARKS_OF_THE_GRAMMAR: [char; 2] = [':', '"'];
const DENIED: char = '-';
const AN_ARTICLE: &str = "the";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meant {
    pub searched: String,
    pub title: String,
    pub artist: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ByArtist {
    pub title: String,
    pub artist: String,
}

impl ByArtist {
    pub fn read(text: &str) -> Option<Self> {
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let plain = tokens.iter().all(|token| {
            !token.contains(MARKS_OF_THE_GRAMMAR)
                && (DASHES.contains(token) || !token.starts_with(DENIED))
        });
        if !plain {
            return None;
        }
        let joined = |words: &[&str]| words.join(" ");
        let split = |at: usize| (at > 0 && at + 1 < tokens.len()).then_some(at);

        if let Some(at) = tokens
            .iter()
            .rposition(|token| token.eq_ignore_ascii_case(BY))
            .and_then(split)
        {
            return Some(Self {
                title: joined(&tokens[..at]),
                artist: joined(&tokens[at + 1..]),
            });
        }

        let at = tokens
            .iter()
            .position(|token| DASHES.contains(token))
            .and_then(split)?;
        Some(Self {
            title: joined(&tokens[at + 1..]),
            artist: joined(&tokens[..at]),
        })
    }

    pub fn answered_by(&self, matched: &RecordingMatch) -> bool {
        self.credits(matched) && self.titles(matched)
    }

    pub fn credits(&self, matched: &RecordingMatch) -> bool {
        let typed = without_an_article(&self.artist);
        !typed.is_empty()
            && iter::once(matched.credited_as())
                .chain(matched.credit.iter().map(|credit| credit.name.clone()))
                .any(|name| spelt_alike(&typed, &without_an_article(&name)))
    }

    fn titles(&self, matched: &RecordingMatch) -> bool {
        let titled = words_of_a_name(&matched.title);
        let typed = words_of_a_name(&self.title);
        let every_word_answers = typed.iter().all(|word| {
            titled
                .iter()
                .any(|named| named.starts_with(word.as_str()) || spelt_alike(word, named))
        });
        every_word_answers || (!typed.is_empty() && titled.concat().contains(&typed.concat()))
    }

    pub fn words(&self) -> String {
        format!("{} {}", self.title, self.artist)
    }

    pub(crate) fn searched_as(&self, artist: &str) -> Option<String> {
        if artist.contains('"') {
            return None;
        }
        let mut searched: Vec<String> = self
            .title
            .split_whitespace()
            .map(|word| {
                Word {
                    column: Some(Column::Title),
                    text: word.to_owned(),
                    reach: Reach::Begins,
                }
                .to_string()
            })
            .collect();
        searched.push(
            Word {
                column: Some(Column::Artist),
                text: artist.to_owned(),
                reach: Reach::Whole,
            }
            .to_string(),
        );

        Some(searched.join(" "))
    }
}

fn without_an_article(name: &str) -> String {
    let words = words_of_a_name(name);
    match words.split_first() {
        Some((first, rest)) if first == AN_ARTICLE && !rest.is_empty() => rest.concat(),
        _ => words.concat(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credit, Mbid};

    fn credited(title: &str, artists: &[&str]) -> RecordingMatch {
        RecordingMatch {
            recording: Mbid::new("8e4b7d3c-6a6c-4b8e-9c5e-1e2f3a4b5c6d").expect("an mbid"),
            score: 100,
            title: title.to_owned(),
            credit: artists
                .iter()
                .map(|name| Credit {
                    name: (*name).to_owned(),
                    joined_by: String::new(),
                    mbid: None,
                })
                .collect(),
            length: None,
            isrcs: Vec::new(),
            releases: Vec::new(),
        }
    }

    #[test]
    fn a_reading_is_borne_out_only_by_an_answer_crediting_the_artist_and_naming_the_title() {
        let stand = ByArtist::read("stand by me").expect("a reading");
        let stela = ByArtist::read("you fo by stella cole").expect("a reading");

        assert!(!stand.answered_by(&credited("Stand Up, Sit Down", &["Akili and Me"])));
        assert!(!stand.answered_by(&credited("チョコレート", &["≠ME"])));
        assert!(stand.credits(&credited("チョコレート", &["≠ME"])));
        assert!(stela.answered_by(&credited("You F.O.", &["Stela Cole"])));
        assert!(!stela.answered_by(&credited("Candyland", &["Stela Cole"])));
        assert!(
            ByArtist::read("creep by the radiohead")
                .expect("a reading")
                .answered_by(&credited("Creep", &["Radiohead"]))
        );
    }

    fn by(title: &str, artist: &str) -> Option<ByArtist> {
        Some(ByArtist {
            title: title.to_owned(),
            artist: artist.to_owned(),
        })
    }

    #[test]
    fn a_title_by_an_artist_is_read_as_the_two() {
        assert_eq!(
            ByArtist::read("You F O by stela cole"),
            by("You F O", "stela cole")
        );
        assert_eq!(
            ByArtist::read("stand by me BY Ben E King"),
            by("stand by me", "Ben E King"),
            "the last by is the one naming the artist"
        );
        assert_eq!(
            ByArtist::read("Stela Cole - You F.O."),
            by("You F.O.", "Stela Cole")
        );
        assert_eq!(ByArtist::read("Stela Cole – You"), by("You", "Stela Cole"));
    }

    #[test]
    fn words_naming_no_title_or_no_artist_or_written_in_the_grammar_are_not_read_so() {
        for text in [
            "stand by",
            "by the way",
            "pink floyd",
            "- echoes",
            "echoes -",
            "title:echoes by pink floyd",
            "\"echoes\" by pink floyd",
            "echoes by -pink floyd",
        ] {
            assert_eq!(ByArtist::read(text), None, "{text}");
        }
    }

    #[test]
    fn a_title_by_an_artist_is_searched_as_its_words_and_the_artist_whole() {
        let read = ByArtist::read("you f.o by stela cole").expect("a reading");

        assert_eq!(
            read.searched_as("Stela Cole").as_deref(),
            Some(r#"title:you title:f.o artist:="Stela Cole""#)
        );
        assert_eq!(read.searched_as("A \"Quoted\" Name"), None);
        assert_eq!(read.words(), "you f.o stela cole");
    }
}
