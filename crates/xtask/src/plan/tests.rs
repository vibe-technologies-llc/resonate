use std::collections::BTreeSet;

use super::{DSP_LAB, Host, Program, Selection, Step, Summary, audit, format, lint, test};

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

const LINUX_TARGET: &str = "x86_64-unknown-linux-gnu";

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

    assert_eq!(steps.len(), 11);
    assert_eq!(
        lines[0],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --workspace --all-targets -- -D warnings"
    );
    assert_eq!(
        lines[1],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --workspace --all-targets --target-dir target/fuzzing -- -D warnings --cfg fuzzing"
    );
    assert_eq!(
        lines[2],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --workspace --exclude resonate-ui --all-targets --no-default-features -- -D warnings"
    );
    assert_eq!(
        lines[3],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --package resonate --all-targets --no-default-features --features online,mcp,discord,sync -- -D warnings"
    );
    assert!(
        lines[4..9]
            .iter()
            .zip(["ui", "online", "mcp", "discord", "sync"])
            .all(|(line, feature)| line.contains(&format!("--features {feature} --")))
    );
    assert_eq!(
        lines[9],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --package resonate-sync --all-targets --no-default-features -- -D warnings"
    );
    assert_eq!(
        lines[10],
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --manifest-path dsp-lab/Cargo.toml --all-targets --no-default-features -- -D warnings"
    );
}

#[test]
fn the_fuzzing_seams_are_linted_with_the_cfg_the_fuzz_build_sets_in_a_target_directory_of_their_own()
 {
    let steps = lint(&Selection::Workspace, Some(LINUX_TARGET), false);
    let fuzzing = steps
        .iter()
        .filter(|step| line(step).contains("--cfg fuzzing"))
        .collect::<Vec<_>>();

    assert_eq!(fuzzing.len(), 1);
    assert_eq!(
        fuzzing[0].name,
        "clippy fuzzing seams (x86_64-unknown-linux-gnu)"
    );
    assert!(line(fuzzing[0]).ends_with("--target-dir target/fuzzing -- -D warnings --cfg fuzzing"));
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
    assert!(steps.iter().all(|step| !line(step).contains("dsp-lab")));
    assert!(steps.iter().all(|step| !line(step).contains("fuzzing")));
}

#[test]
fn changed_packages_are_linted_alone_and_the_binary_lines_come_only_with_the_binary() {
    let library = lint(
        &these(&["resonate-library", "resonate-ui"]),
        Some(LINUX_TARGET),
        false,
    );
    let window = lint(&these(&["resonate-ui"]), Some(LINUX_TARGET), false);
    let binary = lint(&these(&["resonate"]), Some(LINUX_TARGET), false);

    assert_eq!(library.len(), 3);
    assert!(
        line(&library[0])
            .contains("--package resonate-library --package resonate-ui --all-targets")
    );
    assert!(line(&library[1]).contains(
        "--package resonate-library --package resonate-ui --all-targets --target-dir target/fuzzing"
    ));
    assert!(
        line(&library[2])
            .contains("--package resonate-library --all-targets --no-default-features")
    );
    assert_eq!(window.len(), 2);
    assert_eq!(binary.len(), 9);
    assert!(
        lint(
            &Selection::These(BTreeSet::new()),
            Some(LINUX_TARGET),
            false
        )
        .is_empty()
    );
}

#[test]
fn the_sync_crate_is_linted_again_as_the_server_links_it_without_its_device_feature() {
    let sync = lint(
        &these(&["resonate-server", "resonate-sync"]),
        Some(LINUX_TARGET),
        false,
    );
    let server = lint(&these(&["resonate-server"]), Some(LINUX_TARGET), false);

    assert_eq!(sync.len(), 4);
    assert_eq!(
        line(&sync[3]),
        "cargo clippy --target x86_64-unknown-linux-gnu --package resonate-sync --all-targets --no-default-features -- -D warnings"
    );
    assert_eq!(server.len(), 3);
}

#[test]
fn the_dsp_lab_is_linted_and_tested_without_its_references_whenever_it_or_what_it_measures_changed()
{
    let lab = lint(&these(&[DSP_LAB]), Some(LINUX_TARGET), true);
    let measured = lint(&these(&["resonate-engine"]), Some(LINUX_TARGET), false);
    let unrelated = lint(&these(&["resonate-sync"]), Some(LINUX_TARGET), false);
    let tested = test(&these(&[DSP_LAB]), true, &[], LINUX);
    let alone = test(&these(&["resonate-sync"]), false, &[], LINUX);

    assert_eq!(lab.len(), 1);
    assert_eq!(
        line(&lab[0]),
        "cargo clippy --locked --target x86_64-unknown-linux-gnu --manifest-path dsp-lab/Cargo.toml --all-targets --no-default-features -- -D warnings"
    );
    assert!(line(&measured[measured.len() - 1]).contains("--manifest-path dsp-lab/Cargo.toml"));
    assert!(unrelated.iter().all(|step| !line(step).contains("dsp-lab")));
    assert_eq!(tested.len(), 2);
    assert_eq!(
        line(&tested[0]),
        "cargo nextest run --locked --package resonate-words-build"
    );
    assert_eq!(
        line(&tested[1]),
        "cargo nextest run --locked --manifest-path dsp-lab/Cargo.toml --no-default-features --no-tests pass"
    );
    assert_eq!(tested[1].summary, Summary::Nextest);
    assert_eq!(alone.len(), 1);
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
    assert_eq!(off_linux.len(), 1);
    assert_eq!(
        line(&under_bus[1]),
        "cargo nextest run --locked --manifest-path dsp-lab/Cargo.toml --no-default-features --no-tests pass sync::"
    );
}

#[test]
fn formatting_covers_the_workspace_and_every_detached_project() {
    let checked: Vec<String> = format(true).iter().map(line).collect();
    let written: Vec<String> = format(false).iter().map(line).collect();

    assert_eq!(
        checked,
        [
            "rust-formatter --check",
            "rust-formatter --check fuzz",
            "rust-formatter --check dsp-lab"
        ]
    );
    assert_eq!(
        written,
        [
            "rust-formatter ",
            "rust-formatter fuzz",
            "rust-formatter dsp-lab"
        ]
    );
}

#[test]
fn the_advisories_are_one_cargo_audit_step() {
    let steps: Vec<String> = audit().iter().map(line).collect();

    assert_eq!(steps, ["cargo audit"]);
}
