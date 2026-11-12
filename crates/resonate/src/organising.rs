use std::path::{Path, PathBuf};

use resonate_core::MediaLocation;
use resonate_library::{
    Layout, Library, Move, OrganiseOptions, OrganiseSummary, PassKind, Refusal, Refused,
};
use resonate_mpris::Running;

use crate::{
    config::Config,
    error::Result,
    passes::{finished, none_failed, until_told},
    table::Table,
    words,
    words::columns,
};

#[derive(Clone, Copy)]
pub(crate) struct Organising {
    pub(crate) apply: bool,
    pub(crate) walk_back: bool,
}

pub(crate) fn organise(
    library: &Library,
    config: &Config,
    named: Option<&str>,
    roots: &[PathBuf],
    how: Organising,
) -> Result<()> {
    let layout = match named {
        Some(template) => Layout::read(template)?,
        None => config.organise_as(),
    };
    let summary = until_told(library.organise(OrganiseOptions {
        layout,
        roots: filed_from(roots),
        only: Vec::new(),
        apply: how.apply,
        walk_back: how.walk_back,
    })?)?;

    if how.walk_back && summary.plan.moves.is_empty() {
        said!("{}", words::main::nothing_to_put_back());
        return Ok(());
    }
    said_on!("{}", organised(&summary, &library.roots()?, how.apply));
    if how.apply {
        tell_the_players_where_files_went(&summary);
    }
    finished(PassKind::Organise, summary.cancelled)?;
    none_failed(
        PassKind::Organise,
        if how.apply { summary.stats.failed } else { 0 },
    )
}

fn tell_the_players_where_files_went(summary: &OrganiseSummary) {
    let moved: Vec<(MediaLocation, MediaLocation)> = summary
        .plan
        .moves
        .iter()
        .flat_map(|planned| planned.files())
        .map(|(from, to)| (MediaLocation::local(from), MediaLocation::local(to)))
        .collect();
    if moved.is_empty() {
        return;
    }

    let running = match Running::listed() {
        Ok(running) => running,
        Err(error) => {
            tracing::debug!(%error, "no session bus to tell a running player where files went");
            return;
        }
    };
    for player in running {
        if let Err(error) = player.relocate(&moved) {
            tracing::warn!(
                %error,
                name = %player.name(),
                "a running player was not told where its queued files went"
            );
        }
    }
}

pub(crate) fn filed_from(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .map(|root| root.canonicalize().unwrap_or_else(|_| root.clone()))
        .collect()
}

fn organised(summary: &OrganiseSummary, roots: &[PathBuf], apply: bool) -> String {
    let plan = &summary.plan;
    if plan.moves.is_empty() && plan.refused.is_empty() {
        return format!("{}\n", words::main::nothing_to_organise());
    }

    let mut told = String::new();
    if !plan.moves.is_empty() {
        let mut table = Table::new(vec![columns::from(), columns::to()]);
        for (from, to) in plan.moves.iter().flat_map(Move::files) {
            table.push(vec![under_its_root(from, roots), under_its_root(to, roots)]);
        }
        told.push_str(&table.render());
    }

    for refused in &plan.refused {
        told.push_str(&left_standing(refused, roots));
        told.push('\n');
    }

    let stats = &summary.stats;
    let (moved, folders) = if apply {
        (
            words::main::moved(stats.moved),
            words::main::folders_pruned(stats.pruned),
        )
    } else {
        (
            words::main::would_move(plan.files_moving()),
            words::main::folders_to_empty(plan.folders.len()),
        )
    };

    told.push_str(&words::main::organised(
        stats.collided,
        stats.failed,
        folders,
        moved,
        stats.unchanged,
        stats.unidentified,
    ));
    told.push('\n');
    if !apply {
        told.push_str(words::main::nothing_moved());
        told.push('\n');
    }
    if summary.cancelled {
        told.push_str(words::main::cancelled());
        told.push('\n');
    }
    told
}

fn left_standing(refused: &Refused, roots: &[PathBuf]) -> String {
    let standing = under_its_root(&refused.from, roots);
    let why = stands_because(&refused.refusal);

    match &refused.refusal {
        Refusal::Collided { with }
        | Refusal::SharesASheet { sheet: with }
        | Refusal::NamedFromAbove { sheet: with } => {
            words::main::refused_with(standing, why, under_its_root(with, roots))
        }
        Refusal::Unmoved { kind } => words::main::refused_kind(kind.to_string(), standing, why),
        _ => words::main::refused(standing, why),
    }
}

fn stands_because(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::Unidentified => words::main::unidentified(),
        Refusal::Loose => words::main::loose(),
        Refusal::Collided { .. } => words::main::collided(),
        Refusal::SharesASheet { .. } => words::main::shares_a_sheet(),
        Refusal::NamedFromAbove { .. } => words::main::named_from_above(),
        Refusal::OutOfReach => words::main::out_of_reach(),
        Refusal::SourceGone => words::main::source_gone(),
        Refusal::Unmoved { .. } => words::main::unmoved(),
    }
}

pub(crate) fn under_its_root(path: &Path, roots: &[PathBuf]) -> String {
    roots
        .iter()
        .find_map(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}
