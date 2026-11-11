mod comments;
mod prose;

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    ops::Range,
    path::{Path, PathBuf},
};

pub use comments::{Tongue, comments_in, comments_of};
use fluent_syntax::{
    ast::{Entry, Expression, InlineExpression, Pattern, PatternElement},
    parser::{self, ParserError},
};
pub use prose::{Comment, Prose, prose_in, prose_of, reads_as_prose};

const WORDS: &str = "words";
const ENGLISH: &str = "en";
const ENDING: &str = "ftl";
const WRITTEN_TO: &str = "words.rs";
const UNRAISABLE: [&str; 4] = ["self", "Self", "super", "crate"];
const KEYWORDS: [&str; 51] = [
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try",
    "typeof", "unsized", "virtual", "yield", "union", "raw", "safe",
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("CARGO_MANIFEST_DIR names no folder")]
    NoManifest,
    #[error("OUT_DIR names no folder")]
    NoOutput,
    #[error("{path} could not be read")]
    Unreadable {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} could not be written")]
    Unwritten {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} is not spelled in UTF-8")]
    NotUtf8 { path: PathBuf },
    #[error("{path} holds no English words")]
    NoEnglish { path: PathBuf },
    #[error("{path} is not named as an area of words")]
    UnnamedArea { path: PathBuf },
    #[error("{path} does not read as Fluent at bytes {at:?}")]
    Unparsed {
        path: PathBuf,
        at: Range<usize>,
        #[source]
        source: Box<ParserError>,
    },
    #[error("{path} carries a comment, and words carry none")]
    Commented { path: PathBuf },
    #[error("{path} names {id} twice")]
    Twice { path: PathBuf, id: String },
    #[error("{path}'s {id} carries attributes, and a message is its value alone")]
    Attributed { path: PathBuf, id: String },
    #[error("{path}'s {id} has no value")]
    Valueless { path: PathBuf, id: String },
    #[error("{path}'s {id} is no name Rust can take")]
    Unnamable { path: PathBuf, id: String },
    #[error("{path} holds {id}, which the English holds not")]
    NotInEnglish { path: PathBuf, id: String },
    #[error("{path} lacks {id}, which the English holds")]
    Missing { path: PathBuf, id: String },
    #[error("{path}'s {id} takes {found:?} where the English takes {english:?}")]
    Unlike {
        path: PathBuf,
        id: String,
        found: Vec<String>,
        english: Vec<String>,
    },
    #[error("{language} lacks the area {area}, which the English holds")]
    AreaMissing { language: PathBuf, area: String },
    #[error("{path} is an area the English holds not")]
    AreaNotInEnglish { path: PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn write() -> Result<()> {
    let manifest = env::var_os("CARGO_MANIFEST_DIR").ok_or(Error::NoManifest)?;
    let output = env::var_os("OUT_DIR").ok_or(Error::NoOutput)?;
    let words = Path::new(&manifest).join(WORDS);

    println!("cargo::rerun-if-changed={}", words.display());

    let written = generated(&words)?;
    let path = Path::new(&output).join(WRITTEN_TO);
    fs::write(&path, written).map_err(|source| Error::Unwritten { path, source })
}

pub fn generated(words: &Path) -> Result<String> {
    let languages = languages_in(words)?;
    let english = languages
        .get(ENGLISH)
        .ok_or_else(|| Error::NoEnglish {
            path: words.to_owned(),
        })?
        .clone();

    let mut areas = Vec::new();
    for (area, path) in areas_in(&english)? {
        areas.push((area, read_area(&path)?));
    }

    let mut kept_in: BTreeMap<String, Vec<(String, PathBuf)>> = BTreeMap::new();
    for (tag, folder) in &languages {
        let found = areas_in(folder)?;
        for (area, read) in &areas {
            let path = found.get(area).ok_or_else(|| Error::AreaMissing {
                language: folder.clone(),
                area: area.clone(),
            })?;
            if tag != ENGLISH {
                agrees_with(read, &read_area(path)?, path)?;
            }
            kept_in
                .entry(area.clone())
                .or_default()
                .push((tag.clone(), path.clone()));
        }
        if let Some(path) = found
            .iter()
            .find(|(area, _)| !areas.iter().any(|(held, _)| held == *area))
            .map(|(_, path)| path)
        {
            return Err(Error::AreaNotInEnglish { path: path.clone() });
        }
    }

    let mut written = String::new();
    for (area, read) in &areas {
        written.push_str(&module_of(area, read, &kept_in[area])?);
    }
    Ok(written)
}

fn languages_in(words: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut languages = BTreeMap::new();
    for entry in listed(words)? {
        if entry.is_dir()
            && let Some(tag) = entry.file_name().and_then(|name| name.to_str())
        {
            languages.insert(tag.to_owned(), entry);
        }
    }
    Ok(languages)
}

fn areas_in(folder: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut areas = BTreeMap::new();
    for path in listed(folder)? {
        if path.extension().and_then(|ending| ending.to_str()) != Some(ENDING) {
            continue;
        }
        let area = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| names_an_area(stem))
            .ok_or_else(|| Error::UnnamedArea { path: path.clone() })?;
        areas.insert(area.to_owned(), path);
    }
    Ok(areas)
}

