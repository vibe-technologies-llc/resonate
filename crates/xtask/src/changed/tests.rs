use std::{collections::BTreeSet, path::Path};

use super::{Member, Touch, dependents_listed, members_listed, owners, package_name};

fn member(name: &str, folder: &str) -> Member {
    Member {
        name: name.to_owned(),
        folder: folder.to_owned(),
    }
}

fn files(listed: &[&str]) -> BTreeSet<String> {
    listed.iter().map(|file| (*file).to_owned()).collect()
}

fn members() -> Vec<Member> {
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
            ".claude/rules/upnp.md",
        ]),
        &members(),
    );

    assert_eq!(
        touched,
        Touch::These(files(&["resonate-dlna", "resonate-ui"]))
    );
}

#[test]
fn a_workspace_wide_file_touches_every_package() {
    let lock = owners(&files(&["Cargo.lock"]), &members());
    let config = owners(&files(&[".cargo/config.toml"]), &members());
    let inner = owners(&files(&["crates/resonate/Cargo.toml"]), &members());

    assert_eq!(lock, Touch::Everything);
    assert_eq!(config, Touch::Everything);
    assert_eq!(inner, Touch::These(files(&["resonate"])));
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
