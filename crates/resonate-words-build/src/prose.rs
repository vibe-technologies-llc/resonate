use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{Error, Result};

const SAID_FOR_THE_LOG: [&str; 19] = [
    "trace!(",
    "debug!(",
    "info!(",
    "warn!(",
    "error!(",
    "panic!(",
    "unreachable!(",
    "todo!(",
    "unimplemented!(",
    "assert!(",
    "assert_eq!(",
    "assert_ne!(",
    "debug_assert!(",
    "debug_assert_eq!(",
    "debug_assert_ne!(",
    ".expect(",
    ".expect_err(",
    "tracing::",
    "#[",
];

const ONLY_UNDER_TEST: [&str; 2] = ["#[cfg(test)]", "#[cfg(all(test"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub line: usize,
    pub text: String,
}

const COMMENT_SHOWN_FOR: usize = 60;

impl Comment {
    pub(crate) fn at(line: usize, letters: &[char]) -> Self {
        let first: String = letters
            .iter()
            .take_while(|letter| **letter != '\n')
            .take(COMMENT_SHOWN_FOR)
            .collect();
        Self {
            line: line + 1,
            text: first,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prose {
    pub line: usize,
    pub text: String,
}

pub fn prose_in(root: &Path, proper: &[&str]) -> Result<BTreeMap<PathBuf, Vec<Prose>>> {
    let mut found = BTreeMap::new();
    let mut waiting = vec![root.to_owned()];
    while let Some(folder) = waiting.pop() {
        for path in crate::listed(&folder)? {
            if path.is_dir() {
                waiting.push(path);
                continue;
            }
            if path.extension().and_then(|ending| ending.to_str()) != Some("rs") {
                continue;
            }
            let source = fs::read_to_string(&path).map_err(|source| Error::Unreadable {
                path: path.clone(),
                source,
            })?;
            let prose: Vec<Prose> = prose_of(&source)
                .into_iter()
                .filter(|said| !proper.contains(&said.text.as_str()))
                .collect();
            if !prose.is_empty() {
                let named = path.strip_prefix(root).unwrap_or(&path).to_owned();
                found.insert(named, prose);
            }
        }
    }
    Ok(found)
}

pub fn prose_of(source: &str) -> Vec<Prose> {
    let lexed = lexed(source);
    let unread = lines_unread(&lexed.code);

    lexed
        .literals
        .into_iter()
        .filter(|literal| !unread.get(literal.line).copied().unwrap_or(false))
        .filter(|literal| reads_as_prose(&literal.text))
        .map(|literal| Prose {
            line: literal.line + 1,
            text: literal.text,
        })
        .collect()
}

pub fn reads_as_prose(text: &str) -> bool {
    let bare = without_placeholders(text);
    let words: Vec<&str> = bare
        .split(|letter: char| !letter.is_alphabetic() && letter != '\'')
        .filter(|word| word.chars().filter(|letter| letter.is_alphabetic()).count() >= 2)
        .collect();
    let capitalised = words.iter().any(|word| {
        let mut letters = word.chars().filter(|letter| letter.is_alphabetic());
        matches!(
            (letters.next(), letters.next()),
            (Some(first), Some(second)) if first.is_uppercase() && second.is_lowercase()
        )
    });
    let spaced = bare.chars().any(char::is_whitespace) && !words.is_empty();

    capitalised || spaced
}

fn without_placeholders(text: &str) -> String {
    let mut bare = String::with_capacity(text.len());
    let mut depth = 0usize;
    let mut letters = text.chars().peekable();
    while let Some(letter) = letters.next() {
        match letter {
            '{' if letters.peek() == Some(&'{') => {
                letters.next();
                bare.push('{');
            }
            '}' if depth == 0 && letters.peek() == Some(&'}') => {
                letters.next();
                bare.push('}');
            }
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            '\\' if depth == 0 => {
                if letters.next() == Some('n') {
                    bare.push(' ');
                }
            }
            letter if depth == 0 => bare.push(letter),
            _ => {}
        }
    }
    bare
}

struct Literal {
    line: usize,
    text: String,
}

pub(crate) struct Lexed {
    code: Vec<String>,
    literals: Vec<Literal>,
    pub(crate) comments: Vec<Comment>,
}

pub(crate) fn lexed(source: &str) -> Lexed {
    let letters: Vec<char> = source.replace("\r\n", "\n").chars().collect();
    let mut code = String::with_capacity(source.len());
    let mut literals = Vec::new();
    let mut comments = Vec::new();
    let mut line = 0;
    let mut at = 0;

    while at < letters.len() {
        let letter = letters[at];
        let next = letters.get(at + 1).copied();
        let before_is_word = code
            .chars()
            .last()
            .is_some_and(|before| before.is_alphanumeric() || before == '_');

        match letter {
            '/' if next == Some('/') => {
                let from = at;
                while at < letters.len() && letters[at] != '\n' {
                    at += 1;
                }
                comments.push(Comment::at(line, &letters[from..at]));
            }
            '/' if next == Some('*') => {
                let from = at;
                let started = line;
                let mut depth = 0usize;
                while at < letters.len() {
                    if letters[at] == '/' && letters.get(at + 1) == Some(&'*') {
                        depth += 1;
                        at += 2;
                    } else if letters[at] == '*' && letters.get(at + 1) == Some(&'/') {
                        depth -= 1;
                        at += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        if letters[at] == '\n' {
                            line += 1;
                            code.push('\n');
                        }
                        at += 1;
                    }
                }
                comments.push(Comment::at(started, &letters[from..at.min(letters.len())]));
            }
            '"' => {
                let (text, end, lines) = quoted(&letters, at + 1);
                literals.push(Literal { line, text });
                code.push_str("\"\"");
                push_lines(&mut code, lines);
                line += lines;
                at = end;
            }
            'r' if !before_is_word && matches!(next, Some('"' | '#')) => {
                match raw(&letters, at + 1) {
                    Some((text, end, lines)) => {
                        literals.push(Literal { line, text });
                        code.push_str("\"\"");
                        push_lines(&mut code, lines);
                        line += lines;
                        at = end;
                    }
                    None => {
                        code.push(letter);
                        at += 1;
                    }
                }
            }
            'b' if !before_is_word && matches!(next, Some('"' | 'r' | '\'')) => {
                let end = match next {
                    Some('"') => quoted(&letters, at + 2),
                    Some('r') => raw(&letters, at + 2).unwrap_or((String::new(), at + 2, 0)),
                    _ => (String::new(), character_end(&letters, at + 1), 0),
                };
                code.push_str("b\"\"");
                push_lines(&mut code, end.2);
                line += end.2;
                at = end.1;
            }
            '\'' => {
                let end = character_end(&letters, at);
                for skipped in &letters[at..end] {
                    code.push(match skipped {
                        '\n' => '\n',
                        _ => ' ',
                    });
                }
                at = end;
            }
            '\n' => {
                line += 1;
                code.push('\n');
                at += 1;
            }
            letter => {
                code.push(letter);
                at += 1;
            }
        }
    }

    Lexed {
        code: code.split('\n').map(str::to_owned).collect(),
        literals,
        comments,
    }
}

fn push_lines(code: &mut String, lines: usize) {
    for _ in 0..lines {
        code.push('\n');
    }
}

fn quoted(letters: &[char], mut at: usize) -> (String, usize, usize) {
    let mut text = String::new();
    let mut lines = 0;
    while at < letters.len() {
        match letters[at] {
            '\\' => {
                if let Some(escaped) = letters.get(at + 1) {
                    if *escaped == '\n' {
                        lines += 1;
                        at += 2;
                        while letters.get(at).is_some_and(|letter| letter.is_whitespace()) {
                            if letters[at] == '\n' {
                                lines += 1;
                            }
                            at += 1;
                        }
                        continue;
                    }
                    text.push('\\');
                    text.push(*escaped);
                }
                at += 2;
            }
            '"' => return (text, at + 1, lines),
            '\n' => {
                lines += 1;
                text.push('\n');
                at += 1;
            }
            letter => {
                text.push(letter);
                at += 1;
            }
        }
    }
    (text, at, lines)
}

fn raw(letters: &[char], mut at: usize) -> Option<(String, usize, usize)> {
    let mut hashes = 0;
    while letters.get(at) == Some(&'#') {
        hashes += 1;
        at += 1;
    }
    if letters.get(at) != Some(&'"') {
        return None;
    }
    at += 1;
    let mut text = String::new();
    let mut lines = 0;
    while at < letters.len() {
        if letters[at] == '"' && (1..=hashes).all(|offset| letters.get(at + offset) == Some(&'#')) {
            return Some((text, at + 1 + hashes, lines));
        }
        if letters[at] == '\n' {
            lines += 1;
        }
        text.push(letters[at]);
        at += 1;
    }
    Some((text, at, lines))
}

fn character_end(letters: &[char], at: usize) -> usize {
    match (letters.get(at + 1), letters.get(at + 2)) {
        (Some('\\'), _) => {
            let mut end = at + 3;
            while end < letters.len() && letters[end] != '\'' {
                end += 1;
            }
            end + 1
        }
        (Some(_), Some('\'')) => at + 3,
        _ => at + 1,
    }
}

fn lines_unread(code: &[String]) -> Vec<bool> {
    let mut unread = vec![false; code.len()];
    let mut line = 0;
    while line < code.len() {
        let trimmed = code[line].trim_start();
        if ONLY_UNDER_TEST
            .iter()
            .any(|attribute| trimmed.starts_with(attribute))
        {
            let end = item_end(code, line + 1);
            for skipped in unread.iter_mut().take(end + 1).skip(line) {
                *skipped = true;
            }
            line = end + 1;
            continue;
        }
        if let Some(opened) = SAID_FOR_THE_LOG
            .iter()
            .filter_map(|marker| code[line].find(marker))
            .min()
        {
            let end = closing_line(code, line, opened);
            for skipped in unread.iter_mut().take(end + 1).skip(line) {
                *skipped = true;
            }
            line = end + 1;
            continue;
        }
        line += 1;
    }
    unread
}

fn item_end(code: &[String], from: usize) -> usize {
    let mut depth = 0i64;
    let mut opened = false;
    for (line, text) in code.iter().enumerate().skip(from) {
        let trimmed = text.trim_start();
        if !opened && trimmed.starts_with("#[") {
            continue;
        }
        for letter in text.chars() {
            match letter {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                ';' if !opened && depth == 0 => return line,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return line;
        }
    }
    code.len().saturating_sub(1)
}

fn closing_line(code: &[String], from: usize, at: usize) -> usize {
    let mut depth = 0i64;
    let mut opened = false;
    for (line, text) in code.iter().enumerate().skip(from) {
        let start = if line == from { at } else { 0 };
        for letter in text.chars().skip(start) {
            match letter {
                '(' | '[' => {
                    depth += 1;
                    opened = true;
                }
                ')' | ']' => depth -= 1,
                _ => {}
            }
        }
        if !opened || depth <= 0 {
            return line;
        }
    }
    code.len().saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(source: &str) -> Vec<String> {
        prose_of(source).into_iter().map(|said| said.text).collect()
    }

    #[test]
    fn words_a_listener_reads_are_prose_and_names_for_the_code_are_not() {
        for prose in [
            "Play",
            "Took {count} tracks",
            "{} ago",
            "{landed:+.2} dB",
            "just now",
            "Bit-perfect",
        ] {
            assert!(reads_as_prose(prose), "{prose}");
        }
        for code in [
            "tab-visualiser",
            "ctrl-shift-left",
            "{}:{:02}",
            "{hours}:{:02}:{:02}",
            "FLAC",
            " · ",
            "",
            "resonate",
            "{name}",
        ] {
            assert!(!reads_as_prose(code), "{code}");
        }
    }

    #[test]
    fn a_log_a_panic_an_attribute_and_a_test_say_nothing_to_a_listener() {
        let source = r##"
fn drawn() -> &'static str {
    tracing::warn!(
        "a message for the log"
    );
    let _ = thing.expect("the thing is there");
    "Shown to the listener"
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("read for the log")]
    Gone,
}

#[cfg(test)]
fn helper() -> &'static str {
    "Only in a test"
}

const RAW: &str = r#"A raw "quoted" line"#;
const HELD: char = '"';
const MORE: &str = "Said \
                    across lines";

#[cfg(test)]
mod tests {
    #[test]
    fn it() {
        assert_eq!(drawn(), "Shown to the listener");
    }
}
"##;

        assert_eq!(
            texts(source),
            [
                "Shown to the listener",
                "A raw \"quoted\" line",
                "Said across lines"
            ]
        );
    }

    #[test]
    fn a_literal_continued_across_windows_line_endings_reads_as_one_line() {
        let found = prose_of(
            "fn a() -> &'static str {\r\n    \"[{{\\\r\n     \\\"name\\\":1}}]\"\r\n}\r\nfn b() -> &'static str {\r\n    \"Two words\"\r\n}\r\n",
        );

        assert_eq!(
            found,
            [Prose {
                line: 6,
                text: "Two words".to_owned()
            }]
        );
    }

    #[test]
    fn a_prose_literal_names_the_line_it_starts_on() {
        let found = prose_of("fn a() {}\nfn b() -> &'static str {\n    \"Two words\"\n}\n");

        assert_eq!(
            found,
            [Prose {
                line: 3,
                text: "Two words".to_owned()
            }]
        );
    }
}
