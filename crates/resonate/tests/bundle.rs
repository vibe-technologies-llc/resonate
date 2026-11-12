use std::{
    collections::BTreeSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use png::Decoder;
use resonate_core::AUDIO_EXTENSIONS;
use resonate_mpris::APP_USER_MODEL_ID;

const INFO: &str = "macos/Info.plist";
const ENTITLEMENTS: &str = "macos/entitlements.plist";
const BUILD_SCRIPT: &str = "macos/build.sh";
const SIGNING_SCRIPT: &str = "macos/sign.sh";
const ICON: &str = "macos/resonate.icns";
const README: &str = "macos/README.txt";
const FLATPAK_MANIFEST: &str = "org.resonate.Resonate.yml";
const WORKFLOW: &str = "../../.github/workflows/release.yml";
const PLACEHOLDER_FENCE: char = '@';
const ICON_MAGIC: &[u8] = b"icns";
const ICON_HEADER: usize = 8;
const ICON_SIDES: [(&[u8; 4], u32); 8] = [
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
];
const SECRET_VARIABLES: [&str; 5] = [
    "RESONATE_MACOS_CERTIFICATE_BASE64",
    "RESONATE_MACOS_CERTIFICATE_PASSWORD",
    "RESONATE_MACOS_NOTARY_KEY_BASE64",
    "RESONATE_MACOS_NOTARY_KEY_ID",
    "RESONATE_MACOS_NOTARY_ISSUER",
];
const EVERY_FILE: [&str; 5] = [INFO, ENTITLEMENTS, BUILD_SCRIPT, SIGNING_SCRIPT, README];
const ADDRESS_OPENS: &str = "https://";
const DIGEST_HEX_DIGITS: usize = 64;

fn root_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn packaging_path(name: &str) -> PathBuf {
    root_path("../../packaging").join(name)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn packaging(name: &str) -> String {
    read(&packaging_path(name))
}

fn holds_the_line(text: &str, wanted: &str) -> bool {
    text.lines().any(|line| line == wanted)
}

fn after_the_key<'a>(list: &'a str, key: &str) -> Option<&'a str> {
    let named = format!("<key>{key}</key>");

    list.find(&named).map(|at| &list[at + named.len()..])
}

fn between<'a>(text: &'a str, opening: &str, closing: &str) -> Option<&'a str> {
    let after = &text[text.find(opening)? + opening.len()..];

    after.find(closing).map(|end| &after[..end])
}

fn text_of<'a>(list: &'a str, key: &str) -> Option<&'a str> {
    let after = after_the_key(list, key)?.trim_start();

    between(after.strip_prefix("<string>")?, "", "</string>")
}

fn texts_of<'a>(list: &'a str, key: &str) -> Vec<&'a str> {
    let listed = after_the_key(list, key)
        .map(str::trim_start)
        .and_then(|after| after.strip_prefix("<array>"))
        .and_then(|after| between(after, "", "</array>"))
        .unwrap_or_default();

    listed
        .split("<string>")
        .skip(1)
        .filter_map(|rest| between(rest, "", "</string>"))
        .collect()
}

fn keys_of(list: &str) -> BTreeSet<&str> {
    list.split("<key>")
        .skip(1)
        .filter_map(|rest| between(rest, "", "</key>"))
        .collect()
}

fn is_switched_on(list: &str, key: &str) -> bool {
    after_the_key(list, key).is_some_and(|after| after.trim_start().starts_with("<true/>"))
}

fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split(PLACEHOLDER_FENCE).skip(1).step_by(2).collect()
}

fn assigned<'a>(script: &'a str, name: &str) -> Option<&'a str> {
    let opening = format!("{name}=");

    script
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(&opening))
}

fn addresses(text: &str) -> BTreeSet<&str> {
    text.split(ADDRESS_OPENS)
        .skip(1)
        .map(|rest| {
            let end = rest
                .find(|letter: char| letter.is_whitespace() || "\"'<>".contains(letter))
                .unwrap_or(rest.len());

            &rest[..end]
        })
        .collect()
}

