use std::collections::BTreeSet;

use resonate_core::Mbid;
use resonate_library::{
    LOVES_TOLD_AT_ONCE, LookupOp, Love, Loved, LovedNames, Scrobbler, refused_for_good,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, catalog::Catalog, online_tools::ScrobbleAccount,
};

const THE_LOVED_RECORDINGS: &str =
    "SELECT DISTINCT recording FROM documents WHERE favourite = 1 AND recording IS NOT NULL";

const THE_LOVES_TOLD: &str = "SELECT recording FROM loves_told WHERE service = ?1";

const A_LOVE_TOLD: &str =
    "INSERT INTO loves_told (service, recording) VALUES (?1, ?2) ON CONFLICT DO NOTHING";

const A_LOVE_TAKEN_BACK: &str = "DELETE FROM loves_told WHERE service = ?1 AND recording = ?2";

const THE_NAMES_OF_A_RECORDING: &str = "SELECT title, artist FROM documents
      WHERE recording = ?1 AND artist != ''
      ORDER BY favourite DESC, id
      LIMIT 1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct FavouritesTold {
    pub loved: u32,
    pub taken_back: u32,
    pub refused: u32,
}

impl FavouritesTold {
    const fn and(self, other: Self) -> Self {
        Self {
            loved: self.loved + other.loved,
            taken_back: self.taken_back + other.taken_back,
            refused: self.refused + other.refused,
        }
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn tell_loves(&self, account: ScrobbleAccount) -> Result<FavouritesTold> {
        self.require_online()?;
        let mut told = FavouritesTold::default();
        let mut failure = None;
        for client in self.scrobblers(account) {
            match self.catalog.tell_loves(client.as_ref()) {
                Ok(said) => told = told.and(said),
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(told), Err)
    }
}

impl Catalog {
    pub(crate) fn tell_loves(&self, client: &dyn Scrobbler) -> Result<FavouritesTold> {
        let service = client.service().name();
        let owed = self.loves_owed(service)?;

        let mut said = FavouritesTold::default();
        for (loved, love) in &owed {
            match client.love(loved, *love) {
                Ok(()) => match love {
                    Love::Loved => said.loved += 1,
                    Love::TakenBack => said.taken_back += 1,
                },
                Err(error) if refused_for_good(&error) => {
                    tracing::warn!(
                        recording = %loved.recording,
                        ?love,
                        %error,
                        "a love was refused outright and is not told again"
                    );
                    said.refused += 1;
                }
                Err(error) => return Err(Error::reference(LookupOp::Love, error)),
            }
            self.note_told(service, &loved.recording, *love)?;
        }
        Ok(said)
    }

    fn loves_owed(&self, service: &str) -> Result<Vec<(Loved, Love)>> {
        let connection = self.0.lock();
        let loved = recordings(&connection, THE_LOVED_RECORDINGS, &[])?;
        let told = recordings(&connection, THE_LOVES_TOLD, &[service])?;
        loved
            .difference(&told)
            .map(|recording| (recording, Love::Loved))
            .chain(
                told.difference(&loved)
                    .map(|recording| (recording, Love::TakenBack)),
            )
            .take(LOVES_TOLD_AT_ONCE)
            .map(|(recording, love)| {
                Ok((
                    Loved {
                        recording: recording.clone(),
                        named: names_of(&connection, recording)?,
                    },
                    love,
                ))
            })
            .collect()
    }

    fn note_told(&self, service: &str, recording: &Mbid, love: Love) -> Result<()> {
        let sql = match love {
            Love::Loved => A_LOVE_TOLD,
            Love::TakenBack => A_LOVE_TAKEN_BACK,
        };
        self.0
            .lock()
            .execute(sql, params![service, recording.as_str()])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Write, source))
    }
}

fn recordings(connection: &Connection, sql: &str, binds: &[&str]) -> Result<BTreeSet<Mbid>> {
    let mut query = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Read, source))?;
    let held = query
        .query_map(rusqlite::params_from_iter(binds), |row| {
            row.get::<_, String>(0)
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Read, source))?;
    Ok(held
        .iter()
        .filter_map(|text| Mbid::new(text).ok())
        .collect())
}

