#[cfg(test)]
mod tests;

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    error::{Error, Query},
    plan::{DSP_LAB, DSP_LAB_FOLDER},
    run,
};

const MANIFEST: &str = "Cargo.toml";
const MEMBERS_OPEN: &str = "members = [";
const WORKSPACE_WIDE_FILES: [&str; 3] = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"];
const WORKSPACE_WIDE_FOLDER: &str = ".cargo/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub folder: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Touch {
    Everything,
    These(BTreeSet<String>),
}

pub fn files(root: &Path, since: &str) -> Result<BTreeSet<String>, Error> {
    let base = run::asked(
        git(root).args(["merge-base", "HEAD", since]),
        "git",
        Query::MergeBase,
    )?;
    let differing = run::asked(
        git(root).args(["diff", "--name-only", base.trim()]),
        "git",
        Query::Differing,
    )?;
    let untracked = run::asked(
        git(root).args(["ls-files", "--others", "--exclude-standard"]),
        "git",
        Query::Untracked,
    )?;

    Ok(differing
        .lines()
        .chain(untracked.lines())
        .filter(|file| !file.is_empty())
        .map(str::to_owned)
        .collect())
}

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");

    command.current_dir(root);
    command
}

pub fn members(root: &Path) -> Result<Vec<Member>, Error> {
    let manifest = read(&root.join(MANIFEST))?;

    members_listed(&manifest)
        .into_iter()
        .map(|folder| {
            let path = root.join(folder).join(MANIFEST);
            let own = read(&path)?;

            let name = package_name(&own).ok_or(Error::Unnamed { path })?;

            Ok(Member {
                name: name.to_owned(),
                folder: folder.to_owned(),
            })
        })
        .collect()
}

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|source| Error::Read {
        path: PathBuf::from(path),
        source,
    })
}

fn members_listed(manifest: &str) -> Vec<&str> {
    let Some(start) = manifest.find(MEMBERS_OPEN) else {
        return Vec::new();
    };

    let listed = &manifest[start + MEMBERS_OPEN.len()..];
    let listed = &listed[..listed.find(']').unwrap_or(listed.len())];

    listed.split('"').skip(1).step_by(2).collect()
}

fn package_name(manifest: &str) -> Option<&str> {
    let package = &manifest[manifest.find("[package]")?..];

    package
        .lines()
        .find_map(|line| line.trim().strip_prefix("name = \"")?.strip_suffix('"'))
}

pub fn owners(files: &BTreeSet<String>, members: &[Member]) -> Touch {
    if files.iter().any(|file| is_workspace_wide(file)) {
        return Touch::Everything;
    }

    Touch::These(
        files
            .iter()
            .filter_map(|file| owner(file, members).or_else(|| lab_owner(file)))
            .map(str::to_owned)
            .collect(),
    )
}

fn is_workspace_wide(file: &str) -> bool {
    WORKSPACE_WIDE_FILES.contains(&file) || file.starts_with(WORKSPACE_WIDE_FOLDER)
}

fn lab_owner(file: &str) -> Option<&'static str> {
    file.strip_prefix(DSP_LAB_FOLDER)
        .is_some_and(|rest| rest.starts_with('/'))
        .then_some(DSP_LAB)
}

fn owner<'a>(file: &str, members: &'a [Member]) -> Option<&'a str> {
    members
        .iter()
        .filter(|member| {
            file.strip_prefix(member.folder.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|member| member.folder.len())
        .map(|member| member.name.as_str())
}

pub fn dependents(root: &Path, touched: &BTreeSet<String>) -> Result<BTreeSet<String>, Error> {
    let lab = touched.contains(DSP_LAB);
    let touched: BTreeSet<String> = touched
        .iter()
        .filter(|package| *package != DSP_LAB)
        .cloned()
        .collect();

    let mut found = if touched.is_empty() {
        BTreeSet::new()
    } else {
        workspace_dependents(root, &touched)?
    };

    if lab {
        found.insert(DSP_LAB.to_owned());
    }
    Ok(found)
}

fn workspace_dependents(
    root: &Path,
    touched: &BTreeSet<String>,
) -> Result<BTreeSet<String>, Error> {
    let mut command = Command::new("cargo");

    command.current_dir(root).args([
        "tree",
        "--workspace",
        "--edges",
        "normal,build,dev",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ]);
    for package in touched {
        command.args(["--invert", package]);
    }

    let listed = run::asked(&mut command, "cargo", Query::Dependents)?;
    Ok(dependents_listed(&listed, root))
}

fn dependents_listed(listed: &str, root: &Path) -> BTreeSet<String> {
    let within = format!("({}", root.display());

    listed
        .lines()
        .filter(|line| line.contains(&within))
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}
