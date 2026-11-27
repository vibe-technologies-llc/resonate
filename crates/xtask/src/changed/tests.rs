use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use super::{
    Member, Touch, dependents_listed, is_workspace_wide, members, members_listed, owners,
    package_name,
};
use crate::plan::RULES_AND_COMMENTS;

fn member(name: &str, folder: &str) -> Member {
    Member {
        name: name.to_owned(),
        folder: folder.to_owned(),
    }
}

fn files(listed: &[&str]) -> BTreeSet<String> {
    listed.iter().map(|file| (*file).to_owned()).collect()
}

fn some_members() -> Vec<Member> {
    vec![
        member("resonate", "crates/resonate"),
        member("resonate-ui", "crates/resonate-ui"),
        member("resonate-dlna", "crates/providers/dlna"),
    ]
}

#[test]
fn the_workspace_lists_its_members_and_each_manifest_names_its_package() {
    let workspace = "[workspace]\nresolver = \"3\"\nmembers = [\n    \"crates/resonate\",\n    \"crates/providers/dlna\"\n]\n\n[workspace.package]\nversion = \"0.13.0\"\n";
    let own = "[package]\nname = \"resonate-dlna\"\nversion.workspace = true\n\n[dependencies]\nname = \"other\"\n";

    assert_eq!(
        members_listed(workspace),
        ["crates/resonate", "crates/providers/dlna"]
    );
    assert_eq!(package_name(own), Some("resonate-dlna"));
    assert_eq!(package_name("[workspace]\n"), None);
}

#[test]
fn a_file_belongs_to_the_member_whose_folder_holds_it_and_not_one_sharing_its_prefix() {
    let touched = owners(
        &files(&[
            "crates/resonate-ui/src/views/root.rs",
            "crates/providers/dlna/src/lib.rs",
            "docs/BUGS.md",
        ]),
        &some_members(),
    );

    assert_eq!(
        touched,
        Touch::These(files(&["resonate-dlna", "resonate-ui"]))
    );
}

#[test]
fn a_workspace_wide_file_touches_every_package() {
    let lock = owners(&files(&["Cargo.lock"]), &some_members());
    let config = owners(&files(&[".cargo/config.toml"]), &some_members());
    let inner = owners(&files(&["crates/resonate/Cargo.toml"]), &some_members());

    assert_eq!(lock, Touch::Everything);
    assert_eq!(config, Touch::Everything);
    assert_eq!(inner, Touch::These(files(&["resonate"])));
}

#[test]
fn a_file_of_the_detached_lab_belongs_to_the_lab_and_a_neighbour_of_its_name_to_none() {
    let lab = owners(
        &files(&["dsp-lab/src/main.rs", "dsp-lab/Cargo.lock"]),
        &some_members(),
    );
    let neighbour = owners(&files(&["dsp-lab-notes/readme"]), &some_members());

    assert_eq!(lab, Touch::These(files(&["resonate-dsp-lab"])));
    assert_eq!(neighbour, Touch::Everything);
}

#[test]
fn the_inverted_tree_is_read_for_the_workspace_s_own_packages_alone() {
    let root = Path::new("/work/resonate");
    let listed = "resonate-lyrics v0.13.0 (/work/resonate/crates/resonate-lyrics)\n\
                  resonate-ui v0.13.0 (/work/resonate/crates/resonate-ui) (*)\n\
                  resonate-ui v0.13.0 (/work/resonate/crates/resonate-ui)\n\
                  serde v1.0.228\n";

    assert_eq!(
        dependents_listed(listed, root),
        files(&["resonate-lyrics", "resonate-ui"])
    );
}

