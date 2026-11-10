use std::{fs, path::PathBuf};

const DESKTOP_ENTRY: &str = "resonate.desktop";
const FLATPAK_MANIFEST: &str = "org.resonate.Resonate.yml";
const METAINFO: &str = "org.resonate.Resonate.metainfo.xml";
const CONFIG_MAN_PAGE: &str = "resonate-config.toml.5";
const PACKAGE_RECIPES: [&str; 6] = [
    "PKGBUILD",
    "resonate.spec",
    FLATPAK_MANIFEST,
    "release/PKGBUILD",
    DEB_SCRIPT,
    MACOS_SCRIPT,
];
const SERVER_RECIPES: [&str; 4] = [
    "server/PKGBUILD",
    "server/resonate-server.spec",
    "release/server/PKGBUILD",
    DEB_SCRIPT,
];
const RELEASE_RECIPES: [&str; 6] = [
    "resonate.spec",
    "server/resonate-server.spec",
    "release/PKGBUILD",
    "release/server/PKGBUILD",
    "release/signin/PKGBUILD",
    DEB_SCRIPT,
];
const RELEASE_PKGBUILDS: [&str; 3] = [
    "release/PKGBUILD",
    "release/server/PKGBUILD",
    "release/signin/PKGBUILD",
];
const DEB_SCRIPT: &str = "deb/build.sh";
const MACOS_SCRIPT: &str = "macos/build.sh";
const SERVER_FILES: [&str; 2] = ["resonate-server.service", "resonate-server.env"];
const ARCH_FACES: [&str; 2] = ["inter-font", "ttf-jetbrains-mono"];
const FACES_BROUGHT: [(&str, [&str; 2]); 7] = [
    ("PKGBUILD", ARCH_FACES),
    ("release/PKGBUILD", ARCH_FACES),
    (".SRCINFO", ARCH_FACES),
    (
        "resonate.spec",
        ["rsms-inter-fonts", "jetbrains-mono-fonts"],
    ),
    (DEB_SCRIPT, ["fonts-inter", "fonts-jetbrains-mono"]),
    (
        FLATPAK_MANIFEST,
        ["/app/share/fonts/inter", "/app/share/fonts/jetbrains-mono"],
    ),
    (MACOS_SCRIPT, ["Inter-*.ttf", "JetBrainsMono-*.ttf"]),
];

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
fn every_package_brings_the_reading_face_and_the_face_for_codes() {
    for (recipe, faces) in FACES_BROUGHT {
        let written = packaging(recipe);

        for face in faces {
            assert!(written.contains(face), "{recipe} does not bring {face}");
        }
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
fn the_windows_recipe_builds_x86_64_v3_and_arm64_and_reads_its_version_from_cargo() {
    let script = packaging("windows/build.ps1");

    assert!(
        script.contains("x86-64-v3") && !script.contains("target-cpu=native"),
        "the Windows recipe builds for another target than x86-64-v3"
    );
    assert!(
        script.contains("Cargo.toml") && script.contains("$($build.Label)-windows"),
        "the Windows recipe must read the workspace version and name its asset for the architecture"
    );
    assert!(
        script.contains("x86_64_v3") && script.contains("Label = \"aarch64\""),
        "the Windows recipe must name the x86-64-v3 archive and the ARM64 one"
    );
    assert!(
        script.contains("aarch64-pc-windows-msvc") && script.contains("target-cpu=generic"),
        "the Windows recipe must build ARM64 for its own triple at the generic baseline"
    );
    assert!(
        !script
            .lines()
            .any(|line| line.trim_start().starts_with("$Version = \"0")),
        "the Windows recipe hard-codes a version"
    );
    assert_eq!(
        script
            .lines()
            .filter(|line| line.contains("Flags = ") && line.contains("+crt-static"))
            .count(),
        2,
        "each Windows build must link the C runtime statically, since a fresh Windows has no VCRUNTIME140.dll"
    );
}

#[test]
fn a_windows_build_from_the_checkout_links_the_c_runtime_statically() {
    let config = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.cargo/config.toml"),
    )
    .expect("the cargo config");

    assert!(
        config.contains("[target.'cfg(all(windows, target_env = \"msvc\"))']")
            && config.contains("target-feature=+crt-static"),
        "a Windows build from the checkout must link the C runtime statically"
    );
}

#[test]
fn the_release_workflow_attaches_a_windows_archive_and_installer_for_each_architecture() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/release.yml");
    let workflow =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));

    assert!(
        workflow.contains("architecture: [x86_64, aarch64]")
            && workflow.contains("-Architecture ${{ matrix.architecture }}"),
        "release.yml does not build the Windows archive for x86_64 and aarch64"
    );
    assert!(
        workflow.contains("resonate-windows-${{ matrix.architecture }}")
            && workflow.contains("assets/*-setup.exe"),
        "release.yml's Windows files would share one artifact name or leave the installer out"
    );
}

