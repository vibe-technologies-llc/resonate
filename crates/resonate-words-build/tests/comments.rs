use std::path::{Path, PathBuf};

use resonate_words_build::comments_in;

const ROOTS: [&str; 10] = [
    "crates",
    "fuzz",
    "dsp-lab",
    "android",
    "ios",
    "packaging",
    ".github",
    ".claude",
    ".cargo",
    ".config",
];
const MAKEPKG_LEAVES: [&str; 4] = ["src", "pkg", "resonate", "packaging"];
const ROOT_FILES: [&str; 3] = ["Cargo.toml", "rust-formatter.toml", "rust-toolchain.toml"];
const NOT_SOURCE: [&str; 13] = [
    "target",
    ".cargo-target",
    "Generated",
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

fn roots_under(root: &Path, folder: &str) -> Vec<PathBuf> {
    let path = root.join(folder);

    if folder != "packaging" {
        return vec![path];
    }

    let Ok(entries) = std::fs::read_dir(&path) else {
        return Vec::new();
    };
    let mut kept: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|entry| {
            let name = entry.file_name().and_then(|name| name.to_str());
            !name.is_some_and(|name| MAKEPKG_LEAVES.contains(&name))
                && !name
                    .is_some_and(|name| name.ends_with(".tar.zst") || name.contains(".pkg.tar."))
        })
        .collect();

    kept.sort();
    kept
}

#[test]
fn no_source_in_the_workspace_carries_a_comment() {
    let root = workspace();
    let roots: Vec<PathBuf> = ROOTS
        .iter()
        .flat_map(|folder| roots_under(&root, folder))
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
