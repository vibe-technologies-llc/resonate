#[cfg(test)]
mod tests {
    use std::path::{Component, Path};

    const PROPER: &[&str] = &[
        "Resonate",
        "Linux",
        "Windows",
        "macOS",
        "Library/Caches",
        "Library/Application Support",
        "Config",
        "Accounts",
        "LastfmAccount",
        "JellyfinServer",
        "{} ({} on {rig})",
        "resonate: {error}",
        "  caused by: {cause}",
        "resonate: the console the window was started with stays open: {error}",
    ];

    const NOT_WORDS: &[&str] = &[
        "awake.rs",
        "error.rs",
        "config/tests/accounts.rs",
        "config/tests/appearance.rs",
        "config/tests/equaliser.rs",
        "config/tests/man_page.rs",
        "config/tests/reading.rs",
        "config/tests/writing.rs",
    ];

    const STILL_TO_MOVE: &[&str] = &[];

    fn spelled(path: &Path) -> String {
        path.components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    #[test]
    fn every_word_the_terminal_says_is_said_through_its_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let found =
            resonate_words_build::prose_in(&root, PROPER).expect("the terminal's sources read");

        let unmoved: Vec<String> = found
            .iter()
            .filter(|(path, _)| {
                let path = spelled(path);
                !STILL_TO_MOVE.contains(&path.as_str()) && !NOT_WORDS.contains(&path.as_str())
            })
            .flat_map(|(path, prose)| {
                prose
                    .iter()
                    .map(move |said| format!("{}:{} {:?}", path.display(), said.line, said.text))
            })
            .collect();
        let moved: Vec<&&str> = STILL_TO_MOVE
            .iter()
            .filter(|path| !found.keys().any(|found| spelled(found) == **path))
            .collect();

        assert!(
            unmoved.is_empty(),
            "words written in the code:\n{}",
            unmoved.join("\n")
        );
        assert!(
            moved.is_empty(),
            "files whose words are all moved: {moved:?}"
        );
    }
}
