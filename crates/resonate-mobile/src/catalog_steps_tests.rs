use std::{fs, path::Path};

const CATALOG_SOURCE: &str = include_str!("catalog.rs");
const MIGRATIONS_OPENING: &str = "const MIGRATIONS: &[&str] = &[";
const RULES_FOLDER: &str = "../../.claude/rules";
const CITATION_REACH: usize = 48;
const UNITS: [&str; 10] = [
    "", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
];
const TEENS: [&str; 10] = [
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
];
const TENS_OF_UNITS: [&str; 5] = ["", "", "twenty", "thirty", "forty"];
const TENS_ORDINAL: [&str; 5] = ["", "", "twentieth", "thirtieth", "fortieth"];

fn step_names() -> Vec<&'static str> {
    let after = CATALOG_SOURCE
        .split_once(MIGRATIONS_OPENING)
        .expect("the chain of steps")
        .1;
    let list = after.split_once("];").expect("the chain closes").0;
    list.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect()
}

fn ordinal(number: usize) -> String {
    match number {
        1..=9 => UNITS[number].to_owned(),
        10..=19 => TEENS[number - 10].to_owned(),
        _ if number.is_multiple_of(10) => TENS_ORDINAL[number / 10].to_owned(),
        _ => format!("{}-{}", TENS_OF_UNITS[number / 10], UNITS[number % 10]),
    }
}

fn number_before(citation: &str) -> Option<usize> {
    let lead = citation.trim_end_matches(|letter: char| {
        letter.is_whitespace() || matches!(letter, '(' | ',' | ':')
    });
    if let Some(named) = lead
        .strip_suffix("step")
        .or_else(|| lead.strip_suffix("Step"))
    {
        let word = named.trim_end().rsplit(' ').next()?;
        return (1..=step_names().len()).find(|number| ordinal(*number) == word.to_lowercase());
    }
    let digits_start = lead.rfind(|letter: char| !letter.is_ascii_digit())? + 1;
    let digits = &lead[digits_start..];
    let before_digits = lead[..digits_start].trim_end();
    let steps_number = before_digits.ends_with("step") || before_digits.ends_with("Step");
    steps_number.then(|| digits.parse().ok()).flatten()
}

fn wrong_citations(rule: &str, names: &[&str]) -> Vec<String> {
    let flat = rule.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut wrong = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let quoted = format!("`{name}`");
        for (at, _) in flat.match_indices(&quoted) {
            let from = flat.floor_char_boundary(at.saturating_sub(CITATION_REACH));
            let Some(cited) = number_before(&flat[from..at]) else {
                continue;
            };
            if cited != index + 1 {
                wrong.push(format!(
                    "{name} is step {} of the chain, cited as {cited}",
                    index + 1
                ));
            }
        }
    }
    wrong
}

#[test]
fn every_step_the_rules_number_stands_where_the_chain_puts_it() {
    let names = step_names();
    let rules = Path::new(env!("CARGO_MANIFEST_DIR")).join(RULES_FOLDER);

    let mut wrong = Vec::new();
    for entry in fs::read_dir(rules).expect("the rules folder").flatten() {
        let text = fs::read_to_string(entry.path()).expect("a readable rule");
        for citation in wrong_citations(&text, &names) {
            wrong.push(format!("{}: {citation}", entry.file_name().display()));
        }
    }

    assert!(
        wrong.is_empty(),
        "steps cited out of place:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn a_citation_is_read_by_its_digits_or_its_ordinal() {
    let names = ["V1", "FIRST_KEPT", "SECOND_KEPT"];

    assert!(wrong_citations("Step 2 (`FIRST_KEPT`) made it", &names).is_empty());
    assert!(wrong_citations("its third step, `SECOND_KEPT`", &names).is_empty());
    assert_eq!(wrong_citations("step 3 (`FIRST_KEPT`)", &names).len(), 1);
    assert_eq!(
        wrong_citations("the second step, `SECOND_KEPT`", &names).len(),
        1
    );
    assert!(wrong_citations("`FIRST_KEPT` is named with no number", &names).is_empty());
    assert_eq!(ordinal(21), "twenty-first");
    assert_eq!(ordinal(30), "thirtieth");
    assert_eq!(ordinal(12), "twelfth");
}
