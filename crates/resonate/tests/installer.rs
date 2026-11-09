use std::{
    collections::BTreeSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use png::Decoder;
use resonate_core::AUDIO_EXTENSIONS;
use resonate_mpris::APP_USER_MODEL_ID;

const INSTALLER_SCRIPT: &str = "windows/resonate.iss";
const BUILD_SCRIPT: &str = "windows/build.ps1";
const SIGNING_SCRIPT: &str = "windows/sign.ps1";
const FILLING_SCRIPT: &str = "windows/fill-manifests.sh";
const WINDOWS_ICON: &str = "windows/resonate.ico";
const WORKFLOW: &str = "../../.github/workflows/release.yml";
const MANIFEST_TEMPLATES: [&str; 4] = [
    "windows/scoop.json",
    "windows/winget/Resonate.Resonate.yaml",
    "windows/winget/Resonate.Resonate.installer.yaml",
    "windows/winget/Resonate.Resonate.locale.en-US.yaml",
];
const PLACEHOLDER_FENCE: char = '@';
const ICON_SIDES_REQUIRED: [u32; 4] = [16, 32, 48, 256];
const ICON_DIRECTORY_HEADER: usize = 6;
const ICON_DIRECTORY_ENTRY: usize = 16;
const LARGEST_ICON_SIDE: u32 = 256;
const SECRET_VARIABLES: [&str; 2] = [
    "RESONATE_WINDOWS_PFX_BASE64",
    "RESONATE_WINDOWS_PFX_PASSWORD",
];
const BUILD_DEFINITIONS: [&str; 8] = [
    "AppVersion",
    "NumericVersion",
    "InstallArchitecture",
    "SourceExecutable",
    "SourceSignIn",
    "RepositoryRoot",
    "OutputDirectory",
    "OutputName",
];

fn root_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn packaging_path(name: &str) -> PathBuf {
    root_path("../../packaging").join(name)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn holds_the_line(text: &str, wanted: &str) -> bool {
    text.lines().any(|line| line == wanted)
}

fn packaging(name: &str) -> String {
    read(&packaging_path(name))
}

fn registry_lines(script: &str) -> Vec<&str> {
    script
        .lines()
        .filter(|line| line.starts_with("Root: "))
        .collect()
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let opening = format!("{name}: \"");
    let after = &line[line.find(&opening)? + opening.len()..];

    after.find('"').map(|end| &after[..end])
}

fn extensions_listing_the_program(script: &str) -> BTreeSet<String> {
    registry_lines(script)
        .into_iter()
        .filter_map(|line| {
            let key = field(line, "Subkey")?;
            let rest = key.strip_prefix("Software\\Classes\\")?;
            let (extension, tail) = rest.split_once('\\')?;

            (extension.starts_with('.') && tail == "OpenWithProgids").then(|| extension.to_owned())
        })
        .collect()
}

fn extensions_named_under(script: &str, subkey: &str) -> BTreeSet<String> {
    registry_lines(script)
        .into_iter()
        .filter(|line| field(line, "Subkey") == Some(subkey))
        .filter_map(|line| field(line, "ValueName"))
        .filter(|name| name.starts_with('.'))
        .map(str::to_owned)
        .collect()
}

fn the_scans_extensions() -> BTreeSet<String> {
    AUDIO_EXTENSIONS
        .iter()
        .map(|extension| format!(".{extension}"))
        .collect()
}

fn icon_images() -> Vec<(u32, Vec<u8>)> {
    let bytes = fs::read(packaging_path(WINDOWS_ICON)).expect("the committed icon");
    let count = usize::from(u16::from_le_bytes([bytes[4], bytes[5]]));

    assert_eq!(&bytes[..4], &[0, 0, 1, 0], "not an icon file");
    (0..count)
        .map(|index| {
            let entry = ICON_DIRECTORY_HEADER + index * ICON_DIRECTORY_ENTRY;
            let side = match bytes[entry] {
                0 => LARGEST_ICON_SIDE,
                width => u32::from(width),
            };
            let length = u32::from_le_bytes(bytes[entry + 8..entry + 12].try_into().unwrap());
            let offset = u32::from_le_bytes(bytes[entry + 12..entry + 16].try_into().unwrap());
            let start = usize::try_from(offset).unwrap();
            let end = start + usize::try_from(length).unwrap();

            (side, bytes[start..end].to_vec())
        })
        .collect()
}

fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split(PLACEHOLDER_FENCE).skip(1).step_by(2).collect()
}

#[test]
fn the_installer_registers_every_extension_the_scan_admits_and_no_other() {
    let script = packaging(INSTALLER_SCRIPT);
    let expected = the_scans_extensions();

    assert_eq!(
        extensions_listing_the_program(&script),
        expected,
        "the extensions that list the program among those that open them are not the scan's"
    );
    assert_eq!(
        extensions_named_under(
            &script,
            "Software\\{#AppName}\\Capabilities\\FileAssociations"
        ),
        expected,
        "the capabilities' file associations are not the scan's extensions"
    );
    assert_eq!(
        extensions_named_under(
            &script,
            "Software\\Classes\\Applications\\{#Executable}\\SupportedTypes"
        ),
        expected,
        "the application's supported types are not the scan's extensions"
    );
}

#[test]
fn the_installer_writes_the_current_users_registry_and_removes_what_it_wrote() {
    let script = packaging(INSTALLER_SCRIPT);

    assert!(holds_the_line(&script, "PrivilegesRequired=lowest"));
    assert!(!script.contains("PrivilegesRequiredOverridesAllowed"));
    assert!(holds_the_line(&script, "ChangesAssociations=yes"));
    for line in registry_lines(&script) {
        let key = field(line, "Subkey").expect("a subkey");
        let beneath_a_key_that_goes = key.starts_with("Software\\Classes\\{#ProgramId}")
            || key.starts_with("Software\\Classes\\{#LinkScheme}")
            || key.starts_with("Software\\Classes\\Applications\\{#Executable}")
            || key.starts_with("Software\\{#AppName}");

        assert!(line.starts_with("Root: HKCU;"), "{line} is not the user's");
        assert!(
            line.contains("uninsdeletekey")
                || line.contains("uninsdeletevalue")
                || beneath_a_key_that_goes,
            "{line} would outlive the uninstall"
        );
    }
}

#[test]
fn the_installer_hands_resonate_links_to_the_program_as_the_desktop_entry_does() {
    let script = packaging(INSTALLER_SCRIPT);
    let entry = packaging("resonate.desktop");

    let scheme = script
        .lines()
        .find_map(|line| line.strip_prefix("#define LinkScheme "))
        .map(|value| value.trim_matches('"'))
        .expect("the installer script declares the scheme it takes");
    let rows: Vec<&str> = registry_lines(&script)
        .into_iter()
        .filter(|line| {
            field(line, "Subkey")
                .is_some_and(|key| key.starts_with("Software\\Classes\\{#LinkScheme}"))
        })
        .collect();

    assert!(entry.contains(&format!("x-scheme-handler/{scheme};")));
    assert!(
        rows.iter()
            .any(|line| field(line, "ValueName") == Some("URL Protocol")),
        "the scheme is not declared a URL protocol"
    );
    assert!(
        rows.iter().any(|line| line.contains("shell\\open\\command")
            && line.contains("\"\"{app}\\{#Executable}\"\" \"\"%1\"\"")),
        "the scheme's command does not pass the link as an argument"
    );
}

#[test]
fn the_start_menu_shortcut_carries_the_identity_the_toasts_are_raised_under() {
    let script = packaging(INSTALLER_SCRIPT);

    let declared = script
        .lines()
        .find_map(|line| line.strip_prefix("#define AppUserModelId "))
        .map(|value| value.trim_matches('"'))
        .expect("the installer script declares the identity");
    let shortcut = script
        .lines()
        .find(|line| line.starts_with("Name: \"{autoprograms}"))
        .expect("the installer makes a Start menu shortcut");

    assert_eq!(declared, APP_USER_MODEL_ID);
    assert!(
        shortcut.contains("AppUserModelID: \"{#AppUserModelId}\""),
        "the shortcut does not carry the identity"
    );
}

#[test]
fn the_build_script_compiles_one_installer_per_architecture_beside_its_archive() {
    let build = packaging(BUILD_SCRIPT);
    let script = packaging(INSTALLER_SCRIPT);

    assert!(
        build.contains("Installs = \"x64os\"") && build.contains("Installs = \"arm64\""),
        "build.ps1 does not install each archive's architecture alone"
    );
    assert!(build.contains("$name-setup") && build.contains("resonate.iss"));
    for definition in BUILD_DEFINITIONS {
        assert!(
            build.contains(&format!("/D{definition}=")),
            "build.ps1 does not define {definition}"
        );
        assert!(
            script.contains(&format!("{{#{definition}}}")),
            "resonate.iss does not read {definition}"
        );
    }
    assert!(script.contains("ArchitecturesAllowed={#InstallArchitecture}"));
    assert!(holds_the_line(&script, "AppPublisher=Resonate"));
}

#[test]
fn the_icon_holds_the_sizes_windows_asks_for_as_png_of_those_sides() {
    let images = icon_images();

    for side in ICON_SIDES_REQUIRED {
        assert!(images.iter().any(|(held, _)| *held == side), "no {side} px");
    }
    for (side, png) in images {
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
            signing.contains(&format!("$env:{variable}")),
            "sign.ps1 does not read {variable}"
        );
    }
    assert!(
        signing.contains("IsNullOrEmpty") && signing.contains("return"),
        "sign.ps1 must leave the files unsigned, not fail, where no certificate exists"
    );
}

