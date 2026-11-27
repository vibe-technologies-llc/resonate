use std::{
    fs::{self, File},
    io::{self, BufReader, BufWriter, Seek as _, Write as _},
    path::PathBuf,
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::{
    seconds_since_the_epoch,
    writer::{self, Held, Writer},
};
use resonate_library::{
    BroughtIn, HeardElsewhere, HeardOnce, History, HistoryFrom, HistoryPages, HistoryRead,
    KeptListens, ListeningService, LookupOp, SAME_LISTEN_WITHIN, Scrobbler, StoreOp, TokenHeld,
    Unpinned, bring_in_history, bring_in_stream, history_lines,
};
use resonate_online::{Application, LastFm, LastfmHistory, ListenBrainz, ListenBrainzHistory};
use rusqlite::{Connection, OptionalExtension as _, Transaction, params};

use crate::{
    Error, MobilePlayer, Result,
    catalog::{Catalog, read},
    documents::owned,
    downloads::fresh_destination,
    error::HistoryFileOp,
    online_tools::{ScrobbleAccount, ScrobbleService, billed},
    sync::SongsHeld,
};

const SPOOL_FOLDER: &str = "history-spool";

static SPOOLED: AtomicU64 = AtomicU64::new(0);

const THE_HELD_AFTER: &str = "SELECT d.*, l.id AS listen_id, l.began AS listen_began,
        l.heard_ms AS listen_heard_ms, l.elsewhere AS listen_elsewhere
   FROM listens l JOIN documents d ON d.id = l.track
  WHERE l.id > ?1 AND l.counted = 1
  ORDER BY l.id
  LIMIT ?2";

const THE_SERVED_AFTER: &str = "SELECT id, began, heard_ms, elsewhere, served FROM listens
  WHERE id > ?1 AND counted = 1 AND track IS NULL AND served IS NOT NULL
  ORDER BY id
  LIMIT ?2";

const A_SNAPSHOT_BEGUN: &str = "BEGIN DEFERRED; SELECT count(*) FROM sqlite_master";

const THE_DOCUMENT_LENGTH: &str = "SELECT duration_ms FROM documents WHERE id = ?1";

const HELD_AT_THAT_MOMENT: &str = "SELECT EXISTS (
         SELECT 1 FROM listens
          WHERE track = ?1 AND began BETWEEN ?2 - ?3 AND ?2 + ?3
     )";

const CREDITED: &str = "INSERT OR IGNORE INTO listens
         (track, began, heard_ms, counted, elsewhere, origin_device, origin_id)
     VALUES (?1, ?2, ?3, 1, 1, ?4, ?2)";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct HistoryBroughtIn {
    pub credited: u64,
    pub held_already: u64,
    pub unmatched: u64,
}

impl From<BroughtIn> for HistoryBroughtIn {
    fn from(brought: BroughtIn) -> Self {
        Self {
            credited: brought.credited,
            held_already: brought.held_already,
            unmatched: brought.unmatched,
        }
    }
}

pub(crate) struct Spool {
    folder: PathBuf,
}

struct Spooled {
    path: PathBuf,
    held: Held,
}

impl Spool {
    pub(crate) fn under(folder: PathBuf) -> Self {
        Self {
            folder: folder.join(SPOOL_FOLDER),
        }
    }

    fn made_of(&self, source: &mut impl io::Read) -> io::Result<Spooled> {
        fs::create_dir_all(&self.folder)?;
        self.sweep();

        let stamp = process::id();
        let path = self.folder.join(format!(
            "{stamp}-{}",
            SPOOLED.fetch_add(1, Ordering::Relaxed)
        ));
        let spooled = Spooled {
            held: Held::made_closed(&path)?,
            path,
        };

        io::copy(source, &mut spooled.held.file())?;
        Ok(spooled)
    }

    fn sweep(&self) {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stamped) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.split_once('-'))
                .and_then(|(stamp, _)| stamp.parse::<u32>().ok())
            else {
                continue;
            };

            if writer::writer_of(&path, stamped) == Writer::Living {
                continue;
            }
            if let Err(error) = fs::remove_file(&path) {
                tracing::debug!(%error, path = %path.display(), "a spooled history file could not be swept");
            }
        }
    }
}

