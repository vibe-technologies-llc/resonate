const DIGITS_AT_MOST: usize = 3;

const AFTER_A_NUMBER: &[char] = &['-', '.', '_', ')'];

const HYPHEN_BETWEEN: &str = " - ";

const EN_DASH_BETWEEN: &str = " – ";

const EM_DASH_BETWEEN: &str = " — ";

const SEPARATORS: &[&str] = &[HYPHEN_BETWEEN, EN_DASH_BETWEEN, EM_DASH_BETWEEN];

pub(crate) struct Named {
    pub title: String,
    pub artist: Option<String>,
    pub track_number: Option<u32>,
}

pub(crate) fn read(stem: &str) -> Option<Named> {
    let trimmed = stem.trim();
    let (track_number, rest) = leading_number(trimmed);
    let (artist, title) = split_at_a_separator(rest);

    let title = title.trim();
    if title.is_empty() {
        return None;
    }

    let artist = artist.map(str::trim).filter(|artist| !artist.is_empty());
    if artist.is_some_and(only_digits) {
        return None;
    }
    if artist.is_none() && track_number.is_none() && title == trimmed {
        return None;
    }

    Some(Named {
        title: title.to_owned(),
        artist: artist.map(str::to_owned),
        track_number,
    })
}

fn a_boundary(glyph: char) -> bool {
    AFTER_A_NUMBER.contains(&glyph) || glyph.is_whitespace()
}

fn leading_number(stem: &str) -> (Option<u32>, &str) {
    let digits = stem
        .bytes()
        .take(DIGITS_AT_MOST + 1)
        .take_while(u8::is_ascii_digit)
        .count();
    if digits == 0 || digits > DIGITS_AT_MOST {
        return (None, stem);
    }

    let after = &stem[digits..];
    let rest = after.trim_start_matches(a_boundary);
    if rest.len() == after.len() || rest.is_empty() {
        return (None, stem);
    }

    match stem[..digits].parse() {
        Ok(number) => (Some(number), rest),
        Err(_) => (None, stem),
    }
}

fn split_at_a_separator(rest: &str) -> (Option<&str>, &str) {
    let earliest = SEPARATORS
        .iter()
        .filter_map(|separator| rest.find(separator).map(|at| (at, separator.len())))
        .min();

    match earliest {
        Some((at, width)) => (Some(&rest[..at]), &rest[at + width..]),
        None => (None, rest),
    }
}

fn only_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|glyph| glyph.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_as(stem: &str) -> (Option<u32>, Option<String>, String) {
        let named = read(stem).expect("the stem parses");
        (named.track_number, named.artist, named.title)
    }

    #[test]
    fn a_stem_is_read_as_a_number_an_artist_and_a_title() {
        assert_eq!(
            read_as("03 - Miles Davis - So What"),
            (
                Some(3),
                Some("Miles Davis".to_owned()),
                "So What".to_owned()
            )
        );
        assert_eq!(
            read_as("03. Miles Davis – So What"),
            (
                Some(3),
                Some("Miles Davis".to_owned()),
                "So What".to_owned()
            )
        );
        assert_eq!(
            read_as("003_Miles Davis — So What"),
            (
                Some(3),
                Some("Miles Davis".to_owned()),
                "So What".to_owned()
            )
        );
        assert_eq!(
            read_as("7) Miles Davis - So What"),
            (
                Some(7),
                Some("Miles Davis".to_owned()),
                "So What".to_owned()
            )
        );
    }

    #[test]
    fn the_first_separator_is_the_one_that_splits_an_artist_from_a_title() {
        assert_eq!(
            read_as("Miles Davis - So What - Live"),
            (
                None,
                Some("Miles Davis".to_owned()),
                "So What - Live".to_owned()
            )
        );
    }

    #[test]
    fn a_stem_with_no_separator_is_the_title_alone() {
        assert_eq!(read_as("01 So What"), (Some(1), None, "So What".to_owned()));
        assert_eq!(
            read_as("12. So What"),
            (Some(12), None, "So What".to_owned())
        );
    }

    #[test]
    fn a_stem_that_parses_to_nothing_but_itself_is_left_to_the_fallback() {
        assert!(read("sparky").is_none());
        assert!(read("So What").is_none());
        assert!(read("2011").is_none());
        assert!(read("123").is_none());
        assert!(read("12Stones").is_none());
        assert!(read("").is_none());
        assert!(read(" - ").is_none());
    }

    #[test]
    fn an_artist_that_is_only_digits_is_no_artist() {
        assert!(read("1979 - Never Let Me Down").is_none());
        assert!(read("02 - 1979 - Never Let Me Down").is_none());
    }
}
