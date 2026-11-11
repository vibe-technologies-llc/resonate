use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use crate::{Error, Result, prose::Comment};

const SWIFT_TOOLS_DIRECTIVE: &str = "// swift-tools-version:";
const SHEBANG: &str = "#!";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tongue {
    Rust,
    Kotlin,
    Swift,
    Python,
    Toml,
    Yaml,
    Shell,
    Markup,
}

impl Tongue {
    pub fn of(path: &Path) -> Option<Self> {
        if path.file_name().and_then(OsStr::to_str) == Some("PKGBUILD") {
            return Some(Self::Shell);
        }
        match path.extension().and_then(OsStr::to_str)? {
            "rs" => Some(Self::Rust),
            "kt" | "kts" => Some(Self::Kotlin),
            "swift" => Some(Self::Swift),
            "py" => Some(Self::Python),
            "toml" => Some(Self::Toml),
            "yml" | "yaml" => Some(Self::Yaml),
            "sh" | "ps1" | "spec" => Some(Self::Shell),
            "xml" | "plist" => Some(Self::Markup),
            _ => None,
        }
    }
}

pub fn comments_of(source: &str, tongue: Tongue) -> Vec<Comment> {
    let normal = source.replace("\r\n", "\n");
    match tongue {
        Tongue::Rust => crate::prose::lexed(&normal).comments,
        Tongue::Kotlin => Slashed::kotlin().comments(&normal),
        Tongue::Swift => Slashed::swift().comments(&normal),
        Tongue::Python => hashed_across_lines(&normal, true),
        Tongue::Toml => hashed_across_lines(&normal, false),
        Tongue::Shell => hashed_by_line(&normal, true),
        Tongue::Yaml => hashed_by_line(&normal, false),
        Tongue::Markup => marked_up(&normal),
    }
}

pub fn comments_in(roots: &[PathBuf], skipped: &[&str]) -> Result<BTreeMap<PathBuf, Vec<Comment>>> {
    let mut found = BTreeMap::new();
    let mut waiting: Vec<PathBuf> = roots.iter().rev().cloned().collect();
    while let Some(path) = waiting.pop() {
        if path.is_dir() {
            let named = |entry: &PathBuf| {
                entry
                    .file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|name| skipped.contains(&name))
            };
            let mut inside: Vec<PathBuf> = crate::listed(&path)?
                .into_iter()
                .filter(|entry| !named(entry))
                .collect();
            inside.reverse();
            waiting.extend(inside);
            continue;
        }
        let Some(tongue) = Tongue::of(&path) else {
            continue;
        };
        let source = fs::read_to_string(&path).map_err(|source| Error::Unreadable {
            path: path.clone(),
            source,
        })?;
        let comments = comments_of(&source, tongue);
        if !comments.is_empty() {
            found.insert(path, comments);
        }
    }
    Ok(found)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Kotlin,
    Swift,
}

enum Inside {
    Code { depth: usize },
    Text { hashes: usize, long: bool },
}

struct Slashed {
    dialect: Dialect,
}

impl Slashed {
    fn kotlin() -> Self {
        Self {
            dialect: Dialect::Kotlin,
        }
    }

    fn swift() -> Self {
        Self {
            dialect: Dialect::Swift,
        }
    }

