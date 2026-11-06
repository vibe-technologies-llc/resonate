const VERSION_QUALIFIERS: &[&str] = &[
    "albumversion",
    "singleversion",
    "cleanversion",
    "explicitversion",
    "explicit",
    "uncensored",
    "uncensoredversion",
    "dirty",
    "dirtyversion",
    "clean",
    "cleanedit",
    "censored",
    "censoredversion",
    "edited",
    "editedversion",
    "radioedit",
    "remaster",
    "remastered",
    "bonustrack",
    "deluxeedition",
    "monoversion",
    "stereoversion",
    "originalmix",
];

const SAYS_CLEAN: &[&str] = &[
    "cleanversion",
    "clean",
    "cleanedit",
    "censored",
    "censoredversion",
    "edited",
    "editedversion",
];

const SAYS_EXPLICIT: &[&str] = &[
    "explicitversion",
    "explicit",
    "uncensored",
    "uncensoredversion",
    "dirty",
    "dirtyversion",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rating {
    Explicit,
    #[default]
    Unmarked,
    Clean,
}

impl Rating {
    pub const fn flagged(explicit: Option<bool>) -> Option<Self> {
        match explicit {
            Some(true) => Some(Self::Explicit),
            Some(false) | None => None,
        }
    }

    pub fn of(stated: Option<Self>, title: &str, version: Option<&str>) -> Self {
        let said_in_words = version
            .map(says)
            .filter(|rating| *rating != Self::Unmarked)
            .unwrap_or_else(|| rating_in_the_title(title));

        match (stated, said_in_words) {
            (Some(Self::Explicit), _) => Self::Explicit,
            (Some(Self::Clean), _) => Self::Clean,
            (Some(Self::Unmarked) | None, said) => said,
        }
    }
}

fn says(qualifier: &str) -> Rating {
    let squeezed = squeezed(qualifier);
    if SAYS_EXPLICIT.contains(&squeezed.as_str()) {
        Rating::Explicit
    } else if SAYS_CLEAN.contains(&squeezed.as_str()) {
        Rating::Clean
    } else {
        Rating::Unmarked
    }
}

pub fn without_its_rating(title: &str) -> &str {
    without_brackets_that(title, |inside| says(inside) != Rating::Unmarked)
}

pub fn rating_in_the_title(title: &str) -> Rating {
    let mut kept = title.trim();
    loop {
        let bracketed = BRACKETS.iter().find_map(|(opening, closing)| {
            let inside = kept.strip_suffix(*closing)?;
            let opened = inside.rfind(*opening)?;
            let before = inside[..opened].trim_end();

            (!before.is_empty()).then(|| (before, &inside[opened + opening.len_utf8()..]))
        });
        let dashed = || {
            SPACED_DASHES.iter().find_map(|dash| {
                let (before, suffix) = kept.rsplit_once(dash)?;
                let before = before.trim_end();

                (!before.is_empty()).then_some((before, suffix))
            })
        };
        let Some((before, qualifier)) = bracketed.or_else(dashed) else {
            return Rating::Unmarked;
        };
        let rating = says(qualifier);
        if rating != Rating::Unmarked {
            return rating;
        }
        kept = before;
    }
}

const DATED_QUALIFIERS: &[&str] = &["remaster", "remastered"];

const FEATURING: &[&str] = &["feat", "ft", "featuring", "with"];

const BRACKETS: &[(char, char)] = &[('(', ')'), ('[', ']')];

const SPACED_DASHES: &[&str] = &[" - ", " – ", " — "];

const YEAR_DIGITS: usize = 4;

fn squeezed(text: &str) -> String {
    text.chars()
        .filter(|glyph| glyph.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn a_year(text: &str) -> bool {
    text.len() == YEAR_DIGITS && text.bytes().all(|digit| digit.is_ascii_digit())
}

fn a_dated_qualifier(squeezed: &str) -> bool {
    DATED_QUALIFIERS.iter().any(|word| {
        squeezed.strip_suffix(word).is_some_and(a_year)
            || squeezed.strip_prefix(word).is_some_and(a_year)
    })
}

pub fn is_a_version_qualifier(text: &str) -> bool {
    let squeezed = squeezed(text);
    VERSION_QUALIFIERS.contains(&squeezed.as_str()) || a_dated_qualifier(&squeezed)
}

fn names_a_guest(text: &str) -> bool {
    text.split(|glyph: char| !glyph.is_alphanumeric())
        .find(|word| !word.is_empty())
        .is_some_and(|first| FEATURING.contains(&first.to_lowercase().as_str()))
}

fn without_a_bracket_that<'a>(
    title: &'a str,
    qualifies: &impl Fn(&str) -> bool,
) -> Option<&'a str> {
    for (opening, closing) in BRACKETS {
        let Some(inside) = title.strip_suffix(*closing) else {
            continue;
        };
        let Some(opened) = inside.rfind(*opening) else {
            continue;
        };
        if !qualifies(&inside[opened + opening.len_utf8()..]) {
            continue;
        }
        let kept = inside[..opened].trim_end();
        if !kept.is_empty() {
            return Some(kept);
        }
    }

    None
}

pub fn without_brackets_that(title: &str, qualifies: impl Fn(&str) -> bool) -> &str {
    let mut kept = title.trim_end();
    while let Some(shorter) = without_a_bracket_that(kept, &qualifies) {
        kept = shorter;
    }
    kept
}

pub fn dequalified(title: &str) -> &str {
    without_brackets_that(title, is_a_version_qualifier)
}

pub fn without_guests(title: &str) -> &str {
    without_brackets_that(title, names_a_guest)
}

fn without_a_qualifying_suffix(title: &str) -> Option<&str> {
    SPACED_DASHES.iter().find_map(|dash| {
        let (kept, suffix) = title.rsplit_once(dash)?;
        let kept = kept.trim_end();
        (!kept.is_empty() && is_a_version_qualifier(suffix)).then_some(kept)
    })
}

pub fn bare(title: &str) -> &str {
    let mut kept = title.trim();
    loop {
        let shorter = without_brackets_that(kept, |inside| {
            is_a_version_qualifier(inside) || names_a_guest(inside)
        });
        let shorter = without_a_qualifying_suffix(shorter).unwrap_or(shorter);
        if shorter.len() == kept.len() {
            return kept;
        }
        kept = shorter;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_says_whether_it_is_the_clean_or_the_explicit_take_only_in_its_qualifier() {
        assert_eq!(rating_in_the_title("Bad Guy (Clean)"), Rating::Clean);
        assert_eq!(
            rating_in_the_title("Bad Guy [Edited Version]"),
            Rating::Clean
        );
        assert_eq!(rating_in_the_title("Bad Guy - Clean"), Rating::Clean);
        assert_eq!(
            rating_in_the_title("Bad Guy (Clean) [2019 Remaster]"),
            Rating::Clean
        );
        assert_eq!(rating_in_the_title("Bad Guy (Explicit)"), Rating::Explicit);
        assert_eq!(rating_in_the_title("Bad Guy"), Rating::Unmarked);
        assert_eq!(rating_in_the_title("Clean"), Rating::Unmarked);
        assert_eq!(rating_in_the_title("Squeaky Clean"), Rating::Unmarked);
        assert_eq!(rating_in_the_title("Clean (Live)"), Rating::Unmarked);
    }

    #[test]
    fn what_a_service_states_outranks_what_the_words_say_and_a_version_outranks_the_title() {
        assert_eq!(
            Rating::of(Some(Rating::Explicit), "Bad Guy (Clean)", None),
            Rating::Explicit
        );
        assert_eq!(Rating::of(None, "Bad Guy", Some("Clean")), Rating::Clean);
        assert_eq!(
            Rating::of(None, "Bad Guy (Clean)", Some("Explicit")),
            Rating::Explicit
        );
        assert_eq!(Rating::of(None, "Bad Guy", None), Rating::Unmarked);
    }

    #[test]
    fn a_version_bracket_is_taken_off_and_another_recording_is_left_alone() {
        assert_eq!(dequalified("Creep (Remastered 2009)"), "Creep");
        assert_eq!(dequalified("Creep (2009 Remaster) [Explicit]"), "Creep");
        assert_eq!(dequalified("Creep (Acoustic)"), "Creep (Acoustic)");
        assert_eq!(dequalified("(Remastered)"), "(Remastered)");
    }

    #[test]
    fn a_guest_is_left_out_without_losing_the_requested_version() {
        assert_eq!(
            without_guests("Promiscuous (feat. Timbaland)"),
            "Promiscuous"
        );
        for title in [
            "Promiscuous (Radio Edit)",
            "Promiscuous (Remix)",
            "Promiscuous (Live)",
        ] {
            assert_eq!(without_guests(title), title);
        }
        assert_eq!(
            without_guests("Promiscuous (Radio Edit) (feat. Timbaland)"),
            "Promiscuous (Radio Edit)"
        );
    }

    #[test]
    fn a_bare_title_drops_versions_guests_and_a_dashed_version_but_not_a_live_take() {
        assert_eq!(
            bare("Bohemian Rhapsody - Remastered 2011"),
            "Bohemian Rhapsody"
        );
        assert_eq!(bare("Stand By Me - Remastered"), "Stand By Me");
        assert_eq!(bare("Blinding Lights - Radio Edit"), "Blinding Lights");
        assert_eq!(
            bare("C’est La Vie (with bbno$ & Rich Brian)"),
            "C’est La Vie"
        );
        assert_eq!(
            bare("Hot (feat. Gunna) [Remix]"),
            "Hot (feat. Gunna) [Remix]"
        );
        assert_eq!(bare("Hot [Remix] (feat. Gunna)"), "Hot [Remix]");
        assert_eq!(
            bare("Bohemian Rhapsody - Live at Wembley"),
            "Bohemian Rhapsody - Live at Wembley"
        );
        assert_eq!(bare("Wait - (Remastered)"), "Wait -");
        assert_eq!(bare("Remastered"), "Remastered");
    }
}
