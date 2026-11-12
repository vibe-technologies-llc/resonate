use std::{fs, path::Path};

use resonate_words::Language;

const DESKTOP_ENTRY: &str = "resonate.desktop";
const METAINFO: &str = "org.resonate.Resonate.metainfo.xml";

fn packaging(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} reads", path.display()))
}

fn keyed(key: &str, language: Language) -> String {
    if language == Language::English {
        format!("{key}=")
    } else {
        format!("{key}[{}]=", language.tag())
    }
}

fn desktop_value<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    entry.lines().find_map(|line| line.strip_prefix(key))
}

fn tagged(element: &str, language: Language) -> String {
    if language == Language::English {
        format!("<{element}>")
    } else {
        format!("<{element} xml:lang=\"{}\">", language.tag())
    }
}

fn elements(text: &str, opening: &str, element: &str) -> Vec<String> {
    let closing = format!("</{element}>");
    text.split(opening)
        .skip(1)
        .filter_map(|after| after.split_once(closing.as_str()))
        .map(|(inside, _)| inside.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn the_desktop_entry_and_the_metainfo_say_what_the_catalog_says_in_every_language() {
    let entry = packaging(DESKTOP_ENTRY);
    let metainfo = packaging(METAINFO);

    for language in Language::ALL {
        resonate_words::speak(language);
        let said = resonate_cli::presented();

        assert_eq!(
            desktop_value(&entry, &keyed("GenericName", language)),
            Some(said.generic_name),
            "{language}"
        );
        assert_eq!(
            desktop_value(&entry, &keyed("Comment", language)),
            Some(said.summary),
            "{language}"
        );
        assert_eq!(
            desktop_value(&entry, &keyed("Keywords", language)),
            Some(said.keywords),
            "{language}"
        );
        assert_eq!(
            elements(&metainfo, &tagged("summary", language), "summary"),
            [said.summary],
            "{language}"
        );
        assert_eq!(
            elements(&metainfo, &tagged("p", language), "p"),
            said.description,
            "{language}"
        );
    }
}
