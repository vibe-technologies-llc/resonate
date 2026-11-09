#[uniffi::export]
pub fn speak_languages(tags: Vec<String>) -> String {
    let language = resonate_words::negotiated(&tags);
    resonate_words::speak(language);

    language.tag().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phone_is_answered_in_the_first_language_it_lists_that_is_spoken_here() {
        assert_eq!(speak_languages(vec!["de-AT".into(), "en-GB".into()]), "en");
        assert_eq!(speak_languages(Vec::new()), "en");
        assert_eq!(resonate_words::spoken(), resonate_words::Language::English);
    }
}
