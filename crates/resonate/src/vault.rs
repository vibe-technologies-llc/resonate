use std::{path::PathBuf, sync::Arc};

use resonate_library::{
    Form, ImportOptions, ImportSummary, Library, PassKind, Passing, Reframing, Sources, Vault,
    VaultObject, Wanted,
};

use crate::{
    Error, Result, cli::VaultArgs, finished, info::bytes_text, none_failed, table::Table,
    until_told, words,
};

const WRITES_SHOWN: usize = 20;

pub fn run(library: &Library, vault: &Arc<Vault>, args: &VaultArgs) -> Result<()> {
    if args.verify {
        return verify(library, vault, args.apply);
    }
    if args.prune {
        return prune(library, args);
    }
    if args.import {
        return import(library, args);
    }
    if args.release {
        return release(library, args);
    }
    if args.compact {
        return compact(library, args.apply);
    }
    standing(library, vault)
}

fn release(library: &Library, args: &VaultArgs) -> Result<()> {
    let roots = filed_from(&args.roots);
    let released = if args.apply {
        library.release_from_vault(&roots)?
    } else {
        library.vault_release_foretold(&roots)?
    };
    said!(
        "{}",
        if args.apply {
            words::vault::released(released.released, released.stranded)
        } else {
            words::vault::would_release(released.released, released.stranded)
        }
    );
    if !args.apply && released.released > 0 {
        said!("{}", words::vault::nothing_released());
    }
    if args.apply && released.released > 0 {
        said!("{}", words::vault::prune_after());
    }
    Ok(())
}

fn compact(library: &Library, apply: bool) -> Result<()> {
    let compacted = if apply {
        library.compact_the_vault_covers()?
    } else {
        library.vault_covers_compaction_foretold()?
    };
    let now = bytes_text(compacted.now);
    let saved = bytes_text(compacted.was.saturating_sub(compacted.now));
    let was = bytes_text(compacted.was);

    said!(
        "{}",
        if apply {
            words::vault::compacted(
                compacted.covers,
                compacted.kept,
                compacted.left,
                now,
                saved,
                was,
            )
        } else {
            words::vault::would_compact(
                compacted.covers,
                compacted.kept,
                compacted.left,
                now,
                saved,
                was,
            )
        }
    );
    if !apply && compacted.covers > 0 {
        said!("{}", words::vault::nothing_swapped());
    }
    Ok(())
}

fn standing(library: &Library, vault: &Arc<Vault>) -> Result<()> {
    let held = library.holdings()?;
    let objects = library.vault_objects()?;

    said!("{}", words::vault::root(vault.root().display().to_string()));

    if objects.is_empty() {
        said!("{}", words::vault::holds_nothing());
        return Ok(());
    }

    let mut table = Table::new(vec![
        words::vault::form(),
        words::vault::objects(),
        words::vault::held(),
        words::vault::sources(),
        words::vault::saved(),
    ]);
    for form in Form::ALL {
        let of_this_form: Vec<&VaultObject> =
            objects.iter().filter(|held| held.form == form).collect();
        if of_this_form.is_empty() {
            continue;
        }
        let bytes: u64 = of_this_form.iter().map(|held| held.bytes).sum();
        let was: u64 = of_this_form.iter().map(|held| held.was_bytes).sum();
        table.push(vec![
            form.as_str().to_owned(),
            of_this_form.len().to_string(),
            bytes_text(bytes),
            bytes_text(was),
            bytes_text(was.saturating_sub(bytes)),
        ]);
    }
    said_on!("{}", table.render());

    let bytes: u64 = objects.iter().map(|held| held.bytes).sum();
    let was: u64 = objects.iter().map(|held| held.was_bytes).sum();
    let unvalidated = objects.iter().filter(|held| !held.validated).count();
    let loose = library.vault_objects_nothing_names()?.len();

    said!(
        "{}",
        words::vault::holdings(
            held.covers,
            bytes_text(bytes),
            held.objects,
            bytes_text(held.cover_bytes),
            bytes_text(was.saturating_sub(bytes)),
            bytes_text(was),
        )
    );
    if unvalidated > 0 || loose > 0 {
        said!("{}", words::vault::unvalidated(loose, unvalidated));
    }
    Ok(())
}