fn digests(text: &str) -> BTreeSet<&str> {
    text.split(|letter: char| !letter.is_ascii_hexdigit())
        .filter(|word| word.len() == DIGEST_HEX_DIGITS)
        .collect()
}

fn icon_images() -> Vec<([u8; 4], Vec<u8>)> {
    let bytes = fs::read(packaging_path(ICON)).expect("the committed icon");
    let declared = u32::from_be_bytes(bytes[4..ICON_HEADER].try_into().unwrap());
    let mut images = Vec::new();
    let mut at = ICON_HEADER;

    assert_eq!(&bytes[..4], ICON_MAGIC, "not an icon file");
    assert_eq!(
        usize::try_from(declared).unwrap(),
        bytes.len(),
        "the icon declares another length than it has"
    );
    while at < bytes.len() {
        let kind: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        let length = u32::from_be_bytes(bytes[at + 4..at + ICON_HEADER].try_into().unwrap());
        let end = at + usize::try_from(length).unwrap();

        images.push((kind, bytes[at + ICON_HEADER..end].to_vec()));
        at = end;
    }
    images
}

#[test]
fn the_bundle_opens_every_extension_the_scan_admits_and_no_other() {
    let info = packaging(INFO);

    let declared: BTreeSet<&str> = texts_of(&info, "public.filename-extension")
        .into_iter()
        .collect();
    let expected: BTreeSet<&str> = AUDIO_EXTENSIONS.iter().copied().collect();
    let opened = texts_of(&info, "LSItemContentTypes");
    let imported = text_of(&info, "UTTypeIdentifier").expect("the bundle declares a type");

    assert_eq!(
        declared, expected,
        "the extensions the bundle's type names are not the scan's"
    );
    assert!(
        opened.contains(&imported) && opened.contains(&"public.audio"),
        "the bundle's document type does not open the type it declares beside the system's audio"
    );
    assert_eq!(
        text_of(&info, "LSHandlerRank"),
        Some("Alternate"),
        "the bundle would claim to be the default player"
    );
}

#[test]
fn the_bundle_takes_resonate_links_as_the_desktop_entry_does() {
    let info = packaging(INFO);
    let entry = packaging("resonate.desktop");

    let schemes = texts_of(&info, "CFBundleURLSchemes");

    assert_eq!(schemes.len(), 1, "the bundle takes one scheme");
    assert!(entry.contains(&format!("x-scheme-handler/{};", schemes[0])));
}

#[test]
fn the_bundle_is_named_by_the_identity_the_desktop_knows_the_player_by() {
    let info = packaging(INFO);

    assert_eq!(
        text_of(&info, "CFBundleIdentifier"),
        Some(APP_USER_MODEL_ID)
    );
    assert_eq!(text_of(&info, "CFBundlePackageType"), Some("APPL"));
    assert!(is_switched_on(&info, "NSHighResolutionCapable"));
}

#[test]
fn the_build_script_puts_in_the_bundle_what_its_property_list_names() {
    let info = packaging(INFO);
    let build = packaging(BUILD_SCRIPT);

    let executable = text_of(&info, "CFBundleExecutable").expect("an executable");
    let icon = text_of(&info, "CFBundleIconFile").expect("an icon");

    assert!(
        build.contains(&format!("\"$contents/MacOS/{executable}\"")),
        "build.sh does not put {executable} where the property list looks for it"
    );
    assert!(
        build.contains(&format!("\"$resources/{icon}.icns\"")),
        "build.sh does not put {icon}.icns among the bundle's resources"
    );
    assert!(
        build.contains("\"$contents/MacOS/resonate-signin\""),
        "build.sh leaves the sign-in window out of the bundle"
    );
    assert!(build.contains("\"$here/sign.sh\" \"$application\""));
    assert!(build.contains("\"$here/sign.sh\" --image \"$image\""));
    assert!(
        build
            .find("\"$here/sign.sh\" \"$application\"")
            .zip(build.find("hdiutil create"))
            .is_some_and(|(signed, imaged)| signed < imaged),
        "the disk image would hold an application signed after it was copied in"
    );
}

