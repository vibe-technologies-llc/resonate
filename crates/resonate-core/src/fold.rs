use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub fn folded_letters(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for letter in text.to_lowercase().nfd() {
        if is_combining_mark(letter) {
            continue;
        }
        match spelled_out(letter) {
            Some(plainly) => folded.push_str(plainly),
            None => folded.push(letter),
        }
    }

    folded
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