fn import(library: &Library, args: &VaultArgs) -> Result<()> {
    let summary = until_told(library.import(
        Arc::new(Sources::local()),
        ImportOptions {
            roots: filed_from(&args.roots),
            apply: args.apply,
            at_most: args.at_most,
            ..ImportOptions::default()
        },
    )?)?;

    said_on!("{}", imported(&summary, args.apply));
    finished(PassKind::Import, summary.cancelled)?;
    let unread = summary
        .plan
        .passed
        .iter()
        .filter(|passed| passed.why == Passing::Unreadable)
        .count();
    none_failed(PassKind::Import, if args.apply { unread as u64 } else { 0 })
}

fn kept_as(wanted: &Wanted) -> String {
    if wanted.renewing {
        words::vault::weighed_again(wanted.form.as_str())
    } else {
        wanted.form.as_str().to_owned()
    }
}

fn imported(summary: &ImportSummary, apply: bool) -> String {
    let plan = &summary.plan;
    let stats = &summary.stats;
    let mut told = String::new();

    if apply {
        if !plan.vaulted.is_empty() {
            let mut table = Table::new(vec![
                words::vault::file(),
                words::vault::form(),
                words::vault::was(),
                words::vault::now(),
            ]);
            for held in plan.vaulted.iter().take(WRITES_SHOWN) {
                table.push(vec![
                    named(&held.wanted.from),
                    held.wanted.form.as_str().to_owned(),
                    bytes_text(held.wanted.bytes),
                    if held.deduped {
                        words::vault::already_held().to_owned()
                    } else {
                        bytes_text(held.bytes)
                    },
                ]);
            }
            told.push_str(&table.render());
            if plan.vaulted.len() > WRITES_SHOWN {
                told.push_str(&words::vault::more(plan.vaulted.len() - WRITES_SHOWN));
                told.push('\n');
            }
        }
    } else if plan.wanted.is_empty() {
        return format!("{}\n", words::vault::nothing_to_import());
    } else {
        let mut table = Table::new(vec![
            words::vault::file(),
            words::vault::codec(),
            words::vault::size(),
            words::vault::would_be_kept_as(),
        ]);
        for wanted in plan.wanted.iter().take(WRITES_SHOWN) {
            table.push(vec![
                named(&wanted.from),
                wanted.codec.as_str().to_owned(),
                bytes_text(wanted.bytes),
                kept_as(wanted),
            ]);
        }
        told.push_str(&table.render());
        if plan.wanted.len() > WRITES_SHOWN {
            told.push_str(&words::vault::more(plan.wanted.len() - WRITES_SHOWN));
            told.push('\n');
        }
    }

    for passed in &plan.passed {
        told.push_str(&words::vault::passed_over(
            named(&passed.from),
            passed.why.as_str(),
        ));
        told.push('\n');
    }

    if apply {
        told.push_str(&words::vault::imported(
            stats.covers,
            stats.deduped,
            bytes_text(stats.bytes),
            stats.vaulted,
            stats.passed,
            bytes_text(stats.saved()),
            bytes_text(stats.was_bytes),
        ));
        told.push('\n');
        if stats.covers_passed > 0 {
            told.push_str(&words::vault::covers_not_kept(stats.covers_passed));
            told.push('\n');
        }
    } else {
        let renewing = plan.wanted.iter().filter(|wanted| wanted.renewing).count();
        told.push_str(&words::vault::would_keep(renewing, stats.walked));
        told.push('\n');
        told.push_str(words::vault::nothing_written());
        told.push('\n');
    }
    told
}