fn names_an_area(stem: &str) -> bool {
    stem.starts_with(|letter: char| letter.is_ascii_lowercase())
        && stem
            .chars()
            .all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '_')
        && !KEYWORDS.contains(&stem)
        && !UNRAISABLE.contains(&stem)
}

fn listed(folder: &Path) -> Result<Vec<PathBuf>> {
    let unreadable = |source| Error::Unreadable {
        path: folder.to_owned(),
        source,
    };
    let mut listed = Vec::new();
    for entry in fs::read_dir(folder).map_err(unreadable)? {
        listed.push(entry.map_err(unreadable)?.path());
    }
    listed.sort();
    Ok(listed)
}

#[derive(Debug)]
struct Message {
    id: String,
    takes: BTreeSet<String>,
}

fn read_area(path: &Path) -> Result<Vec<Message>> {
    let text = fs::read_to_string(path).map_err(|source| Error::Unreadable {
        path: path.to_owned(),
        source,
    })?;
    let resource = parser::parse(text.as_str()).map_err(|(_, mut errors)| {
        let first = errors.remove(0);
        Error::Unparsed {
            path: path.to_owned(),
            at: first.pos.clone(),
            source: Box::new(first),
        }
    })?;

    let mut own: Vec<(String, BTreeSet<String>, BTreeSet<String>)> = Vec::new();
    for entry in &resource.body {
        match entry {
            Entry::Message(message) => {
                let id = message.id.name.to_owned();
                if message.comment.is_some() {
                    return Err(Error::Commented {
                        path: path.to_owned(),
                    });
                }
                if !message.attributes.is_empty() {
                    return Err(Error::Attributed {
                        path: path.to_owned(),
                        id,
                    });
                }
                if own.iter().any(|(held, ..)| *held == id) {
                    return Err(Error::Twice {
                        path: path.to_owned(),
                        id,
                    });
                }
                if !names_a_function(&id) {
                    return Err(Error::Unnamable {
                        path: path.to_owned(),
                        id,
                    });
                }
                let value = message.value.as_ref().ok_or_else(|| Error::Valueless {
                    path: path.to_owned(),
                    id: id.clone(),
                })?;

                let mut takes = BTreeSet::new();
                let mut refers_to = BTreeSet::new();
                walk_pattern(value, &mut takes, &mut refers_to);
                for variable in &takes {
                    if !names_a_function(variable) {
                        return Err(Error::Unnamable {
                            path: path.to_owned(),
                            id: variable.clone(),
                        });
                    }
                }
                own.push((id, takes, refers_to));
            }
            Entry::Term(term) => {
                if term.comment.is_some() {
                    return Err(Error::Commented {
                        path: path.to_owned(),
                    });
                }
            }
            Entry::Comment(_) | Entry::GroupComment(_) | Entry::ResourceComment(_) => {
                return Err(Error::Commented {
                    path: path.to_owned(),
                });
            }
            Entry::Junk { .. } => {}
        }
    }

    Ok(own
        .iter()
        .map(|(id, ..)| Message {
            id: id.clone(),
            takes: taken_through(id, &own),
        })
        .collect())
}

fn taken_through(
    id: &str,
    own: &[(String, BTreeSet<String>, BTreeSet<String>)],
) -> BTreeSet<String> {
    let mut takes = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut waiting = vec![id.to_owned()];
    while let Some(next) = waiting.pop() {
        if !seen.insert(next.clone()) {
            continue;
        }
        if let Some((_, variables, refers_to)) = own.iter().find(|(held, ..)| *held == next) {
            takes.extend(variables.iter().cloned());
            waiting.extend(refers_to.iter().cloned());
        }
    }
    takes
}

fn walk_pattern(
    pattern: &Pattern<&str>,
    takes: &mut BTreeSet<String>,
    refers_to: &mut BTreeSet<String>,
) {
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            walk_expression(expression, takes, refers_to);
        }
    }
}