impl Drop for Spooled {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            tracing::debug!(%error, path = %self.path.display(), "a spooled history file could not be removed");
        }
    }
}

fn unread(source: io::Error) -> Error {
    Error::HistoryFile {
        op: HistoryFileOp::Read,
        source,
    }
}

fn imported(kept: &dyn KeptListens, spool: &Spool, mut file: File) -> Result<HistoryBroughtIn> {
    let brought = match file.stream_position() {
        Ok(_) => bring_in_stream(kept, BufReader::new(file), &mut |_| {}),
        Err(_) => {
            let spooled = spool.made_of(&mut file).map_err(unread)?;

            bring_in_stream(kept, BufReader::new(spooled.held.file()), &mut |_| {})
        }
    };

    brought.map(Into::into).map_err(|error| match error {
        resonate_library::Error::Unread { source } => unread(source),
        error => Error::history(error),
    })
}

#[uniffi::export]
impl MobilePlayer {
    pub fn export_history(&self, descriptor: i32) -> Result<u64> {
        let failed = |source| Error::HistoryFile {
            op: HistoryFileOp::Write,
            source,
        };
        let mut written = BufWriter::new(fresh_destination(descriptor)?);

        let mut listens = 0_u64;

        for line in history_lines(&PhoneListens(&self.catalog)) {
            writeln!(written, "{}", line.map_err(Error::history)?).map_err(failed)?;
            listens += 1;
        }
        written.flush().map_err(failed)?;

        Ok(listens)
    }

    pub fn import_history_file(&self, descriptor: i32) -> Result<HistoryBroughtIn> {
        let file = owned(descriptor).map_err(unread)?;

        imported(&PhoneListens(&self.catalog), &self.history_spool, file)
    }

    pub fn import_history(
        &self,
        service: ScrobbleService,
        user: String,
        account: ScrobbleAccount,
    ) -> Result<HistoryBroughtIn> {
        self.require_online()?;

        let history = self.history_of(service, user.trim(), account)?;

        self.brought_in_from(history.as_ref())
    }
}

impl MobilePlayer {
    fn brought_in_from(&self, history: &dyn History) -> Result<HistoryBroughtIn> {
        bring_in_history(&PhoneListens(&self.catalog), history, &mut |_| {})
            .map(Into::into)
            .map_err(Error::history)
    }

    fn history_of(
        &self,
        service: ScrobbleService,
        user: &str,
        account: ScrobbleAccount,
    ) -> Result<Box<dyn History>> {
        let client = Arc::clone(self.services.online.client());
        let named = (!user.is_empty()).then(|| user.to_owned());
        match service {
            ScrobbleService::ListenBrainz => {
                let user = match named {
                    Some(user) => user,
                    None if !account.listenbrainz_token.is_empty() => signed_in(
                        &ListenBrainz::new(Arc::clone(&client), account.listenbrainz_token),
                        service,
                    )?,
                    None => return Err(Error::NoHistoryUser { service }),
                };
                Ok(Box::new(ListenBrainzHistory::new(client, user)))
            }
            ScrobbleService::LastFm => {
                if account.lastfm_key.is_empty() || account.lastfm_secret.is_empty() {
                    return Err(Error::NoLastfmApplication);
                }

                let application = Application {
                    key: account.lastfm_key,
                    secret: account.lastfm_secret,
                };
                let user = match named {
                    Some(user) => user,
                    None if !account.lastfm_session.is_empty() => signed_in(
                        &LastFm::new(
                            Arc::clone(&client),
                            application.clone(),
                            account.lastfm_session,
                        ),
                        service,
                    )?,
                    None => return Err(Error::NoHistoryUser { service }),
                };
                Ok(Box::new(LastfmHistory::new(client, application, user)))
            }
        }
    }
}

fn signed_in(scrobbler: &dyn Scrobbler, service: ScrobbleService) -> Result<String> {
    match scrobbler
        .token_held()
        .map_err(|source| Error::reference(LookupOp::Token, source))?
    {
        TokenHeld::By(user) => Ok(user),
        TokenHeld::Unknown => Err(Error::NoHistoryUser { service }),
    }
}

