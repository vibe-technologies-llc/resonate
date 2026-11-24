use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
    process,
    sync::atomic::Ordering,
};

use resonate_core::{
    Recording, WholeFile,
    writer::{self, Held, Writer},
};
use resonate_sync::Refused;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{Answered, DocumentCatalog, file_of, keys_of, note_received, unavailable};
use crate::{catalog::Catalog, playlists};

const NO_STAMP: u32 = 0;

enum Landing {
    KnownByItsFile,
    FoldedIntoTheWaiting,
    Waiting,
}

impl DocumentCatalog {
    pub(super) fn landed_in_holding(
        &self,
        recording: &Recording,
        extension: &str,
        staged: &Path,
    ) -> Answered<bool> {
        fs::create_dir_all(&self.received).map_err(|_| Refused::Unwritable)?;

        let path = self.landed_name(extension);
        let text = path.to_str().ok_or(Refused::Unwritable)?.to_owned();
        let held = Held::resumed(staged).map_err(|_| Refused::Unwritable)?;

        fs::rename(staged, &path).map_err(|_| Refused::Unwritable)?;

        let landing = self.kept_in_holding(recording, extension, &path, &text);

        if !matches!(landing, Ok(Landing::Waiting)) {
            removed(&path);
        }
        drop(held);

        match landing? {
            Landing::KnownByItsFile | Landing::Waiting => self.arrived(),
            Landing::FoldedIntoTheWaiting => {}
        }
        Ok(true)
    }

    fn kept_in_holding(
        &self,
        recording: &Recording,
        extension: &str,
        path: &Path,
        text: &str,
    ) -> Answered<Landing> {
        let file = file_of(&self.sources, path);

        if let Some(file) = file
            && let Some(track) = self.held_as_the_same_file(file)?
        {
            note_received(&self.catalog.0.lock(), &keys_of(recording), track)
                .map_err(unavailable)?;

            return Ok(Landing::KnownByItsFile);
        }

        let mut connection = self.catalog.0.lock();

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;

        let waiting = match file {
            Some(file) => waiting_as_the_same_file(&transaction, file).map_err(unavailable)?,
            None => None,
        };
        let landing = match waiting {
            Some(waiting) => {
                folded_into_the_waiting(&transaction, waiting, recording).map_err(unavailable)?;
                Landing::FoldedIntoTheWaiting
            }
            None => {
                kept_waiting(&transaction, recording, extension, text, file)
                    .map_err(unavailable)?;
                Landing::Waiting
            }
        };

        transaction.commit().map_err(unavailable)?;
        Ok(landing)
    }

    fn landed_name(&self, extension: &str) -> PathBuf {
        let stamp = playlists::created_now();
        let nth = self.landings.fetch_add(1, Ordering::AcqRel);
        self.received
            .join(format!("{}-{stamp}-{nth}.{extension}", process::id()))
    }
}

fn waiting_as_the_same_file(
    connection: &Connection,
    file: WholeFile,
) -> rusqlite::Result<Option<i64>> {
    connection
        .query_row(
            "SELECT id FROM received WHERE packets = ?1 AND frames = ?2 AND rate = ?3 ORDER BY id",
            params![
                file.packets.cast_signed(),
                file.frames.cast_signed(),
                file.rate
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()
}

fn folded_into_the_waiting(
    connection: &Connection,
    waiting: i64,
    recording: &Recording,
) -> rusqlite::Result<()> {
    for key in keys_of(recording) {
        connection.execute(
            "INSERT OR IGNORE INTO received_also (received, recording) VALUES (?1, ?2)",
            params![waiting, key],
        )?;
    }
    Ok(())
}

fn kept_waiting(
    connection: &Connection,
    recording: &Recording,
    extension: &str,
    text: &str,
    file: Option<WholeFile>,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO received(path, extension, title, artist, album, recording, duration_ms, packets, frames, rate) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                text,
                extension,
                recording.title,
                recording.artist,
                recording.album,
                recording.mbid.as_ref().map(ToString::to_string),
                recording
                    .length
                    .map(|length| i64::try_from(length.as_millis()).unwrap_or(i64::MAX)),
                file.map(|file| file.packets.cast_signed()),
                file.map(|file| file.frames.cast_signed()),
                file.map(|file| file.rate),
            ],
        )
        .map(drop)
}

pub(super) fn kept_at(holding: &Path, stored: &Path) -> PathBuf {
    stored
        .file_name()
        .map_or_else(|| stored.to_path_buf(), |name| holding.join(name))
}

