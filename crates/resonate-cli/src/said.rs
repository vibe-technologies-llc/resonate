use std::ffi::OsString;

use clap::{Arg, ArgAction, Command, CommandFactory as _, FromArgMatches as _};

use crate::{Cli, words};

const HELP: &str = "help";
const VERSION: &str = "version";

const SAID_LAST: usize = usize::MAX;

const BEFORE_HELP: &str = "{before-help}";
const ABOUT: &str = "{about-with-newline}";
const USAGE: &str = "{usage}";
const SUBCOMMANDS: &str = "{subcommands}";
const POSITIONALS: &str = "{positionals}";
const OPTIONS: &str = "{options}";
const AFTER_HELP: &str = "{after-help}";

pub fn command() -> Command {
    let command = said(Cli::command());
    command.disable_version_flag(true).arg(
        Arg::new(VERSION)
            .short('V')
            .long(VERSION)
            .action(ArgAction::Version)
            .display_order(SAID_LAST)
            .help(words::said::print_version()),
    )
}

pub fn parsed_from(arguments: impl IntoIterator<Item = impl Into<OsString> + Clone>) -> Cli {
    let mut command = command();
    let mut matches = command.clone().get_matches_from(arguments);
    Cli::from_arg_matches_mut(&mut matches)
        .unwrap_or_else(|refused| refused.format(&mut command).exit())
}

fn said(command: Command) -> Command {
    let template = template_for(&command);
    command
        .help_template(template)
        .disable_help_flag(true)
        .arg(
            Arg::new(HELP)
                .short('h')
                .long(HELP)
                .action(ArgAction::Help)
                .display_order(SAID_LAST)
                .help(words::said::print_help()),
        )
        .mut_subcommands(said)
}

fn template_for(command: &Command) -> String {
    let mut template = format!("{BEFORE_HELP}{ABOUT}\n{} {USAGE}\n", words::said::usage());
    if command.has_subcommands() {
        template.push_str(&format!("\n{}\n{SUBCOMMANDS}\n", words::said::commands()));
    }
    if command.get_positionals().next().is_some() {
        template.push_str(&format!("\n{}\n{POSITIONALS}\n", words::said::arguments()));
    }
    template.push_str(&format!("\n{}\n{OPTIONS}\n", words::said::options()));
    template.push_str(AFTER_HELP);
    template
}

pub fn man_heading(english: &str) -> Option<&'static str> {
    match english {
        "NAME" => Some(words::said::man_name()),
        "SYNOPSIS" => Some(words::said::man_synopsis()),
        "DESCRIPTION" => Some(words::said::man_description()),
        "OPTIONS" => Some(words::said::man_options()),
        "SUBCOMMANDS" => Some(words::said::man_subcommands()),
        "VERSION" => Some(words::said::man_version()),
        _ => None,
    }
}
