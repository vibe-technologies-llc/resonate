#[uniffi::export]
pub fn speak_languages(tags: Vec<String>) -> String {
    let language = resonate_words::negotiated(&tags);
    resonate_words::speak(language);

    language.tag().to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const PROPER: &[&str] = &[
        "Album",
        "Single",
        "Monochrome",
        "Android",
        "Shazam",
        "AudD",
        "Android audio output",
        "iOS audio route",
        "System route",
        "OSStatus {}",
        "d.title, d.artist, d.album, d.recording, d.duration_ms",
    ];

    const ONLY_UNDER_TEST: &[&str] = &["harness.rs", "resuming_tests.rs"];

    const SQL_OPENINGS: &[&str] = &[
        "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "WITH", "AND", "FROM", "PRAGMA",
    ];

    fn reads_as_sql(text: &str) -> bool {
        let mut rest = text.trim_start();
        while let Some(after) = rest.strip_prefix('{') {
            rest = after
                .split_once('}')
                .map_or("", |(_, after)| after)
                .trim_start();
        }
        let first = rest.split_whitespace().next().unwrap_or_default();

        SQL_OPENINGS.contains(&first)
    }

    #[test]
    fn every_word_the_phones_show_is_said_through_the_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let found =
            resonate_words_build::prose_in(&root, PROPER).expect("the boundary's sources read");

        let unmoved: Vec<String> = found
            .iter()
            .filter(|(path, _)| !ONLY_UNDER_TEST.contains(&path.to_string_lossy().as_ref()))
            .flat_map(|(path, prose)| {
                prose
                    .iter()
                    .filter(|said| !reads_as_sql(&said.text))
                    .map(move |said| format!("{}:{} {:?}", path.display(), said.line, said.text))
            })
            .collect();

        assert!(
            unmoved.is_empty(),
            "words written in the code:\n{}",
            unmoved.join("\n")
        );
    }

    #[test]
    fn the_phone_is_answered_in_the_first_language_it_lists_that_is_spoken_here() {
        assert_eq!(speak_languages(vec!["de-AT".into(), "en-GB".into()]), "en");
        assert_eq!(speak_languages(Vec::new()), "en");
        assert_eq!(resonate_words::spoken(), resonate_words::Language::English);
    }
}
