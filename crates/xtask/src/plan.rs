#[cfg(test)]
mod tests;

use std::{collections::BTreeSet, env, path::Path};

pub const BINARY: &str = "resonate";
pub const WINDOW: &str = "resonate-ui";
pub const RULES_AND_COMMENTS: &str = "resonate-words-build";
pub const SYNC: &str = "resonate-sync";
pub const OFF_LINUX: [&str; 3] = ["resonate-mobile", "resonate-bindgen", "resonate-pipewire"];
pub const BINARY_FEATURES_TOGETHER: &str = "online,mcp,discord,sync";
pub const BINARY_FEATURES_ALONE: [&str; 5] = ["ui", "online", "mcp", "discord", "sync"];

pub const DSP_LAB: &str = "resonate-dsp-lab";
pub const DSP_LAB_FOLDER: &str = "dsp-lab";
const DSP_LAB_MANIFEST: &str = "dsp-lab/Cargo.toml";
const DSP_LAB_NEEDS: [&str; 2] = ["resonate-dsp", "resonate-engine"];
const NO_REFERENCES: &str = "--no-default-features";

const DETACHED: [&str; 2] = ["fuzz", DSP_LAB_FOLDER];
const SESSION_BUS: &str = "dbus-run-session";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub program: Program,
    pub args: Vec<String>,
    pub summary: Summary,
}

impl Step {
    fn new(
        name: impl Into<String>,
        program: Program,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            program,
            args: args.into_iter().map(Into::into).collect(),
            summary: Summary::Nothing,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Cargo,
    Formatter,
    Python,
    Gradle,
    SessionBus,
}

impl Program {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Formatter => "rust-formatter",
            Self::Python => "python3",
            Self::Gradle => "android/gradlew",
            Self::SessionBus => SESSION_BUS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Summary {
    Nothing,
    Nextest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Workspace,
    These(BTreeSet<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Host {
    pub linux: bool,
    pub bus: bool,
}

impl Host {
    pub fn here() -> Self {
        let linux = cfg!(target_os = "linux");

        Self {
            linux,
            bus: linux && on_the_path(SESSION_BUS),
        }
    }
}

fn on_the_path(program: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|folder| Path::new(&folder).join(program).is_file())
    })
}

pub fn format(check: bool) -> Vec<Step> {
    let checked: &[&str] = if check { &["--check"] } else { &[] };
    let verb = if check { "format check" } else { "format" };

    [Step::new(verb, Program::Formatter, checked.iter().copied())]
        .into_iter()
        .chain(DETACHED.iter().map(|folder| {
            Step::new(
                format!("{verb} {folder}/"),
                Program::Formatter,
                checked.iter().copied().chain([*folder]),
            )
        }))
        .collect()
}

struct Linted {
    every: Option<Vec<String>>,
    headless: Option<Vec<String>>,
    binary: bool,
    sync: bool,
    lab: bool,
}

fn lab_wanted<'a>(packages: impl IntoIterator<Item = &'a str>) -> bool {
    packages
        .into_iter()
        .any(|package| package == DSP_LAB || DSP_LAB_NEEDS.contains(&package))
}

fn linted(selection: &Selection, linux: bool) -> Option<Linted> {
    match selection {
        Selection::Workspace => {
            let excluded = if linux { &[][..] } else { &OFF_LINUX[..] };
            let every = workspace_except(excluded.iter().copied());
            let headless = workspace_except([WINDOW].into_iter().chain(excluded.iter().copied()));

            Some(Linted {
                every: Some(every),
                headless: Some(headless),
                binary: true,
                sync: true,
                lab: linux,
            })
        }
        Selection::These(packages) => {
            let lab = linux && lab_wanted(packages.iter().map(String::as_str));
            let kept: BTreeSet<&str> = packages
                .iter()
                .map(String::as_str)
                .filter(|package| *package != DSP_LAB)
                .filter(|package| linux || !OFF_LINUX.contains(package))
                .collect();
            let headless: Vec<&str> = kept
                .iter()
                .copied()
                .filter(|package| *package != WINDOW)
                .collect();

            (!kept.is_empty() || lab).then(|| Linted {
                every: (!kept.is_empty()).then(|| each_package(kept.iter().copied())),
                headless: (!headless.is_empty()).then(|| each_package(headless)),
                binary: kept.contains(BINARY),
                sync: kept.contains(SYNC),
                lab,
            })
        }
    }
}

fn workspace_except<'a>(excluded: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut args = vec!["--workspace".to_owned()];

    for package in excluded {
        args.extend(["--exclude".to_owned(), package.to_owned()]);
    }
    args
}

fn each_package<'a>(packages: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    packages
        .into_iter()
        .flat_map(|package| ["--package".to_owned(), package.to_owned()])
        .collect()
}

fn is_desktop_linux(target: &str) -> bool {
    target.contains("-linux-") && !target.contains("android")
}

