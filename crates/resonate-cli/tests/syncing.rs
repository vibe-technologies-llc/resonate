use clap::error::ErrorKind;

fn parsed(arguments: &[&str]) -> Result<clap::ArgMatches, clap::Error> {
    resonate_cli::command().try_get_matches_from(
        ["resonate", "sync"]
            .into_iter()
            .chain(arguments.iter().copied()),
    )
}

#[test]
fn a_source_device_alone_or_beside_a_handoff_is_refused() {
    let alone = parsed(&["--from", "Desk"]).expect_err("refused");
    let beside_a_handoff = parsed(&["--handoff", "Phone", "--from", "Desk"]).expect_err("refused");

    assert_eq!(alone.kind(), ErrorKind::MissingRequiredArgument);
    assert_eq!(beside_a_handoff.kind(), ErrorKind::ArgumentConflict);
}

#[test]
fn a_fetch_and_a_handoff_still_parse_in_their_own_forms() {
    assert!(parsed(&["--fetch", "Mix", "--from", "Desk"]).is_ok());
    assert!(parsed(&["--handoff", "Phone"]).is_ok());
    assert!(parsed(&["--fetch", "Mix"]).is_err());
}
