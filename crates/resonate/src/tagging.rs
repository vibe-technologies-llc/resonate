use std::{
    mem,
    path::{Path, PathBuf},
    sync::Arc,
};

use resonate_codec::{CoverArt, Popularity};
use resonate_library::{FileTags, Library, PassKind, RetagOptions, RetagSummary, SheetScope};
use resonate_lyrics::{Beside, KeptSheet, Standing, plan_beside, write_beside};

use crate::{
    arguments::{from_here, local_path},
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

pub(crate) fn lyrics_beside(
    library: &Library,
    files: &[PathBuf],
    apply: bool,
    replace: bool,
) -> Result<()> {
    let standing = match replace {
        true => Standing::Replaced,
        false => Standing::LeftAlone,
    };
    let mut failed = 0;
    let mut planned = 0;
    for file in files {
        let path = from_here(&local_path(file));
        let named = path.display().to_string();
        let told = match sheeted(library, &path, standing, apply)? {
            Sheeted::Beside(Beside::Written(sheet)) if apply => {
                words::beside::written(named, sheet.display().to_string())
            }
            Sheeted::Beside(Beside::Written(sheet)) => {
                planned += 1;
                words::beside::would_write(named, sheet.display().to_string())
            }
            Sheeted::Beside(Beside::AlreadySaid(sheet)) => {
                words::beside::already_said(named, sheet.display().to_string())
            }
            Sheeted::NotInTheCatalog => words::beside::not_in_the_catalog(named),
            Sheeted::NotAFileOfItsOwn => words::beside::not_a_file_of_its_own(named),
            Sheeted::NothingKept => words::beside::nothing_kept(named),
            Sheeted::Refused(resonate_lyrics::Error::SidecarStands { path }) => {
                words::beside::stands(named, path.display().to_string())
            }
            Sheeted::Refused(resonate_lyrics::Error::SidecarOutranked { path }) => {
                words::beside::outranked(named, path.display().to_string())
            }
            Sheeted::Refused(error) => {
                failed += 1;
                words::beside::unwritten(named, error.to_string())
            }
        };
        said!("{told}");
    }
    if !apply && planned > 0 {
        said!("{}", words::main::nothing_written());
    }

    none_failed(PassKind::Retag, failed)
}

enum Sheeted {
    Beside(Beside),
    NotInTheCatalog,
    NotAFileOfItsOwn,
    NothingKept,
    Refused(resonate_lyrics::Error),
}

fn sheeted(library: &Library, path: &Path, standing: Standing, apply: bool) -> Result<Sheeted> {
    let Some(track) = library.track_at(path, None)? else {
        return Ok(Sheeted::NotInTheCatalog);
    };
    if track.span.is_some() || !track.location.is_local() {
        return Ok(Sheeted::NotAFileOfItsOwn);
    }
    let Some(sung) = library
        .kept_lyrics(&track.location, None)?
        .and_then(|kept| kept.sung)
    else {
        return Ok(Sheeted::NothingKept);
    };
    let sheet = KeptSheet {
        text: &sung.text,
        document: sung.lyricsfile.as_deref(),
    };
    let beside = match apply {
        true => write_beside(path, sheet, standing),
        false => plan_beside(path, sheet, standing),
    };

    Ok(beside.map_or_else(Sheeted::Refused, Sheeted::Beside))
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

#[cfg(test)]
mod tests {
    use std::fs;

    use resonate_core::MediaLocation;
    use resonate_library::LyricText;

    use super::*;
    use crate::{
        cli::Sub,
        fixtures::{parsed, scanned, scratch, silent_wave},
    };

    const KEPT: &str = "[00:01.00]all that you touch\n[00:05.00]all that you see\n";

    fn kept_beside(name: &str) -> (Library, PathBuf) {
        let root = scratch(name);
        let track = root.join("Eclipse.wav");
        fs::write(&track, silent_wave(10)).expect("a track");
        let library = scanned(&root);
        library
            .keep_lyrics(
                &MediaLocation::local(&track),
                None,
                Some(&LyricText {
                    text: KEPT.to_owned(),
                    synced: true,
                    lyricsfile: None,
                }),
            )
            .expect("lyrics kept");

        (library, track)
    }

    #[test]
    fn the_lyrics_kept_for_a_file_are_planned_then_written_beside_it_and_said_once_written() {
        let (library, track) = kept_beside("lyrics-beside");
        let sheet = track.with_extension("lrc");

        let planned = sheeted(&library, &track, Standing::LeftAlone, false).expect("a plan");
        let unwritten = sheet.exists();
        let written = sheeted(&library, &track, Standing::LeftAlone, true).expect("a write");
        let again = sheeted(&library, &track, Standing::LeftAlone, true).expect("a second write");

        assert!(matches!(planned, Sheeted::Beside(Beside::Written(ref at)) if *at == sheet));
        assert!(!unwritten);
        assert!(matches!(written, Sheeted::Beside(Beside::Written(ref at)) if *at == sheet));
        assert_eq!(fs::read_to_string(&sheet).expect("the sheet"), KEPT);
        assert!(matches!(again, Sheeted::Beside(Beside::AlreadySaid(_))));
        lyrics_beside(&library, &[track], true, false).expect("nothing failed");
    }

    #[test]
    fn a_sheet_of_the_listeners_beside_the_file_is_left_alone_until_replace_is_asked() {
        let (library, track) = kept_beside("lyrics-left-alone");
        let sheet = track.with_extension("lrc");
        fs::write(&sheet, "[00:02.00]their own").expect("a sheet of their own");

        let left = sheeted(&library, &track, Standing::LeftAlone, true).expect("a refusal");
        let after = fs::read_to_string(&sheet).expect("their sheet");
        let replaced = sheeted(&library, &track, Standing::Replaced, true).expect("a write");

        assert!(matches!(
            left,
            Sheeted::Refused(resonate_lyrics::Error::SidecarStands { .. })
        ));
        assert_eq!(after, "[00:02.00]their own");
        assert!(matches!(replaced, Sheeted::Beside(Beside::Written(_))));
        assert_eq!(fs::read_to_string(&sheet).expect("the sheet"), KEPT);
    }

    #[test]
    fn a_file_outside_the_catalog_writes_nothing() {
        let (library, track) = kept_beside("lyrics-none");
        let other = track.with_file_name("Other.wav");
        fs::write(&other, silent_wave(10)).expect("a track");

        let outside = sheeted(&library, &other, Standing::LeftAlone, true).expect("an answer");

        assert!(matches!(outside, Sheeted::NotInTheCatalog));
        assert!(!other.with_extension("lrc").exists());
    }

    #[test]
    fn tag_takes_the_files_whose_lyrics_to_write_and_whether_to_replace_a_sheet() {
        let cli = parsed(&[
            "resonate",
            "tag",
            "--lyrics",
            "a.flac",
            "b.flac",
            "--replace",
            "--apply",
        ]);

        let Some(Sub::Tag {
            lyrics,
            replace,
            apply,
            ..
        }) = cli.command
        else {
            panic!("tag was not read");
        };
        assert_eq!(lyrics, [PathBuf::from("a.flac"), PathBuf::from("b.flac")]);
        assert!(replace && apply);
    }
}
