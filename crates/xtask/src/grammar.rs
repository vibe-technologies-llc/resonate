#[cfg(test)]
mod tests;

use crate::{error::Error, run::Output};

pub const DEFAULT_SINCE: &str = "origin/master";

pub const USAGE: &str = "\
cargo xtask <task> [flags]

tasks:
  fmt [--check]             rust-formatter over the workspace, fuzz/ and dsp-lab/
  lint                      every clippy line CI runs, -D warnings, and the DSP lab's on Linux
  test [-- <nextest args>]  cargo nextest run over the workspace and the DSP lab, under dbus-run-session where there is one
  audit                     cargo audit over Cargo.lock against the advisory database, the ignores in .cargo/audit.toml
  check                     fmt --check, audit, lint and test: the gate before a push
  android                   the debug APK, its lint and unit tests, then the native check (ANDROID_HOME)
  ios [--build]             the iOS host tests and the Swift logic; with --build the native library and a clean build
  help                      this

flags:
  --changed                 lint and test only the packages changed since origin/master and those
                            depending on them; resonate-words-build is always tested
  --since <rev>             what --changed compares against (implies --changed)
  --target <triple>         lint for another target; off Linux the phones' boundary, its binding
                            tool and PipeWire are left out
  --locked                  pass --locked to cargo
  --keep-going              run every step though one fails
  -v, --verbose             stream each step's output instead of keeping it under target/xtask/
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Format,
    Lint,
    Test,
    Audit,
    Check,
    Android,
    Ios,
    Help,
}

impl Task {
    const ALL: [Self; 8] = [
        Self::Format,
        Self::Lint,
        Self::Test,
        Self::Audit,
        Self::Check,
        Self::Android,
        Self::Ios,
        Self::Help,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Format => "fmt",
            Self::Lint => "lint",
            Self::Test => "test",
            Self::Audit => "audit",
            Self::Check => "check",
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Help => "help",
        }
    }

    fn named(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|task| task.word() == word)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Whole,
    Changed { since: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    pub task: Task,
    pub check: bool,
    pub build: bool,
    pub scope: Scope,
    pub target: Option<String>,
    pub locked: bool,
    pub keep_going: bool,
    pub output: Output,
    pub passed_on: Vec<String>,
}

#[derive(Default)]
struct Flags {
    check: bool,
    build: bool,
    changed: bool,
    since: Option<String>,
    target: Option<String>,
    locked: bool,
    keep_going: bool,
    verbose: bool,
    help: bool,
    passed_on: Vec<String>,
}

pub fn parsed(words: impl IntoIterator<Item = String>) -> Result<Asked, Error> {
    let mut words = words.into_iter();
    let mut flags = Flags::default();
    let mut task = None;

    while let Some(word) = words.next() {
        match word.as_str() {
            "--" => {
                flags.passed_on.extend(words.by_ref());
                break;
            }
            "--check" => flags.check = true,
            "--build" => flags.build = true,
            "--changed" => flags.changed = true,
            "--since" => flags.since = Some(value_of(&mut words, "--since")?),
            "--target" => flags.target = Some(value_of(&mut words, "--target")?),
            "--locked" => flags.locked = true,
            "--keep-going" => flags.keep_going = true,
            "-v" | "--verbose" => flags.verbose = true,
            "-h" | "--help" => flags.help = true,
            flag if flag.starts_with('-') => return Err(Error::UnknownFlag { flag: word }),
            named => match (task, Task::named(named)) {
                (None, Some(named)) => task = Some(named),
                _ => return Err(Error::UnknownTask { word }),
            },
        }
    }

    let task = match (flags.help, task) {
        (true, _) => Task::Help,
        (false, Some(task)) => task,
        (false, None) => return Err(Error::NoTask),
    };

    refuse_what_does_not_apply(task, &flags)?;
    Ok(asked(task, flags))
}

fn value_of(words: &mut impl Iterator<Item = String>, flag: &'static str) -> Result<String, Error> {
    words.next().ok_or(Error::MissingValue { flag })
}

fn refuse_what_does_not_apply(task: Task, flags: &Flags) -> Result<(), Error> {
    if task == Task::Help {
        return Ok(());
    }

    let cargo = [Task::Lint, Task::Test, Task::Check];
    let given: [(&'static str, bool, &[Task]); 6] = [
        ("--check", flags.check, &[Task::Format]),
        ("--build", flags.build, &[Task::Ios]),
        ("--target", flags.target.is_some(), &[Task::Lint]),
        ("--changed", flags.changed || flags.since.is_some(), &cargo),
        ("--locked", flags.locked, &cargo),
        ("--", !flags.passed_on.is_empty(), &[Task::Test]),
    ];

    match given
        .into_iter()
        .find(|(_, set, tasks)| *set && !tasks.contains(&task))
    {
        Some((flag, ..)) => Err(Error::NotForTask {
            flag,
            task: task.word(),
        }),
        None => Ok(()),
    }
}

fn asked(task: Task, flags: Flags) -> Asked {
    let scope = match (flags.changed, flags.since) {
        (_, Some(since)) => Scope::Changed { since },
        (true, None) => Scope::Changed {
            since: DEFAULT_SINCE.to_owned(),
        },
        (false, None) => Scope::Whole,
    };

    Asked {
        task,
        check: flags.check,
        build: flags.build,
        scope,
        target: flags.target,
        locked: flags.locked,
        keep_going: flags.keep_going,
        output: if flags.verbose {
            Output::Streamed
        } else {
            Output::Kept
        },
        passed_on: flags.passed_on,
    }
}