    fn comments(&self, source: &str) -> Vec<Comment> {
        let letters: Vec<char> = source.chars().collect();
        let mut comments = Vec::new();
        let mut stack = vec![Inside::Code { depth: 0 }];
        let mut line = 0;
        let mut at = 0;

        if self.dialect == Dialect::Swift && source.starts_with(SWIFT_TOOLS_DIRECTIVE) {
            at = self.line_end(&letters, at);
        }

        while at < letters.len() {
            let interpolated = stack.len() > 1;
            match stack.last_mut() {
                Some(Inside::Text { hashes, long }) => {
                    let (hashes, long) = (*hashes, *long);
                    match self.in_text(&letters, at, hashes, long) {
                        Step::Skip(letters_passed) => {
                            line += self.newlines(&letters[at..at + letters_passed]);
                            at += letters_passed;
                        }
                        Step::Interpolate(letters_passed) => {
                            stack.push(Inside::Code { depth: 0 });
                            at += letters_passed;
                        }
                        Step::Close(letters_passed) => {
                            stack.pop();
                            at += letters_passed;
                        }
                    }
                }
                Some(Inside::Code { depth }) => {
                    let letter = letters[at];
                    let next = letters.get(at + 1).copied();
                    match letter {
                        '\n' => {
                            line += 1;
                            at += 1;
                        }
                        '/' if next == Some('/') => {
                            let end = self.line_end(&letters, at);
                            comments.push(Comment::at(line, &letters[at..end]));
                            at = end;
                        }
                        '/' if next == Some('*') => {
                            let end = self.block_end(&letters, at);
                            comments.push(Comment::at(line, &letters[at..end]));
                            line += self.newlines(&letters[at..end]);
                            at = end;
                        }
                        '"' => {
                            let long = letters.get(at + 1) == Some(&'"')
                                && letters.get(at + 2) == Some(&'"');
                            stack.push(Inside::Text { hashes: 0, long });
                            at += if long { 3 } else { 1 };
                        }
                        '#' if self.dialect == Dialect::Swift => {
                            let hashes = self.hashes_from(&letters, at);
                            if letters.get(at + hashes) == Some(&'"') {
                                let long = letters.get(at + hashes + 1) == Some(&'"')
                                    && letters.get(at + hashes + 2) == Some(&'"');
                                stack.push(Inside::Text { hashes, long });
                                at += hashes + if long { 3 } else { 1 };
                            } else {
                                at += hashes;
                            }
                        }
                        '\'' if self.dialect == Dialect::Kotlin => {
                            at = self.character_end(&letters, at);
                        }
                        '(' if self.dialect == Dialect::Swift => {
                            *depth += 1;
                            at += 1;
                        }
                        '{' if self.dialect == Dialect::Kotlin => {
                            *depth += 1;
                            at += 1;
                        }
                        ')' if self.dialect == Dialect::Swift => {
                            if *depth == 0 && interpolated {
                                stack.pop();
                            } else {
                                *depth = depth.saturating_sub(1);
                            }
                            at += 1;
                        }
                        '}' if self.dialect == Dialect::Kotlin => {
                            if *depth == 0 && interpolated {
                                stack.pop();
                            } else {
                                *depth = depth.saturating_sub(1);
                            }
                            at += 1;
                        }
                        _ => at += 1,
                    }
                }
                None => break,
            }
        }
        comments
    }

    fn in_text(&self, letters: &[char], at: usize, hashes: usize, long: bool) -> Step {
        let letter = letters[at];
        let quotes = if long { 3 } else { 1 };
        if letter == '"'
            && (0..quotes).all(|offset| letters.get(at + offset) == Some(&'"'))
            && (0..hashes).all(|offset| letters.get(at + quotes + offset) == Some(&'#'))
        {
            let run_goes_on = long && hashes == 0 && letters.get(at + quotes) == Some(&'"');
            return if run_goes_on {
                Step::Skip(1)
            } else {
                Step::Close(quotes + hashes)
            };
        }
        match self.dialect {
            Dialect::Kotlin => {
                if letter == '\\' && !long {
                    return Step::Skip(2.min(letters.len() - at));
                }
                if letter == '$' && letters.get(at + 1) == Some(&'{') {
                    return Step::Interpolate(2);
                }
            }
            Dialect::Swift => {
                if letter == '\\'
                    && (1..=hashes).all(|offset| letters.get(at + offset) == Some(&'#'))
                {
                    let after = at + 1 + hashes;
                    return match letters.get(after) {
                        Some('(') => Step::Interpolate(hashes + 2),
                        Some(_) => Step::Skip(hashes + 2),
                        None => Step::Skip(hashes + 1),
                    };
                }
            }
        }
        Step::Skip(1)
    }