struct PhoneListens<'a>(&'a Catalog);

impl KeptListens for PhoneListens<'_> {
    fn history_from(
        &self,
        from: HistoryFrom,
        most: usize,
    ) -> resonate_library::Result<HistoryRead> {
        history_page(&self.0.0.lock(), from, most)
    }

    fn history_pages(&self) -> resonate_library::Result<Box<dyn HistoryPages + '_>> {
        let store = |op| move |source| resonate_library::Error::store(op, source);

        let Some(reader) = self.0.reader_of_its_own().map_err(store(StoreOp::Open))? else {
            return Ok(Box::new(Unpinned(self)));
        };

        reader
            .execute_batch(A_SNAPSHOT_BEGUN)
            .map_err(store(StoreOp::Transaction))?;
        Ok(Box::new(PinnedListens(reader)))
    }

    fn bring_in(
        &self,
        service: ListeningService,
        heard: &[HeardElsewhere],
    ) -> resonate_library::Result<BroughtIn> {
        let mut connection = self.0.0.lock();
        let store = |op| move |source| resonate_library::Error::store(op, source);

        let transaction = connection.transaction().map_err(store(StoreOp::Update))?;
        let held = SongsHeld::read(&transaction).map_err(store(StoreOp::Query))?;

        let mut brought = BroughtIn::default();

        for listen in heard {
            match credited(&transaction, &held, service, listen).map_err(store(StoreOp::Insert))? {
                Landed::Credited => brought.credited += 1,
                Landed::HeldAlready => brought.held_already += 1,
                Landed::Unmatched => brought.unmatched += 1,
            }
        }
        transaction.commit().map_err(store(StoreOp::Update))?;
        Ok(brought)
    }
}

struct PinnedListens(Connection);

impl HistoryPages for PinnedListens {
    fn page(&mut self, from: HistoryFrom, most: usize) -> resonate_library::Result<HistoryRead> {
        history_page(&self.0, from, most)
    }
}

fn history_page(
    connection: &Connection,
    from: HistoryFrom,
    most: usize,
) -> resonate_library::Result<HistoryRead> {
    let most = i64::try_from(most.max(1)).unwrap_or(i64::MAX);

    let rows = match from {
        HistoryFrom::Held(after) => held_after(connection, after, most),
        HistoryFrom::Unheld(after) => served_after(connection, after, most),
    }
    .map_err(|source| resonate_library::Error::store(StoreOp::Query, source))?;

    let full = rows.len() == usize::try_from(most).unwrap_or(usize::MAX);
    let last = rows.last().map(|(id, _)| (*id).max(0).cast_unsigned());
    let next = match (from, last, full) {
        (HistoryFrom::Held(_), Some(last), true) => Some(HistoryFrom::Held(last)),
        (HistoryFrom::Held(_), _, false) => Some(HistoryFrom::Unheld(0)),
        (HistoryFrom::Unheld(_), Some(last), true) => Some(HistoryFrom::Unheld(last)),
        (HistoryFrom::Held(_) | HistoryFrom::Unheld(_), _, _) => None,
    };
    Ok(HistoryRead {
        heard: rows.into_iter().map(|(_, heard)| heard).collect(),
        next,
    })
}

fn held_after(
    connection: &Connection,
    after: u64,
    most: i64,
) -> rusqlite::Result<Vec<(i64, HeardOnce)>> {
    connection
        .prepare_cached(THE_HELD_AFTER)?
        .query_map(params![after.cast_signed(), most], |row| {
            let track = read(row)?;
            Ok((
                row.get::<_, i64>("listen_id")?,
                HeardOnce {
                    began: moment(row.get("listen_began")?),
                    heard: heard_for(row.get("listen_heard_ms")?),
                    elsewhere: row.get("listen_elsewhere")?,
                    billed: Some(billed(&track)),
                    location: track.uri,
                },
            ))
        })?
        .collect()
}

