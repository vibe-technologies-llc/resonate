mod grammar;
mod presented;
mod said;

pub use grammar::*;
pub use presented::{Presented, presented};
pub use said::{command, man_heading, parsed_from};

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    const SAID_BY_AN_ATTRIBUTE: [&str; 4] = [
        "help = \"",
        "about = \"",
        "long_help = \"",
        "long_about = \"",
    ];

    fn sources() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    #[test]
    fn every_word_the_grammar_says_is_said_through_its_catalog() {
        let found = resonate_words_build::prose_in(&sources(), &[]).expect("the sources read");

        let written: Vec<String> = found
            .iter()
            .flat_map(|(path, prose)| {
                prose
                    .iter()
                    .map(move |said| format!("{}:{} {:?}", path.display(), said.line, said.text))
            })
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }

    #[test]
    fn no_help_the_grammar_gives_is_a_literal_in_an_attribute() {
        let grammar = fs::read_to_string(sources().join("grammar.rs")).expect("the grammar reads");

        let literal: Vec<(usize, &str)> = grammar
            .lines()
            .enumerate()
            .filter(|(_, line)| SAID_BY_AN_ATTRIBUTE.iter().any(|said| line.contains(said)))
            .map(|(index, line)| (index + 1, line.trim()))
            .collect();

        assert!(
            literal.is_empty(),
            "help written in the grammar: {literal:?}"
        );
    }
}