pub fn lint(selection: &Selection, target: Option<&str>, locked: bool) -> Vec<Step> {
    let linux = target.map_or(cfg!(target_os = "linux"), is_desktop_linux);
    let Some(linted) = linted(selection, linux) else {
        return Vec::new();
    };

    let clippy = |what: &str, packages: &[String], features: &[&str]| {
        let mut args = vec!["clippy".to_owned()];

        if locked {
            args.push("--locked".to_owned());
        }
        if let Some(target) = target {
            args.extend(["--target".to_owned(), target.to_owned()]);
        }
        args.extend(packages.iter().cloned());
        args.push("--all-targets".to_owned());
        args.extend(features.iter().map(|feature| (*feature).to_owned()));
        args.extend(["--", "-D", "warnings"].map(str::to_owned));

        let name = match target {
            Some(target) => format!("clippy {what} ({target})"),
            None => format!("clippy {what}"),
        };
        Step::new(name, Program::Cargo, args)
    };

    let mut steps = Vec::new();

    if let Some(every) = &linted.every {
        steps.push(clippy("default features", every, &[]));
    }

    if let Some(headless) = &linted.headless {
        steps.push(clippy("headless", headless, &["--no-default-features"]));
    }
    if linted.binary {
        let binary = each_package([BINARY]);
        for features in [BINARY_FEATURES_TOGETHER]
            .into_iter()
            .chain(BINARY_FEATURES_ALONE)
        {
            steps.push(clippy(
                &format!("binary with {features}"),
                &binary,
                &["--no-default-features", "--features", features],
            ));
        }
    }
    if linted.sync {
        steps.push(clippy(
            "sync without its device feature",
            &each_package([SYNC]),
            &["--no-default-features"],
        ));
    }
    if linted.lab {
        steps.push(clippy(
            "dsp-lab without its references",
            &lab_manifest(),
            &[NO_REFERENCES],
        ));
    }
    steps
}

fn lab_manifest() -> Vec<String> {
    ["--manifest-path", DSP_LAB_MANIFEST]
        .map(str::to_owned)
        .to_vec()
}

pub fn test(selection: &Selection, locked: bool, passed_on: &[String], host: Host) -> Vec<Step> {
    let lab = host.linux
        && match selection {
            Selection::Workspace => true,
            Selection::These(packages) => lab_wanted(packages.iter().map(String::as_str)),
        };
    let packages = match selection {
        Selection::Workspace if host.linux => workspace_except([]),
        Selection::Workspace => workspace_except(OFF_LINUX),
        Selection::These(packages) => each_package(
            packages
                .iter()
                .map(String::as_str)
                .chain([RULES_AND_COMMENTS])
                .filter(|package| *package != DSP_LAB)
                .filter(|package| host.linux || !OFF_LINUX.contains(package))
                .collect::<BTreeSet<_>>(),
        ),
    };

    let mut nextest = vec!["nextest".to_owned(), "run".to_owned()];

    if locked {
        nextest.push("--locked".to_owned());
    }
    nextest.extend(packages);
    nextest.extend(passed_on.iter().cloned());

    let step = if host.bus {
        Step::new(
            "nextest under a session bus",
            Program::SessionBus,
            ["--".to_owned(), Program::Cargo.path().to_owned()]
                .into_iter()
                .chain(nextest),
        )
    } else {
        Step::new("nextest", Program::Cargo, nextest)
    };

    let mut steps = vec![Step {
        summary: Summary::Nextest,
        ..step
    }];

    if lab {
        let mut nextest = vec!["nextest".to_owned(), "run".to_owned()];

        if locked {
            nextest.push("--locked".to_owned());
        }
        nextest.extend(lab_manifest());
        nextest.extend([NO_REFERENCES, "--no-tests", "pass"].map(str::to_owned));
        nextest.extend(passed_on.iter().cloned());

        steps.push(Step {
            summary: Summary::Nextest,
            ..Step::new("nextest dsp-lab", Program::Cargo, nextest)
        });
    }
    steps
}

pub fn audit() -> Vec<Step> {
    vec![Step::new("advisories", Program::Cargo, ["audit"])]
}

pub fn android(sdk: &str) -> Vec<Step> {
    vec![
        Step::new(
            "android debug APK, lint and unit tests",
            Program::Gradle,
            [
                "-p",
                "android",
                ":app:assembleDebug",
                ":app:lintDebug",
                ":app:testDebugUnitTest",
            ],
        ),
        Step::new(
            "android native alignment and clippy",
            Program::Python,
            ["android/check-native.py", "--sdk", sdk],
        ),
    ]
}

pub fn ios(build: bool) -> Vec<Step> {
    let mut steps = vec![
        Step::new(
            "ios host tests",
            Program::Python,
            ["-m", "unittest", "discover", "-s", "ios"],
        ),
        Step::new("ios swift logic", Program::Python, ["ios/test-logic.py"]),
    ];

    if build {
        steps.extend([
            Step::new(
                "ios native library and bindings",
                Program::Python,
                ["ios/build-native.py"],
            ),
            Step::new("ios clean build", Program::Python, ["ios/check-build.py"]),
        ]);
    }
    steps
}
