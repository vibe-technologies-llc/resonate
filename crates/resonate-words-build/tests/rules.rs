use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const RULES: &str = ".claude/rules";
const SOURCE_ROOTS: &[&str] = &[
    "crates",
    "fuzz",
    "soxr-comparison",
    "android",
    "ios",
    ".github",
];
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "kt", "kts", "swift", "py", "toml", "ftl", "xml", "gradle", "yml",
];
const NOT_SOURCE: &[&str] = &[
    "target",
    ".git",
    ".gradle",
    "build",
    ".build",
    "node_modules",
];
const OTHER_PROGRAMS_AND_PROTOCOLS: &[&str] = &[
    "accesskit_unix",
    "content_size",
    "destination_id",
    "hoverable_tooltip",
    "in_focus",
    "load_icon",
    "matrix_mixdown_idx",
    "payload_binary",
    "payload_type",
    "payload_utf8",
    "pitch_downsample",
    "pitch_search",
    "pre_exec",
    "pseudo_surround",
    "remove_doubling",
    "pw_stream_destroy",
    "pw_stream_update_properties",
    "replaces_id",
    "spa_hook_remove",
    "sqlite_stat1",
    "sun_path",
    "thread_loop",
    "web_music",
    "workflow_dispatch",
    "xkb_keysym_get_name",
];
const CRATES_AND_THEIR_FEATURES: &[&str] = &[
    "coinit_apartmentthreaded",
    "const_generics",
    "gpui_apple",
    "gpui_macos",
    "id3v2_compression_support",
    "lock_api",
    "macos_kqueue",
    "miniz_oxide",
    "parking_lot_core",
    "rust_backend",
    "rwh_06",
    "v0_3_50",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn source_files(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if NOT_SOURCE.iter().any(|skipped| name == *skipped) {
            continue;
        }
        if path.is_dir() {
            source_files(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| SOURCE_EXTENSIONS.iter().any(|known| extension == *known))
        {
            found.push(path);
        }
    }
}

fn all_source(root: &Path) -> String {
    let mut files = Vec::new();
    for folder in SOURCE_ROOTS {
        source_files(&root.join(folder), &mut files);
    }
    files
        .iter()
        .filter_map(|file| fs::read_to_string(file).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_snake_case_name(word: &str) -> bool {
    word.contains('_')
        && !word.starts_with('_')
        && !word.ends_with('_')
        && !word.contains("__")
        && word.starts_with(|first: char| first.is_ascii_lowercase())
        && word
            .chars()
            .all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '_')
}

fn function_name_of(span: &str) -> Option<&str> {
    let called = span.strip_suffix("()").unwrap_or(span);
    if !called
        .chars()
        .all(|letter| letter.is_ascii_alphanumeric() || letter == '_' || letter == ':')
    {
        return None;
    }
    let last = called.rsplit("::").next()?;
    is_snake_case_name(last).then_some(last)
}

fn written_by_others(name: &str) -> bool {
    OTHER_PROGRAMS_AND_PROTOCOLS.contains(&name) || CRATES_AND_THEIR_FEATURES.contains(&name)
}

fn names_in(rule: &str) -> BTreeSet<String> {
    rule.split('`')
        .skip(1)
        .step_by(2)
        .filter_map(function_name_of)
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_function_a_rule_names_is_written_somewhere_in_the_source() {
    let root = workspace();
    let source = all_source(&root);

    let mut missing = Vec::new();
    let mut named = BTreeSet::new();
    let rules = fs::read_dir(root.join(RULES)).expect("the rules folder");
    for entry in rules.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("a readable rule");
        for name in names_in(&text) {
            if !written_by_others(&name) && !source.contains(&name) {
                missing.push(format!("{} names {name}", entry.file_name().display()));
            }
            named.insert(name);
        }
    }

    let unnamed: Vec<&str> = OTHER_PROGRAMS_AND_PROTOCOLS
        .iter()
        .chain(CRATES_AND_THEIR_FEATURES)
        .copied()
        .filter(|name| !named.contains(*name))
        .collect();
    assert!(
        unnamed.is_empty(),
        "names set aside as other programs' that no rule names:\n{}",
        unnamed.join("\n")
    );

    assert!(
        missing.is_empty(),
        "rules name code that is gone:\n{}",
        missing.join("\n")
    );
}

#[test]
fn a_name_is_told_from_a_word() {
    let spans = [
        ("foo_bar", Some("foo_bar")),
        ("foo_bar()", Some("foo_bar")),
        ("Shelf::lists_every_item", Some("lists_every_item")),
        ("foo", None),
        ("Foo_bar", None),
        ("foo_bar.rs", None),
        ("--foo_bar", None),
        ("foo_bar(x)", None),
    ];

    for (span, expected) in spans {
        assert_eq!(function_name_of(span), expected, "{span}");
    }
}
