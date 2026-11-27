use resonate_core::RecordingKey;
use rusqlite::{Connection, functions::FunctionFlags};

use crate::{Error, Result, StoreOp};

const REFOLDED_KEY: &str = "refolded_key";

pub(super) const RECORDINGS_ARE_KEYED_BY_THE_WORDS_OF_THEIR_NAMES: &str = "
CREATE TEMP TABLE refolded AS
    SELECT rowid AS id, key AS was, refolded_key(key) AS key, nanos, stamped_by, here
      FROM favourites WHERE key LIKE 'named:%';
UPDATE favourites SET here = 1 WHERE here = 0 AND rowid IN (
    SELECT kept.id FROM temp.refolded kept
      JOIN temp.refolded other ON other.key = kept.key AND other.here = 1);
DELETE FROM favourites WHERE rowid IN (
    SELECT older.id FROM temp.refolded older
      JOIN temp.refolded newer ON newer.key = older.key
       AND (newer.nanos, newer.stamped_by, newer.id) > (older.nanos, older.stamped_by, older.id));
UPDATE favourites SET key = '#' || rowid
 WHERE rowid IN (SELECT id FROM temp.refolded WHERE key <> was);
UPDATE favourites SET key = (SELECT r.key FROM temp.refolded r WHERE r.id = favourites.rowid)
 WHERE rowid IN (SELECT id FROM temp.refolded WHERE key <> was);
DROP TABLE temp.refolded;

CREATE TEMP TABLE refolded AS
    SELECT rowid AS id, key AS was, refolded_key(key) AS key, kind, file, nanos, stamped_by
      FROM answers WHERE key LIKE 'named:%';
DELETE FROM answers WHERE rowid IN (
    SELECT older.id FROM temp.refolded older
      JOIN temp.refolded newer
        ON newer.key = older.key AND newer.kind = older.kind AND newer.file = older.file
       AND (newer.nanos, newer.stamped_by, newer.id) > (older.nanos, older.stamped_by, older.id));
UPDATE answers SET key = '#' || rowid
 WHERE rowid IN (SELECT id FROM temp.refolded WHERE key <> was);
UPDATE answers SET key = (SELECT r.key FROM temp.refolded r WHERE r.id = answers.rowid)
 WHERE rowid IN (SELECT id FROM temp.refolded WHERE key <> was);
DROP TABLE temp.refolded;
UPDATE answers SET named = refolded_key(named) WHERE named LIKE 'named:%';

CREATE TEMP TABLE refolded AS
    SELECT rowid AS id, key AS was, refolded_key(key) AS key, nanos, stamped_by
      FROM deletions WHERE key LIKE 'named:%';
DELETE FROM deletions WHERE rowid IN (
    SELECT older.id FROM temp.refolded older
      JOIN temp.refolded newer ON newer.key = older.key
       AND (newer.nanos, newer.stamped_by, newer.id) > (older.nanos, older.stamped_by, older.id));
UPDATE deletions SET key = '#' || rowid
 WHERE rowid IN (SELECT id FROM temp.refolded WHERE key <> was);
UPDATE deletions SET key = (SELECT r.key FROM temp.refolded r WHERE r.id = deletions.rowid)
 WHERE rowid IN (SELECT id FROM temp.refolded WHERE key <> was);
DROP TABLE temp.refolded;
UPDATE deletions SET named = refolded_key(named) WHERE named LIKE 'named:%';

CREATE TEMP TABLE refolded AS
    SELECT rowid AS id, key AS was, refolded_key(key) AS key, refused
      FROM refusals WHERE key LIKE 'named:%';
DELETE FROM refusals WHERE rowid IN (
    SELECT older.id FROM temp.refolded older
      JOIN temp.refolded newer ON newer.key = older.key
       AND (newer.refused, newer.id) > (older.refused, older.id));
UPDATE refusals SET key = '#' || rowid
 WHERE rowid IN (SELECT id FROM temp.refolded WHERE key <> was);
UPDATE refusals SET key = (SELECT r.key FROM temp.refolded r WHERE r.id = refusals.rowid)
 WHERE rowid IN (SELECT id FROM temp.refolded WHERE key <> was);