fn selected(listed: &[&str]) -> BTreeSet<String> {
    match owners(&files(listed), &some_members()) {
        Touch::These(packages) => packages,
        Touch::Everything => panic!("{listed:?} selected every package"),
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn tracked(root: &Path) -> Vec<String> {
    let listed = Command::new("git")
        .current_dir(root)
        .args(["ls-files"])
        .output()
        .expect("git lists the files");

    String::from_utf8(listed.stdout)
        .expect("file names are text")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn sources_under(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            sources_under(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

const PARENT: &str = "../";
const ENDS_A_PATH: [char; 8] = ['"', '\'', ')', ',', ' ', '\n', '}', ';'];

fn paths_above_a_crate(source: &str) -> Vec<&str> {
    source
        .match_indices(PARENT)
        .filter(|(at, _)| !source[..*at].ends_with(PARENT))
        .filter_map(|(at, _)| {
            let mut rest = &source[at..];
            let mut climbed = 0;

            while let Some(after) = rest.strip_prefix(PARENT) {
                climbed += 1;
                rest = after;
            }

            let path = &rest[..rest.find(ENDS_A_PATH).unwrap_or(rest.len())];

            (climbed >= 2 && !path.is_empty()).then_some(path)
        })
        .collect()
}

#[test]
fn a_packaging_file_selects_the_crates_whose_tests_read_it() {
    let selected = selected(&["packaging/resonate.spec"]);

    assert!(selected.contains("resonate"));
    assert!(selected.contains("resonate-server"));
    assert!(selected.contains("resonate-cli"));
    assert!(selected.contains(RULES_AND_COMMENTS));
}

#[test]
fn a_workflow_selects_the_release_checks_and_the_comment_walk() {
    assert_eq!(
        selected(&[".github/workflows/ci.yml"]),
        files(&["resonate", RULES_AND_COMMENTS])
    );
}

#[test]
fn a_seed_selects_the_crate_that_replays_it_and_no_other() {
    assert_eq!(
        selected(&["fuzz/seeds/probe/flac"]),
        files(&["resonate-codec", RULES_AND_COMMENTS])
    );
    assert_eq!(
        selected(&["fuzz/seeds/sync/export"]),
        files(&["resonate-sync", RULES_AND_COMMENTS])
    );
    assert_eq!(
        selected(&["fuzz/seeds/lrc/plain"]),
        files(&[RULES_AND_COMMENTS])
    );
    assert_eq!(
        selected(&["fuzz/fuzz_targets/lrc.rs"]),
        files(&[RULES_AND_COMMENTS])
    );
}

#[test]
fn the_rules_the_phones_number_and_the_phones_plist_select_the_phones_boundary() {
    assert_eq!(
        selected(&[".claude/rules/upnp.md"]),
        files(&["resonate-mobile", RULES_AND_COMMENTS])
    );
    assert_eq!(
        selected(&["ios/Resonate/Info.plist"]),
        files(&["resonate-mobile", RULES_AND_COMMENTS])
    );
    assert_eq!(
        selected(&["ios/Resonate/Sources/App.swift"]),
        files(&[RULES_AND_COMMENTS])
    );
}

#[test]
fn prose_nobody_reads_selects_nothing_and_a_file_nobody_placed_selects_everything() {
    assert_eq!(selected(&["docs/ROADMAP.md", "README.md"]), files(&[]));
    assert_eq!(
        owners(&files(&["scripts/release.sh"]), &some_members()),
        Touch::Everything
    );
    assert_eq!(
        owners(&files(&[".config/nextest.toml"]), &some_members()),
        Touch::Everything
    );
}

#[test]
fn a_folder_is_read_by_the_longest_prefix_naming_it_and_not_one_sharing_its_letters() {
    assert_eq!(
        owners(&files(&["fuzz/seeds/sync-extra/x"]), &some_members()),
        Touch::These(files(&[RULES_AND_COMMENTS]))
    );
    assert_eq!(
        owners(&files(&["packaging-notes/x"]), &some_members()),
        Touch::Everything
    );
}

#[test]
fn every_file_the_repository_tracks_is_placed_by_an_owner_a_reader_or_a_wide_rule() {
    let root = workspace();
    let members = members(&root).expect("the workspace's members");

    let unplaced: Vec<String> = tracked(&root)
        .into_iter()
        .filter(|file| !is_workspace_wide(file))
        .filter(|file| owners(&files(&[file]), &members) == Touch::Everything)
        .collect();

    assert!(
        unplaced.is_empty(),
        "files no member holds and no reading names, so each selects every package: {unplaced:?}"
    );
}

#[test]
fn every_path_a_crate_reads_above_itself_selects_that_crate() {
    let root = workspace();
    let members = members(&root).expect("the workspace's members");
    let tops: BTreeSet<String> = fs::read_dir(&root)
        .expect("the workspace is readable")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "crates" && name != "target" && name != ".git")
        .collect();

    let mut sources = Vec::new();

    sources_under(&root.join("crates"), &mut sources);

    let mut checked = 0;
    let mut missed = Vec::new();

    for source in sources {
        let relative = source
            .strip_prefix(&root)
            .expect("under the root")
            .to_string_lossy()
            .into_owned();
        let Some(reader) = members
            .iter()
            .filter(|member| relative.starts_with(&format!("{}/", member.folder)))
            .max_by_key(|member| member.folder.len())
        else {
            continue;
        };

        if reader.name == "xtask" {
            continue;
        }

        let text = fs::read_to_string(&source).expect("a source is text");

        for path in paths_above_a_crate(&text) {
            let top = path.split('/').next().unwrap_or_default();

            if !tops.contains(top) {
                continue;
            }

            checked += 1;

            let selected = match owners(&files(&[path]), &members) {
                Touch::Everything => true,
                Touch::These(packages) => packages.contains(&reader.name),
            };
            if !selected {
                missed.push(format!("{relative} reads {path}"));
            }
        }
    }

    assert!(
        checked > 20,
        "only {checked} reads above a crate were found"
    );
    assert!(
        missed.is_empty(),
        "a test reads a path that does not select its crate: {missed:?}"
    );
}