#[test]
fn the_release_workflow_attaches_the_macos_disk_image_and_publishes_without_it_where_it_failed() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/release.yml");
    let workflow =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let script = packaging(MACOS_SCRIPT);

    assert!(
        workflow.contains("runs-on: macos-latest")
            && workflow.contains("packaging/macos/build.sh \"$VERSION\" assets"),
        "release.yml does not build the disk image on a Mac"
    );
    assert!(
        workflow.contains("name: resonate-macos-aarch64") && workflow.contains("assets/*.dmg"),
        "release.yml's disk image would not be collected with the other assets"
    );
    assert!(
        workflow
            .lines()
            .any(|line| line.trim_start().starts_with("needs: [") && line.contains("macos-dmg")),
        "the checksums would be written before the disk image exists"
    );
    assert!(
        workflow.contains("if: ${{ !cancelled() && needs.release.result == 'success' }}"),
        "a failed disk image would keep every other asset off the release"
    );
    assert!(
        script.contains("resonate-$version-aarch64-macos.dmg"),
        "the macOS recipe names its disk image for something other than the version and the architecture"
    );
}

#[test]
fn no_release_recipe_builds_a_debug_package() {
    for spec in ["resonate.spec", "server/resonate-server.spec"] {
        assert!(
            packaging(spec).contains("%global debug_package %{nil}"),
            "{spec} would attach a debuginfo and a debugsource RPM to the release"
        );
    }
    for recipe in [
        "PKGBUILD",
        "release/PKGBUILD",
        "server/PKGBUILD",
        "release/signin/PKGBUILD",
    ] {
        assert!(
            packaging(recipe).contains("'!debug'"),
            "{recipe} would build a -debug package beside the real one"
        );
    }
}

#[test]
fn no_recipe_builds_the_player_beside_the_sign_in_windows_engine() {
    for recipe in [
        "PKGBUILD",
        "release/PKGBUILD",
        "resonate.spec",
        DEB_SCRIPT,
        "org.resonate.Resonate.yml",
        MACOS_SCRIPT,
    ] {
        let text = packaging(recipe);

        assert!(
            !text.contains("--workspace --bin resonate"),
            "{recipe} builds the player with every member selected, which turns on resonate-signin's \
             window feature in the library the player links"
        );
        assert!(
            text.contains("--package resonate --bin resonate"),
            "{recipe} does not build the player as a package of its own"
        );
    }
}

#[test]
fn the_player_is_named_resonate_in_every_release_format() {
    assert!(
        packaging("resonate.spec")
            .lines()
            .any(|line| line.trim_end() == "Name:           resonate"),
        "the RPM is not named resonate"
    );
    assert!(
        packaging("release/PKGBUILD").lines().next() == Some("pkgname=resonate"),
        "the Arch package is not named resonate"
    );
    assert!(
        packaging(DEB_SCRIPT)
            .lines()
            .any(|line| line.trim() == "seal resonate"),
        "the DEB is not named resonate"
    );
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