fn served_after(
    connection: &Connection,
    after: u64,
    most: i64,
) -> rusqlite::Result<Vec<(i64, HeardOnce)>> {
    connection
        .prepare_cached(THE_SERVED_AFTER)?
        .query_map(params![after.cast_signed(), most], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                HeardOnce {
                    began: moment(row.get(1)?),
                    heard: heard_for(row.get(2)?),
                    elsewhere: row.get(3)?,
                    billed: None,
                    location: row.get(4)?,
                },
            ))
        })?
        .collect()
}

fn moment(seconds: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds.max(0).cast_unsigned())
}

fn heard_for(milliseconds: i64) -> Duration {
    Duration::from_millis(milliseconds.max(0).cast_unsigned())
}

enum Landed {
    Credited,
    HeldAlready,
    Unmatched,
}

fn credited(
    transaction: &Transaction<'_>,
    held: &SongsHeld,
    service: ListeningService,
    listen: &HeardElsewhere,
) -> rusqlite::Result<Landed> {
    let Some(track) = held.resolved(transaction, &listen.recording)? else {
        return Ok(Landed::Unmatched);
    };
    let track = track.cast_signed();
    let at = seconds_since_the_epoch(listen.at);
    let within = SAME_LISTEN_WITHIN.as_secs().cast_signed();

    if transaction.query_row(HELD_AT_THAT_MOMENT, params![track, at, within], |row| {
        row.get::<_, bool>(0)
    })? {
        return Ok(Landed::HeldAlready);
    }

    let heard_ms = match listen.recording.length {
        Some(length) => i64::try_from(length.as_millis()).unwrap_or(i64::MAX),
        None => transaction
            .query_row(THE_DOCUMENT_LENGTH, [track], |row| row.get::<_, i64>(0))
            .optional()?
            .unwrap_or(0),
    };
    let inserted = transaction.execute(
        CREDITED,
        params![track, at, heard_ms, service.name().as_bytes()],
    )?;
    Ok(if inserted == 0 {
        Landed::HeldAlready
    } else {
        Landed::Credited
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        env, fs, io,
        os::fd::IntoRawFd as _,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    use parking_lot::Mutex;
    use resonate_core::Recording;
    use resonate_library::{HeardPage, LISTENS_READ_AT_ONCE, Mbid};

    use super::*;
    use crate::{Track, playlists::tests::player};

    const SONGS: [(&str, &str, &str); 2] = [
        ("Slow Water", "Example Band", "Low Tide"),
        ("Say", "Low", "Words"),
    ];

    const MOMENTS: [i64; 3] = [1_700_000_000, 1_700_100_000, 1_700_200_000];

    static SCRATCH: AtomicU64 = AtomicU64::new(0);

    fn scratch(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "resonate-mobile-history-{}-{}-{name}",
            std::process::id(),
            SCRATCH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    pub(crate) fn songs_held(player: &MobilePlayer) -> [i64; 2] {
        SONGS.map(|(title, artist, album)| {
            player
                .catalog
                .upsert(Track {
                    uri: format!("content://test/{title}.flac"),
                    name: format!("{title}.flac"),
                    title: title.into(),
                    artist: artist.into(),
                    album: album.into(),
                    duration_ms: 240_000,
                    ..Track::default()
                })
                .expect("a song")
                .id
                .cast_signed()
        })
    }

    pub(crate) fn listened(player: &MobilePlayer, sql: &str, values: impl rusqlite::Params) {
        player
            .catalog
            .0
            .lock()
            .execute(sql, values)
            .expect("a listen");
    }

    pub(crate) fn heard_here(player: &MobilePlayer, track: i64, began: i64, heard_ms: i64) {
        listened(
            player,
            "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, ?2, ?3, 1)",
            params![track, began, heard_ms],
        );
    }

    pub(crate) fn passed_over(player: &MobilePlayer, began: i64) {
        listened(
            player,
            "INSERT INTO listens (track, began, heard_ms, counted) VALUES (NULL, ?1, 5000, 0)",
            [began],
        );
    }

    pub(crate) fn listens(player: &MobilePlayer) -> Vec<(Option<i64>, i64, i64, bool, bool)> {
        player
            .catalog
            .0
            .lock()
            .prepare(
                "SELECT track, began, heard_ms, counted, elsewhere FROM listens ORDER BY began, id",
            )
            .and_then(|mut query| {
                query
                    .query_map([], |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    })?
                    .collect()
            })
            .expect("the listens")
    }

    fn written_to(path: &PathBuf) -> i32 {
        fs::File::create(path).expect("a document").into_raw_fd()
    }

    fn read_from(path: &PathBuf) -> i32 {
        fs::File::open(path).expect("a document").into_raw_fd()
    }

    #[test]
    fn a_history_exported_from_one_phone_lands_on_another_once_and_a_pass_stays_behind() {
        let from = player();
        let to = player();
        let [slow_water, say] = songs_held(&from);
        let [held_slow_water, held_say] = songs_held(&to);

        heard_here(&from, slow_water, MOMENTS[0], 200_000);
        heard_here(&from, say, MOMENTS[1], 100_000);
        heard_here(&from, slow_water, MOMENTS[2], 30_000);
        passed_over(&from, MOMENTS[2] + 60);
        listened(
            &from,
            "INSERT INTO listens (track, began, heard_ms, counted, served) VALUES (NULL, ?1, 90000, 1, 'subsonic://server/song/7')",
            [MOMENTS[2] + 600],
        );

        let file = scratch("history.jsonl");

        let written = from.export_history(written_to(&file)).expect("exported");
        let first = to.import_history_file(read_from(&file)).expect("imported");
        let again = to
            .import_history_file(read_from(&file))
            .expect("imported again");
        let text = fs::read_to_string(&file).expect("the file");

        fs::remove_file(&file).expect("cleaned");

        assert_eq!(written, 4);
        assert_eq!(text.lines().count(), 4);
        assert!(text.contains("\"origin_url\":\"subsonic://server/song/7\""));
        assert!(text.contains("\"origin_url\":\"content://test/Slow Water.flac\""));
        assert_eq!(
            first,
            HistoryBroughtIn {
                credited: 3,
                held_already: 0,
                unmatched: 0,
            }
        );
        assert_eq!(
            again,
            HistoryBroughtIn {
                credited: 0,
                held_already: 3,
                unmatched: 0,
            }
        );
        assert_eq!(
            listens(&to),
            [
                (Some(held_slow_water), MOMENTS[0], 240_000, true, true),
                (Some(held_say), MOMENTS[1], 240_000, true, true),
                (Some(held_slow_water), MOMENTS[2], 240_000, true, true),
            ]
        );
    }

    fn every_listen_read_by_pages_of_one(
        pages: &mut dyn HistoryPages,
        between: &mut dyn FnMut(),
    ) -> Vec<(SystemTime, String)> {
        let mut read = Vec::new();
        let mut from = Some(HistoryFrom::START);

        while let Some(at) = from {
            let page = pages.page(at, 1).expect("a page reads");

            read.extend(
                page.heard
                    .into_iter()
                    .map(|heard| (heard.began, heard.location)),
            );
            from = page.next;
            between();
        }
        read.sort();
        read
    }

    #[test]
    fn an_export_reads_one_snapshot_whatever_is_counted_or_forgotten_between_its_pages_and_leaves_the_catalog_free()
     {
        let folder = scratch("snapshot");

        fs::create_dir_all(&folder).expect("a folder");

        let phone = MobilePlayer {
            catalog: Arc::new(Catalog::open(&folder.join("catalog.sqlite")).expect("a catalog")),
            ..player()
        };
        let [slow_water, say] = songs_held(&phone);

        heard_here(&phone, slow_water, MOMENTS[0], 200_000);
        heard_here(&phone, say, MOMENTS[1], 100_000);
        heard_here(&phone, slow_water, MOMENTS[2], 30_000);
        listened(
            &phone,
            "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, ?2, 5000, 0)",
            params![say, MOMENTS[2] + 60],
        );
        listened(
            &phone,
            "INSERT INTO listens (track, began, heard_ms, counted, served) VALUES (NULL, ?1, 90000, 1, 'subsonic://server/song/7')",
            [MOMENTS[2] + 600],
        );

        let kept = PhoneListens(&phone.catalog);
        let slow_water_at = "content://test/Slow Water.flac".to_owned();
        let at_its_start = vec![
            (moment(MOMENTS[0]), slow_water_at.clone()),
            (moment(MOMENTS[1]), "content://test/Say.flac".to_owned()),
            (moment(MOMENTS[2]), slow_water_at),
            (
                moment(MOMENTS[2] + 600),
                "subsonic://server/song/7".to_owned(),
            ),
        ];

        let mut free_between = Vec::new();
        let mut moved = false;

        let read = every_listen_read_by_pages_of_one(
            &mut *kept.history_pages().expect("a snapshot begins"),
            &mut || {
                free_between.push(phone.catalog.0.try_lock().is_some());
                if !moved {
                    listened(&phone, "DELETE FROM listens WHERE began = ?1", [MOMENTS[2]]);
                    listened(
                        &phone,
                        "UPDATE listens SET counted = 1 WHERE counted = 0",
                        [],
                    );
                    listened(
                        &phone,
                        "INSERT INTO listens (track, began, heard_ms, counted, served) VALUES (NULL, ?1, 90000, 1, 'subsonic://server/song/8')",
                        [MOMENTS[2] + 900],
                    );
                    moved = true;
                }
            },
        );
        let after = every_listen_read_by_pages_of_one(
            &mut *kept.history_pages().expect("a snapshot begins"),
            &mut || {},
        );

        drop(phone);
        fs::remove_dir_all(&folder).expect("cleaned");

        assert_eq!(read, at_its_start);
        assert!(free_between.iter().all(|free| *free));
        assert_eq!(free_between.len(), 6);
        assert_eq!(after.len(), 5);
        assert!(!after.contains(&at_its_start[2]));
    }

    fn listens_of(count: u64) -> String {
        (0..count)
            .map(|index| {
                format!(
                    "{{\"listened_at\":{},\"track_metadata\":{{\"artist_name\":\"Low\",\"track_name\":\"Say\"}}}}\n",
                    MOMENTS[0] + i64::try_from(index).expect("an index") * 10
                )
            })
            .collect()
    }

    #[test]
    fn a_file_of_more_than_a_page_of_listens_is_read_from_its_descriptor_a_page_at_a_time() {
        const LISTENS: u64 = LISTENS_READ_AT_ONCE as u64 * 2 + 500;

        let to = player();

        songs_held(&to);

        let file = scratch("pages.jsonl");

        fs::write(&file, listens_of(LISTENS)).expect("a file");

        let brought = to.import_history_file(read_from(&file)).expect("imported");

        fs::remove_file(&file).expect("cleaned");

        assert_eq!(
            brought,
            HistoryBroughtIn {
                credited: LISTENS,
                held_already: 0,
                unmatched: 0,
            }
        );
        assert_eq!(
            listens(&to).len(),
            usize::try_from(LISTENS).expect("a count")
        );
    }

    struct Watching<'a> {
        inside: PhoneListens<'a>,
        spool: PathBuf,
        pages: Mutex<Vec<usize>>,
        spooled: Mutex<Vec<usize>>,
    }

    impl KeptListens for Watching<'_> {
        fn history_from(
            &self,
            from: HistoryFrom,
            most: usize,
        ) -> resonate_library::Result<HistoryRead> {
            self.inside.history_from(from, most)
        }

        fn bring_in(
            &self,
            service: ListeningService,
            heard: &[HeardElsewhere],
        ) -> resonate_library::Result<BroughtIn> {
            self.pages.lock().push(heard.len());
            self.spooled.lock().push(
                fs::read_dir(self.spool.join(SPOOL_FOLDER))
                    .map(|entries| entries.count())
                    .unwrap_or_default(),
            );
            self.inside.bring_in(service, heard)
        }
    }

    fn pipe_holding(text: String) -> (File, thread::JoinHandle<()>) {
        let (reader, mut writer) = io::pipe().expect("a pipe");

        let writing = thread::spawn(move || {
            writer.write_all(text.as_bytes()).expect("written");
        });

        (File::from(std::os::fd::OwnedFd::from(reader)), writing)
    }

    #[test]
    fn a_descriptor_that_cannot_be_rewound_is_spooled_and_brings_in_the_same() {
        let to = player();

        songs_held(&to);

        let (reader, writing) = pipe_holding(listens_of(3));

        let brought = to
            .import_history_file(reader.into_raw_fd())
            .expect("imported");

        writing.join().expect("the writer finished");

        assert_eq!(
            brought,
            HistoryBroughtIn {
                credited: 3,
                held_already: 0,
                unmatched: 0,
            }
        );
    }

    #[test]
    fn a_pipe_of_more_than_a_page_of_listens_is_brought_in_a_page_at_a_time_from_a_held_spool() {
        const LISTENS: u64 = LISTENS_READ_AT_ONCE as u64 * 2 + 500;

        let to = player();

        songs_held(&to);

        let folder = scratch("spool");
        let spool = Spool::under(folder.clone());
        let kept = Watching {
            inside: PhoneListens(&to.catalog),
            spool: folder.clone(),
            pages: Mutex::new(Vec::new()),
            spooled: Mutex::new(Vec::new()),
        };
        let (reader, writing) = pipe_holding(listens_of(LISTENS));

        let brought = imported(&kept, &spool, reader).expect("imported");

        writing.join().expect("the writer finished");

        assert_eq!(brought.credited, LISTENS);
        assert_eq!(
            *kept.pages.lock(),
            [LISTENS_READ_AT_ONCE, LISTENS_READ_AT_ONCE, 500]
        );
        assert_eq!(*kept.spooled.lock(), [1, 1, 1]);
        assert_eq!(
            fs::read_dir(folder.join(SPOOL_FOLDER))
                .expect("the spool folder")
                .count(),
            0
        );

        fs::remove_dir_all(&folder).expect("cleaned");
    }

    #[test]
    fn a_pipe_with_a_bad_line_brings_nothing_in_and_leaves_no_spool_behind() {
        let to = player();

        songs_held(&to);

        let folder = scratch("refused-spool");
        let spool = Spool::under(folder.clone());
        let (reader, writing) = pipe_holding(format!("{}not a listen\n", listens_of(2)));

        let refused = imported(&PhoneListens(&to.catalog), &spool, reader);

        writing.join().expect("the writer finished");

        assert!(matches!(refused, Err(Error::History { .. })));
        assert!(listens(&to).is_empty());
        assert_eq!(
            fs::read_dir(folder.join(SPOOL_FOLDER))
                .expect("the spool folder")
                .count(),
            0
        );

        fs::remove_dir_all(&folder).expect("cleaned");
    }

    #[test]
    fn a_spool_left_by_a_run_that_is_gone_is_swept_and_one_held_by_a_living_writer_is_not() {
        let folder = scratch("swept-spool");
        let spool = Spool::under(folder.clone());
        let left = folder.join(SPOOL_FOLDER).join(format!("{}-0", u32::MAX));

        fs::create_dir_all(folder.join(SPOOL_FOLDER)).expect("a folder");
        fs::write(&left, b"left behind").expect("a stale spool");

        let living = spool.made_of(&mut io::empty()).expect("a spool");

        let swept = spool.made_of(&mut io::empty()).expect("another spool");

        assert!(!left.exists());
        assert!(living.path.exists());

        drop((living, swept));
        fs::remove_dir_all(&folder).expect("cleaned");
    }

    #[test]
    fn a_file_with_a_line_that_is_no_listen_brings_nothing_in_and_names_the_line() {
        let to = player();

        songs_held(&to);

        let file = scratch("refused.jsonl");

        fs::write(
            &file,
            "{\"listened_at\":1700000000,\"track_metadata\":{\"artist_name\":\"Low\",\"track_name\":\"Say\"}}\n\nnot a listen\n",
        )
        .expect("a file");

        let refused = to.import_history_file(read_from(&file));

        fs::remove_file(&file).expect("cleaned");

        assert!(matches!(
            refused,
            Err(Error::History { ref source })
                if matches!(**source, resonate_library::Error::ListenLine { line: 3, .. })
        ));
        assert_eq!(
            refused.expect_err("refused").to_string(),
            "Line 3 of the file is not a listen as ListenBrainz writes one: it is not JSON"
        );
        assert!(listens(&to).is_empty());
    }

    #[test]
    fn a_descriptor_that_is_not_there_is_refused_for_what_it_was_opened_for() {
        let phone = player();

        assert!(matches!(
            phone.import_history_file(crate::documents::document_not_found()),
            Err(Error::HistoryFile {
                op: HistoryFileOp::Read,
                ..
            })
        ));
        assert!(matches!(
            phone.export_history(-1),
            Err(Error::InvalidDescriptor)
        ));
    }

    struct LastfmServing {
        pages: Vec<HeardPage>,
        asked: Mutex<Vec<Option<u64>>>,
    }

    impl History for LastfmServing {
        fn service(&self) -> ListeningService {
            ListeningService::LastFm
        }

        fn page(&self, from: Option<u64>) -> resonate_library::Result<HeardPage> {
            self.asked.lock().push(from);

            let at = usize::try_from(from.unwrap_or(1)).expect("a page") - 1;
            Ok(self.pages[at].clone())
        }
    }

    fn scrobbled(title: &str, artist: &str, second: i64, mbid: Option<&str>) -> HeardElsewhere {
        HeardElsewhere {
            at: UNIX_EPOCH + Duration::from_secs(second.cast_unsigned()),
            recording: Recording {
                mbid: mbid.map(|mbid| Mbid::new(mbid).expect("an mbid")),
                title: title.to_owned(),
                artist: artist.to_owned(),
                album: String::new(),
                length: None,
            },
        }
    }

    #[test]
    fn lastfm_scrobbles_land_on_the_songs_held_once_and_one_scrobbled_from_here_is_not_counted_again()
     {
        let phone = player();
        let [slow_water, say] = songs_held(&phone);

        heard_here(&phone, say, MOMENTS[1] + 1, 100_000);

        let lastfm = LastfmServing {
            pages: vec![
                HeardPage {
                    heard: vec![
                        scrobbled("Slow Water", "Example Band", MOMENTS[0], None),
                        scrobbled("Say", "Low", MOMENTS[1], None),
                    ],
                    next: Some(2),
                },
                HeardPage {
                    heard: vec![scrobbled("Unheld", "Nobody", MOMENTS[2], None)],
                    next: None,
                },
            ],
            asked: Mutex::new(Vec::new()),
        };

        let first = phone.brought_in_from(&lastfm).expect("brought in");
        let again = phone.brought_in_from(&lastfm).expect("brought in again");
        let origin: (Vec<u8>, i64) = phone
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT origin_device, origin_id FROM listens WHERE elsewhere = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the listen brought in");

        assert_eq!(
            first,
            HistoryBroughtIn {
                credited: 1,
                held_already: 1,
                unmatched: 1,
            }
        );
        assert_eq!(
            again,
            HistoryBroughtIn {
                credited: 0,
                held_already: 2,
                unmatched: 1,
            }
        );
        assert_eq!(*lastfm.asked.lock(), [None, Some(2), None, Some(2)]);
        assert_eq!(origin, (b"lastfm".to_vec(), MOMENTS[0]));
        assert_eq!(
            listens(&phone),
            [
                (Some(slow_water), MOMENTS[0], 240_000, true, true),
                (Some(say), MOMENTS[1] + 1, 100_000, true, false),
            ]
        );
    }

    #[test]
    fn a_service_import_asks_for_online_a_user_and_for_lastfm_an_application() {
        let phone = player();
        let none = ScrobbleAccount {
            listenbrainz_token: String::new(),
            lastfm_key: String::new(),
            lastfm_secret: String::new(),
            lastfm_session: String::new(),
        };

        let offline = phone.import_history(
            ScrobbleService::ListenBrainz,
            "someone".into(),
            none.clone(),
        );

        phone.configure_online(true);

        let nobody = phone.import_history(ScrobbleService::ListenBrainz, "  ".into(), none.clone());
        let no_application = phone.import_history(ScrobbleService::LastFm, "someone".into(), none);

        assert!(matches!(offline, Err(Error::OnlineOff)));
        assert!(matches!(
            nobody,
            Err(Error::NoHistoryUser {
                service: ScrobbleService::ListenBrainz
            })
        ));
        assert!(matches!(no_application, Err(Error::NoLastfmApplication)));
    }
}
