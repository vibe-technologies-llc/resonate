use std::collections::BTreeSet;

use super::{Host, Program, Selection, Step, Summary, format, lint, test};

fn these(packages: &[&str]) -> Selection {
    Selection::These(
        packages
            .iter()
            .map(|package| (*package).to_owned())
            .collect(),
    )
}

fn line(step: &Step) -> String {
    format!("{} {}", step.program.path(), step.args.join(" "))
}

const LINUX: Host = Host {
    linux: true,
    bus: false,
};

#[test]
fn the_whole_workspace_is_linted_by_every_line_ci_runs() {
    let steps = lint(
        &Selection::Workspace,
        Some("x86_64-unknown-linux-gnu"),
        true,
    );

    let lines: Vec<String> = steps.iter().map(line).collect();

    assert_eq!(steps.len(), 8);
    assert_eq!(
        lines[0],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --workspace --all-targets -- -D warnings"
    );
    assert_eq!(
        lines[1],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --workspace --exclude resonate-ui --all-targets --no-default-features -- -D warnings"
    );
    assert_eq!(
        lines[2],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --package resonate --all-targets --no-default-features --features online,mcp,discord,sync -- -D warnings"
    );
    assert!(
        lines[3..]
            .iter()
            .zip(["ui", "online", "mcp", "discord", "sync"])
            .all(|(line, feature)| line.contains(&format!("--features {feature} --")))
    );
}

#[test]
fn off_linux_the_phones_boundary_its_binding_tool_and_pipewire_are_left_out() {
    let steps = lint(
        &Selection::Workspace,
        Some("aarch64-pc-windows-msvc"),
        false,
    );
    let changed = lint(
        &these(&["resonate-pipewire", "resonate-mobile"]),
        Some("aarch64-apple-darwin"),
        false,
    );

    assert!(line(&steps[0]).contains(
        "--workspace --exclude resonate-mobile --exclude resonate-bindgen --exclude resonate-pipewire --all-targets"
    ));
    assert!(line(&steps[1]).contains(
        "--workspace --exclude resonate-ui --exclude resonate-mobile --exclude resonate-bindgen --exclude resonate-pipewire --all-targets"
    ));
    assert!(changed.is_empty());
}

#[test]
fn changed_packages_are_linted_alone_and_the_binary_lines_come_only_with_the_binary() {
    let library = lint(&these(&["resonate-library", "resonate-ui"]), None, false);
    let window = lint(&these(&["resonate-ui"]), None, false);
    let binary = lint(&these(&["resonate"]), None, false);

    assert_eq!(library.len(), 2);
    assert!(
        line(&library[0])
            .contains("--package resonate-library --package resonate-ui --all-targets")
    );
    assert!(
        line(&library[1])
            .contains("--package resonate-library --all-targets --no-default-features")
    );
    assert_eq!(window.len(), 1);
    assert_eq!(binary.len(), 8);
    assert!(lint(&Selection::These(BTreeSet::new()), None, false).is_empty());
}

#[test]
fn a_changed_test_run_always_holds_the_comment_and_rules_checks() {
    let steps = test(&these(&["resonate-sync"]), false, &[], LINUX);

    assert_eq!(steps.len(), 1);
    assert_eq!(
        line(&steps[0]),
        "cargo nextest run --package resonate-sync --package resonate-words-build"
    );
    assert_eq!(steps[0].summary, Summary::Nextest);
}

#[test]
fn the_tests_run_under_a_session_bus_where_there_is_one_and_take_what_is_passed_on() {
    let bus = Host {
        linux: true,
        bus: true,
    };
    let elsewhere = Host {
        linux: false,
        bus: false,
    };

    let under_bus = test(&Selection::Workspace, true, &["sync::".to_owned()], bus);
    let off_linux = test(&Selection::Workspace, false, &[], elsewhere);

    assert_eq!(under_bus[0].program, Program::SessionBus);
    assert_eq!(
        line(&under_bus[0]),
        "dbus-run-session -- cargo nextest run --locked --workspace sync::"
    );
    assert_eq!(
        line(&off_linux[0]),
        "cargo nextest run --workspace --exclude resonate-mobile --exclude resonate-bindgen --exclude resonate-pipewire"
    );
}

#[test]
fn formatting_covers_the_workspace_and_the_detached_fuzz_project() {
    let checked: Vec<String> = format(true).iter().map(line).collect();
    let written: Vec<String> = format(false).iter().map(line).collect();

    assert_eq!(
        checked,
        ["rust-formatter --check", "rust-formatter --check fuzz"]
    );
    assert_eq!(written, ["rust-formatter ", "rust-formatter fuzz"]);
}
