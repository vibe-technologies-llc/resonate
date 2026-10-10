use std::time::Duration;

use resonate_core::{Isrc, titles, words_of_a_name};

use crate::Identity;

pub const LENGTHS_AGREE_WITHIN: Duration = Duration::from_secs(5);

pub const A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY: Duration = Duration::from_secs(2);

const JOIN_PHRASES: [&str; 12] = [
    " feat. ",
    " feat ",
    " ft. ",
    " ft ",
    " featuring ",
    " & ",
    " x ",
    " × ",
    ", ",
    " and ",
    " with ",
    " vs. ",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    ByItsCode,
    NamedAlike,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listed<'a> {
    pub isrcs: Vec<&'a str>,
    pub title: &'a str,
    pub version: Option<&'a str>,
    pub artists: Vec<&'a str>,
    pub length: Option<Duration>,
}

impl Listed<'_> {
    fn titled(&self) -> String {
        match self
            .version
            .map(str::trim)
            .filter(|version| !version.is_empty())
        {
            Some(version) => format!("{} ({version})", self.title),
            None => self.title.to_owned(),
        }
    }
}

#[derive(Debug)]
pub struct Choosing<T> {
    coded: Vec<(Duration, T)>,
    named: Vec<(Duration, T)>,
    seen: usize,
}

impl<T> Default for Choosing<T> {
    fn default() -> Self {
        Self {
            coded: Vec::new(),
            named: Vec::new(),
            seen: 0,
        }
    }
}

impl<T: PartialEq> Choosing<T> {
    pub fn weigh(&mut self, identity: &Identity, listed: &Listed<'_>, item: T) {
        self.seen += 1;
        let apart = identity.apart_in_length(listed.length);
        let coded = listed
            .isrcs
            .iter()
            .filter_map(|code| Isrc::new(code.trim()).ok())
            .any(|code| identity.isrcs.contains(&code));
        if coded {
            if apart.is_none_or(|apart| apart <= LENGTHS_AGREE_WITHIN) {
                self.coded.push((apart.unwrap_or_default(), item));
            }
            return;
        }
        if identity.named_alike(listed) {
            self.named.push((apart.unwrap_or_default(), item));
        }
    }

    pub const fn holds_a_coded_listing(&self) -> bool {
        !self.coded.is_empty()
    }

    pub const fn seen(&self) -> usize {
        self.seen
    }

    pub fn ranked(self) -> Vec<(T, Taken)> {
        let mut ranked: Vec<(T, Taken)> = Vec::new();
        for (listed, taken) in [
            (self.coded, Taken::ByItsCode),
            (self.named, Taken::NamedAlike),
        ] {
            if taken == Taken::NamedAlike && !ranked.is_empty() {
                break;
            }
            let mut listed = listed;
            listed.sort_by_key(|(apart, _)| *apart);
            for (_, item) in listed {
                if !ranked.iter().any(|(kept, _)| *kept == item) {
                    ranked.push((item, taken));
                }
            }
        }
        ranked
    }

    pub fn chosen(self) -> Option<(T, Taken)> {
        self.ranked().into_iter().next()
    }
}

fn led_by(credit: &str) -> &str {
    let lowered = credit.to_lowercase();
    let cut = JOIN_PHRASES
        .iter()
        .filter_map(|phrase| lowered.find(phrase))
        .min()
        .filter(|cut| credit.is_char_boundary(*cut));
    match cut {
        Some(cut) if lowered.len() == credit.len() => credit[..cut].trim(),
        _ => credit.trim(),
    }
}

fn spelt_as(name: &str) -> String {
    words_of_a_name(name).concat()
}

impl Identity {
    pub fn lead_artist(&self) -> Option<&str> {
        self.artist
            .as_deref()
            .map(led_by)
            .filter(|lead| !lead.is_empty())
    }

    pub fn wordings(&self) -> Vec<String> {
        let title = self.title.trim();
        if title.is_empty() {
            return Vec::new();
        }
        let bare = titles::bare(title);
        let worded = match self.lead_artist() {
            Some(lead) => vec![
                format!("{title} {lead}"),
                format!("{lead} {title}"),
                format!("{bare} {lead}"),
                title.to_owned(),
            ],
            None => vec![title.to_owned(), bare.to_owned()],
        };

        let mut wordings: Vec<String> = Vec::with_capacity(worded.len());
        for words in worded {
            if !wordings.contains(&words) {
                wordings.push(words);
            }
        }
        wordings
    }

    pub fn may_be_listed(&self) -> bool {
        !self.title.trim().is_empty()
            && (!self.isrcs.is_empty() || (self.lead_artist().is_some() && self.length.is_some()))
    }

    pub fn apart_in_length(&self, length: Option<Duration>) -> Option<Duration> {
        Some(self.length?.abs_diff(length?))
    }