DROP TABLE temp.refolded;
";

pub(super) fn refolding_on(connection: &Connection) -> Result<()> {
    connection
        .create_scalar_function(
            REFOLDED_KEY,
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let kept: Option<String> = context.get(0)?;
                Ok(kept.map(|kept| RecordingKey::refolded(&kept).as_str().to_owned()))
            },
        )
        .map_err(|source| Error::store(StoreOp::Open, source))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use resonate_core::{Mbid, Recording, folded_letters, title_words};
    use rusqlite::{Connection, params};

    use super::*;
    use crate::{
        DeviceId, LocalListen, LocalPlaylist, LocalPlaylistId, Opened, Refused, RemoteListen,
        Resumed, SongTags,
        catalog::{Catalog, Held},
        change::{Change, DeletionNote, FavouriteNote, Stamp},
        ledger::{Ledger, MIGRATIONS},
        wire::encoded_recording,
    };

    const PEER: DeviceId = DeviceId([2; 16]);
    const SLOW_WATER: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const AN_INSTANT: i64 = 1_700_000_000_000_000_000;

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

    fn song(artist: &str, album: &str) -> Recording {
        Recording {
            mbid: None,
            title: "Paradise City".into(),
            artist: artist.into(),
            album: album.into(),
            length: Some(Duration::from_secs(406)),
        }
    }

    fn quoted() -> Recording {
        song("Guns N' Roses", "Appetite for Destruction")
    }

    fn plain() -> Recording {
        song("Guns N Roses", "Appetite For Destruction!")
    }

    fn identified() -> Recording {
        Recording {
            mbid: Some(Mbid::new(SLOW_WATER).expect("an mbid")),
            ..quoted()
        }
    }

    fn kept_under_the_earlier_fold(recording: &Recording) -> String {
        format!(
            "named:{}\u{1f}{}\u{1f}{}",
            folded_letters(recording.artist.trim()),
            title_words(&recording.title),
            folded_letters(recording.album.trim())
        )
    }

    fn laid_before_the_refold() -> Connection {
        let connection = Connection::open_in_memory().expect("a connection");
        let before = MIGRATIONS
            .iter()
            .position(|step| *step == RECORDINGS_ARE_KEYED_BY_THE_WORDS_OF_THEIR_NAMES)
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
            .pragma_update(
                None,
                "user_version",
                u32::try_from(before).expect("a count"),
            )
            .expect("stamped");
        connection
    }

    fn favourite(
        connection: &Connection,
        recording: &Recording,
        favoured: bool,
        here: bool,
        nanos: i64,
    ) {
        connection
            .execute(
                "INSERT INTO favourites (key, recording, favoured, here, nanos, stamped_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    kept_under_the_earlier_fold(recording),
                    encoded_recording(recording),
                    favoured,
                    here,
                    nanos,
                    PEER.0.as_slice()
                ],
            )
            .expect("a favourite kept the old way");
    }

    fn deletion(
        connection: &Connection,
        key: &str,
        recording: &Recording,
        deleted: bool,
        nanos: i64,
    ) {
        connection
            .execute(
                "INSERT INTO deletions (key, named, recording, deleted, nanos, stamped_by, heard)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5)",
                params![
                    key,
                    kept_under_the_earlier_fold(recording),
                    encoded_recording(recording),
                    deleted,
                    nanos,
                    PEER.0.as_slice()
                ],
            )
            .expect("a deletion kept the old way");
    }

    fn answer(
        connection: &Connection,
        key: &str,
        recording: &Recording,
        answer: &[u8],
        nanos: i64,
    ) {
        connection
            .execute(
                "INSERT INTO answers (key, kind, file, recording, answer, nanos, stamped_by, named)
                 VALUES (?1, 0, X'', ?2, ?3, ?4, ?5, ?6)",
                params![
                    key,
                    encoded_recording(recording),
                    answer,
                    nanos,
                    PEER.0.as_slice(),
                    kept_under_the_earlier_fold(recording)
                ],
            )
            .expect("an answer kept the old way");
    }

    fn rows(ledger: &Ledger, query: &str) -> Vec<(String, i64)> {
        ledger
            .connection
            .prepare(query)
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("the rows read back")
    }

    #[test]
    fn keys_kept_under_the_earlier_fold_are_refolded_and_one_song_keeps_its_newest_word() {
        let connection = laid_before_the_refold();

        favourite(&connection, &quoted(), true, true, 10);
        favourite(&connection, &plain(), false, false, 20);
        answer(
            &connection,
            &kept_under_the_earlier_fold(&quoted()),
            &quoted(),
            b"newer",
            40,
        );
        answer(
            &connection,
            &kept_under_the_earlier_fold(&plain()),
            &plain(),
            b"older",
            30,
        );
        answer(
            &connection,
            &identified().key().to_string(),
            &identified(),
            b"identified",
            50,
        );
        deletion(
            &connection,
            &kept_under_the_earlier_fold(&quoted()),
            &quoted(),
            true,
            60,
        );
        deletion(
            &connection,
            &kept_under_the_earlier_fold(&plain()),
            &plain(),
            false,
            70,
        );
        deletion(
            &connection,
            &identified().key().to_string(),
            &identified(),
            true,
            80,
        );
        connection
            .execute(
                "INSERT INTO refusals (key, refused) VALUES (?1, 5), (?2, 9)",
                params![
                    kept_under_the_earlier_fold(&quoted()),
                    kept_under_the_earlier_fold(&plain())
                ],
            )
            .expect("refusals kept the old way");

        let ledger = Ledger::over(connection).expect("carried forward");

        let named = quoted().named_key().to_string();
        let favourites: Vec<(String, i64, bool, bool)> = ledger
            .connection
            .prepare("SELECT key, nanos, favoured, here FROM favourites")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("the favourites read back");
        let answers = rows(
            &ledger,
            "SELECT key || '/' || named, nanos FROM answers ORDER BY nanos",
        );
        let deletions = rows(
            &ledger,
            "SELECT key || '/' || named, nanos FROM deletions ORDER BY nanos",
        );
        let refusals = rows(&ledger, "SELECT key, refused FROM refusals");

        assert_eq!(named, plain().named_key().to_string());
        assert_ne!(named, kept_under_the_earlier_fold(&quoted()));
        assert_eq!(favourites, [(named.clone(), 20, false, true)]);
        assert_eq!(
            answers,
            [
                (format!("{named}/{named}"), 40),
                (format!("{}/{named}", identified().key()), 50)
            ]
        );
        assert_eq!(
            deletions,
            [
                (format!("{named}/{named}"), 70),
                (format!("{}/{named}", identified().key()), 80)
            ]
        );
        assert_eq!(refusals, [(named, 9)]);
    }

    #[test]
    fn a_word_a_peer_tells_under_another_spelling_meets_the_row_kept_under_the_earlier_fold() {
        let connection = laid_before_the_refold();

        favourite(&connection, &quoted(), true, false, AN_INSTANT + 20);
        deletion(
            &connection,
            &kept_under_the_earlier_fold(&quoted()),
            &quoted(),
            true,
            AN_INSTANT + 20,
        );

        let mut ledger = Ledger::over(connection).expect("carried forward");

        let older = Stamp {
            nanos: AN_INSTANT + 10,
            device: PEER,
        };

        ledger
            .arrived(
                &Bare,
                &[
                    Change::Favourite(FavouriteNote {
                        recording: plain(),
                        favoured: false,
                        stamp: older,
                    }),
                    Change::Deletion(DeletionNote {
                        recording: plain(),
                        deleted: false,
                        stamp: older,
                    }),
                ],
            )
            .expect("arrived");

        let favourites = rows(&ledger, "SELECT key, favoured FROM favourites");
        let deletions = rows(&ledger, "SELECT key, deleted FROM deletions");

        assert_eq!(favourites, [(plain().named_key().to_string(), 1)]);
        assert_eq!(deletions, [(plain().named_key().to_string(), 1)]);
        assert!(ledger.deleted_everywhere(&plain()).expect("weighed"));
        assert!(ledger.deleted_everywhere(&quoted()).expect("weighed"));
    }
}
