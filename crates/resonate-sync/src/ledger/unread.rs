use ring::digest::{SHA256, digest};
use rusqlite::{TransactionBehavior, params};

use super::{BUNDLE_BYTES, Ledger};
use crate::{
    Error, Result, StoreOp,
    catalog::Catalog,
    wire::{Unread, bundle_of_encoded, encoded_bundle, read_change},
};

pub(super) const CHANGES_UNREAD_ARE_CARRIED_FORWARD: &str = "
CREATE TABLE unread (
    digest BLOB PRIMARY KEY,
    change BLOB NOT NULL,
    refused INTEGER NOT NULL
) STRICT;
";

struct Kept {
    digest: Vec<u8>,
    change: Vec<u8>,
}

impl Ledger {
    pub fn unread_arrived(&mut self, unread: &[Unread]) -> Result<()> {
        if unread.is_empty() {
            return Ok(());
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;

        for change in unread {
            let encoded = change.encoded();

            transaction
                .execute(
                    "INSERT INTO unread (digest, change, refused) VALUES (?1, ?2, ?3)
                     ON CONFLICT (digest) DO NOTHING",
                    params![
                        digest(&SHA256, encoded).as_ref(),
                        encoded,
                        matches!(change, Unread::Refused(_))
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn snapshot_bundles(&self) -> Result<Vec<Vec<u8>>> {
        let mut bundles = self
            .snapshot()?
            .iter()
            .map(|changes| encoded_bundle(changes))
            .collect::<Vec<_>>();

        bundles.extend(
            cut_into_bundles(self.kept_unread(false)?)
                .iter()
                .map(|changes| bundle_of_encoded(changes)),
        );
        bundles.extend(
            self.kept_unread(true)?
                .iter()
                .map(|change| bundle_of_encoded(&[&change.change])),
        );
        Ok(bundles)
    }

    pub(super) fn read_again_what_was_unread(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        if self.unread_read_again {
            return Ok(0);
        }

        let mut kept = self.kept_unread(false)?;

        kept.extend(self.kept_unread(true)?);

        let (read, digests): (Vec<_>, Vec<_>) = kept
            .into_iter()
            .filter_map(|kept| match read_change(&kept.change) {
                Ok(Some(change)) => Some((change, kept.digest)),
                Ok(None) | Err(_) => None,
            })
            .unzip();
        let changed = if read.is_empty() {
            0
        } else {
            self.arrived(catalog, &read)?.changed_here
        };
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;

        for digest in &digests {
            transaction
                .execute("DELETE FROM unread WHERE digest = ?1", params![digest])
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        self.unread_read_again = true;
        Ok(changed)
    }

    fn kept_unread(&self, refused: bool) -> Result<Vec<Kept>> {
        let mut statement = self
            .connection
            .prepare("SELECT digest, change FROM unread WHERE refused = ?1 ORDER BY rowid")
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        let rows = statement
            .query_map(params![refused], |row| {
                Ok(Kept {
                    digest: row.get(0)?,
                    change: row.get(1)?,
                })
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }
}

fn cut_into_bundles(kept: Vec<Kept>) -> Vec<Vec<Vec<u8>>> {
    let mut bundles = Vec::new();
    let mut bundle = Vec::new();
    let mut bytes = 0;

    for Kept { change, .. } in kept {
        if bytes + change.len() > BUNDLE_BYTES && !bundle.is_empty() {
            bundles.push(std::mem::take(&mut bundle));
            bytes = 0;
        }
        bytes += change.len();
        bundle.push(change);
    }
    if !bundle.is_empty() {
        bundles.push(bundle);
    }
    bundles
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use resonate_core::{Answer, Recording, SharedBandPeaks, SharedLyrics, WholeFile};
    use rusqlite::Connection;

    use super::*;
    use crate::{
        DeviceId, LocalListen, LocalPlaylist, LocalPlaylistId, Opened, Refused, RemoteListen,
        Resumed, SongTags,
        catalog::Held,
        change::{AnswerNote, Change, DeviceNote, Platform, Stamp},
        ledger::MIGRATIONS,
        wire::{
            ANSWER_LYRICS, ANSWER_RELEASE, ANSWER_STUDY, ReadBundle, decoded_bundle,
            encoded_change, read_bundle, read_bundle_knowing,
        },
    };

    const WRITER: DeviceId = DeviceId([7; 16]);
    const AN_INSTANT: i64 = 1_700_000_000_000_000_000;
    const A_RECORDING_TAG_CUT_SHORT: [u8; 3] = [2, 1, 0];

    struct Bare;

    impl Catalog for Bare {
        fn playlists(&self) -> std::result::Result<Vec<LocalPlaylist>, Refused> {
            Ok(Vec::new())
        }

        fn write_playlist(
            &self,
            _: Option<LocalPlaylistId>,
            _: &str,
            _: &[Recording],
        ) -> std::result::Result<LocalPlaylist, Refused> {
            Err(Refused::Unwritable)
        }

        fn remove_playlist(&self, _: LocalPlaylistId) -> std::result::Result<(), Refused> {
            Ok(())
        }

        fn favourites(&self) -> std::result::Result<Vec<Recording>, Refused> {
            Ok(Vec::new())
        }

        fn favour(&self, _: &Recording, _: bool) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn listens_after(
            &self,
            _: u64,
            _: usize,
        ) -> std::result::Result<Vec<LocalListen>, Refused> {
            Ok(Vec::new())
        }

        fn credit(&self, _: &RemoteListen) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn resumption(&self) -> std::result::Result<Option<Resumed>, Refused> {
            Ok(None)
        }

        fn holds(&self, _: &Recording) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn holdings(&self) -> std::result::Result<Vec<Held>, Refused> {
            Ok(Vec::new())
        }

        fn open(&self, _: &Recording) -> std::result::Result<Option<Opened>, Refused> {
            Ok(None)
        }

        fn land(
            &self,
            _: &Recording,
            _: &SongTags,
            _: &str,
            _: &Path,
        ) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn revision(&self) -> u64 {
            0
        }
    }

    fn echoes() -> Recording {
        Recording {
            mbid: None,
            title: "Echoes".into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: Some(Duration::from_secs(1_403)),
        }
    }

    fn stamp() -> Stamp {
        Stamp {
            nanos: AN_INSTANT,
            device: WRITER,
        }
    }

    fn answered(answer: Answer) -> Change {
        Change::Answer(AnswerNote {
            recording: echoes(),
            stamp: stamp(),
            answer,
        })
    }

    fn lyrics() -> Change {
        answered(Answer::Lyrics(SharedLyrics {
            text: "Overhead the albatross".into(),
            synced: false,
        }))
    }

    fn peaks() -> Change {
        answered(Answer::BandPeaks(SharedBandPeaks {
            file: WholeFile {
                packets: 9,
                frames: 61_871_400,
                rate: 44_100,
            },
            judged_under: 1,
            peaks: vec![3; 554],
        }))
    }

    fn writer_named() -> Change {
        Change::Device(DeviceNote {
            device: WRITER,
            name: "Desk".into(),
            platform: Platform::Linux,
            stamp: stamp(),
        })
    }

    fn took(ledger: &mut Ledger, read: &ReadBundle) {
        ledger.arrived(&Bare, &read.changes).expect("arrived");
        ledger.unread_arrived(&read.unread).expect("kept");
    }

    fn joined_from(parts: &[Vec<u8>]) -> Ledger {
        let mut joining = Ledger::in_memory().expect("a ledger");

        for part in parts {
            took(&mut joining, &read_bundle(part).expect("a part reads"));
        }
        joining
    }

    fn held(ledger: &Ledger) -> Vec<Change> {
        ledger
            .snapshot()
            .expect("a snapshot")
            .into_iter()
            .flatten()
            .collect()
    }

    #[test]
    fn an_answer_an_older_reader_cannot_read_reaches_a_device_joining_from_its_compaction() {
        let bundle = encoded_bundle(&[lyrics(), peaks()]);
        let mut older = Ledger::in_memory().expect("a ledger");

        let read = read_bundle_knowing(&bundle, &[ANSWER_LYRICS, ANSWER_STUDY, ANSWER_RELEASE])
            .expect("the bundle reads");

        took(&mut older, &read);

        let parts = older.snapshot_bundles().expect("the compaction");
        let joining = joined_from(&parts);

        assert_eq!(read.changes, [lyrics()]);
        assert_eq!(read.unread, [Unread::Later(encoded_change(&peaks()))]);
        assert!(!held(&older).contains(&peaks()));
        assert!(
            parts
                .iter()
                .all(|part| decoded_bundle(part).is_ok_and(|changes| !changes.is_empty()))
        );
        assert!(held(&joining).contains(&peaks()));
        assert!(held(&joining).contains(&lyrics()));
        assert_eq!(
            joining
                .snapshot_bundles()
                .expect("its own compaction")
                .len(),
            joining.snapshot().expect("its snapshot").len()
        );
    }

    #[test]
    fn a_change_that_does_not_read_travels_alone_and_its_siblings_survive_the_compaction() {
        let bundle = [
            encoded_change(&writer_named()),
            A_RECORDING_TAG_CUT_SHORT.to_vec(),
            encoded_change(&lyrics()),
        ];

        let mut reader = Ledger::in_memory().expect("a ledger");

        let read = read_bundle(&bundle_of_encoded(&bundle)).expect("the bundle reads");

        took(&mut reader, &read);

        let parts = reader.snapshot_bundles().expect("the compaction");
        let (alone, rest) = parts.split_last().expect("parts");
        let joining = joined_from(&parts);

        assert!(decoded_bundle(&bundle_of_encoded(&bundle)).is_err());
        assert_eq!(read.changes, [writer_named(), lyrics()]);
        assert_eq!(
            read.unread,
            [Unread::Refused(A_RECORDING_TAG_CUT_SHORT.to_vec())]
        );
        assert_eq!(
            read_bundle(alone).expect("the part reads"),
            ReadBundle {
                changes: Vec::new(),
                unread: vec![Unread::Refused(A_RECORDING_TAG_CUT_SHORT.to_vec())],
            }
        );
        assert!(rest.iter().all(|part| decoded_bundle(part).is_ok()));
        assert!(held(&joining).contains(&writer_named()));
        assert!(held(&joining).contains(&lyrics()));
        assert_eq!(
            joining.snapshot_bundles().expect("again").last(),
            Some(alone)
        );
    }

    #[test]
    fn a_change_kept_unread_is_taken_once_the_build_reads_it_and_kept_no_longer() {
        let mut upgraded = Ledger::in_memory().expect("a ledger");

        upgraded
            .unread_arrived(&[
                Unread::Later(encoded_change(&peaks())),
                Unread::Refused(A_RECORDING_TAG_CUT_SHORT.to_vec()),
            ])
            .expect("kept by the build before");
        upgraded.settle(&Bare).expect("settled");

        let parts = upgraded.snapshot_bundles().expect("the compaction");

        assert!(held(&upgraded).contains(&peaks()));
        assert_eq!(
            read_bundle(parts.last().expect("a part")).expect("the part reads"),
            ReadBundle {
                changes: Vec::new(),
                unread: vec![Unread::Refused(A_RECORDING_TAG_CUT_SHORT.to_vec())],
            }
        );
        assert_eq!(
            parts.len(),
            upgraded.snapshot().expect("a snapshot").len() + 1
        );
    }

    #[test]
    fn a_ledger_laid_before_unread_changes_were_kept_carries_what_it_held_and_keeps_them() {
        let connection = Connection::open_in_memory().expect("a connection");
        let before = MIGRATIONS
            .iter()
            .position(|step| *step == CHANGES_UNREAD_ARE_CARRIED_FORWARD)
            .expect("the step is in the chain");

        for step in &MIGRATIONS[..before] {
            connection
                .execute_batch(step)
                .expect("the ledger of before");
        }
        connection
            .execute(
                "INSERT INTO state (id, device) VALUES (1, ?1)",
                params![[3_u8; 16].as_slice()],
            )
            .expect("a state row");
        connection
            .execute(
                "INSERT INTO devices (device, name, platform, nanos, stamped_by) VALUES (?1, 'Desk', 0, ?2, ?1)",
                params![WRITER.0.as_slice(), stamp().nanos],
            )
            .expect("a device it knew");
        connection
            .pragma_update(
                None,
                "user_version",
                u32::try_from(before).expect("a count"),
            )
            .expect("stamped");

        let mut ledger = Ledger::over(connection).expect("carried forward");

        ledger
            .unread_arrived(&[Unread::Later(encoded_change(&peaks()))])
            .expect("kept");

        let parts = ledger.snapshot_bundles().expect("the compaction");
        let joining = joined_from(&parts);

        assert!(held(&joining).iter().any(|change| matches!(
            change,
            Change::Device(note) if note.device == WRITER && note.name == "Desk"
        )));
        assert!(held(&joining).contains(&peaks()));
    }
}
