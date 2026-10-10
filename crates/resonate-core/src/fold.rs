use std::ops::RangeInclusive;

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub fn folded_letters(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    let mut accented = false;
    for letter in text.to_lowercase().nfd() {
        if is_combining_mark(letter) {
            if !accented {
                folded.push(letter);
            }
            continue;
        }
        accented = takes_accents(letter);
        match spelled_out(letter) {
            Some(plainly) => folded.push_str(plainly),
            None => folded.push(letter),
        }
    }

    folded.nfc().collect()
}

const JOINING_MARKS: [char; 4] = ['\'', '\u{2019}', '\u{02bc}', '`'];

pub fn words_of_a_name(text: &str) -> Vec<String> {
    folded_letters(text)
        .chars()
        .filter(|glyph| !JOINING_MARKS.contains(glyph))
        .collect::<String>()
        .split(|glyph: char| !is_lettered(glyph))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn is_lettered(glyph: char) -> bool {
    glyph.is_alphanumeric() || is_combining_mark(glyph)
}

const ACCENTED_SCRIPTS: [RangeInclusive<char>; 11] = [
    '\u{0000}'..='\u{024f}',
    '\u{0370}'..='\u{03ff}',
    '\u{0400}'..='\u{052f}',
    '\u{1c80}'..='\u{1c8f}',
    '\u{1d00}'..='\u{1dbf}',
    '\u{1e00}'..='\u{1fff}',
    '\u{2c60}'..='\u{2c7f}',
    '\u{2de0}'..='\u{2dff}',
    '\u{a640}'..='\u{a69f}',
    '\u{a720}'..='\u{a7ff}',
    '\u{ab30}'..='\u{ab6f}',
];

fn takes_accents(letter: char) -> bool {
    ACCENTED_SCRIPTS
        .iter()
        .any(|script| script.contains(&letter))
}

const fn spelled_out(letter: char) -> Option<&'static str> {
    Some(match letter {
        'ł' => "l",
        'ø' => "o",
        'đ' | 'ð' => "d",
        'þ' => "th",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        'ı' => "i",
        'ħ' => "h",
        'ŋ' => "n",
        'ŧ' => "t",
        'ĸ' => "k",
        'ſ' => "s",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_of_a_name_are_folded_joined_across_an_apostrophe_and_cut_at_anything_else() {
        assert_eq!(
            words_of_a_name("Don’t Stop Me Now"),
            ["dont", "stop", "me", "now"]
        );
        assert_eq!(words_of_a_name("Can't"), ["cant"]);
        assert_eq!(words_of_a_name("You F.O."), ["you", "f", "o"]);
        assert_eq!(words_of_a_name("AC/DC"), ["ac", "dc"]);
        assert_eq!(words_of_a_name("Björk — Jóga"), ["bjork", "joga"]);
        assert!(words_of_a_name("!!!").is_empty());
    }

    #[test]
    fn a_mark_is_an_accent_only_on_a_latin_greek_or_cyrillic_letter() {
        assert_eq!(folded_letters("Björk"), "bjork");
        assert_eq!(folded_letters("Ελλάδα"), "ελλαδα");
        assert_eq!(folded_letters("Йога"), "иога");
        assert_ne!(folded_letters("ガラス"), folded_letters("カラス"));
        assert_ne!(folded_letters("バンド"), folded_letters("ハンド"));
        assert_eq!(folded_letters("ガラス"), "ガラス");
        assert_eq!(folded_letters("ｶﾞﾗｽ").chars().count(), "ｶﾞﾗｽ".chars().count());
        assert_ne!(folded_letters("हिन्दी"), folded_letters("हनद"));
        assert_eq!(folded_letters("हिन्दी"), "हिन्दी");
    }

    #[test]
    fn a_combining_mark_belongs_to_the_word_it_sits_in() {
        assert!(is_lettered('\u{0308}'));
        assert!(is_lettered('\u{093f}'));
        assert!(is_lettered('o'));
        assert!(!is_lettered(' '));
        assert!(!is_lettered('-'));
    }

    #[test]
    fn a_kept_mark_folds_the_same_whether_composed_or_not() {
        assert_eq!(folded_letters("ガ"), folded_letters("カ\u{3099}"));
        assert_eq!(
            folded_letters(&folded_letters("ガラス")),
            folded_letters("ガラス")
        );
    }
}
