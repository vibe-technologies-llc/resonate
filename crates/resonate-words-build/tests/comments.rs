use std::path::{Path, PathBuf};

use resonate_words_build::comments_in;

const ROOTS: [&str; 8] = [
    "crates",
    "fuzz",
    "soxr-comparison",
    "android",
    "ios",
    "packaging",
    ".github",
    ".claude",
];
const ROOT_FILES: [&str; 3] = ["Cargo.toml", "rust-formatter.toml", "rust-toolchain.toml"];
const NOT_SOURCE: [&str; 11] = [
    "target",
    ".git",
    ".gradle",
    "build",
    ".build",
    "node_modules",
    "corpus",
    "seeds",
    "fixtures",
    "artifacts",
    "xtool",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

#[test]
fn no_source_in_the_workspace_carries_a_comment() {
    let root = workspace();
    let roots: Vec<PathBuf> = ROOTS
        .iter()
        .map(|folder| root.join(folder))
        .chain(ROOT_FILES.iter().map(|file| root.join(file)))
        .filter(|path| path.exists())
        .collect();

    let found = comments_in(&roots, &NOT_SOURCE).expect("the workspace is readable");

    let written: Vec<String> = found
        .iter()
        .flat_map(|(path, comments)| {
            let named = path.strip_prefix(&root).unwrap_or(path).to_owned();
            comments.iter().map(move |comment| {
                format!("{}:{} {}", named.display(), comment.line, comment.text)
            })
        })
        .collect();

    assert!(
        written.is_empty(),
        "comments written in the code:\n{}",
        written.join("\n")
    );
}
