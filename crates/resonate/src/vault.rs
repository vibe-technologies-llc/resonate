use std::{path::PathBuf, sync::Arc};

use resonate_library::{
    Form, ImportOptions, ImportSummary, Library, PassKind, Passing, Sources, Vault, VaultObject,
    Wanted,
};

use crate::{
    Error, Result, cli::VaultArgs, finished, info::bytes_text, none_failed, table::Table,
    until_told,
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
    standing(library, vault)
}

fn release(library: &Library, args: &VaultArgs) -> Result<()> {
    let roots = filed_from(&args.roots);
    let released = if args.apply {
        library.release_from_vault(&roots)?
    } else {
        library.vault_release_foretold(&roots)?
    };
    let verb = if args.apply {
        "released"
    } else {
        "would release"
    };
    said!(
        "{verb} {} | kept {} the vault holds the only copy of",
        released.released,
        released.stranded
    );
    if !args.apply && released.released > 0 {
        said!("nothing was released; --apply makes it so");
    }
    if args.apply && released.released > 0 {
        said!("resonate vault --prune takes away what nothing names any more");
    }
    Ok(())
}

fn standing(library: &Library, vault: &Arc<Vault>) -> Result<()> {
    let held = library.holdings()?;
    let objects = library.vault_objects()?;

    said!("root {}", vault.root().display());

    if objects.is_empty() {
        said!("the vault holds nothing yet; resonate vault --import says what it would take");
        return Ok(());
    }

    let mut table = Table::new(vec!["FORM", "OBJECTS", "HELD", "SOURCES", "SAVED"]);
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
        "objects {} | held {} | sources {} | saved {} | covers {} | pictures {}",
        held.objects,
        bytes_text(bytes),
        bytes_text(was),
        bytes_text(was.saturating_sub(bytes)),
        held.covers,
        bytes_text(held.cover_bytes)
    );
    if unvalidated > 0 || loose > 0 {
        said!("unvalidated {unvalidated} | named by nothing {loose}");
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
        format!("{}, weighed again", wanted.form.as_str())
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
            let mut table = Table::new(vec!["FILE", "FORM", "WAS", "NOW"]);
            for held in plan.vaulted.iter().take(WRITES_SHOWN) {
                table.push(vec![
                    named(&held.wanted.from),
                    held.wanted.form.as_str().to_owned(),
                    bytes_text(held.wanted.bytes),
                    if held.deduped {
                        "already held".to_owned()
                    } else {
                        bytes_text(held.bytes)
                    },
                ]);
            }
            told.push_str(&table.render());
            if plan.vaulted.len() > WRITES_SHOWN {
                told.push_str(&format!("and {} more\n", plan.vaulted.len() - WRITES_SHOWN));
            }
        }
    } else if plan.wanted.is_empty() {
        return "nothing to import\n".to_owned();
    } else {
        let mut table = Table::new(vec!["FILE", "CODEC", "SIZE", "WOULD BE KEPT AS"]);
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
            told.push_str(&format!("and {} more\n", plan.wanted.len() - WRITES_SHOWN));
        }
    }

    for passed in &plan.passed {
        told.push_str(&format!(
            "passed over {}: it {}\n",
            named(&passed.from),
            passed.why.as_str()
        ));
    }

    if apply {
        told.push_str(&format!(
            "kept {} | already held {} | covers {} | passed over {} | sources {} | held {} | \
             saved {}\n",
            stats.vaulted,
            stats.deduped,
            stats.covers,
            stats.passed,
            bytes_text(stats.was_bytes),
            bytes_text(stats.bytes),
            bytes_text(stats.saved())
        ));
        if stats.covers_passed > 0 {
            told.push_str(&format!(
                "covers the vault could not keep {}\n",
                stats.covers_passed
            ));
        }
    } else {
        let renewing = plan.wanted.iter().filter(|wanted| wanted.renewing).count();
        told.push_str(&format!(
            "would keep {} | of them weighed again under this build's encoder {renewing}\n",
            stats.walked
        ));
        told.push_str("nothing was written; pass --apply to do it\n");
    }
    told
}

fn verify(library: &Library, vault: &Arc<Vault>, mending: bool) -> Result<()> {
    let objects = library.vault_objects()?;
    let covers = vault.covers()?;
    if objects.is_empty() && covers.is_empty() {
        said!("the vault holds nothing to weigh");
        return Ok(());
    }

    let mut held = 0_u64;
    let mut moved = Vec::new();
    let mut unreached = Vec::new();
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
        said!("did not read back as what went in: {}", path.display());
    }
    for (path, error) in &unreached {
        said!(
            "could not be read, so it was not weighed: {} ({error})",
            path.display()
        );
    }
    said!(
        "weighed {} | held {held} | moved {} | covers weighed {} | covers held {covers_held} | \
         covers moved {} | unreached {}",
        objects.len(),
        moved.len(),
        covers.len(),
        covers_moved.len(),
        unreached.len()
    );

    let failing = moved.len() + covers_moved.len();
    if failing > 0 && mending {
        let keys: Vec<_> = moved.iter().map(|(key, _)| *key).collect();
        let released = library.release_the_rows_of(&keys)?;
        let covers: Vec<_> = covers_moved.iter().map(|(key, _)| *key).collect();
        let let_go = library.let_go_of_vault_covers(&covers)?;
        said!(
            "mended: tracks pointed back at their own files {} | tracks the vault holds the only \
             copy of {} | albums letting go of a cover {let_go}",
            released.released,
            released.stranded
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
        said!(
            "nothing was mended; --apply points each track back at its own file and lets the covers go"
        );
    }
    match (failing as u64, unreached.len() as u64) {
        (0, 0) => Ok(()),
        (moved, unreached) => Err(Error::ObjectsUnverified { moved, unreached }),
    }
}

fn prune(library: &Library, args: &VaultArgs) -> Result<()> {
    let pruned = if args.apply {
        library.prune_the_vault()?
    } else {
        library.vault_prune_foretold()?
    };
    let verb = if args.apply { "" } else { "would take " };
    said!(
        "{verb}objects {} | covers {} | staged {} | left {}",
        pruned.objects,
        pruned.covers,
        pruned.staged,
        pruned.left
    );
    if !args.apply && pruned.objects + pruned.covers + pruned.staged > 0 {
        said!("nothing was taken away; --apply makes it so");
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
