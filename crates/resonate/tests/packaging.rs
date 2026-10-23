use std::{fs, path::PathBuf};

const DESKTOP_ENTRY: &str = "resonate.desktop";
const FLATPAK_MANIFEST: &str = "org.resonate.Resonate.yml";
const METAINFO: &str = "org.resonate.Resonate.metainfo.xml";
const CONFIG_MAN_PAGE: &str = "resonate-config.toml.5";
const PACKAGE_RECIPES: [&str; 5] = [
    "PKGBUILD",
    "resonate.spec",
    FLATPAK_MANIFEST,
    "release/PKGBUILD",
    DEB_SCRIPT,
];
const SERVER_RECIPES: [&str; 4] = [
    "server/PKGBUILD",
    "server/resonate-server.spec",
    "release/server/PKGBUILD",
    DEB_SCRIPT,
];
const RELEASE_RECIPES: [&str; 5] = [
    "resonate.spec",
    "server/resonate-server.spec",
    "release/PKGBUILD",
    "release/server/PKGBUILD",
    DEB_SCRIPT,
];
const RELEASE_PKGBUILDS: [&str; 2] = ["release/PKGBUILD", "release/server/PKGBUILD"];
const DEB_SCRIPT: &str = "deb/build.sh";
const SERVER_FILES: [&str; 2] = ["resonate-server.service", "resonate-server.env"];

fn packaging(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging")
        .join(name);

    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn manifest_value<'a>(manifest: &'a str, key: &str) -> Option<&'a str> {
    let opening = format!("{key}: ");

    manifest
        .lines()
        .find_map(|line| line.strip_prefix(&opening))
}

fn desktop_value<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    let opening = format!("{key}=");

    entry.lines().find_map(|line| line.strip_prefix(&opening))
}

fn element<'a>(document: &'a str, opening: &str, closing: &str) -> Option<&'a str> {
    let after = &document[document.find(opening)? + opening.len()..];

    after.find(closing).map(|end| &after[..end])
}

#[test]
fn every_package_installs_the_manual_of_the_settings_file() {
    packaging(CONFIG_MAN_PAGE);

    for recipe in PACKAGE_RECIPES {
        assert!(
            packaging(recipe).contains(CONFIG_MAN_PAGE),
            "{recipe} does not install {CONFIG_MAN_PAGE}"
        );
    }
}

#[test]
fn the_flatpak_exports_the_desktop_entry_and_icon_under_its_id() {
    let manifest = packaging(FLATPAK_MANIFEST);
    let entry = packaging(DESKTOP_ENTRY);
    let metainfo = packaging(METAINFO);

    let id = manifest_value(&manifest, "id").expect("the manifest names its id");
    let renamed_entry = manifest_value(&manifest, "rename-desktop-file")
        .expect("the manifest renames the desktop entry to one flatpak-builder exports");
    let renamed_icon = manifest_value(&manifest, "rename-icon")
        .expect("the manifest renames the icon to one flatpak-builder exports");
    let installed_entry = format!("/app/share/applications/{DESKTOP_ENTRY}");

    assert_eq!(renamed_entry, DESKTOP_ENTRY);
    assert!(
        manifest.contains(&installed_entry),
        "the manifest renames an entry it does not install"
    );
    assert_eq!(
        Some(renamed_icon),
        desktop_value(&entry, "Icon"),
        "the icon renamed is not the one the entry names"
    );
    assert!(
        !manifest.contains(&format!("apps/{id}.")),
        "an icon installed under the id by hand stands where the renamed one lands"
    );
    assert_eq!(element(&metainfo, "<id>", "</id>"), Some(id));
    assert_eq!(
        element(
            &metainfo,
            "<launchable type=\"desktop-id\">",
            "</launchable>"
        ),
        Some(renamed_entry),
        "flatpak-builder rewrites a launchable naming the entry it renames, and no other"
    );
}

#[test]
fn the_rpm_spec_builds_the_version_cargo_declares() {
    let spec = packaging("resonate.spec");

    let declared = spec
        .lines()
        .find_map(|line| line.strip_prefix("Version:"))
        .map(str::trim)
        .expect("the spec declares a version");

    assert_eq!(
        declared,
        env!("CARGO_PKG_VERSION").replace('-', "~"),
        "the spec's Version is not the workspace's; a release archive would not match it"
    );
}

#[test]
fn the_server_spec_builds_the_version_cargo_declares() {
    let spec = packaging("server/resonate-server.spec");

    let declared = spec
        .lines()
        .find_map(|line| line.strip_prefix("Version:"))
        .map(str::trim)
        .expect("the server spec declares a version");

    assert_eq!(
        declared,
        env!("CARGO_PKG_VERSION").replace('-', "~"),
        "the server spec's Version is not the workspace's; a release archive would not match it"
    );
}

#[test]
fn the_release_pkgbuilds_build_the_version_cargo_declares() {
    for recipe in RELEASE_PKGBUILDS {
        let text = packaging(recipe);

        let declared = text
            .lines()
            .find_map(|line| line.strip_prefix("_version="))
            .expect("the release PKGBUILD declares the version it fetches");
        let pkgver = text
            .lines()
            .find_map(|line| line.strip_prefix("pkgver="))
            .expect("the release PKGBUILD declares a pkgver");

        assert_eq!(
            declared,
            env!("CARGO_PKG_VERSION"),
            "{recipe} fetches a tag that is not the workspace's version"
        );
        assert_eq!(
            pkgver, "${_version//-/}",
            "{recipe}: pkgver must be the version with its hyphens removed"
        );
    }
}

#[test]
fn the_deb_script_derives_its_version_from_cargo() {
    let script = packaging(DEB_SCRIPT);

    assert!(
        script.contains("Cargo.toml") && script.contains("${cargo_version//-/\\~}"),
        "the deb script must read the workspace version and map its hyphen to a tilde"
    );
    assert!(
        !script
            .lines()
            .any(|line| line.trim_start().starts_with("version=\"0")),
        "the deb script hard-codes a version"
    );
}

#[test]
fn every_release_recipe_builds_for_x86_64_v3_alone() {
    for recipe in RELEASE_RECIPES {
        let text = packaging(recipe);

        assert!(
            text.contains("x86-64-v3"),
            "{recipe} does not build for x86-64-v3"
        );
        assert!(
            !text.contains("target-cpu=native") && !text.contains("aarch64"),
            "{recipe} builds for another target than x86-64-v3"
        );
    }
}

#[test]
fn every_server_package_installs_the_unit_and_its_environment() {
    for file in SERVER_FILES {
        packaging(&format!("server/{file}"));
    }

    for recipe in SERVER_RECIPES {
        let text = packaging(recipe);
        for file in SERVER_FILES {
            assert!(text.contains(file), "{recipe} does not install {file}");
        }
    }
}

#[test]
fn the_flatpak_grants_no_x11_to_a_wayland_only_build() {
    let manifest = packaging(FLATPAK_MANIFEST);

    let granted: Vec<&str> = manifest
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- "))
        .map(|grant| grant.trim_matches('"'))
        .collect();

    for refused in ["--share=ipc", "--socket=x11", "--socket=fallback-x11"] {
        assert!(
            !granted.contains(&refused),
            "the manifest grants {refused}, though gpui is built for Wayland alone"
        );
    }
}
