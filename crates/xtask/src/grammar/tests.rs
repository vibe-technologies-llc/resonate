use super::{DEFAULT_SINCE, Scope, Task, parsed};
use crate::{error::Error, run::Output};

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn a_task_alone_runs_over_the_whole_workspace_and_keeps_its_output() {
    let asked = parsed(words("lint")).unwrap();

    assert_eq!(asked.task, Task::Lint);
    assert_eq!(asked.scope, Scope::Whole);
    assert_eq!(asked.output, Output::Kept);
    assert!(!asked.locked && !asked.keep_going);
}

#[test]
fn changed_compares_against_origin_master_unless_since_names_another() {
    let changed = parsed(words("check --changed")).unwrap();
    let since = parsed(words("test --since HEAD~3")).unwrap();

    assert_eq!(
        changed.scope,
        Scope::Changed {
            since: DEFAULT_SINCE.to_owned()
        }
    );
    assert_eq!(
        since.scope,
        Scope::Changed {
            since: "HEAD~3".to_owned()
        }
    );
}

#[test]
fn what_follows_two_dashes_is_handed_to_nextest_whole() {
    let asked = parsed(words("test --locked -- sync:: --no-capture")).unwrap();

    assert!(asked.locked);
    assert_eq!(asked.passed_on, words("sync:: --no-capture"));
}

#[test]
fn a_flag_is_refused_where_its_task_has_no_use_for_it() {
    let format_target = parsed(words("fmt --target aarch64-apple-darwin"));
    let lint_check = parsed(words("lint --check"));
    let check_passed_on = parsed(words("check -- sync::"));

    assert!(matches!(
        format_target,
        Err(Error::NotForTask {
            flag: "--target",
            task: "fmt"
        })
    ));
    assert!(matches!(
        lint_check,
        Err(Error::NotForTask {
            flag: "--check",
            task: "lint"
        })
    ));
    assert!(matches!(
        check_passed_on,
        Err(Error::NotForTask {
            flag: "--",
            task: "check"
        })
    ));
}

#[test]
fn an_unknown_word_a_second_task_and_a_missing_value_are_each_refused() {
    let unknown = parsed(words("lnit"));
    let second = parsed(words("lint test"));
    let flag = parsed(words("lint --fast"));
    let missing = parsed(words("lint --target"));
    let nothing = parsed(words(""));

    assert!(matches!(unknown, Err(Error::UnknownTask { word }) if word == "lnit"));
    assert!(matches!(second, Err(Error::UnknownTask { word }) if word == "test"));
    assert!(matches!(flag, Err(Error::UnknownFlag { flag }) if flag == "--fast"));
    assert!(matches!(
        missing,
        Err(Error::MissingValue { flag: "--target" })
    ));
    assert!(matches!(nothing, Err(Error::NoTask)));
}

#[test]
fn help_is_answered_whatever_else_is_asked() {
    let word = parsed(words("help")).unwrap();
    let flag = parsed(words("lint --check --help")).unwrap();
    let verbose = parsed(words("ios --build -v")).unwrap();

    assert_eq!(word.task, Task::Help);
    assert_eq!(flag.task, Task::Help);
    assert_eq!(verbose.output, Output::Streamed);
    assert!(verbose.build);
}

#[test]
fn audit_is_a_task_that_takes_no_cargo_flag() {
    let asked = parsed(words("audit")).unwrap();
    let locked = parsed(words("audit --locked"));

    assert_eq!(asked.task, Task::Audit);
    assert!(matches!(
        locked,
        Err(Error::NotForTask {
            flag: "--locked",
            task: "audit"
        })
    ));
}
