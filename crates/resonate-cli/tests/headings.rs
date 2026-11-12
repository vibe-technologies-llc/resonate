const SECTION: &str = ".SH ";

#[test]
fn every_heading_a_man_page_carries_is_said_in_the_language_spoken() {
    let command = resonate_cli::command();
    let mut pages = vec![command.clone()];
    pages.extend(command.get_subcommands().cloned());

    let mut unsaid = Vec::new();
    for page in pages {
        let mut written = Vec::new();
        clap_mangen::Man::new(page)
            .render(&mut written)
            .expect("a man page renders");
        let written = String::from_utf8(written).expect("a man page is UTF-8");
        unsaid.extend(
            written
                .lines()
                .filter_map(|line| line.strip_prefix(SECTION))
                .filter(|heading| resonate_cli::man_heading(heading).is_none())
                .map(str::to_owned),
        );
    }

    assert!(unsaid.is_empty(), "headings no word says: {unsaid:?}");
}
