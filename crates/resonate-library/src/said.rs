use std::{fs, path::Path};

const SAYS_WORDS: &[&str] = &[
    "import.rs",
    "outcomes.rs",
    "playlist.rs",
    "reference.rs",
    "retag.rs",
    "suggest.rs",
    "synced.rs",
    "take_in.rs",
];

const PROPER: &[&str] = &[];

const SQL_WORDS: [&str; 26] = [
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "FROM", "WHERE", "JOIN",
    "AND", "OR", "NOT", "NULL", "IS", "IN", "CASE", "WHEN", "COLLATE", "LIMIT", "DESC", "ASC",
    "LIKE", "DISTINCT", "VALUES", "SET",
];

fn speaks_sql(text: &str) -> bool {
    let shouted = text
        .split(|letter: char| !letter.is_ascii_alphabetic())
        .any(|word| SQL_WORDS.contains(&word));
    let names_a_column = text.as_bytes().windows(3).any(|three| {
        three[0].is_ascii_lowercase() && three[1] == b'.' && three[2].is_ascii_lowercase()
    });

    shouted || names_a_column
}

#[test]
fn every_word_the_library_says_is_said_through_its_catalog() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut unmoved = Vec::new();
    for file in SAYS_WORDS {
        let source = fs::read_to_string(root.join(file)).expect("a file that says words is read");
        unmoved.extend(
            resonate_words_build::prose_of(&source)
                .into_iter()
                .filter(|said| !speaks_sql(&said.text) && !PROPER.contains(&said.text.as_str()))
                .map(|said| format!("{file}:{} {:?}", said.line, said.text)),
        );
    }

    assert!(
        unmoved.is_empty(),
        "words written in the code:\n{}",
        unmoved.join("\n")
    );
}

#[test]
fn sql_is_told_apart_from_words() {
    assert!(speaks_sql("SELECT name FROM playlists WHERE id = ?1"));
    assert!(speaks_sql("tracks.hidden = 0"));
    assert!(!speaks_sql("Everything from the nineties"));
    assert!(!speaks_sql("a recording by its ISRC"));
}
