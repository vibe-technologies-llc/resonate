use std::borrow::Cow;

pub(crate) const FIELD_BYTES_AT_MOST: usize = 4 * 1024;
pub(crate) const LYRICS_BYTES_AT_MOST: usize = 32 * 1024;

pub(crate) fn fitted(text: &str, at_most: usize) -> Cow<'_, str> {
    let (kept, _) = text.split_at(text.floor_char_boundary(at_most));

    if kept.chars().all(is_shown_character) {
        Cow::Borrowed(kept)
    } else {
        Cow::Owned(
            kept.chars()
                .filter(|letter| is_shown_character(*letter))
                .collect(),
        )
    }
}

pub(crate) fn field(text: &str) -> String {
    fitted(text, FIELD_BYTES_AT_MOST).into_owned()
}

pub(crate) const fn is_shown_character(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nul_and_the_other_controls_xml_refuses_are_left_out_and_the_rest_kept() {
        assert_eq!(fitted("a\0b", FIELD_BYTES_AT_MOST), "ab");
        assert_eq!(
            fitted(
                "Example\u{1}Band\u{1F}\u{FFFE}\u{FFFF}",
                FIELD_BYTES_AT_MOST
            ),
            "ExampleBand"
        );
        assert_eq!(
            fitted("one\ttwo\r\nthree \u{7F}\u{85}", FIELD_BYTES_AT_MOST),
            "one\ttwo\r\nthree \u{7F}\u{85}"
        );
    }

    #[test]
    fn text_already_fit_for_the_bus_is_lent_rather_than_copied() {
        assert!(matches!(
            fitted("Slow Water", FIELD_BYTES_AT_MOST),
            Cow::Borrowed("Slow Water")
        ));
    }

    #[test]
    fn an_oversized_value_is_cut_to_the_cap_on_a_letter() {
        let sprawling = "é".repeat(FIELD_BYTES_AT_MOST);
        let cut = fitted(&sprawling, FIELD_BYTES_AT_MOST);

        assert_eq!(cut.len(), FIELD_BYTES_AT_MOST);
        assert!(sprawling.starts_with(cut.as_ref()));

        let odd = format!("a{sprawling}");
        let cut = fitted(&odd, FIELD_BYTES_AT_MOST);

        assert_eq!(cut.len(), FIELD_BYTES_AT_MOST - 1);
        assert!(odd.starts_with(cut.as_ref()));

        let lyrics = "la ".repeat(LYRICS_BYTES_AT_MOST);

        assert_eq!(
            fitted(&lyrics, LYRICS_BYTES_AT_MOST).len(),
            LYRICS_BYTES_AT_MOST
        );
        assert_eq!(field(&lyrics).len(), FIELD_BYTES_AT_MOST);
    }
}