#[test]
fn every_placeholder_the_property_list_holds_is_filled_with_the_version_cargo_declares() {
    let info = packaging(INFO);
    let build = packaging(BUILD_SCRIPT);

    let held = placeholders(&info);

    assert_eq!(held, BTreeSet::from(["VERSION"]));
    for placeholder in held {
        assert!(
            build.contains(&format!("s/@{placeholder}@/")),
            "the property list holds @{placeholder}@, which build.sh never fills"
        );
    }
    assert_eq!(
        text_of(&info, "CFBundleShortVersionString"),
        text_of(&info, "CFBundleVersion")
    );
    assert!(
        build.contains("Cargo.toml") && assigned(&build, "version") == Some("${1:-}"),
        "build.sh must read the workspace version where it is given none"
    );
    assert!(
        !build
            .lines()
            .any(|line| line.trim_start().starts_with("version=\"0")
                || line.trim_start().starts_with("version=0")),
        "build.sh hard-codes a version"
    );
}

#[test]
fn the_bundle_asks_for_the_system_the_build_targets() {
    let info = packaging(INFO);
    let build = packaging(BUILD_SCRIPT);

    let minimum = text_of(&info, "LSMinimumSystemVersion").expect("a minimum system");

    assert!(
        holds_the_line(
            &build,
            &format!("export MACOSX_DEPLOYMENT_TARGET={minimum}")
        ),
        "build.sh targets another system than the {minimum} the bundle asks for"
    );
}

#[test]
fn the_build_script_builds_apple_silicon_and_intel_over_the_checkouts_native_flags() {
    let build = packaging(BUILD_SCRIPT);
    let triples: Vec<&str> = build
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("triple="))
        .collect();
    let flags: Vec<&str> = build
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("flags="))
        .collect();

    assert!(holds_the_line(&build, "export RUSTFLAGS=\"$flags\""));
    assert_eq!(triples, ["aarch64-apple-darwin", "x86_64-apple-darwin"]);
    assert_eq!(flags, ["\"\"", "\"-C target-cpu=x86-64-v3\""]);
    assert_eq!(assigned(&build, "architecture"), Some("${3:-aarch64}"));
    assert!(!build.contains("target-cpu=native"));
    for package in [
        "--package resonate --bin resonate",
        "--package resonate-signin",
    ] {
        assert!(
            holds_the_line(
                &build,
                &format!("cargo build --release --locked --target \"$triple\" {package}")
            ),
            "build.sh does not build {package} in a run of its own"
        );
    }
    assert!(
        build.contains("resonate-$version-$named-macos.dmg")
            && holds_the_line(&build, "        named=aarch64")
            && holds_the_line(&build, "        named=x86_64_v3"),
        "build.sh names its disk image for something other than the version and the architecture"
    );
}

#[test]
fn the_cask_names_the_two_images_build_sh_makes_and_every_placeholder_is_filled() {
    let cask = packaging("macos/resonate.rb");
    let filling = packaging("macos/fill-cask.sh");

    for placeholder in [
        "@VERSION@",
        "@REPOSITORY_URL@",
        "@DOWNLOAD_URL@",
        "@ARM64_IMAGE_SHA256@",
        "@X64_IMAGE_SHA256@",
    ] {
        assert!(
            cask.contains(placeholder),
            "the cask holds no {placeholder}"
        );
        assert!(
            filling.contains(placeholder),
            "fill-cask.sh never fills {placeholder}"
        );
    }
    assert!(
        cask.contains("arch arm: \"aarch64\", intel: \"x86_64_v3\"")
            && cask.contains("resonate-#{version}-#{arch}-macos.dmg"),
        "the cask names images build.sh does not make"
    );
    assert!(
        filling.contains("resonate-$version-aarch64-macos.dmg")
            && filling.contains("resonate-$version-x86_64_v3-macos.dmg"),
        "fill-cask.sh weighs images build.sh does not make"
    );
    assert!(!cask.contains("github.com") && !filling.contains("github.com"));
}

