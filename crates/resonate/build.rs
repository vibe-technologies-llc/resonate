use std::{env, io, path::PathBuf};

use clap::CommandFactory as _;
use clap_complete::Shell;

mod cli {
    include!("src/cli.rs");
}

const SHELLS: [Shell; 3] = [Shell::Bash, Shell::Fish, Shell::Zsh];

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=src/cli.rs");
    let Some(out) = env::var_os("OUT_DIR").map(PathBuf::from) else {
        return Err(io::Error::other("cargo names no output directory"));
    };

    let mut command = cli::Cli::command();
    clap_mangen::generate_to(command.clone(), &out)?;
    for shell in SHELLS {
        clap_complete::generate_to(shell, &mut command, env!("CARGO_PKG_NAME"), &out)?;
    }
    Ok(())
}
