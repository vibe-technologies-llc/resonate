use std::{
    fs,
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
    process,
    time::SystemTime,
};

use resonate_codec::{
    CueFile, DEEPEST_FOLDER_A_SHEET_NAMES, Placing, SheetEdit, SheetField, SheetScope, SheetText,
    TagField, carries_what_the_file_did, read_cue, the_folder_a_cue_names,
};
use resonate_core::{
    Frames, MediaLocation, SampleRate, TrackId, naming::named_within, writer::Held,
};
use rusqlite::{Connection, Transaction, params};

use crate::{
    Library, MoveOp, StoreOp,
    error::{Error, Result},
    organise::{self, STAGED},
    retag::{self, TrackToTag, Unwritten},
    scan::{self, LARGEST_SHEET_ON_DISC},
    store,
};

const DISC_SCOPE: i64 = -1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetWrite {
    pub sheet: PathBuf,
    pub file: PathBuf,
    pub edits: Vec<SheetEdit>,
    pub was: Vec<Option<String>>,
    pub wanted: Vec<SheetWanted>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetWanted {
    pub track: u32,
    pub field: TagField,
    pub value: String,
}

pub(crate) enum Planned {
    Write(SheetWrite),
    Unchanged,
    PassedOver(Unwritten),
}

pub(crate) struct Cutting {
    pub sheet: PathBuf,
    pub bytes: Vec<u8>,
    pub cut: CueFile,
}

pub(crate) fn cutting(file: &Path) -> Option<Cutting> {
    let folder = file.parent()?;
    let audio = audio_in(folder);
    let mut above: Vec<&Path> = folder
        .ancestors()
        .skip(1)
        .take(DEEPEST_FOLDER_A_SHEET_NAMES)
        .collect();
    above.reverse();

    above.into_iter().chain([folder]).find_map(|at| {
        sheets_in(at)
            .into_iter()
            .find_map(|sheet| cut_of(&sheet, at, folder, file, &audio))
    })
}

fn cut_of(
    sheet: &Path,
    at: &Path,
    folder: &Path,
    file: &Path,
    audio: &[PathBuf],
) -> Option<Cutting> {
    let bytes = read_whole(sheet).ok()?;
    let cut = read_cue(&bytes)
        .files
        .into_iter()
        .filter(|cut| cut.audio_tracks().next().is_some())
        .find(|cut| {
            the_folder_a_cue_names(at, &cut.named) == folder
                && scan::claimed_beside(&cut.named, audio).is_some_and(|claimed| claimed == file)
        })?;

    Some(Cutting {
        sheet: sheet.to_path_buf(),
        bytes,
        cut,
    })
}

fn read_whole(sheet: &Path) -> io::Result<Vec<u8>> {
    let mut held = Vec::new();
    fs::File::open(sheet)?
        .take(LARGEST_SHEET_ON_DISC.saturating_add(1))
        .read_to_end(&mut held)?;
    if held.len() as u64 > LARGEST_SHEET_ON_DISC {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    Ok(held)
}

fn sheets_in(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut sheets: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| scan::is_a_sheet(path) && !scan::is_passed_over(path) && path.is_file())
        .collect();
    sheets.sort();
    sheets
}

fn audio_in(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| scan::is_audio(path) && !scan::is_passed_over(path) && path.is_file())
        .collect()
}

pub(crate) fn planned(rows: &[TrackToTag]) -> Planned {
    let Some(first) = rows.first() else {
        return Planned::Unchanged;
    };
    if rows.iter().any(|row| row.sheet_modified.is_none()) {
        return Planned::PassedOver(Unwritten::Cut);
    }
    let Some(found) = cutting(&first.path) else {
        return Planned::PassedOver(Unwritten::Cut);
    };
    let stamped = fs::metadata(&found.sheet)
        .and_then(|held| held.modified())
        .map(store::to_nanos)
        .ok();
    if rows.iter().any(|row| row.sheet_modified != stamped) {
        return Planned::PassedOver(Unwritten::SheetChanged);
    }

    let text = SheetText::read(&found.bytes);
    let mut placing = Placings::default();
    for row in rows {
        let Some(track) = SampleRate::new(row.rate)
            .ok()
            .and_then(|rate| found.cut.cut_at(Frames(row.span_start), rate))
        else {
            return Planned::PassedOver(Unwritten::SheetChanged);
        };
        if !text.names_the_track_once(track.number) {
            tracing::debug!(
                sheet = %found.sheet.display(),
                track = track.number,
                "a sheet naming a track more than once is not written to"
            );
            return Planned::PassedOver(Unwritten::Refused);
        }

        for (field, value) in retag::offered(row) {
            let Some((said, placed)) = SheetField::of(field) else {
                continue;
            };
            if field.read(&track.tags).as_deref() == Some(value.as_str()) {
                continue;
            }
            let own = SheetScope::Track(track.number);
            let wanted = SheetWanted {
                track: track.number,
                field,
                value,
            };
            match placed {
                Placing::Track => placing.on(own, said, wanted),
                Placing::Either if text.own(own, said).is_some() => placing.on(own, said, wanted),
                Placing::Either | Placing::Disc => placing.album_wide(said, placed, wanted),
            }
        }
    }

    let (edits, wanted) = placing.settled();
    if edits.is_empty() {
        return Planned::Unchanged;
    }
    let was = edits
        .iter()
        .map(|edit| text.own(edit.scope, edit.field))
        .collect();
    Planned::Write(SheetWrite {
        sheet: found.sheet,
        file: first.path.clone(),
        edits,
        was,
        wanted,
    })
}

