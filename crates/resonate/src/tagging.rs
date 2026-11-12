use std::{mem, path::PathBuf, sync::Arc};

use resonate_codec::{CoverArt, Popularity};
use resonate_library::{FileTags, Library, PassKind, RetagOptions, RetagSummary, SheetScope};

use crate::{
    error::Result,
    info::bytes_text,
    organising::{filed_from, under_its_root},
    passes::{finished, none_failed, until_told},
    table::Table,
    words,
    words::columns,
};

pub(crate) fn tag(library: &Library, roots: &[PathBuf], apply: bool, undo: bool) -> Result<()> {
    let summary = until_told(library.retag(
        Arc::new(FileTags::default()),
        RetagOptions {
            roots: filed_from(roots),
            apply,
            undo,
        },
    )?)?;

    said_on!("{}", tagged(&summary, &library.roots()?, apply));
    finished(PassKind::Retag, summary.cancelled)?;
    let failed = summary
        .retagging
        .passed_over
        .iter()
        .filter(|passed| passed.why.is_a_failure())
        .count();
    none_failed(PassKind::Retag, if apply { failed as u64 } else { 0 })
}

pub(crate) fn dedupe(library: &Library, apply: bool) -> Result<()> {
    let copies = library.identical_copies()?;
    for copy in &copies.identical {
        said!(
            "{}",
            words::main::copies(
                copy.copy.display().to_string(),
                copy.kept.display().to_string()
            )
        );
    }
    for differing in &copies.differing {
        said!("{}", words::main::differs(differing.display().to_string()));
    }
    if !apply {
        said!("{}", words::main::to_fold(copies.identical.len()));
        return Ok(());
    }
    let deleted = library.fold_copies(&copies.identical)?;
    said!("{}", words::main::folded(deleted.files));
    if deleted.kept > 0 {
        said!("{}", words::main::trash_refused(deleted.kept));
    }
    Ok(())
}

pub(crate) fn pictured(picture: &CoverArt) -> String {
    format!(
        "{} · {}",
        picture.format.extension().to_uppercase(),
        bytes_text(picture.bytes.len() as u64)
    )
}

fn rated(popularity: Popularity) -> String {
    let plays = words::main::plays(popularity.plays);
    if popularity.favourite {
        words::main::favourite_plays(plays)
    } else {
        words::main::not_favourite_plays(plays)
    }
}

fn tagged(summary: &RetagSummary, roots: &[PathBuf], apply: bool) -> String {
    let retagging = &summary.retagging;
    if retagging.writes.is_empty()
        && retagging.sheets.is_empty()
        && retagging.passed_over.is_empty()
    {
        return format!("{}\n", words::main::nothing_to_tag());
    }

    let mut told = String::new();
    if !retagging.writes.is_empty() || !retagging.sheets.is_empty() {
        let mut table = Table::new(vec![columns::file(), columns::field(), columns::value()]);
        for write in &retagging.writes {
            let mut named = under_its_root(&write.path, roots);
            for edit in &write.edits {
                table.push(vec![
                    mem::take(&mut named),
                    edit.field.to_string(),
                    edit.value.clone(),
                ]);
            }
            for field in &write.taken {
                table.push(vec![
                    mem::take(&mut named),
                    field.to_string(),
                    words::main::taken_away().to_owned(),
                ]);
            }
            if let Some(picture) = &write.picture {
                table.push(vec![
                    mem::take(&mut named),
                    words::main::cover_art().to_owned(),
                    pictured(picture),
                ]);
            }
            if write.unpictured {
                table.push(vec![
                    mem::take(&mut named),
                    words::main::cover_art().to_owned(),
                    words::main::taken_away().to_owned(),
                ]);
            }
            if let Some(popularity) = write.popularity {
                table.push(vec![
                    mem::take(&mut named),
                    words::main::rating().to_owned(),
                    rated(popularity),
                ]);
            }
        }
        for write in &retagging.sheets {
            let mut named = under_its_root(&write.sheet, roots);
            for edit in &write.edits {
                let field = match edit.scope {
                    SheetScope::Disc => edit.field.to_string(),
                    SheetScope::Track(number) => {
                        words::main::sheet_track(edit.field.to_string(), format!("{number:02}"))
                    }
                };
                table.push(vec![
                    mem::take(&mut named),
                    field,
                    edit.value
                        .clone()
                        .unwrap_or_else(|| words::main::taken_away().to_owned()),
                ]);
            }
        }
        told.push_str(&table.render());
    }

    for over in &retagging.passed_over {
        told.push_str(&format!(
            "{}: {}\n",
            under_its_root(&over.path, roots),
            over.why.as_str()
        ));
    }

    let stats = &summary.stats;
    let written = if apply {
        words::main::written(stats.fields, stats.pictures, stats.ratings, stats.written)
    } else {
        words::main::would_write(
            retagging
                .writes
                .iter()
                .map(|write| write.edits.len() + write.taken.len())
                .chain(retagging.sheets.iter().map(|write| write.edits.len()))
                .sum::<usize>(),
            retagging
                .writes
                .iter()
                .filter(|write| write.picture.is_some())
                .count(),
            retagging
                .writes
                .iter()
                .filter(|write| write.popularity.is_some())
                .count(),
            retagging.writes.len() + retagging.sheets.len(),
        )
    };

    told.push_str(&words::main::tagged(
        stats.passed_over,
        stats.unchanged,
        stats.walked,
        written,
    ));
    told.push('\n');
    if !apply {
        told.push_str(words::main::nothing_written());
        told.push('\n');
    }
    if summary.cancelled {
        told.push_str(words::main::cancelled());
        told.push('\n');
    }
    told
}