pub(super) fn forgotten(
    connection: &Connection,
    waiting: i64,
    path: &Path,
) -> rusqlite::Result<()> {
    connection.execute("DELETE FROM received WHERE id = ?1", [waiting])?;

    if let Some(name) = path.file_name().and_then(OsStr::to_str) {
        connection.execute(
            "INSERT OR IGNORE INTO received_leftovers (name) VALUES (?1)",
            [name],
        )?;
    }
    Ok(())
}

pub(super) fn let_go(catalog: &Catalog, holding: &Path, path: &Path) {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return;
    };

    if !removed(&holding.join(name)) {
        return;
    }
    if let Err(error) = catalog
        .0
        .lock()
        .execute("DELETE FROM received_leftovers WHERE name = ?1", [name])
    {
        tracing::warn!(%error, "a received file removed is still listed to be removed");
    }
}

pub(super) fn swept(catalog: &Catalog, holding: &Path) -> rusqlite::Result<()> {
    let mut connection = catalog.0.lock();

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let waiting = names_in(&transaction, "SELECT path FROM received")?;
    let leftovers = names_in(&transaction, "SELECT name FROM received_leftovers")?;

    for name in &leftovers {
        if removed(&holding.join(name)) {
            transaction.execute(
                "DELETE FROM received_leftovers WHERE name = ?1",
                [name.to_string_lossy()],
            )?;
        }
    }

    let Ok(entries) = fs::read_dir(holding) else {
        return transaction.commit();
    };

    for entry in entries.flatten() {
        let name = entry.file_name();

        if waiting.contains(&name)
            || leftovers.contains(&name)
            || !entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            continue;
        }

        let path = entry.path();

        if writer::writer_of(&path, stamp_of(&name)) == Writer::Gone {
            removed(&path);
        }
    }
    transaction.commit()
}

fn names_in(connection: &Connection, query: &str) -> rusqlite::Result<BTreeSet<OsString>> {
    let mut statement = connection.prepare(query)?;
    statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|stored| {
            stored.map(|stored| {
                Path::new(&stored)
                    .file_name()
                    .map_or_else(|| OsString::from(&stored), OsStr::to_owned)
            })
        })
        .collect()
}

fn stamp_of(name: &OsStr) -> u32 {
    let Some(stem) = Path::new(name).file_stem().and_then(OsStr::to_str) else {
        return NO_STAMP;
    };
    let mut parts = stem.split('-');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(process), Some(_), Some(_), None) => process.parse().unwrap_or(NO_STAMP),
        _ => NO_STAMP,
    }
}

fn removed(path: &Path) -> bool {
    match unlinked(path) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "a received file could not be removed and waits for the next sweep");
            false
        }
    }
}

