use std::{
    env, io, panic,
    path::{Path, PathBuf},
    thread,
};

use clap::CommandFactory as _;
use clap_complete::Shell;

mod cli {
    include!("src/cli.rs");
}

#[cfg(windows)]
mod icon_resources;
#[cfg(windows)]
mod resources;

const SHELLS: [Shell; 3] = [Shell::Bash, Shell::Fish, Shell::Zsh];

const GRAMMAR_STACK_BYTES: usize = 16 * 1024 * 1024;

#[cfg(windows)]
const WINDOWS: &str = "windows";

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=src/cli.rs");
    let Some(out) = env::var_os("OUT_DIR").map(PathBuf::from) else {
        return Err(io::Error::other("cargo names no output directory"));
    };

    let writer = thread::Builder::new()
        .name("grammar".into())
        .stack_size(GRAMMAR_STACK_BYTES)
        .spawn(move || write_grammar(&out))?;
    writer
        .join()
        .unwrap_or_else(|panicked| panic::resume_unwind(panicked))?;

    #[cfg(windows)]
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == WINDOWS) {
        resources::embed().map_err(io::Error::other)?;
    }
    Ok(())
}

fn write_grammar(out: &Path) -> io::Result<()> {
    let mut command = cli::Cli::command();
    clap_mangen::generate_to(command.clone(), out)?;
    for shell in SHELLS {
        clap_complete::generate_to(shell, &mut command, env!("CARGO_PKG_NAME"), out)?;
    }
    Ok(())
}