    fn line_end(&self, letters: &[char], from: usize) -> usize {
        letters[from..]
            .iter()
            .position(|letter| *letter == '\n')
            .map_or(letters.len(), |offset| from + offset)
    }

    fn block_end(&self, letters: &[char], from: usize) -> usize {
        let mut depth = 0usize;
        let mut at = from;
        while at < letters.len() {
            if letters[at] == '/' && letters.get(at + 1) == Some(&'*') {
                depth += 1;
                at += 2;
            } else if letters[at] == '*' && letters.get(at + 1) == Some(&'/') {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    return at;
                }
            } else {
                at += 1;
            }
        }
        letters.len()
    }

    fn newlines(&self, passed: &[char]) -> usize {
        passed.iter().filter(|letter| **letter == '\n').count()
    }

    fn hashes_from(&self, letters: &[char], from: usize) -> usize {
        letters[from..]
            .iter()
            .take_while(|letter| **letter == '#')
            .count()
    }

    fn character_end(&self, letters: &[char], at: usize) -> usize {
        match (letters.get(at + 1), letters.get(at + 2)) {
            (Some('\\'), _) => {
                let mut end = at + 3;
                while end < letters.len() && letters[end] != '\'' && letters[end] != '\n' {
                    end += 1;
                }
                end + 1
            }
            (Some(_), Some('\'')) => at + 3,
            _ => at + 1,
        }
    }
}

enum Step {
    Skip(usize),
    Interpolate(usize),
    Close(usize),
}

fn marked_up(source: &str) -> Vec<Comment> {
    let letters: Vec<char> = source.chars().collect();
    let mut comments = Vec::new();
    let mut line = 0;
    let mut at = 0;
    while at < letters.len() {
        if letters[at] == '\n' {
            line += 1;
        }
        if letters[at..].starts_with(&['<', '!', '-', '-']) {
            let from = at;
            let started = line;
            while at < letters.len() && !letters[at..].starts_with(&['-', '-', '>']) {
                if letters[at] == '\n' {
                    line += 1;
                }
                at += 1;
            }
            comments.push(Comment::at(started, &letters[from..at.min(letters.len())]));
            at += 3;
            continue;
        }
        at += 1;
    }
    comments
}

fn hashed_across_lines(source: &str, shebang_allowed: bool) -> Vec<Comment> {
    let letters: Vec<char> = source.chars().collect();
    let mut comments = Vec::new();
    let mut line = 0;
    let mut at = 0;

    while at < letters.len() {
        let letter = letters[at];
        match letter {
            '\n' => {
                line += 1;
                at += 1;
            }
            '#' => {
                let shebang = shebang_allowed && at == 0 && source.starts_with(SHEBANG);
                let from = at;
                while at < letters.len() && letters[at] != '\n' {
                    at += 1;
                }
                if !shebang {
                    comments.push(Comment::at(line, &letters[from..at]));
                }
            }
            '"' | '\'' => {
                let long =
                    letters.get(at + 1) == Some(&letter) && letters.get(at + 2) == Some(&letter);
                let quotes = if long { 3 } else { 1 };
                at += quotes;
                while at < letters.len() {
                    if letters[at] == '\\' && letter == '"' || letters[at] == '\\' && !long {
                        if letters.get(at + 1) == Some(&'\n') {
                            line += 1;
                        }
                        at += 2;
                        continue;
                    }
                    if letters[at] == '\n' {
                        if !long {
                            break;
                        }
                        line += 1;
                    }
                    if (0..quotes).all(|offset| letters.get(at + offset) == Some(&letter)) {
                        at += quotes;
                        break;
                    }
                    at += 1;
                }
            }
            _ => at += 1,
        }
    }
    comments
}