fn unlinked(path: &Path) -> io::Result<()> {
    #[cfg(test)]
    if tests::UNLINKS_REFUSED.get() {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs::File, sync::Arc, thread, time::SystemTime};

    use resonate_core::writer::UNHELD_AND_UNTOUCHED_FOR;
    use resonate_sync::{Catalog as _, SongTags};

    use super::*;
    use crate::sync::{
        MobilePlayer, RECEIVED,
        tests::{folder, granted, landed_waiting, two_seconds_of},
    };

    thread_local! {
        pub(super) static UNLINKS_REFUSED: Cell<bool> = const { Cell::new(false) };
    }

    fn held_here(folder: &Path) -> BTreeSet<OsString> {
        fs::read_dir(folder.join(RECEIVED))
            .map(|entries| entries.flatten().map(|entry| entry.file_name()).collect())
            .unwrap_or_default()
    }

    fn tracked(player: &MobilePlayer) -> BTreeSet<OsString> {
        let connection = player.catalog.0.lock();
        let mut tracked = names_in(&connection, "SELECT path FROM received").expect("the waiting");

        tracked.extend(
            names_in(&connection, "SELECT name FROM received_leftovers").expect("the leftovers"),
        );
        tracked
    }

    fn leftovers(player: &MobilePlayer) -> usize {
        names_in(
            &player.catalog.0.lock(),
            "SELECT name FROM received_leftovers",
        )
        .expect("the leftovers")
        .len()
    }

    fn aged(folder: &Path) {
        let long_ago = SystemTime::now() - UNHELD_AND_UNTOUCHED_FOR * 2;

        for name in held_here(folder) {
            File::options()
                .write(true)
                .open(folder.join(RECEIVED).join(name))
                .and_then(|file| file.set_modified(long_ago))
                .expect("an aged file");
        }
    }

    fn sweep(player: &MobilePlayer) {
        swept(&player.catalog, &player.syncing.holding()).expect("a sweep");
    }

    fn refusing_aliases(player: &MobilePlayer, refused: bool) {
        let statement = if refused {
            "CREATE TEMP TRIGGER songs_received_refused BEFORE INSERT ON songs_received
                 BEGIN SELECT RAISE(ABORT, 'refused'); END;
             CREATE TEMP TRIGGER received_also_refused BEFORE INSERT ON received_also
                 BEGIN SELECT RAISE(ABORT, 'refused'); END;"
        } else {
            "DROP TRIGGER temp.songs_received_refused; DROP TRIGGER temp.received_also_refused;"
        };

        player
            .catalog
            .0
            .lock()
            .execute_batch(statement)
            .expect("the aliases refused or allowed");
    }

    fn landed_again(
        documents: &DocumentCatalog,
        folder: &Path,
        recording: &Recording,
    ) -> Answered<bool> {
        let staged = folder.join(format!("{}.part", recording.title));

        fs::write(&staged, crate::harness::silence()).expect("a staged song");
        documents.land(recording, &SongTags::default(), "wav", &staged)
    }

    fn landed_by_the_front_end(player: &MobilePlayer, folder: &Path) {
        use std::os::fd::IntoRawFd;

        let waiting = player.sync_received().expect("the waiting songs");
        let descriptor = File::create(folder.join("landed.wav"))
            .expect("a destination")
            .into_raw_fd();

        player
            .sync_receive(
                waiting[0].id,
                descriptor,
                "content://test/landed.wav".into(),
            )
            .expect("a landed song");
    }

    #[test]
    fn a_failed_alias_write_after_the_rename_leaves_the_received_file_tracked_or_reclaimed() {
        let folder = folder("alias-refused");
        let (player, documents) = granted(&folder);
        let hunter = two_seconds_of("Hunter");
        let renamed = two_seconds_of("Hunter (Remastered)");
        let live = two_seconds_of("Hunter (Live)");

        landed_waiting(&documents, &folder, &hunter);
        refusing_aliases(&player, true);

        let folded = landed_again(&documents, &folder, &renamed);
        let after_the_fold = (held_here(&folder), tracked(&player));

        UNLINKS_REFUSED.set(true);

        let refused_twice = landed_again(&documents, &folder, &renamed);
        let left_untracked = held_here(&folder).len() - tracked(&player).len();

        UNLINKS_REFUSED.set(false);
        aged(&folder);
        sweep(&player);

        let after_the_sweep = (held_here(&folder), tracked(&player));

        refusing_aliases(&player, false);
        landed_by_the_front_end(&player, &folder);
        refusing_aliases(&player, true);

        let known = landed_again(&documents, &folder, &live);
        let after_the_known = held_here(&folder);

        refusing_aliases(&player, false);

        let known_again = landed_again(&documents, &folder, &live);
        let held = (
            documents.holds(&renamed).expect("an answer"),
            documents.holds(&live).expect("an answer"),
        );

        let _ = fs::remove_dir_all(&folder);

        assert!(matches!(folded, Err(Refused::Unavailable)));
        assert_eq!(after_the_fold.0, after_the_fold.1);
        assert_eq!(after_the_fold.0.len(), 1);
        assert!(matches!(refused_twice, Err(Refused::Unavailable)));
        assert_eq!(left_untracked, 1);
        assert_eq!(after_the_sweep.0, after_the_sweep.1);
        assert_eq!(after_the_sweep.0, after_the_fold.0);
        assert!(matches!(known, Err(Refused::Unavailable)));
        assert!(after_the_known.is_empty());
        assert!(matches!(known_again, Ok(true)));
        assert!(held_here(&folder).is_empty());
        assert_eq!(held, (false, true));
    }

    #[test]
    fn a_failed_unlink_keeps_a_row_the_next_sweep_removes_with_its_file() {
        let folder = folder("unlink-refused");
        let (player, documents) = granted(&folder);
        let hunter = two_seconds_of("Hunter");
        let joga = two_seconds_of("Jóga");

        landed_waiting(&documents, &folder, &hunter);
        UNLINKS_REFUSED.set(true);
        landed_by_the_front_end(&player, &folder);

        let after_landing = (held_here(&folder), tracked(&player), leftovers(&player));
        let waiting_after_landing = player.sync_received().expect("the waiting").len();

        UNLINKS_REFUSED.set(false);
        fs::write(folder.join("Jóga.part"), b"fLaC").expect("a damaged song");
        documents
            .land(
                &joga,
                &SongTags::default(),
                "flac",
                &folder.join("Jóga.part"),
            )
            .expect("a landing");
        UNLINKS_REFUSED.set(true);

        let refused = player.sync_received().expect("the waiting")[0].id;

        player.sync_refuse_received(refused).expect("given up");

        let after_refusing = (held_here(&folder), tracked(&player), leftovers(&player));

        sweep(&player);

        let swept_while_refused = held_here(&folder).len();

        UNLINKS_REFUSED.set(false);
        sweep(&player);

        let after_the_sweep = (held_here(&folder), leftovers(&player));
        let waiting = player.sync_received().expect("the waiting").len();

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(after_landing.0, after_landing.1);
        assert_eq!(after_landing.0.len(), 1);
        assert_eq!(after_landing.2, 1);
        assert_eq!(waiting_after_landing, 0);
        assert_eq!(after_refusing.0, after_refusing.1);
        assert_eq!(after_refusing.0.len(), 2);
        assert_eq!(after_refusing.2, 2);
        assert_eq!(swept_while_refused, 2);
        assert!(after_the_sweep.0.is_empty());
        assert_eq!(after_the_sweep.1, 0);
        assert_eq!(waiting, 0);
    }

    #[test]
    fn the_same_file_landed_at_once_under_many_spellings_waits_once_and_leaves_nothing_untracked() {
        const SPELLINGS: usize = 8;

        let folder = folder("adopted-at-once");
        let (player, documents) = granted(&folder);
        let documents = Arc::new(documents);
        let spellings = (0..SPELLINGS)
            .map(|nth| two_seconds_of(&format!("Hunter {nth}")))
            .collect::<Vec<_>>();

        let landings = spellings
            .iter()
            .cloned()
            .map(|recording| {
                let documents = Arc::clone(&documents);
                let folder = folder.clone();
                thread::spawn(move || landed_again(&documents, &folder, &recording))
            })
            .collect::<Vec<_>>();
        let landed = landings
            .into_iter()
            .map(|landing| landing.join().expect("a landing thread"))
            .collect::<Vec<_>>();

        let waiting = player.sync_received().expect("the waiting songs");
        let after = (held_here(&folder), tracked(&player));
        let every_spelling_held = spellings
            .iter()
            .all(|recording| documents.holds(recording).expect("an answer"));

        let _ = fs::remove_dir_all(&folder);

        assert!(landed.iter().all(|landing| matches!(landing, Ok(true))));
        assert_eq!(waiting.len(), 1);
        assert_eq!(after.0, after.1);
        assert_eq!(after.0.len(), 1);
        assert!(every_spelling_held);
    }

    #[test]
    fn a_catalog_of_before_leftovers_keeps_its_waiting_songs_and_its_orphans_are_swept() {
        let folder = folder("before-leftovers");
        let path = folder.join("documents.sqlite");
        let holding = folder.join(RECEIVED);
        let moved_from = Path::new("/an/older/container").join(RECEIVED);
        let before = crate::catalog::MIGRATIONS
            .iter()
            .position(|step| {
                *step == crate::catalog::A_RECEIVED_FILE_IS_FORGOTTEN_ONLY_ONCE_REMOVED
            })
            .expect("the leftovers step");

        fs::create_dir_all(&holding).expect("the holding folder");
        fs::write(holding.join("1700000000000000000-0.flac"), b"fLaC").expect("a waiting file");
        fs::write(holding.join("1700000000000000000-1.flac"), b"fLaC").expect("an orphan");
        aged(&folder);

        let laid = Connection::open(&path).expect("a catalog");

        laid.create_scalar_function(
            "folded",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
        )
        .expect("the fold");
        for step in &crate::catalog::MIGRATIONS[..before] {
            laid.execute_batch(step).expect("an earlier step");
        }
        laid.execute(
            "INSERT INTO received (id, path, extension, title, artist, album) VALUES (4, ?1, 'flac', 'Hunter', 'Björk', 'Homogenic')",
            [moved_from
                .join("1700000000000000000-0.flac")
                .to_str()
                .expect("a path")],
        )
        .expect("a waiting song");
        laid.pragma_update(
            None,
            "user_version",
            u32::try_from(before).expect("a count"),
        )
        .expect("the stamp");
        drop(laid);

        let catalog = Catalog::open(&path).expect("the catalog is carried forward");

        swept(&catalog, &holding).expect("a sweep");

        let kept: Vec<i64> = catalog
            .0
            .lock()
            .prepare("SELECT id FROM received")
            .and_then(|mut query| query.query_map([], |row| row.get(0))?.collect())
            .expect("the waiting songs");
        let left = held_here(&folder);

        drop(catalog);

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(kept, vec![4]);
        assert_eq!(
            left,
            BTreeSet::from([OsString::from("1700000000000000000-0.flac")])
        );
    }

    #[test]
    fn a_landing_name_carries_the_process_that_holds_it_and_an_older_name_carries_none() {
        let name = OsString::from(format!("{}-1700000000000000000-3.flac", process::id()));

        assert_eq!(stamp_of(&name), process::id());
        assert_eq!(stamp_of(OsStr::new("1700000000000000000-3.flac")), NO_STAMP);
        assert_eq!(stamp_of(OsStr::new("song.flac")), NO_STAMP);
    }
}