#[test]
fn the_bundle_says_why_it_asks_for_the_microphone_the_macs_own_sound_and_the_network() {
    let info = packaging(INFO);
    let entitlements = packaging(ENTITLEMENTS);

    for prompt in [
        "NSMicrophoneUsageDescription",
        "NSAudioCaptureUsageDescription",
        "NSLocalNetworkUsageDescription",
    ] {
        assert!(
            text_of(&info, prompt).is_some_and(|said| said.starts_with("Resonate ")),
            "the bundle asks without {prompt}"
        );
    }
    assert_eq!(
        keys_of(&entitlements),
        BTreeSet::from(["com.apple.security.device.audio-input"]),
        "the hardened runtime is asked for more than the microphone"
    );
    assert!(is_switched_on(
        &entitlements,
        "com.apple.security.device.audio-input"
    ));
}

#[test]
fn the_icon_holds_each_size_macos_asks_for_as_a_png_of_that_side() {
    let images = icon_images();

    for (kind, _) in ICON_SIDES {
        assert!(
            images.iter().any(|(held, _)| held == kind),
            "no {} entry",
            String::from_utf8_lossy(kind)
        );
    }
    for (kind, png) in images {
        let side = ICON_SIDES
            .iter()
            .find(|(named, _)| **named == kind)
            .map(|(_, side)| *side)
            .unwrap_or_else(|| panic!("an entry of a kind no size is known for: {kind:?}"));
        let reader = Decoder::new(Cursor::new(png.as_slice()))
            .read_info()
            .expect("a PNG image");

        assert_eq!(
            (reader.info().width, reader.info().height),
            (side, side),
            "an entry names a side its image does not have"
        );
    }
}

#[test]
fn the_release_workflow_signs_only_with_the_secrets_the_script_reads() {
    let workflow = read(&root_path(WORKFLOW));
    let signing = packaging(SIGNING_SCRIPT);

    for variable in SECRET_VARIABLES {
        assert!(
            workflow.contains(&format!("{variable}: ${{{{ secrets.")),
            "release.yml does not hand {variable} to the build"
        );
        assert!(
            signing.contains(&format!("${{{variable}:-}}")),
            "sign.sh does not read {variable}, or fails where it is not set"
        );
    }
    assert_eq!(
        workflow.matches("RESONATE_MACOS_").count(),
        SECRET_VARIABLES.len(),
        "release.yml hands the build a variable sign.sh does not read"
    );
    assert!(
        signing.contains("codesign --force --sign - \"$application\""),
        "sign.sh must sign ad hoc, not fail, where no certificate exists"
    );
    assert!(
        signing.contains("--options runtime")
            && signing.contains("--entitlements \"$here/entitlements.plist\""),
        "a Developer ID signature must harden the runtime under the bundle's entitlements"
    );
    assert!(
        signing
            .find("notarytool submit")
            .zip(signing.find("stapler staple"))
            .is_some_and(|(submitted, stapled)| submitted < stapled),
        "the ticket would be stapled before the notary answered, or never"
    );
}

#[test]
fn nothing_the_bundle_is_made_of_names_a_maintainer_or_a_repository() {
    let flatpak = packaging(FLATPAK_MANIFEST);
    let build = packaging(BUILD_SCRIPT);

    let fetched = addresses(&build);

    assert_eq!(fetched.len(), 2, "build.sh fetches the two faces alone");
    for address in &fetched {
        assert!(
            flatpak.contains(&format!("{ADDRESS_OPENS}{address}")),
            "build.sh fetches {address}, which the Flatpak does not"
        );
    }
    assert_eq!(
        digests(&build),
        digests(&flatpak),
        "build.sh holds the faces to other digests than the Flatpak's"
    );
    for name in EVERY_FILE {
        let text = packaging(name);

        for address in addresses(&text) {
            assert!(fetched.contains(address), "{name} names {address}");
        }
        for placeholder in placeholders(&text) {
            assert_eq!(
                placeholder, "VERSION",
                "{name} holds an address or an unnamed placeholder: @{placeholder}@"
            );
        }
    }
}