fn verify(library: &Library, vault: &Arc<Vault>, mending: bool) -> Result<()> {
    let objects = library.vault_objects()?;
    let covers = vault.covers()?;
    if objects.is_empty() && covers.is_empty() {
        said!("{}", words::vault::nothing_to_weigh());
        return Ok(());
    }

    let mut held = 0_u64;
    let mut moved = Vec::new();
    let mut unreached = Vec::new();
    let mut unframed = Vec::new();
    for object in &objects {
        let reads_back = match vault.verify(&object.path, object.form) {
            Ok(reads_back) => reads_back,
            Err(error) if vault.failed_itself(&error) => {
                unreached.push((object.path.clone(), error));
                continue;
            }
            Err(error) => {
                tracing::debug!(%error, path = %object.path.display(), "a vault object failed as it was read back");
                false
            }
        };
        library.note_validated(&object.key, reads_back)?;
        if reads_back {
            held += 1;
            let before_the_frames = object.form == Form::Wave
                && vault
                    .packed_before_the_frames(&object.path)
                    .inspect_err(|error| {
                        tracing::debug!(
                            %error,
                            path = %object.path.display(),
                            "a vault object's frames could not be walked"
                        );
                    })
                    .unwrap_or(false);
            if before_the_frames {
                unframed.push(object.path.clone());
            }
        } else {
            moved.push((object.key, object.path.clone()));
        }
    }

    let mut covers_held = 0_u64;
    let mut covers_moved = Vec::new();
    for cover in &covers {
        match vault.verify_cover(&cover.path) {
            Ok(true) => covers_held += 1,
            Ok(false) => covers_moved.push((cover.key, cover.path.clone())),
            Err(error) if vault.failed_itself(&error) => {
                unreached.push((cover.path.clone(), error));
            }
            Err(error) => {
                tracing::debug!(%error, path = %cover.path.display(), "a vault cover failed as it was read back");
                covers_moved.push((cover.key, cover.path.clone()));
            }
        }
    }

    for (_, path) in moved.iter().chain(&covers_moved) {
        said!(
            "{}",
            words::vault::did_not_read_back(path.display().to_string())
        );
    }
    for (path, error) in &unreached {
        said!(
            "{}",
            words::vault::not_weighed(error.to_string(), path.display().to_string())
        );
    }
    said!(
        "{}",
        words::vault::weighed(
            covers.len(),
            covers_held,
            covers_moved.len(),
            held,
            moved.len(),
            unreached.len(),
            objects.len(),
        )
    );

    reframed(vault, &unframed, mending)?;

    let failing = moved.len() + covers_moved.len();
    if failing > 0 && mending {
        let keys: Vec<_> = moved.iter().map(|(key, _)| *key).collect();
        let released = library.release_the_rows_of(&keys)?;
        let covers: Vec<_> = covers_moved.iter().map(|(key, _)| *key).collect();
        let let_go = library.let_go_of_vault_covers(&covers)?;
        said!(
            "{}",
            words::vault::mended(let_go, released.released, released.stranded)
        );
        return match unreached.len() as u64 {
            0 => Ok(()),
            unreached => Err(Error::ObjectsUnverified {
                moved: 0,
                unreached,
            }),
        };
    }
    if failing > 0 {
        said!("{}", words::vault::nothing_mended());
    }
    match (failing as u64, unreached.len() as u64) {
        (0, 0) => Ok(()),
        (moved, unreached) => Err(Error::ObjectsUnverified { moved, unreached }),
    }
}

fn reframed(vault: &Vault, unframed: &[PathBuf], mending: bool) -> Result<()> {
    if unframed.is_empty() {
        return Ok(());
    }
    if !mending {
        said!("{}", words::vault::unframed(unframed.len()));
        return Ok(());
    }

    let mut repacked = 0_u64;
    for path in unframed {
        match vault.reframe(path)? {
            Reframing::Reframed => repacked += 1,
            Reframing::AlreadyFramed => {}
            Reframing::NotHeld => said!(
                "{}",
                words::vault::did_not_repack(path.display().to_string())
            ),
        }
    }
    said!("{}", words::vault::repacked(repacked, unframed.len()));
    Ok(())
}

fn prune(library: &Library, args: &VaultArgs) -> Result<()> {
    let pruned = if args.apply {
        library.prune_the_vault()?
    } else {
        library.vault_prune_foretold()?
    };
    said!(
        "{}",
        if args.apply {
            words::vault::pruned(pruned.covers, pruned.left, pruned.objects, pruned.staged)
        } else {
            words::vault::would_prune(pruned.covers, pruned.left, pruned.objects, pruned.staged)
        }
    );
    if !args.apply && pruned.objects + pruned.covers + pruned.staged > 0 {
        said!("{}", words::vault::nothing_taken());
    }
    Ok(())
}

fn filed_from(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .map(|root| root.canonicalize().unwrap_or_else(|_| root.clone()))
        .collect()
}

fn named(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