fn names_of(connection: &Connection, recording: &Mbid) -> Result<Option<LovedNames>> {
    connection
        .query_row(THE_NAMES_OF_A_RECORDING, [recording.as_str()], |row| {
            Ok(LovedNames {
                title: row.get(0)?,
                artist: row.get(1)?,
            })
        })
        .optional()
        .map_err(|source| Error::store(StoreOp::Read, source))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use parking_lot::Mutex;
    use resonate_library::{Billed, ListeningService, Scrobble, TokenHeld};

    use super::*;
    use crate::Track;

    const ONE: &str = "b1a9c0e9-d987-4042-ae91-78d6a3267d69";
    const TWO: &str = "5b11f4ce-a62d-471e-81fc-a69a8278c7da";
    const REFUSED_OUTRIGHT: u16 = 404;
    const REFUSED_FOR_NOW: u16 = 429;

    #[derive(Default)]
    struct Told {
        said: Mutex<Vec<(String, Option<String>, Love)>>,
        refusing: Option<u16>,
    }

    impl Scrobbler for Told {
        fn service(&self) -> ListeningService {
            ListeningService::ListenBrainz
        }

        fn submit(&self, _listens: &[Scrobble]) -> resonate_library::Result<()> {
            Ok(())
        }

        fn playing_now(&self, _playing: &Billed) -> resonate_library::Result<()> {
            Ok(())
        }

        fn love(&self, loved: &Loved, love: Love) -> resonate_library::Result<()> {
            if let Some(status) = self.refusing {
                return Err(resonate_library::Error::Refused {
                    op: LookupOp::Love,
                    status,
                });
            }
            self.said.lock().push((
                loved.recording.to_string(),
                loved.named.as_ref().map(|named| named.title.clone()),
                love,
            ));
            Ok(())
        }

        fn token_held(&self) -> resonate_library::Result<TokenHeld> {
            Ok(TokenHeld::Unknown)
        }
    }

    fn song(catalog: &Catalog, name: &str, recording: &str, favourite: bool) -> u64 {
        let track = catalog
            .upsert(Track {
                uri: format!("content://test/{name}.flac"),
                name: format!("{name}.flac"),
                title: name.to_owned(),
                artist: "Artist".into(),
                duration_ms: 30000,
                ..Track::default()
            })
            .expect("a song");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET recording = ?1 WHERE id = ?2",
                params![recording, track.id as i64],
            )
            .expect("a recording");
        catalog.favourite(track.id, favourite).expect("a favourite");
        track.id
    }

    #[test]
    fn a_favourite_is_loved_once_and_taken_back_when_it_no_longer_is() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let told = Told::default();

        let one = song(&catalog, "One", ONE, true);
        song(&catalog, "Two", TWO, false);

        let first = catalog.tell_loves(&told).expect("loves told");
        let again = catalog.tell_loves(&told).expect("nothing owed");
        catalog
            .favourite(one, false)
            .expect("a favourite taken back");
        let back = catalog.tell_loves(&told).expect("a love taken back");

        assert_eq!(first.loved, 1);
        assert_eq!(again, FavouritesTold::default());
        assert_eq!(back.taken_back, 1);
        assert_eq!(
            *told.said.lock(),
            vec![
                (ONE.to_owned(), Some("One".to_owned()), Love::Loved),
                (ONE.to_owned(), Some("One".to_owned()), Love::TakenBack),
            ]
        );
    }

    #[test]
    fn a_love_refused_outright_is_not_asked_again_and_one_refused_for_now_is() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let outright = Told {
            refusing: Some(REFUSED_OUTRIGHT),
            ..Told::default()
        };
        let busy = Told {
            refusing: Some(REFUSED_FOR_NOW),
            ..Told::default()
        };

        song(&catalog, "One", ONE, true);

        let refused = catalog.tell_loves(&busy);
        let still_owed = catalog.loves_owed("listenbrainz").expect("the loves owed");
        let given_up = catalog.tell_loves(&outright).expect("a refusal noted");
        let none_owed = catalog.loves_owed("listenbrainz").expect("the loves owed");

        assert!(refused.is_err());
        assert_eq!(still_owed.len(), 1);
        assert_eq!(given_up.refused, 1);
        assert!(none_owed.is_empty());
    }
}
