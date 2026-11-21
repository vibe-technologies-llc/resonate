mod changed;
mod error;
mod grammar;
mod plan;
mod run;

use std::{collections::BTreeSet, env, path::Path, process::ExitCode};

use crate::{
    changed::Touch,
    error::Error,
    grammar::{Asked, Scope, Task},
    plan::{Host, Selection, Step},
};

const ANDROID_SDK: &str = "ANDROID_HOME";

fn main() -> ExitCode {
    match done() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn done() -> Result<(), Error> {
    let asked = grammar::parsed(env::args().skip(1))?;

    if asked.task == Task::Help {
        print!("{}", grammar::USAGE);
        return Ok(());
    }

    let root = run::workspace_root()?;
    let steps = steps_for(&asked, &root)?;

    run::all(&root, &steps, asked.output, asked.keep_going)
}

fn steps_for(asked: &Asked, root: &Path) -> Result<Vec<Step>, Error> {
    let host = Host::here();

    Ok(match asked.task {
        Task::Help => Vec::new(),
        Task::Format => plan::format(asked.check),
        Task::Lint => plan::lint(
            &selection(asked, root)?,
            asked.target.as_deref(),
            asked.locked,
        ),
        Task::Test => plan::test(
            &selection(asked, root)?,
            asked.locked,
            &asked.passed_on,
            host,
        ),
        Task::Check => {
            let selection = selection(asked, root)?;

            let mut steps = plan::format(true);

            steps.extend(plan::lint(&selection, None, asked.locked));
            steps.extend(plan::test(&selection, asked.locked, &[], host));
            steps
        }
        Task::Android => plan::android(&android_sdk()?),
        Task::Ios => plan::ios(asked.build),
    })
}

fn selection(asked: &Asked, root: &Path) -> Result<Selection, Error> {
    let Scope::Changed { since } = &asked.scope else {
        return Ok(Selection::Workspace);
    };

    let files = changed::files(root, since)?;
    let members = changed::members(root)?;

    let selection = match changed::owners(&files, &members) {
        Touch::Everything => Selection::Workspace,
        Touch::These(touched) if touched.is_empty() => Selection::These(touched),
        Touch::These(touched) => Selection::These(changed::dependents(root, &touched)?),
    };

    tell(&selection, since, files.len());
    Ok(selection)
}

fn tell(selection: &Selection, since: &str, files: usize) {
    match selection {
        Selection::Workspace => {
            println!(
                "{files} files changed since {since}, one of them workspace-wide: every package"
            )
        }
        Selection::These(packages) => println!(
            "{files} files changed since {since}: {} packages changed or depending on one{}",
            packages.len(),
            listed(packages)
        ),
    }
}

fn listed(packages: &BTreeSet<String>) -> String {
    if packages.is_empty() {
        String::new()
    } else {
        format!(
            ": {}",
            packages
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn android_sdk() -> Result<String, Error> {
    env::var(ANDROID_SDK)
        .ok()
        .filter(|sdk| !sdk.is_empty())
        .ok_or(Error::NoAndroidSdk)
}
