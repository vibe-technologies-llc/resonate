#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{io, path::PathBuf, process};

use clap::Parser;
use resonate_signin::{FAILED, sign_in};

#[derive(Debug, Parser)]
#[command(
    name = "resonate-signin",
    version,
    about = "Sign in to YouTube Music in a window of its own and keep the account's cookies for \
             Resonate"
)]
struct Asked {
    #[arg(
        long,
        value_name = "FILE",
        help = "Where the cookie jar is written once signed in"
    )]
    jar: PathBuf,

    #[arg(long, value_name = "FOLDER", help = "The window's own browser profile")]
    profile: PathBuf,
}

fn main() {
    tracing_subscriber::fmt().with_writer(io::stderr).init();
    let asked = Asked::parse();
    match sign_in(asked.jar, asked.profile) {
        Ok(never) => match never {},
        Err(error) => {
            tracing::error!(%error, "the sign-in window could not be opened");
            process::exit(FAILED);
        }
    }
}