#[test]
fn the_release_workflow_fills_the_manifests_before_the_checksums_and_only_with_the_files_built() {
    let workflow = read(&root_path(WORKFLOW));

    assert!(
        workflow.contains("needs.windows-zip.result == 'success'"),
        "the manifests would be filled from files a failed job never built"
    );
    assert!(
        workflow.find("fill-manifests.sh") < workflow.find("sha256sum -- *"),
        "the manifests would be left out of SHA256SUMS"
    );
}

#[test]
fn nothing_the_windows_installer_is_made_of_names_a_maintainer_or_a_repository() {
    for name in MANIFEST_TEMPLATES.into_iter().chain([
        INSTALLER_SCRIPT,
        BUILD_SCRIPT,
        SIGNING_SCRIPT,
        FILLING_SCRIPT,
        "windows/README.txt",
    ]) {
        let text = packaging(name);

        assert!(!text.contains("github.com"), "{name} names a repository");
    }
    for name in MANIFEST_TEMPLATES {
        for placeholder in placeholders(&packaging(name)) {
            assert!(
                placeholder.chars().all(|letter| {
                    letter.is_ascii_uppercase() || letter.is_ascii_digit() || letter == '_'
                }),
                "{name} holds an address or an unnamed placeholder: @{placeholder}@"
            );
        }
    }
}

