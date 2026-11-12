use std::{
    env, fs, io, panic,
    path::{Path, PathBuf},
    thread,
};

use clap_complete::Shell;
use resonate_words::Language;

#[cfg(windows)]
mod icon_resources;
#[cfg(windows)]
mod resources;

const SHELLS: [Shell; 3] = [Shell::Bash, Shell::Fish, Shell::Zsh];

const GRAMMAR_STACK_BYTES: usize = 16 * 1024 * 1024;

const MAN_SECTION: &str = ".SH ";

const MAN_ENDING: &str = "1";

#[cfg(windows)]
const WINDOWS: &str = "windows";

fn main() -> io::Result<()> {
    let Some(out) = env::var_os("OUT_DIR").map(PathBuf::from) else {
        return Err(io::Error::other("cargo names no output directory"));
    };

    let writer = thread::Builder::new()
        .name("grammar".into())
        .stack_size(GRAMMAR_STACK_BYTES)
        .spawn(move || write_every_grammar(&out))?;
    writer
        .join()
        .unwrap_or_else(|panicked| panic::resume_unwind(panicked))?;

    resonate_words_build::write().map_err(io::Error::other)?;

    #[cfg(windows)]
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == WINDOWS) {
        resources::embed().map_err(io::Error::other)?;
    }
    Ok(())
}

fn write_every_grammar(out: &Path) -> io::Result<()> {
    for language in Language::ALL {
        let folder = folder_for(out, language);
        fs::create_dir_all(&folder)?;
        resonate_words::speak(language);
        write_man_pages(&folder)?;
    }

    resonate_words::speak(Language::English);
    let mut command = resonate_cli::command();
    for shell in SHELLS {
        clap_complete::generate_to(shell, &mut command, env!("CARGO_PKG_NAME"), out)?;
    }
    Ok(())
}

fn folder_for(out: &Path, language: Language) -> PathBuf {
    if language == Language::English {
        out.to_path_buf()
    } else {
        out.join(language.tag())
    }
}

fn write_man_pages(folder: &Path) -> io::Result<()> {
    clap_mangen::generate_to(resonate_cli::command(), folder)?;
    headed_in_the_language_spoken(folder)
}

fn headed_in_the_language_spoken(folder: &Path) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ending| ending != MAN_ENDING) {
            continue;
        }
        let page = fs::read_to_string(&path)?;
        let headed: Vec<String> = page
            .lines()
            .map(|line| {
                line.strip_prefix(MAN_SECTION)
                    .and_then(resonate_cli::man_heading)
                    .map_or_else(
                        || line.to_owned(),
                        |heading| format!("{MAN_SECTION}{heading}"),
                    )
            })
            .collect();
        fs::write(&path, headed.join("\n") + "\n")?;
    }
    Ok(())
}
