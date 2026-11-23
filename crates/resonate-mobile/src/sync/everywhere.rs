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
    use std::{fs, time::Duration};

    use resonate_core::{Mbid, Recording};
    use resonate_sync::{Catalog, Ledger, Me, Platform, SpaceId, wall_nanos};

    use super::super::{
        DocumentCatalog,
        tests::{documents_of, folder, player, song},
    };
    use crate::{MobilePlayer, Track};

    const A_MINUTE_IN_NANOS: i64 = 60 * 1_000_000_000;
    const JOGA: &str = "5c1f2b7e-3d9a-4e6b-8f0c-1a2b3c4d5e6f";
    const ANOTHER_JOGA: &str = "9e8d7c6b-5a4f-4e3d-8c2b-1a0f9e8d7c6b";
    const LIVE_MS: u64 = 342_000;

    struct Joga<'a> {
        album: &'a str,
        duration_ms: u64,
        recording: Option<&'a str>,
    }

    const ON_HOMOGENIC: Joga<'static> = Joga {
        album: "Homogenic",
        duration_ms: 305_000,
        recording: None,
    };

    fn joga(player: &MobilePlayer, uri: &str, joga: &Joga<'_>) -> Track {
        let track = player
            .catalog
            .upsert(Track {
                uri: uri.into(),
                name: "Jóga.flac".into(),
                title: "Jóga".into(),
                artist: "Björk".into(),
                album: joga.album.into(),
                duration_ms: joga.duration_ms,
                rate: 44_100,
                codec: "FLAC".into(),
                ..Track::default()
            })
            .expect("an indexed song");

        player
            .catalog
            .learned(track.id, joga.recording, None, None, "")
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

    fn homogenic(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: "Björk".into(),
            album: "Homogenic".into(),
            length: Some(Duration::from_secs(305)),
        }
    }

    #[test]
    fn a_song_deleted_from_every_device_waits_to_be_told_and_one_deleted_here_does_not() {
        let folder = folder("deleted-everywhere");
        let player = player(&folder);

        let documents = documents_of(&player, &folder);
        let hunter = song(&player, "content://test/hunter.flac", "Hunter", None);
        let joga = song(&player, "content://test/joga.flac", "Jóga", None);

        player
            .forget_deleted_songs(vec![joga.id])
            .expect("deleted here");

        let deleted = player
            .forget_songs_deleted_everywhere(vec![hunter.id])
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
            ["Hunter"]
        );
        assert!(told.is_empty());
        assert!(documents.deleted_here(&homogenic("Hunter")).expect("asked"));
    }

    #[test]
    fn a_deletion_another_device_told_waits_for_the_platform_only_for_a_copy_older_than_it() {
        let folder = folder("deleted-elsewhere");
        let player = player(&folder);

        let documents = documents_of(&player, &folder);
        let bachelorette = song(&player, "content://test/b.flac", "Bachelorette", None);
        let told = homogenic("Bachelorette");
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
            [bachelorette.id]
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
        let hunter = song(&deleting, "content://test/hunter.flac", "Hunter", None);
        let copy = song(&kept, "content://test/hunter.flac", "Hunter", None);

        song(&refused, "content://test/hunter.flac", "Hunter", None);

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
            .forget_songs_deleted_everywhere(vec![hunter.id])
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
            .deleted_everywhere(&homogenic("Hunter"))
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

    #[test]
    fn a_deletion_another_device_told_waits_only_for_its_recording_and_never_a_namesake() {
        let folder = folder("deleted-by-id");
        let player = player(&folder);

        let named = joga(
            &player,
            "content://test/joga.flac",
            &Joga {
                recording: Some(JOGA),
                ..ON_HOMOGENIC
            },
        );

        joga(
            &player,
            "content://test/joga-debut.flac",
            &Joga {
                album: "Debut",
                ..ON_HOMOGENIC
            },
        );
        joga(
            &player,
            "content://test/joga-live.flac",
            &Joga {
                duration_ms: LIVE_MS,
                ..ON_HOMOGENIC
            },
        );
        joga(
            &player,
            "content://test/joga-another.flac",
            &Joga {
                recording: Some(ANOTHER_JOGA),
                ..ON_HOMOGENIC
            },
        );

        let told = Recording {
            mbid: Some(Mbid::new(JOGA).expect("an mbid")),
            ..homogenic("Jóga")
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

        let named = joga(&player, "content://test/joga.flac", &ON_HOMOGENIC);

        joga(
            &player,
            "content://test/joga-debut.flac",
            &Joga {
                album: "Debut",
                ..ON_HOMOGENIC
            },
        );
        joga(
            &player,
            "content://test/joga-live.flac",
            &Joga {
                duration_ms: LIVE_MS,
                ..ON_HOMOGENIC
            },
        );

        let (taken, for_the_platform) = waiting_for_the_platform(&player, &homogenic("Jóga"));

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(taken, 1);
        assert_eq!(for_the_platform, [named.id]);
    }

    #[test]
    fn a_deletion_naming_no_album_length_or_recording_id_waits_for_nothing() {
        let folder = folder("deleted-bare");
        let player = player(&folder);

        joga(&player, "content://test/joga.flac", &ON_HOMOGENIC);
        joga(
            &player,
            "content://test/joga-debut.flac",
            &Joga {
                album: "Debut",
                ..ON_HOMOGENIC
            },
        );

        let told = Recording {
            album: String::new(),
            length: None,
            ..homogenic("Jóga")
        };
        let (taken, for_the_platform) = waiting_for_the_platform(&player, &told);

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(taken, 0);
        assert!(for_the_platform.is_empty());
    }
}