#[derive(Default)]
struct Placings {
    edits: Vec<SheetEdit>,
    wanted: Vec<SheetWanted>,
    album_wide: Vec<AlbumWide>,
}

struct AlbumWide {
    field: SheetField,
    placed: Placing,
    wanted: Vec<SheetWanted>,
}

impl Placings {
    fn on(&mut self, scope: SheetScope, field: SheetField, wanted: SheetWanted) {
        self.edits
            .retain(|edit| edit.scope != scope || edit.field != field);
        self.edits.push(SheetEdit {
            scope,
            field,
            value: Some(wanted.value.clone()),
        });
        self.wanted.push(wanted);
    }

    fn album_wide(&mut self, field: SheetField, placed: Placing, wanted: SheetWanted) {
        match self.album_wide.iter_mut().find(|held| held.field == field) {
            Some(held) => held.wanted.push(wanted),
            None => self.album_wide.push(AlbumWide {
                field,
                placed,
                wanted: vec![wanted],
            }),
        }
    }

    fn settled(mut self) -> (Vec<SheetEdit>, Vec<SheetWanted>) {
        for wide in std::mem::take(&mut self.album_wide) {
            let agreed = wide
                .wanted
                .windows(2)
                .all(|pair| pair[0].value == pair[1].value);
            match (agreed, wide.placed) {
                (true, _) => {
                    self.edits.push(SheetEdit {
                        scope: SheetScope::Disc,
                        field: wide.field,
                        value: Some(wide.wanted[0].value.clone()),
                    });
                    self.wanted.extend(wide.wanted);
                }
                (false, Placing::Either) => {
                    for wanted in wide.wanted {
                        self.on(SheetScope::Track(wanted.track), wide.field, wanted);
                    }
                }
                (false, Placing::Track | Placing::Disc) => {
                    tracing::debug!(
                        field = %wide.field,
                        "the rows of one sheet disagree on what it names them all, so none is written"
                    );
                }
            }
        }
        (self.edits, self.wanted)
    }
}

pub(crate) struct SheetFollowed {
    pub rows: Vec<RowFollowed>,
    pub sheet_modified: SystemTime,
}

pub(crate) struct RowFollowed {
    pub track: TrackId,
    pub title: Option<String>,
    pub artist: Option<String>,
}

pub(crate) fn written(
    library: &Library,
    write: &SheetWrite,
) -> std::result::Result<SheetFollowed, Unwritten> {
    let location = MediaLocation::local(&write.sheet);
    let held = read_whole(&write.sheet).map_err(|source| {
        let error = Error::Move {
            op: MoveOp::Rewrite,
            path: write.sheet.clone(),
            source,
        };
        tracing::warn!(%error, "a cue sheet could not be read to be written");
        Unwritten::Unreadable
    })?;
    let text = SheetText::read(&held);
    let standing = write
        .edits
        .iter()
        .zip(&write.was)
        .all(|(edit, was)| text.own(edit.scope, edit.field) == *was);
    if !standing {
        let error = Error::Tags {
            path: write.sheet.clone(),
            source: Box::new(resonate_codec::Error::ChangedWhileWritten {
                location: location.clone(),
            }),
        };
        tracing::warn!(%error, "a cue sheet edited since it was read is not written to");
        return Err(Unwritten::Refused);
    }

    let rewritten = text.edited(&location, &write.edits).map_err(|source| {
        let error = Error::Tags {
            path: write.sheet.clone(),
            source: Box::new(source),
        };
        tracing::warn!(%error, "a cue sheet could not be written");
        Unwritten::Refused
    })?;
    if let Err(error) = landed(library, &write.sheet, &held, &rewritten) {
        tracing::warn!(%error, "a cue sheet could not be written");
        return Err(Unwritten::Refused);
    }

    read_back(library, write)
}

