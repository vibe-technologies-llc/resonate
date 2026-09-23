use std::{path::PathBuf, sync::Arc};

use resonate_library::{
    Form, ImportOptions, ImportSummary, Library, Sources, Vault, VaultObject, Wanted,
};

use crate::{Result, cli::VaultArgs, info::bytes_text, table::Table, until_told};

const WRITES_SHOWN: usize = 20;

pub fn run(library: &Library, vault: &Arc<Vault>, args: &VaultArgs) -> Result<()> {
    if args.verify {
        return verify(library, vault);
    }
    if args.prune {
        return prune(library);
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
    let released = library.release_from_vault(&filed_from(&args.roots))?;
    println!(
        "released {} | kept {} the vault holds the only copy of",
        released.released, released.stranded
    );
    if released.released > 0 {
        println!("resonate vault --prune takes away what nothing names any more");
    }
    Ok(())
}

fn standing(library: &Library, vault: &Arc<Vault>) -> Result<()> {
    let held = library.holdings()?;
    let objects = library.vault_objects()?;

    println!("root {}", vault.root().display());

    if objects.is_empty() {
        println!("the vault holds nothing yet; resonate vault --import says what it would take");
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
    print!("{}", table.render());

    let bytes: u64 = objects.iter().map(|held| held.bytes).sum();
    let was: u64 = objects.iter().map(|held| held.was_bytes).sum();
    let unvalidated = objects.iter().filter(|held| !held.validated).count();
    let loose = library.vault_objects_nothing_names()?.len();

    println!(
        "objects {} | held {} | sources {} | saved {} | covers {} | pictures {}",
        held.objects,
        bytes_text(bytes),
        bytes_text(was),
        bytes_text(was.saturating_sub(bytes)),
        held.covers,
        bytes_text(held.cover_bytes)
    );
    if unvalidated > 0 || loose > 0 {
        println!("unvalidated {unvalidated} | named by nothing {loose}");
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

    print!("{}", imported(&summary, args.apply));
    Ok(())
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

fn verify(library: &Library, vault: &Arc<Vault>) -> Result<()> {
    let objects = library.vault_objects()?;
    if objects.is_empty() {
        println!("the vault holds nothing to weigh");
        return Ok(());
    }

    let mut held = 0_u64;
    let mut moved = Vec::new();
    for object in &objects {
        let reads_back = vault.verify(&object.path, object.form).unwrap_or(false);
        library.note_validated(&object.key, reads_back)?;
        if reads_back {
            held += 1;
        } else {
            moved.push(object.path.clone());
        }
    }

    for path in &moved {
        println!("did not read back as what went in: {}", path.display());
    }
    println!(
        "weighed {} | held {held} | moved {}",
        objects.len(),
        moved.len()
    );
    Ok(())
}

fn prune(library: &Library) -> Result<()> {
    let pruned = library.prune_the_vault()?;
    println!(
        "objects {} | covers {} | staged {}",
        pruned.objects, pruned.covers, pruned.staged
    );
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
