use std::time::Duration;

use resonate_core::{Mbid, Recording};
use resonate_sync::DeletedEverywhere;
use rusqlite::{OptionalExtension, params};

use super::{Answered, DocumentCatalog, Found, RECORDING_COLUMNS, recording_at, unavailable};

const NANOS_A_SECOND: i64 = 1_000_000_000;

impl DocumentCatalog {
    pub(super) fn waiting_to_be_told(&self) -> Answered<Vec<DeletedEverywhere>> {
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare_cached(
                "SELECT id, title, artist, album, mbid, length FROM songs_deleted_everywhere ORDER BY id",
            )
            .map_err(unavailable)?;
        query
            .query_map([], |row| {
                Ok(DeletedEverywhere {
                    id: row.get(0)?,
                    recording: Recording {
                        title: row.get(1)?,
                        artist: row.get(2)?,
                        album: row.get(3)?,
                        mbid: row
                            .get::<_, Option<String>>(4)?
                            .and_then(|mbid| Mbid::new(&mbid).ok()),
                        length: row
                            .get::<_, Option<i64>>(5)?
                            .and_then(|millis| u64::try_from(millis).ok())
                            .map(Duration::from_millis),
                    },
                })
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(unavailable)
    }

    pub(super) fn told_to_every_device(&self, told: &[i64]) -> Answered<()> {
        let connection = self.catalog.0.lock();
        let mut forgetting = connection
            .prepare_cached("DELETE FROM songs_deleted_everywhere WHERE id = ?1")
            .map_err(unavailable)?;

        for id in told {
            forgetting.execute(params![id]).map_err(unavailable)?;
        }
        Ok(())
    }

    pub(super) fn deletion_waits_for_the_platform(
        &self,
        recording: &Recording,
        before: i64,
    ) -> Answered<u64> {
        let mut waiting = 0;

        for id in self.visible_copies(recording)? {
            let Some(added) = self.added_at(id)? else {
                continue;
            };

            if added.saturating_mul(NANOS_A_SECOND) >= before
                || !self.surely_holds(id, recording)?
            {
                continue;
            }

            let connection = self.catalog.0.lock();

            waiting += connection
                .execute(
                    "INSERT OR IGNORE INTO deletions_from_other_devices (track, told) VALUES (?1, ?2)",
                    params![id.cast_signed(), before],
                )
                .map_err(unavailable)?;
        }
        Ok(u64::try_from(waiting).unwrap_or(u64::MAX))
    }

    pub(super) fn deletion_still_waits(&self, recording: &Recording) -> Answered<bool> {
        for id in self.copies(recording)? {
            let connection = self.catalog.0.lock();

            if connection
                .query_row(
                    "SELECT 1 FROM deletions_from_other_devices WHERE track = ?1",
                    params![id.cast_signed()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(unavailable)?
                .is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn deletion_withdrawn(&self, recording: &Recording) -> Answered<()> {
        for id in self.copies(recording)? {
            let connection = self.catalog.0.lock();

            connection
                .execute(
                    "DELETE FROM deletions_from_other_devices WHERE track = ?1",
                    params![id.cast_signed()],
                )
                .map_err(unavailable)?;
        }
        Ok(())
    }

    pub(super) fn added_here_since(&self, recording: &Recording, since: i64) -> Answered<bool> {
        for id in self.visible_copies(recording)? {
            let received = {
                let connection = self.catalog.0.lock();
                connection
                    .query_row(
                        "SELECT 1 FROM songs_received WHERE track = ?1",
                        params![id.cast_signed()],
                        |_| Ok(()),
                    )
                    .optional()
                    .map_err(unavailable)?
                    .is_some()
            };
            let added_since = self
                .added_at(id)?
                .is_some_and(|added| added.saturating_mul(NANOS_A_SECOND) > since);
            if added_since && !received {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn visible_copies(&self, recording: &Recording) -> Answered<Vec<u64>> {
        Ok(self
            .candidates(recording)?
            .into_iter()
            .filter_map(|found| match found {
                Found::Visible(id) => Some(id),
                Found::Withdrawn(_) => None,
            })
            .collect())
    }

    fn copies(&self, recording: &Recording) -> Answered<Vec<u64>> {
        Ok(self
            .candidates(recording)?
            .into_iter()
            .map(|found| match found {
                Found::Visible(id) | Found::Withdrawn(id) => id,
            })
            .collect())
    }

    fn surely_holds(&self, id: u64, recording: &Recording) -> Answered<bool> {
        let connection = self.catalog.0.lock();
        let held = connection
            .query_row(
                &format!("SELECT {RECORDING_COLUMNS} FROM documents d WHERE d.id = ?1"),
                params![id.cast_signed()],
                |row| recording_at(row, 0),
            )
            .optional()
            .map_err(unavailable)?;
        Ok(held.is_some_and(|held| recording.surely_names(&held)))
    }

    fn added_at(&self, id: u64) -> Answered<Option<i64>> {
        let connection = self.catalog.0.lock();
        connection
            .query_row(
                "SELECT added FROM documents WHERE id = ?1",
                params![id.cast_signed()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(unavailable)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc, time::Duration};

    use resonate_core::{Mbid, Recording};
    use resonate_sync::{Catalog, Ledger, Me, Platform, SpaceId, wall_nanos};
    use rusqlite::Connection;

    use super::super::{
        DocumentCatalog, LEDGER,
        tests::{documents_of, folder, player, song},
    };
    use crate::{MobilePlayer, Track};

    const A_MINUTE_IN_NANOS: i64 = 60 * 1_000_000_000;
    const LUNA: &str = "5c1f2b7e-3d9a-4e6b-8f0c-1a2b3c4d5e6f";
    const ANOTHER_LUNA: &str = "9e8d7c6b-5a4f-4e3d-8c2b-1a0f9e8d7c6b";
    const LIVE_MS: u64 = 342_000;

    struct Luna<'a> {
        album: &'a str,
        duration_ms: u64,
        recording: Option<&'a str>,
    }

    const ON_SEABROOK: Luna<'static> = Luna {
        album: "Seabrook",
        duration_ms: 305_000,
        recording: None,
    };

    fn luna(player: &MobilePlayer, uri: &str, luna: &Luna<'_>) -> Track {
        let track = player
            .catalog
            .upsert(Track {
                uri: uri.into(),
                name: "Lúna.flac".into(),
                title: "Lúna".into(),
                artist: "Sölvi".into(),
                album: luna.album.into(),
                duration_ms: luna.duration_ms,
                rate: 44_100,
                codec: "FLAC".into(),
                ..Track::default()
            })
            .expect("an indexed song");

        player
            .catalog
            .learned(track.id, luna.recording, None, None, "")
            .expect("a recording identity");
        track
    }

    fn waiting_for_the_platform(player: &MobilePlayer, told: &Recording) -> (u64, Vec<u64>) {
        let folder = folder("deleted-namesakes");
        let documents = documents_of(player, &folder);

        let taken = documents
            .take_deletion(told, wall_nanos() + A_MINUTE_IN_NANOS)
            .expect("weighed");
        let for_the_platform = player
            .take_deletions_from_other_devices()
            .expect("the waiting");

        let _ = fs::remove_dir_all(&folder);
        (
            taken,
            for_the_platform.iter().map(|track| track.id).collect(),
        )
    }

    fn seabrook(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: "Sölvi".into(),
            album: "Seabrook".into(),
            length: Some(Duration::from_secs(305)),
        }
    }

    #[test]
    fn a_song_deleted_from_every_device_waits_to_be_told_and_one_deleted_here_does_not() {
        let folder = folder("deleted-everywhere");
        let player = player(&folder);

        let documents = documents_of(&player, &folder);
        let wavebreak = song(&player, "content://test/wavebreak.flac", "Wavebreak", None);
        let luna = song(&player, "content://test/luna.flac", "Lúna", None);

        player
            .forget_deleted_songs(vec![luna.id])
            .expect("deleted here");

        let deleted = player
            .forget_songs_deleted_everywhere(vec![wavebreak.id])
            .expect("deleted everywhere");
        let waiting = documents.deleted_everywhere().expect("the waiting");

        documents
            .deletions_told(&waiting.iter().map(|told| told.id).collect::<Vec<_>>())
            .expect("told");

        let told = documents.deleted_everywhere().expect("the waiting");

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(deleted.forgotten, 1);
        assert_eq!(
            waiting
                .iter()
                .map(|told| told.recording.title.as_str())
                .collect::<Vec<_>>(),
            ["Wavebreak"]
        );
        assert!(told.is_empty());
        assert!(
            documents
                .deleted_here(&seabrook("Wavebreak"))
                .expect("asked")
        );
    }

    #[test]
    fn a_deletion_another_device_told_waits_for_the_platform_only_for_a_copy_older_than_it() {
        let folder = folder("deleted-elsewhere");
        let player = player(&folder);

        let documents = documents_of(&player, &folder);
        let mooring = song(&player, "content://test/b.flac", "Mooring", None);
        let told = seabrook("Mooring");
        let a_minute_ago = wall_nanos() - A_MINUTE_IN_NANOS;
        let in_a_minute = wall_nanos() + A_MINUTE_IN_NANOS;

        let too_old = documents
            .take_deletion(&told, a_minute_ago)
            .expect("weighed");
        let added_since_a_minute_ago = documents
            .added_by_hand_since(&told, a_minute_ago)
            .expect("asked");
        let added_since_a_minute_on = documents
            .added_by_hand_since(&told, in_a_minute)
            .expect("asked");
        let taken = documents
            .take_deletion(&told, in_a_minute)
            .expect("weighed");
        let for_the_platform = player
            .take_deletions_from_other_devices()
            .expect("the waiting");
        let once_taken = player
            .take_deletions_from_other_devices()
            .expect("the waiting");

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(too_old, 0);
        assert!(added_since_a_minute_ago);
        assert!(!added_since_a_minute_on);
        assert_eq!(taken, 1);
        assert_eq!(
            for_the_platform
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            [mooring.id]
        );
        assert!(once_taken.is_empty());
    }

    fn ledger() -> Ledger {
        let mut ledger = Ledger::in_memory().expect("a ledger");

        ledger.enter_space(SpaceId([4; 16])).expect("a space");
        ledger
    }

    fn told_by(ledger: &mut Ledger, documents: &DocumentCatalog) -> Vec<resonate_sync::Change> {
        let me = Me {
            name: "Phone".into(),
            platform: Platform::Android,
        };

        ledger.look(documents, &me).expect("a look");

        let told = ledger
            .owed()
            .expect("owed")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        ledger.sent(&told).expect("sent");
        told
    }

    #[test]
    fn a_song_one_phone_deletes_from_every_device_waits_for_the_other_unless_it_refuses() {
        let here = folder("deleting-phone");
        let there = folder("deleted-on-phone");
        let refusing = folder("refusing-phone");

        let (deleting, kept, refused) = (player(&here), player(&there), player(&refusing));
        let wavebreak = song(
            &deleting,
            "content://test/wavebreak.flac",
            "Wavebreak",
            None,
        );
        let copy = song(&kept, "content://test/wavebreak.flac", "Wavebreak", None);

        song(&refused, "content://test/wavebreak.flac", "Wavebreak", None);

        let (from, to) = (documents_of(&deleting, &here), documents_of(&kept, &there));
        let unwilling = DocumentCatalog {
            takes_deletions: false,
            ..documents_of(&refused, &refusing)
        };
        let (mut from_ledger, mut to_ledger, mut unwilling_ledger) = (ledger(), ledger(), ledger());

        told_by(&mut from_ledger, &from);
        told_by(&mut to_ledger, &to);
        told_by(&mut unwilling_ledger, &unwilling);

        deleting
            .forget_songs_deleted_everywhere(vec![wavebreak.id])
            .expect("deleted everywhere");

        let told = told_by(&mut from_ledger, &from);

        to_ledger.arrived(&to, &told).expect("arrived");
        unwilling_ledger
            .arrived(&unwilling, &told)
            .expect("arrived");

        let for_the_platform = kept.take_deletions_from_other_devices().expect("waiting");
        let refused_waiting = refused
            .take_deletions_from_other_devices()
            .expect("waiting");
        let followed_back = unwilling_ledger
            .deleted_everywhere(&seabrook("Wavebreak"))
            .expect("asked");

        for folder in [here, there, refusing] {
            let _ = fs::remove_dir_all(&folder);
        }

        assert_eq!(
            for_the_platform
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            [copy.id]
        );
        assert!(refused_waiting.is_empty());
        assert!(followed_back);
    }

    struct Told {
        deleting: MobilePlayer,
        from: DocumentCatalog,
        from_ledger: Ledger,
        copy: Track,
    }

    fn told_a_deletion(
        here: &Path,
        kept: &MobilePlayer,
        to: &DocumentCatalog,
        to_ledger: &mut Ledger,
    ) -> Told {
        let deleting = player(here);

        let wavebreak = song(
            &deleting,
            "content://test/wavebreak.flac",
            "Wavebreak",
            None,
        );
        let copy = song(kept, "content://test/wavebreak.flac", "Wavebreak", None);
        let from = documents_of(&deleting, here);
        let mut from_ledger = ledger();

        told_by(&mut from_ledger, &from);
        told_by(to_ledger, to);
        deleting
            .forget_songs_deleted_everywhere(vec![wavebreak.id])
            .expect("deleted everywhere");

        let told = told_by(&mut from_ledger, &from);

        to_ledger.arrived(to, &told).expect("arrived");
        Told {
            deleting,
            from,
            from_ledger,
            copy,
        }
    }

    fn over_a_file(folder: &Path) -> MobilePlayer {
        let mut kept = player(folder);

        kept.catalog = Arc::new(
            crate::catalog::Catalog::open(&folder.join("catalog.sqlite")).expect("a catalog"),
        );
        kept
    }

    fn ledger_over_a_file(folder: &Path) -> Ledger {
        let mut ledger = Ledger::open(&folder.join(LEDGER)).expect("a ledger");

        ledger.enter_space(SpaceId([4; 16])).expect("a space");
        ledger
    }

    fn settled(folder: &Path) -> Vec<bool> {
        let connection = Connection::open(folder.join(LEDGER)).expect("the ledger's file");
        let mut query = connection
            .prepare("SELECT settled FROM deletions ORDER BY key")
            .expect("a query");

        query
            .query_map([], |row| row.get(0))
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .expect("the deletions")
    }

    fn ids(tracks: &[Track]) -> Vec<u64> {
        tracks.iter().map(|track| track.id).collect()
    }

    fn added_a_minute_on(player: &MobilePlayer, track: &Track) {
        player
            .catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET added = added + 60 WHERE id = ?1",
                [track.id.cast_signed()],
            )
            .expect("a later addition");
    }

    #[test]
    fn a_removal_the_platform_refuses_waits_over_a_reopen_and_one_that_succeeds_settles_once() {
        let here = folder("deleting-for-a-reopen");
        let there = folder("refusing-before-a-reopen");

        let kept = over_a_file(&there);
        let to = documents_of(&kept, &there);
        let mut to_ledger = ledger_over_a_file(&there);

        let told = told_a_deletion(&here, &kept, &to, &mut to_ledger);

        let handed = kept.take_deletions_from_other_devices().expect("waiting");
        let handed_again_before_a_reopen =
            kept.take_deletions_from_other_devices().expect("waiting");

        to_ledger.settle(&to).expect("settled");

        let waiting_before_a_reopen = settled(&there);

        drop((to_ledger, to, kept));

        let kept = over_a_file(&there);
        let to = documents_of(&kept, &there);
        let mut to_ledger = Ledger::open(&there.join(LEDGER)).expect("the ledger again");

        to_ledger.settle(&to).expect("settled");

        let handed_after_a_reopen = kept.take_deletions_from_other_devices().expect("waiting");
        let forgotten = kept
            .forget_deleted_songs(ids(&handed_after_a_reopen))
            .expect("removed");

        to_ledger.settle(&to).expect("settled");

        let settled_once_removed = settled(&there);

        to_ledger.settle(&to).expect("settled");

        let settled_again = settled(&there);
        let handed_once_removed = kept.take_deletions_from_other_devices().expect("waiting");
        let copy = told.copy.id;

        drop((to_ledger, to, kept, told));
        for folder in [here, there] {
            let _ = fs::remove_dir_all(&folder);
        }

        assert_eq!(ids(&handed), [copy]);
        assert!(handed_again_before_a_reopen.is_empty());
        assert_eq!(waiting_before_a_reopen, [false]);
        assert_eq!(ids(&handed_after_a_reopen), [copy]);
        assert_eq!(forgotten.forgotten, 1);
        assert_eq!(settled_once_removed, [true]);
        assert_eq!(settled_again, [true]);
        assert!(handed_once_removed.is_empty());
    }

    #[test]
    fn a_removal_waiting_for_the_platform_is_cancelled_by_the_song_added_again_anywhere() {
        let deleting_there = folder("deleting-then-adding-again");
        let there = folder("answered-there");
        let deleting_here = folder("deleting-for-an-addition-here");
        let here = folder("added-again-here");

        let answered = player(&there);
        let to_answered = documents_of(&answered, &there);
        let mut answered_ledger = ledger();

        let mut answering = told_a_deletion(
            &deleting_there,
            &answered,
            &to_answered,
            &mut answered_ledger,
        );

        let added_there = song(
            &answering.deleting,
            "content://test/wavebreak-again.flac",
            "Wavebreak",
            None,
        );

        added_a_minute_on(&answering.deleting, &added_there);

        let answer = told_by(&mut answering.from_ledger, &answering.from);

        answered_ledger
            .arrived(&to_answered, &answer)
            .expect("arrived");

        let handed_after_the_answer = answered
            .take_deletions_from_other_devices()
            .expect("waiting");
        let standing_there = answered_ledger
            .deleted_everywhere(&seabrook("Wavebreak"))
            .expect("asked");

        let adding = player(&here);
        let to_adding = documents_of(&adding, &here);
        let mut adding_ledger = ledger();

        let told_here = told_a_deletion(&deleting_here, &adding, &to_adding, &mut adding_ledger);

        let added_here = song(
            &adding,
            "content://test/wavebreak-again.flac",
            "Wavebreak",
            None,
        );

        added_a_minute_on(&adding, &added_here);

        let told_back = deletions(&told_by(&mut adding_ledger, &to_adding));
        let handed_after_the_addition =
            adding.take_deletions_from_other_devices().expect("waiting");
        let held_here = adding.catalog.track(told_here.copy.id).is_ok();

        drop((answering, told_here));
        for folder in [deleting_there, there, deleting_here, here] {
            let _ = fs::remove_dir_all(&folder);
        }

        assert_eq!(deletions(&answer), [false]);
        assert!(handed_after_the_answer.is_empty());
        assert!(!standing_there);
        assert_eq!(told_back, [false]);
        assert!(handed_after_the_addition.is_empty());
        assert!(held_here);
    }

    fn deletions(changes: &[resonate_sync::Change]) -> Vec<bool> {
        changes
            .iter()
            .filter_map(|change| match change {
                resonate_sync::Change::Deletion(note) => Some(note.deleted),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_deletion_another_device_told_waits_only_for_its_recording_and_never_a_namesake() {
        let folder = folder("deleted-by-id");
        let player = player(&folder);

        let named = luna(
            &player,
            "content://test/luna.flac",
            &Luna {
                recording: Some(LUNA),
                ..ON_SEABROOK
            },
        );

        luna(
            &player,
            "content://test/luna-debut.flac",
            &Luna {
                album: "Early Days",
                ..ON_SEABROOK
            },
        );
        luna(
            &player,
            "content://test/luna-live.flac",
            &Luna {
                duration_ms: LIVE_MS,
                ..ON_SEABROOK
            },
        );
        luna(
            &player,
            "content://test/luna-another.flac",
            &Luna {
                recording: Some(ANOTHER_LUNA),
                ..ON_SEABROOK
            },
        );

        let told = Recording {
            mbid: Some(Mbid::new(LUNA).expect("an mbid")),
            ..seabrook("Lúna")
        };
        let (taken, for_the_platform) = waiting_for_the_platform(&player, &told);

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(taken, 1);
        assert_eq!(for_the_platform, [named.id]);
    }

    #[test]
    fn a_deletion_named_without_a_recording_id_waits_only_for_the_copy_of_its_album_and_length() {
        let folder = folder("deleted-by-name");
        let player = player(&folder);

        let named = luna(&player, "content://test/luna.flac", &ON_SEABROOK);

        luna(
            &player,
            "content://test/luna-debut.flac",
            &Luna {
                album: "Early Days",
                ..ON_SEABROOK
            },
        );
        luna(
            &player,
            "content://test/luna-live.flac",
            &Luna {
                duration_ms: LIVE_MS,
                ..ON_SEABROOK
            },
        );

        let (taken, for_the_platform) = waiting_for_the_platform(&player, &seabrook("Lúna"));

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(taken, 1);
        assert_eq!(for_the_platform, [named.id]);
    }

    #[test]
    fn a_deletion_naming_no_album_length_or_recording_id_waits_for_nothing() {
        let folder = folder("deleted-bare");
        let player = player(&folder);

        luna(&player, "content://test/luna.flac", &ON_SEABROOK);
        luna(
            &player,
            "content://test/luna-debut.flac",
            &Luna {
                album: "Early Days",
                ..ON_SEABROOK
            },
        );

        let told = Recording {
            album: String::new(),
            length: None,
            ..seabrook("Lúna")
        };
        let (taken, for_the_platform) = waiting_for_the_platform(&player, &told);

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(taken, 0);
        assert!(for_the_platform.is_empty());
    }
}