fn landed(library: &Library, sheet: &Path, held: &[u8], rewritten: &[u8]) -> Result<()> {
    let target = fs::canonicalize(sheet).map_err(|source| Error::Move {
        op: MoveOp::Rewrite,
        path: sheet.to_path_buf(),
        source,
    })?;
    let named = target.file_name().unwrap_or_default();
    let staged = target.with_file_name(named_within(
        ".",
        named,
        format!(".{}{STAGED}", process::id()),
    ));
    library.staging(&staged)?;

    let outcome = staged_into(&target, &staged, held, rewritten);
    organise::staged_left(library, &staged);
    outcome
}

fn staged_into(target: &Path, staged: &Path, held: &[u8], rewritten: &[u8]) -> Result<()> {
    let staging = |source| Error::Move {
        op: MoveOp::Rewrite,
        path: staged.to_path_buf(),
        source,
    };
    let landing = |source| Error::Move {
        op: MoveOp::Rewrite,
        path: target.to_path_buf(),
        source,
    };

    let copy = Held::made(staged).map_err(staging)?;
    let mut file = copy.file();
    file.write_all(rewritten)
        .and_then(|()| file.sync_all())
        .map_err(staging)?;
    let standing = fs::metadata(target).map_err(landing)?;
    carries_what_the_file_did(&standing, target, staged);

    if read_whole(target).map_err(landing)? != held {
        return Err(Error::Tags {
            path: target.to_path_buf(),
            source: Box::new(resonate_codec::Error::ChangedWhileWritten {
                location: MediaLocation::local(target),
            }),
        });
    }
    fs::rename(staged, target).map_err(landing)?;
    drop(copy);

    if let Some(folder) = target.parent() {
        organise::settled_folder(folder);
    }
    Ok(())
}