fn walk_expression(
    expression: &Expression<&str>,
    takes: &mut BTreeSet<String>,
    refers_to: &mut BTreeSet<String>,
) {
    match expression {
        Expression::Select { selector, variants } => {
            walk_inline(selector, takes, refers_to);
            for variant in variants {
                walk_pattern(&variant.value, takes, refers_to);
            }
        }
        Expression::Inline(inline) => walk_inline(inline, takes, refers_to),
    }
}

fn walk_inline(
    inline: &InlineExpression<&str>,
    takes: &mut BTreeSet<String>,
    refers_to: &mut BTreeSet<String>,
) {
    match inline {
        InlineExpression::VariableReference { id } => {
            takes.insert(id.name.to_owned());
        }
        InlineExpression::MessageReference { id, .. } => {
            refers_to.insert(id.name.to_owned());
        }
        InlineExpression::FunctionReference { arguments, .. } => {
            for positional in &arguments.positional {
                walk_inline(positional, takes, refers_to);
            }
            for named in &arguments.named {
                walk_inline(&named.value, takes, refers_to);
            }
        }
        InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            for positional in &arguments.positional {
                walk_inline(positional, takes, refers_to);
            }
            for named in &arguments.named {
                walk_inline(&named.value, takes, refers_to);
            }
        }
        InlineExpression::Placeable { expression } => walk_expression(expression, takes, refers_to),
        InlineExpression::TermReference {
            arguments: None, ..
        }
        | InlineExpression::StringLiteral { .. }
        | InlineExpression::NumberLiteral { .. } => {}
    }
}

fn agrees_with(english: &[Message], other: &[Message], path: &Path) -> Result<()> {
    if let Some(stray) = other
        .iter()
        .find(|message| !english.iter().any(|held| held.id == message.id))
    {
        return Err(Error::NotInEnglish {
            path: path.to_owned(),
            id: stray.id.clone(),
        });
    }
    for held in english {
        let found = other
            .iter()
            .find(|message| message.id == held.id)
            .ok_or_else(|| Error::Missing {
                path: path.to_owned(),
                id: held.id.clone(),
            })?;
        if found.takes != held.takes {
            return Err(Error::Unlike {
                path: path.to_owned(),
                id: held.id.clone(),
                found: found.takes.iter().cloned().collect(),
                english: held.takes.iter().cloned().collect(),
            });
        }
    }
    Ok(())
}

fn names_a_function(id: &str) -> bool {
    let named = rust_name(id);
    !UNRAISABLE.contains(&named.as_str())
        && named.starts_with(|letter: char| letter.is_ascii_alphabetic())
}

fn rust_name(id: &str) -> String {
    let named: String = id
        .chars()
        .map(|letter| match letter {
            '-' => '_',
            letter => letter.to_ascii_lowercase(),
        })
        .collect();
    named
}

fn raised(id: &str) -> String {
    let named = rust_name(id);
    match KEYWORDS.contains(&named.as_str()) {
        true => format!("r#{named}"),
        false => named,
    }
}

fn spelled(path: &Path) -> Result<String> {
    path.to_str()
        .map(|text| format!("{text:?}"))
        .ok_or_else(|| Error::NotUtf8 {
            path: path.to_owned(),
        })
}