#[test]
fn every_placeholder_a_manifest_template_holds_is_filled_by_the_script() {
    let filling = packaging(FILLING_SCRIPT);

    for name in MANIFEST_TEMPLATES {
        for placeholder in placeholders(&packaging(name)) {
            assert!(
                filling.contains(&format!("@{placeholder}@")),
                "{name} holds @{placeholder}@, which fill-manifests.sh never fills"
            );
        }
    }
    for asset in [
        "x86_64_v3-windows.zip",
        "aarch64-windows.zip",
        "x86_64_v3-windows-setup.exe",
        "aarch64-windows-setup.exe",
    ] {
        assert!(
            filling.contains(asset),
            "{asset} is not an asset build.ps1 makes"
        );
    }
}

#[test]
fn the_installers_product_code_is_the_one_the_winget_manifest_is_given() {
    let script = packaging(INSTALLER_SCRIPT);
    let filling = packaging(FILLING_SCRIPT);

    assert!(
        script
            .lines()
            .any(|line| line.starts_with("AppId={{") && line.ends_with('}'))
    );
    assert!(filling.contains("AppId={{"));
    assert!(filling.contains("{$app_id}_is1"));
    assert!(
        packaging("windows/winget/Resonate.Resonate.installer.yaml").contains("@PRODUCT_CODE@")
    );
}