fn hashed_by_line(source: &str, shebang_allowed: bool) -> Vec<Comment> {
    let mut comments = Vec::new();
    for (line, text) in source.lines().enumerate() {
        if shebang_allowed && line == 0 && text.starts_with(SHEBANG) {
            continue;
        }
        let letters: Vec<char> = text.chars().collect();
        let mut quote: Option<char> = None;
        for (at, letter) in letters.iter().enumerate() {
            match (quote, *letter) {
                (Some(open), letter) if letter == open => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(*letter),
                (None, '#') if at == 0 || letters[at - 1].is_whitespace() => {
                    comments.push(Comment::at(line, &letters[at..]));
                    break;
                }
                (None, _) => {}
            }
        }
    }
    comments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &str, tongue: Tongue) -> Vec<usize> {
        comments_of(source, tongue)
            .into_iter()
            .map(|comment| comment.line)
            .collect()
    }

    #[test]
    fn rust_comments_are_found_outside_literals_alone() {
        let source = r##"fn a() {
    let url = "https://example.org/a";
    let raw = r#"a // b and /* c */ "quoted""#;
    let slash = '/';
    let quote = '"';
    let apostrophe = '\'';
    let lifetime: &'static str = "x";
    let bytes = b"// no";
    // one
    /// two
    let after = 1; // three
    /* four /* nested */ still */
    let last = 2;
}
//! five
"##;

        assert_eq!(lines(source, Tongue::Rust), [9, 10, 11, 12, 15]);
    }

    #[test]
    fn a_block_comment_names_the_line_it_starts_on() {
        assert_eq!(
            lines("fn a() {}\n\n/*\n x\n*/\nfn b() {}\n", Tongue::Rust),
            [3]
        );
    }

    #[test]
    fn kotlin_comments_are_found_outside_strings_and_templates_alone() {
        let source = r#"val ends = Regex("""^v = "([^"]+)"""", Opt)
val url = "https://example.org/$name"
val template = "a ${b("// not")} c"
val raw = """
    // not a comment
    "quoted" /* nor this */
"""
val slash = '/'
val quote = '"'
val apostrophe = '\''
// one
val after = 1 // two
/** three /* nested */ */
"#;

        assert_eq!(lines(source, Tongue::Kotlin), [11, 12, 13]);
    }

    #[test]
    fn swift_comments_are_found_outside_strings_and_interpolations_alone() {
        let source = r##"// swift-tools-version: 6.2
let url = "https://example.org"
let shown = "a \(b("// not")) c"
let raw = #"a "quoted" // not \#(x)"#
let long = """
    // not
    """
// one
let after = 1 // two
/* three /* nested */ */
"##;

        assert_eq!(lines(source, Tongue::Swift), [8, 9, 10]);
    }

    #[test]
    fn python_and_toml_comments_are_found_outside_strings_alone() {
        let source = "#!/usr/bin/env python3\nurl = \"https://x/#frag\"\nplain = 'it # is'\nlong = \"\"\"\n# not\n\"\"\"\nvalue = 1  # one\n# two\nescaped = \"a \\\" # b\"\n";

        assert_eq!(lines(source, Tongue::Python), [7, 8]);
        assert_eq!(lines(source, Tongue::Toml), [1, 7, 8]);
    }

    #[test]
    fn yaml_and_shell_comments_follow_whitespace_outside_quotes() {
        let source = "#!/bin/sh\nrun: echo \"$#\" '#x' ${#list} a#b\n# one\nrun: echo ok # two\n";

        assert_eq!(lines(source, Tongue::Shell), [3, 4]);
        assert_eq!(lines(source, Tongue::Yaml), [1, 3, 4]);
    }

    #[test]
    fn markup_comments_are_found_across_lines() {
        let source =
            "<?xml version=\"1.0\"?>\n<a>\n<!-- one\n two -->\n<b/>\n<!-- three -->\n</a>\n";

        assert_eq!(lines(source, Tongue::Markup), [3, 6]);
    }

    #[test]
    fn the_swift_tools_directive_is_no_comment_but_only_on_the_first_line() {
        assert!(lines("// swift-tools-version: 6.2\nlet a = 1\n", Tongue::Swift).is_empty());
        assert_eq!(
            lines("let a = 1\n// swift-tools-version: 6.2\n", Tongue::Swift),
            [2]
        );
    }
}