fn module_of(area: &str, messages: &[Message], kept_in: &[(String, PathBuf)]) -> Result<String> {
    let ids: Vec<String> = messages
        .iter()
        .map(|message| format!("{:?}", message.id))
        .collect();
    let mut sources = Vec::new();
    for (tag, path) in kept_in {
        sources.push(format!(
            "(::resonate_words::Language::by_tag({tag:?}), include_str!({}))",
            spelled(path)?
        ));
    }

    let mut written = format!(
        "pub(crate) mod {area} {{\n    static AREA: ::resonate_words::Area = \
         ::resonate_words::Area::new(&[{}], &[{}]);\n",
        ids.join(", "),
        sources.join(", ")
    );
    for (index, message) in messages.iter().enumerate() {
        let name = raised(&message.id);
        match message.takes.is_empty() {
            true => written.push_str(&format!(
                "\n    pub(crate) fn {name}() -> &'static str {{\n        AREA.plain({index})\n    \
                 }}\n"
            )),
            false => {
                let parameters: Vec<String> = message
                    .takes
                    .iter()
                    .map(|variable| {
                        format!("{}: impl Into<::resonate_words::Arg<'a>>", raised(variable))
                    })
                    .collect();
                let arguments: Vec<String> = message
                    .takes
                    .iter()
                    .map(|variable| format!("({variable:?}, {}.into())", raised(variable)))
                    .collect();
                written.push_str(&format!(
                    "\n    pub(crate) fn {name}<'a>({}) -> String {{\n        AREA.formatted({index}, \
                     [{}])\n    }}\n",
                    parameters.join(", "),
                    arguments.join(", ")
                ));
            }
        }
    }
    written.push_str("}\n");
    Ok(written)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path =
                env::temp_dir().join(format!("resonate-words-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a temporary folder");
            Self(path)
        }

        fn holding(self, language: &str, area: &str, text: &str) -> Self {
            let folder = self.0.join(language);
            fs::create_dir_all(&folder).expect("a language folder");
            fs::write(folder.join(format!("{area}.ftl")), text).expect("an area written");
            self
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }

    #[test]
    fn a_plain_message_is_a_static_word_and_one_with_variables_takes_them() {
        let words = Folder::new("plain").holding(
            "en",
            "queue",
            "just-now = just now\ntook-out = { $count ->\n    [one] Took one track\n   *[other] \
             Took { $count } tracks\n}\nnamed = { took-out } from { $place }\n",
        );

        let written = generated(&words.0).expect("the words generate");

        assert!(written.contains("pub(crate) mod queue {"));
        assert!(written.contains("pub(crate) fn just_now() -> &'static str"));
        assert!(written.contains("pub(crate) fn took_out<'a>(count: impl Into<"));
        assert!(written.contains(
            "pub(crate) fn named<'a>(count: impl Into<::resonate_words::Arg<'a>>, place: impl Into<"
        ));
    }

    #[test]
    fn a_keyword_is_raised_and_a_comment_is_refused() {
        let raised = Folder::new("raised").holding("en", "sorting", "type = Type\n");
        let commented =
            Folder::new("commented").holding("en", "sorting", "# a note\ntype = Type\n");

        assert!(
            generated(&raised.0)
                .expect("a keyword is raised")
                .contains("fn r#type()")
        );
        assert!(matches!(
            generated(&commented.0),
            Err(Error::Commented { .. })
        ));
    }

    #[test]
    fn a_second_language_must_hold_every_message_with_the_same_variables() {
        let english = "took-out = Took { $count } tracks\nplay = Play\n";
        let agreeing = Folder::new("agreeing")
            .holding("en", "queue", english)
            .holding(
                "de",
                "queue",
                "took-out = { $count } Titel entfernt\nplay = Abspielen\n",
            );
        let lacking = Folder::new("lacking")
            .holding("en", "queue", english)
            .holding("de", "queue", "took-out = { $count } Titel entfernt\n");
        let unlike = Folder::new("unlike")
            .holding("en", "queue", english)
            .holding(
                "de",
                "queue",
                "took-out = Titel entfernt\nplay = Abspielen\n",
            );
        let stray = Folder::new("stray")
            .holding("en", "queue", english)
            .holding(
                "de",
                "queue",
                "took-out = { $count } Titel\nplay = Abspielen\nmore = Mehr\n",
            );
        let no_area = Folder::new("no-area")
            .holding("en", "queue", english)
            .holding("de", "other", "play = Abspielen\n");

        let written = generated(&agreeing.0).expect("an agreeing language generates");

        assert!(written.contains("Language::by_tag(\"de\")"));
        assert!(written.contains("Language::by_tag(\"en\")"));
        assert!(matches!(generated(&lacking.0), Err(Error::Missing { id, .. }) if id == "play"));
        assert!(matches!(generated(&unlike.0), Err(Error::Unlike { id, .. }) if id == "took-out"));
        assert!(matches!(generated(&stray.0), Err(Error::NotInEnglish { id, .. }) if id == "more"));
        assert!(matches!(
            generated(&no_area.0),
            Err(Error::AreaMissing { .. })
        ));
    }

    #[test]
    fn no_english_and_a_twice_named_message_are_refused() {
        let foreign = Folder::new("foreign").holding("de", "queue", "play = Abspielen\n");
        let twice = Folder::new("twice").holding("en", "queue", "play = Play\nplay = Again\n");
        let broken = Folder::new("broken").holding("en", "queue", "play = { $\n");

        assert!(matches!(
            generated(&foreign.0),
            Err(Error::NoEnglish { .. })
        ));
        assert!(matches!(generated(&twice.0), Err(Error::Twice { .. })));
        assert!(matches!(generated(&broken.0), Err(Error::Unparsed { .. })));
    }
}
