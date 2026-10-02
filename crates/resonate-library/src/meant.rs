use crate::{Column, Reach, Word};

const BY: &str = "by";
const DASHES: [&str; 3] = ["-", "–", "—"];
const MARKS_OF_THE_GRAMMAR: [char; 2] = [':', '"'];
const DENIED: char = '-';

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

#[cfg(test)]
mod tests {
    use super::*;

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
