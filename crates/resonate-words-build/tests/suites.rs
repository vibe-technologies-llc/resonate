use std::{
    fs,
    path::{Path, PathBuf},
};

const SUITE: &str = "suite";
const DECLARED_SUITE: &str = "[[test]]\nname = \"suite\"\npath = \"tests/suite.rs\"\n";
const NO_INFERRED_TESTS: &str = "autotests = false\n";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn crates_under(folder: &Path, found: &mut Vec<PathBuf>) {
    if folder.join("Cargo.toml").is_file() {
        found.push(folder.to_owned());
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            crates_under(&entry.path(), found);
        }
    }
}

fn integration_files(krate: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(krate.join("tests")) else {
        return Vec::new();
    };
    let mut stems: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rs"))
        .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
        .collect();
    stems.sort();
    stems
}

fn modules_named_by(suite: &str) -> Vec<String> {
    let mut named: Vec<String> = suite
        .lines()
        .filter_map(|line| line.strip_prefix("mod ")?.strip_suffix(';'))
        .map(str::to_owned)
        .collect();
    named.sort();
    named
}

fn what_is_wrong_with(krate: &Path) -> Option<String> {
    let files = integration_files(krate);
    let gathered = files.iter().any(|stem| stem == SUITE);
    if files.len() < 2 && !gathered {
        return None;
    }
    if !gathered {
        return Some(format!(
            "has {} test files, each its own binary: {}",
            files.len(),
            files.join(", ")
        ));
    }

    let manifest = fs::read_to_string(krate.join("Cargo.toml")).expect("the manifest is readable");
    if !manifest.contains(NO_INFERRED_TESTS) || !manifest.contains(DECLARED_SUITE) {
        return Some("holds a suite its manifest does not declare as its one test".to_owned());
    }

    let suite =
        fs::read_to_string(krate.join("tests/suite.rs")).expect("the suite root is readable");
    let named = modules_named_by(&suite);
    let missing: Vec<&String> = files
        .iter()
        .filter(|stem| *stem != SUITE && !named.contains(stem))
        .collect();
    (!missing.is_empty()).then(|| {
        format!(
            "never compiles {}",
            missing
                .iter()
                .map(|stem| format!("tests/{stem}.rs"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

#[test]
fn every_crate_links_its_integration_tests_into_one_suite_that_names_each_file() {
    let root = workspace();
    let mut crates = Vec::new();
    crates_under(&root.join("crates"), &mut crates);
    crates.sort();

    let wrong: Vec<String> = crates
        .iter()
        .filter_map(|krate| {
            let named = krate
                .strip_prefix(&root)
                .unwrap_or(krate)
                .display()
                .to_string();
            what_is_wrong_with(krate).map(|why| format!("{named} {why}"))
        })
        .collect();

    assert!(!crates.is_empty(), "the workspace's crates are found");
    assert!(
        wrong.is_empty(),
        "integration tests out of their suite:\n{}",
        wrong.join("\n")
    );
}