    pub fn named_alike(&self, listed: &Listed<'_>) -> bool {
        let Some(apart) = self.apart_in_length(listed.length) else {
            return false;
        };
        let Some(lead) = self.lead_artist().map(spelt_as) else {
            return false;
        };
        let titled = spelt_as(titles::bare(&self.title));

        apart <= A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY
            && !titled.is_empty()
            && !lead.is_empty()
            && spelt_as(titles::bare(&listed.titled())) == titled
            && listed
                .artists
                .iter()
                .any(|artist| spelt_as(led_by(artist)) == lead)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted() -> Identity {
        Identity {
            isrcs: vec![
                Isrc::new("GBUM71029604").expect("an isrc"),
                Isrc::new("GBCEE0400076").expect("an isrc"),
            ],
            artist: Some("Queen feat. Nobody".to_owned()),
            length: Some(Duration::from_secs(355)),
            ..Identity::named("Bohemian Rhapsody (Remastered 2011)")
        }
    }

    fn listed<'a>(isrc: &'a str, title: &'a str, artist: &'a str, seconds: u64) -> Listed<'a> {
        Listed {
            isrcs: vec![isrc],
            title,
            version: None,
            artists: vec![artist],
            length: Some(Duration::from_secs(seconds)),
        }
    }

    #[test]
    fn a_want_is_searched_for_by_its_lead_artist_both_ways_round_and_by_its_bare_title() {
        assert_eq!(
            wanted().wordings(),
            [
                "Bohemian Rhapsody (Remastered 2011) Queen",
                "Queen Bohemian Rhapsody (Remastered 2011)",
                "Bohemian Rhapsody Queen",
                "Bohemian Rhapsody (Remastered 2011)",
            ]
        );
        assert_eq!(
            Identity::named("Echoes").wordings(),
            ["Echoes"],
            "a want with no artist is asked by its title alone"
        );
        assert!(Identity::named("  ").wordings().is_empty());
    }

    #[test]
    fn any_of_the_wants_codes_takes_a_listing_as_long_as_it_and_the_nearest_wins() {
        let identity = wanted();
        let mut choosing = Choosing::default();

        choosing.weigh(&identity, &listed("GBCEE0400076", "x", "y", 370), 1);
        choosing.weigh(&identity, &listed("GBCEE0400076", "x", "y", 358), 2);
        choosing.weigh(&identity, &listed("GB-CEE-04-00076", "x", "y", 356), 3);

        assert!(choosing.holds_a_coded_listing());
        assert_eq!(choosing.seen(), 3);
        assert_eq!(
            choosing.ranked(),
            [(3, Taken::ByItsCode), (2, Taken::ByItsCode)],
            "a listing longer than the want by more than a skip is never offered"
        );
    }

    #[test]
    fn a_listing_under_another_code_is_taken_only_named_alike_and_only_where_none_is_coded() {
        let identity = wanted();
        let mut choosing = Choosing::default();

        for (item, listing) in [
            (
                1,
                listed("USHR10421348", "Bohemian Rhapsody - Live", "Queen", 355),
            ),
            (2, listed("USHR10421348", "Bohemian Rhapsody", "Queen", 358)),
            (
                3,
                listed(
                    "USHR10421348",
                    "Bohemian Rhapsody",
                    "Panic! at the Disco",
                    355,
                ),
            ),
            (
                4,
                listed(
                    "USHR10421348",
                    "Bohemian Rhapsody - 2011 Remaster",
                    "Queen",
                    356,
                ),
            ),
        ] {
            choosing.weigh(&identity, &listing, item);
        }
        assert!(!choosing.holds_a_coded_listing());
        assert_eq!(choosing.ranked(), [(4, Taken::NamedAlike)]);

        let mut coded = Choosing::default();
        coded.weigh(
            &identity,
            &listed("USHR10421348", "Bohemian Rhapsody", "Queen", 355),
            1,
        );
        coded.weigh(
            &identity,
            &listed("GBUM71029604", "Bohemian Rhapsody", "Queen", 359),
            2,
        );
        assert_eq!(coded.chosen(), Some((2, Taken::ByItsCode)));
    }

    #[test]
    fn a_want_of_no_length_or_no_artist_is_never_taken_by_its_name() {
        let listing = listed("USHR10421348", "Bohemian Rhapsody", "Queen", 355);
        let unmeasured = Identity {
            length: None,
            ..wanted()
        };
        let uncredited = Identity {
            artist: None,
            ..wanted()
        };

        assert!(wanted().named_alike(&listing));
        assert!(!unmeasured.named_alike(&listing));
        assert!(!uncredited.named_alike(&listing));
        assert!(
            wanted().named_alike(&Listed {
                version: Some("Remastered 2011"),
                artists: vec!["Queen & David Bowie"],
                ..listing.clone()
            }),
            "a version a listing names apart and a guest beside its lead are weighed as written"
        );
        assert!(!wanted().named_alike(&Listed {
            version: Some("Live Aid"),
            ..listing
        }));
    }

    #[test]
    fn a_want_is_worth_a_search_where_it_has_a_code_or_an_artist_and_a_length() {
        assert!(wanted().may_be_listed());
        assert!(
            Identity {
                isrcs: Vec::new(),
                ..wanted()
            }
            .may_be_listed()
        );
        assert!(
            !Identity {
                isrcs: Vec::new(),
                length: None,
                ..wanted()
            }
            .may_be_listed()
        );
        assert!(!Identity::named("Echoes").may_be_listed());
    }

    #[test]
    fn the_lead_artist_is_the_credit_before_its_first_join() {
        let led = |credit: &str| {
            Identity {
                artist: Some(credit.to_owned()),
                ..Identity::default()
            }
            .lead_artist()
            .map(str::to_owned)
        };

        assert_eq!(led("Janji & Johnning").as_deref(), Some("Janji"));
        assert_eq!(
            led("Yung Gravy, bbno$ and Rich Brian").as_deref(),
            Some("Yung Gravy")
        );
        assert_eq!(led("Ben E. King").as_deref(), Some("Ben E. King"));
        assert_eq!(led("  ").as_deref(), None);
    }
}