fn read_back(
    library: &Library,
    write: &SheetWrite,
) -> std::result::Result<SheetFollowed, Unwritten> {
    let unconfirmed = |why: &str| {
        tracing::warn!(
            sheet = %write.sheet.display(),
            why,
            "what was written into a cue sheet does not read back, so the catalog does not follow it"
        );
        Unwritten::Unconfirmed
    };

    let back = read_whole(&write.sheet).map_err(|_| unconfirmed("unreadable"))?;
    let text = SheetText::read(&back);
    if write
        .edits
        .iter()
        .any(|edit| text.own(edit.scope, edit.field) != edit.value)
    {
        return Err(unconfirmed("a line"));
    }

    let Some(found) = cutting(&write.file).filter(|found| found.sheet == write.sheet) else {
        return Err(unconfirmed("the cut"));
    };
    for wanted in &write.wanted {
        let read = found
            .cut
            .tracks
            .iter()
            .find(|track| track.number == wanted.track)
            .and_then(|track| wanted.field.read(&track.tags));
        if read.as_deref() != Some(wanted.value.as_str()) {
            return Err(unconfirmed("a track"));
        }
    }

    let sheet_modified = fs::metadata(&write.sheet)
        .and_then(|held| held.modified())
        .map_err(|_| unconfirmed("unstamped"))?;
    let rows = library
        .rows_cut_from(&write.file)
        .map_err(|_| unconfirmed("the catalog"))?
        .into_iter()
        .filter_map(|(track, span_start, rate)| {
            let rate = SampleRate::new(rate).ok()?;
            let titled = found.cut.cut_at(Frames(span_start), rate)?.titled();
            Some(RowFollowed {
                track,
                title: titled.title,
                artist: titled.artist,
            })
        })
        .collect();

    Ok(SheetFollowed {
        rows,
        sheet_modified,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeptSheet {
    pub sheet: PathBuf,
    pub file: PathBuf,
    pub fields: Vec<KeptLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeptLine {
    pub scope: SheetScope,
    pub field: SheetField,
    pub was: Option<String>,
    pub wrote: Option<String>,
}

pub(crate) fn undone(kept: &[KeptSheet]) -> (Vec<SheetWrite>, Vec<(PathBuf, Unwritten)>) {
    let mut writes = Vec::new();
    let mut passed = Vec::new();
    for sheet in kept {
        let Ok(held) = read_whole(&sheet.sheet) else {
            passed.push((sheet.sheet.clone(), Unwritten::Unreadable));
            continue;
        };
        let text = SheetText::read(&held);
        let standing: Vec<&KeptLine> = sheet
            .fields
            .iter()
            .filter(|line| text.own(line.scope, line.field) == line.wrote)
            .collect();
        if standing.len() < sheet.fields.len() {
            tracing::debug!(
                sheet = %sheet.sheet.display(),
                "a line edited since the run wrote it is left as it now stands"
            );
        }
        if standing.is_empty() {
            continue;
        }
        writes.push(SheetWrite {
            sheet: sheet.sheet.clone(),
            file: sheet.file.clone(),
            edits: standing
                .iter()
                .map(|line| SheetEdit {
                    scope: line.scope,
                    field: line.field,
                    value: line.was.clone(),
                })
                .collect(),
            was: standing.iter().map(|line| line.wrote.clone()).collect(),
            wanted: Vec::new(),
        });
    }
    (writes, passed)
}

const fn scope_kept(scope: SheetScope) -> i64 {
    match scope {
        SheetScope::Disc => DISC_SCOPE,
        SheetScope::Track(number) => number as i64,
    }
}

fn scope_read(kept: i64) -> Option<SheetScope> {
    match kept {
        DISC_SCOPE => Some(SheetScope::Disc),
        number => u32::try_from(number).ok().map(SheetScope::Track),
    }
}

pub(crate) fn note_what_was_there(tx: &Transaction<'_>, writes: &[SheetWrite]) -> Result<()> {
    for write in writes {
        let kept = KeptSheet {
            sheet: write.sheet.clone(),
            file: write.file.clone(),
            fields: write
                .edits
                .iter()
                .zip(&write.was)
                .map(|(edit, was)| KeptLine {
                    scope: edit.scope,
                    field: edit.field,
                    was: was.clone(),
                    wrote: edit.value.clone(),
                })
                .collect(),
        };
        note(tx, &kept)?;
    }
    Ok(())
}

fn note(tx: &Transaction<'_>, kept: &KeptSheet) -> Result<()> {
    let sheet = store::path_text(&kept.sheet);
    let file = store::path_text(&kept.file);
    for line in &kept.fields {
        tx.prepare_cached(
            "INSERT OR REPLACE INTO retagged_sheets (sheet, file, scope, field, was, wrote)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .and_then(|mut statement| {
            statement.execute(params![
                sheet,
                file,
                scope_kept(line.scope),
                line.field.as_str(),
                line.was,
                line.wrote
            ])
        })
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }
    Ok(())
}

pub(crate) fn forget_the_notes_of(tx: &Transaction<'_>, sheets: &[PathBuf]) -> Result<()> {
    for sheet in sheets {
        tx.prepare_cached("DELETE FROM retagged_sheets WHERE sheet = ?1")
            .and_then(|mut statement| statement.execute(params![store::path_text(sheet)]))
            .map_err(|source| Error::store(StoreOp::Delete, source))?;
    }
    Ok(())
}

pub(crate) fn sheets_retagged(
    tx: &Transaction<'_>,
    followed: &[SheetFollowed],
    unwritten: &[PathBuf],
    noted_again: Option<&[KeptSheet]>,
) -> Result<()> {
    for sheet in followed {
        for row in &sheet.rows {
            tx.prepare_cached(
                "UPDATE tracks SET tagged_title = ?2, tagged_artist = ?3, sheet_modified = ?4
                  WHERE id = ?1",
            )
            .and_then(|mut statement| {
                statement.execute(params![
                    row.track.get() as i64,
                    row.title,
                    row.artist,
                    store::to_nanos(sheet.sheet_modified)
                ])
            })
            .map_err(|source| Error::store(StoreOp::Update, source))?;
        }
    }

    forget_the_notes_of(tx, unwritten)?;
    for kept in noted_again.unwrap_or_default() {
        note(tx, kept)?;
    }
    Ok(())
}

pub(crate) fn last_resheet(connection: &Connection) -> Result<Vec<KeptSheet>> {
    let mut statement = connection
        .prepare(
            "SELECT sheet, file, scope, field, was, wrote FROM retagged_sheets
              ORDER BY sheet, scope, field",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut kept: Vec<KeptSheet> = Vec::new();
    for (sheet, file, scope, field, was, wrote) in rows {
        let sheet = store::path_of(&sheet);
        if kept.last().is_none_or(|last| last.sheet != sheet) {
            kept.push(KeptSheet {
                sheet,
                file: store::path_of(&file),
                fields: Vec::new(),
            });
        }
        if let (Some(last), Some(scope), Some(field)) = (
            kept.last_mut(),
            scope_read(scope),
            SheetField::named(&field),
        ) {
            last.fields.push(KeptLine {
                scope,
                field,
                was,
                wrote,
            });
        }
    }
    Ok(kept)
}

pub(crate) fn rows_cut_from(
    connection: &Connection,
    file: &Path,
) -> Result<Vec<(TrackId, u64, u32)>> {
    let mut statement = connection
        .prepare_cached("SELECT id, span_start, sample_rate FROM tracks WHERE path = ?1")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(params![store::path_text(file)], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    rows.into_iter()
        .map(|(id, span_start, rate)| {
            Ok((
                TrackId::new(id as u64)?,
                span_start.max(0) as u64,
                u32::try_from(rate).unwrap_or_default(),
            ))
        })
        .collect()
}
