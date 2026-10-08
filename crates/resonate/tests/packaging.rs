use std::{fs, path::PathBuf};

const DESKTOP_ENTRY: &str = "resonate.desktop";
const FLATPAK_MANIFEST: &str = "org.resonate.Resonate.yml";
const METAINFO: &str = "org.resonate.Resonate.metainfo.xml";
const SRCINFO_FIELDS: [&str; 7] = [
    "pkgver",
    "pkgrel",
    "arch",
    "license",
    "depends",
    "makedepends",
    "options",
];

fn packaging(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging")
        .join(name);

    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn pkgbuild_values(pkgbuild: &str, field: &str) -> Vec<String> {
    let opening = format!("{field}=");
    let Some(start) = pkgbuild.lines().position(|line| line.starts_with(&opening)) else {
        return Vec::new();
    };

    let mut written = String::new();
    for line in pkgbuild.lines().skip(start) {
        written.push_str(line);
        written.push(' ');
        if !written.starts_with(&format!("{opening}(")) || line.trim_end().ends_with(')') {
            break;
        }
    }

    let value = written[opening.len()..].trim();
    let listed = value
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(value);
    listed
        .split_whitespace()
        .map(|item| {
            item.trim_matches(|quote| quote == '\'' || quote == '"')
                .to_owned()
        })
        .collect()
}

fn srcinfo_values(srcinfo: &str, field: &str) -> Vec<String> {
    let opening = format!("{field} = ");

    srcinfo
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&opening))
        .map(str::to_owned)
        .collect()
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
fn the_srcinfo_says_what_the_pkgbuild_says() {
    let pkgbuild = packaging("PKGBUILD");
    let srcinfo = packaging(".SRCINFO");

    for field in SRCINFO_FIELDS {
        let declared = pkgbuild_values(&pkgbuild, field);
        let summarised = srcinfo_values(&srcinfo, field);

        assert!(!declared.is_empty(), "the PKGBUILD declares no {field}");
        assert_eq!(
            summarised, declared,
            "the .SRCINFO's {field} is not the PKGBUILD's; run makepkg --printsrcinfo"
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
        env!("CARGO_PKG_VERSION"),
        "the spec's Version is not the workspace's; a release archive would not match it"
    );
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
