use std::{
    env, fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    process::{self, Command},
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_core::Recording;
use resonate_server::{Allowance, Limits, Server, Store};
use resonate_sync::{
    LocalPlaylistId, Refused, Server as Reached,
    shared::{LocalShared, Shared, SharedCatalog, SharedSettings, SharedShown, SharedStanding},
};
use tokio::net::{TcpListener, TcpStream};

const SETTLES_WITHIN: Duration = Duration::from_secs(30);

fn song(title: &str) -> Recording {
    Recording {
        mbid: None,
        title: title.to_owned(),
        artist: "Ada".to_owned(),
        album: "Album".to_owned(),
        length: Some(Duration::from_secs(200)),
    }
}

#[derive(Default)]
struct Held {
    next: i64,
    lists: Vec<Row>,
    holds_nothing_named: Vec<String>,
}

struct Row {
    at: LocalPlaylistId,
    name: String,
    entries: Vec<Option<Recording>>,
    flagged: bool,
    editable: bool,
}

#[derive(Default)]
struct Shelf {
    held: Mutex<Held>,
    revision: AtomicU64,
}

impl Shelf {
    fn lacking(titles: &[&str]) -> Self {
        let shelf = Self::default();
        shelf.held.lock().holds_nothing_named =
            titles.iter().map(|title| (*title).to_owned()).collect();

        shelf
    }

    fn made(&self, name: &str, titles: &[&str]) -> LocalPlaylistId {
        let mut held = self.held.lock();
        held.next += 1;
        let at = LocalPlaylistId {
            id: held.next,
            created: held.next,
        };
        held.lists.push(Row {
            at,
            name: name.to_owned(),
            entries: titles.iter().map(|title| Some(song(title))).collect(),
            flagged: false,
            editable: true,
        });
        drop(held);
        self.revision.fetch_add(1, Ordering::Relaxed);

        at
    }

    fn edit(&self, at: LocalPlaylistId, titles: &[&str]) {
        let mut held = self.held.lock();
        if let Some(row) = held.lists.iter_mut().find(|row| row.at == at) {
            row.entries = titles.iter().map(|title| Some(song(title))).collect();
        }
        drop(held);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    fn renamed(&self, at: LocalPlaylistId, name: &str) {
        let mut held = self.held.lock();
        if let Some(row) = held.lists.iter_mut().find(|row| row.at == at) {
            row.name = name.to_owned();
        }
        drop(held);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    fn removed(&self, at: LocalPlaylistId) {
        self.held.lock().lists.retain(|row| row.at != at);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    fn titles_of(&self, at: LocalPlaylistId) -> Option<Vec<String>> {
        self.held
            .lock()
            .lists
            .iter()
            .find(|row| row.at == at)
            .map(|row| {
                row.entries
                    .iter()
                    .flatten()
                    .map(|recording| recording.title.clone())
                    .collect()
            })
    }

    fn named(&self, at: LocalPlaylistId) -> Option<String> {
        self.held
            .lock()
            .lists
            .iter()
            .find(|row| row.at == at)
            .map(|row| row.name.clone())
    }

    fn is_flagged(&self, at: LocalPlaylistId) -> bool {
        self.held
            .lock()
            .lists
            .iter()
            .any(|row| row.at == at && row.flagged)
    }
}

impl SharedCatalog for Shelf {
    fn shared_read(&self, at: LocalPlaylistId) -> Result<Option<LocalShared>, Refused> {
        Ok(self
            .held
            .lock()
            .lists
            .iter()
            .find(|row| row.at == at)
            .map(|row| LocalShared {
                at,
                name: row.name.clone(),
                entries: row.entries.clone(),
            }))
    }

    fn shared_write(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
        editable: bool,
    ) -> Result<LocalShared, Refused> {
        let mut held = self.held.lock();
        let kept: Vec<Option<Recording>> = entries
            .iter()
            .filter(|recording| !held.holds_nothing_named.contains(&recording.title))
            .cloned()
            .map(Some)
            .collect();
        let at = at.unwrap_or_else(|| {
            held.next += 1;
            let at = LocalPlaylistId {
                id: held.next,
                created: held.next,
            };
            held.lists.push(Row {
                at,
                name: name.to_owned(),
                entries: Vec::new(),
                flagged: true,
                editable,
            });

            at
        });
        let row = held
            .lists
            .iter_mut()
            .find(|row| row.at == at)
            .ok_or(Refused::NotHere)?;
        row.name = name.to_owned();
        row.entries.clone_from(&kept);
        row.flagged = true;
        row.editable = editable;
        drop(held);
        self.revision.fetch_add(1, Ordering::Relaxed);

        Ok(LocalShared {
            at,
            name: name.to_owned(),
            entries: kept,
        })
    }

    fn shared_flag(
        &self,
        at: LocalPlaylistId,
        editable: bool,
    ) -> Result<Option<LocalShared>, Refused> {
        let mut held = self.held.lock();
        let Some(row) = held.lists.iter_mut().find(|row| row.at == at) else {
            return Ok(None);
        };
        row.flagged = true;
        row.editable = editable;

        Ok(Some(LocalShared {
            at,
            name: row.name.clone(),
            entries: row.entries.clone(),
        }))
    }

    fn shared_unflag(&self, at: LocalPlaylistId) -> Result<(), Refused> {
        if let Some(row) = self.held.lock().lists.iter_mut().find(|row| row.at == at) {
            row.flagged = false;
        }

        Ok(())
    }

    fn shared_held_as(&self, _recording: &Recording) -> Result<Option<Recording>, Refused> {
        Ok(None)
    }

    fn shared_revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
}

struct Hosted {
    address: SocketAddr,
    folder: PathBuf,
    _runtime: tokio::runtime::Runtime,
}

fn hosted(name: &str) -> Hosted {
    let folder = env::temp_dir().join(format!(
        "resonate-shared-{name}-{}-{}",
        process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    fs::create_dir_all(&folder).expect("a scratch folder");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let listener = runtime
        .block_on(TcpListener::bind(SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            0,
        ))))
        .expect("listening");
    let address = listener.local_addr().expect("an address");
    let limits = Limits {
        allowance: Allowance {
            attempts_per_minute: u32::MAX,
            ..Allowance::default()
        },
        ..Limits::default()
    };
    let server = Server::new(Store::in_memory().expect("a store"), limits);
    runtime.spawn(server.serve_web(listener, None));

    Hosted {
        address,
        folder,
        _runtime: runtime,
    }
}

struct Person {
    shelf: Arc<Shelf>,
    shared: Shared,
}

impl Hosted {
    fn person(&self, name: &str, shelf: Shelf) -> Person {
        let shelf = Arc::new(shelf);
        let shared = Shared::start(
            SharedSettings {
                server: Reached::At(format!("ws://{}/", self.address)),
                trusted: None,
                store: self.folder.join(format!("{name}.sqlite")),
                name: name.to_owned(),
            },
            Arc::clone(&shelf) as Arc<dyn SharedCatalog>,
        )
        .expect("started");

        Person { shelf, shared }
    }
}

struct Doorman {
    address: SocketAddr,
    _runtime: tokio::runtime::Runtime,
}

impl Doorman {
    fn closing_then_passing_to(server: SocketAddr, clean_closes: u32) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let listener = runtime
            .block_on(TcpListener::bind(SocketAddr::from((
                Ipv4Addr::LOCALHOST,
                0,
            ))))
            .expect("listening");
        let address = listener.local_addr().expect("an address");
        let closing = Arc::new(AtomicU32::new(clean_closes));
        runtime.spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let closes_cleanly = closing
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                        left.checked_sub(1)
                    })
                    .is_ok();
                if closes_cleanly {
                    tokio::spawn(async move {
                        if let Ok(mut socket) = tokio_tungstenite::accept_async(client).await {
                            let _ = socket.close(None).await;
                        }
                    });
                } else {
                    tokio::spawn(async move {
                        let mut client = client;
                        if let Ok(mut upstream) = TcpStream::connect(server).await {
                            let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                        }
                    });
                }
            }
        });

        Self {
            address,
            _runtime: runtime,
        }
    }
}

impl Hosted {
    fn person_through(&self, name: &str, door: &Doorman) -> Person {
        let shelf = Arc::new(Shelf::default());
        let shared = Shared::start(
            SharedSettings {
                server: Reached::At(format!("ws://{}/", door.address)),
                trusted: None,
                store: self.folder.join(format!("{name}.sqlite")),
                name: name.to_owned(),
            },
            Arc::clone(&shelf) as Arc<dyn SharedCatalog>,
        )
        .expect("started");

        Person { shelf, shared }
    }
}

fn standing_after_forcing(person: &Person, until: impl Fn(SharedStanding) -> bool) -> bool {
    let deadline = Instant::now() + SETTLES_WITHIN;
    let mut ever_too_old = false;
    while Instant::now() < deadline {
        person.shared.force();
        let standing = shown_on(person).first().map(|shown| shown.standing);
        ever_too_old |= standing == Some(SharedStanding::ServerTooOld);
        if standing.is_some_and(&until) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    ever_too_old
}

#[test]
fn one_clean_close_is_a_server_lost_and_not_a_server_too_old() {
    let hosted = hosted("closes");
    let door = Doorman::closing_then_passing_to(hosted.address, 2);
    let ada = hosted.person_through("Ada", &door);
    let list = ada.shelf.made("Mix", &["One"]);
    ada.shared.share(list).expect("shared");

    let ever_too_old = standing_after_forcing(&ada, |standing| standing == SharedStanding::Reached);

    assert!(!ever_too_old);
    assert_eq!(shown_on(&ada)[0].standing, SharedStanding::Reached);
}

#[test]
fn a_server_that_closes_every_connection_reads_as_too_old() {
    let hosted = hosted("closes-always");
    let door = Doorman::closing_then_passing_to(hosted.address, u32::MAX);
    let ada = hosted.person_through("Ada", &door);
    let list = ada.shelf.made("Mix", &["One"]);
    ada.shared.share(list).expect("shared");

    let ever_too_old =
        standing_after_forcing(&ada, |standing| standing == SharedStanding::ServerTooOld);

    assert!(ever_too_old);
}

fn eventually(what: &str, person: &Person, done: impl Fn() -> bool) {
    let deadline = Instant::now() + SETTLES_WITHIN;
    while !done() {
        person.shared.force();
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(100));
    }
}

fn shown_on(person: &Person) -> Vec<SharedShown> {
    person.shared.shown()
}

fn local_of(person: &Person) -> LocalPlaylistId {
    shown_on(person)
        .first()
        .and_then(|shown| shown.at)
        .expect("a list held here")
}

fn code_in(link: &str) -> String {
    link.rsplit('/')
        .next()
        .and_then(|code| code.split('?').next())
        .expect("a code")
        .to_owned()
}

fn two_sharing(
    hosted: &Hosted,
    titles: &[&str],
) -> (Person, LocalPlaylistId, Person, LocalPlaylistId) {
    let ada = hosted.person("Ada", Shelf::default());
    let list = ada.shelf.made("Mix", titles);
    let invites = ada.shared.share(list).expect("shared");
    assert!(
        invites.view.contains("server="),
        "a list on a server of its own names that server in its link"
    );
    assert!(
        !invites
            .view_page
            .as_deref()
            .expect("a page")
            .contains("resonate.tecnio.dev"),
        "a list on a server of its own is not sent to the hosted page"
    );

    let ben = hosted.person("Ben", Shelf::default());
    ben.shared
        .join(&code_in(&invites.edit.expect("an edit link")), None)
        .expect("joined");
    eventually("Ben's copy arriving", &ben, || {
        shown_on(&ben).first().and_then(|shown| shown.at).is_some()
    });
    let theirs = local_of(&ben);

    (ada, list, ben, theirs)
}

#[test]
fn someone_given_the_edit_link_holds_the_list_and_both_are_named_on_it() {
    let hosted = hosted("joined");
    let (ada, list, ben, theirs) = two_sharing(&hosted, &["One", "Two"]);

    eventually("the songs arriving", &ben, || {
        ben.shelf.titles_of(theirs) == Some(vec!["One".to_owned(), "Two".to_owned()])
    });
    assert_eq!(ben.shelf.named(theirs).as_deref(), Some("Mix"));
    assert!(ben.shelf.is_flagged(theirs));
    assert!(ada.shelf.is_flagged(list));

    eventually("Ada seeing Ben", &ada, || {
        shown_on(&ada)[0]
            .people
            .iter()
            .any(|person| person.name == "Ben" && !person.me)
    });
}

#[test]
fn a_person_renamed_after_joining_is_renamed_on_the_list_they_joined() {
    let hosted = hosted("named-after-joining");
    let (ada, _list, ben, _theirs) = two_sharing(&hosted, &["One"]);
    eventually("Ada seeing Ben", &ada, || {
        shown_on(&ada)[0]
            .people
            .iter()
            .any(|person| person.name == "Ben" && !person.me)
    });

    ben.shared.named("Benedict".to_owned());

    eventually("Ada seeing Benedict", &ada, || {
        let people = shown_on(&ada)[0].people.clone();
        people
            .iter()
            .any(|person| person.name == "Benedict" && !person.me)
            && !people.iter().any(|person| person.name == "Ben")
    });
}

#[test]
fn a_name_told_again_is_not_written_again_to_a_list_that_already_carries_it() {
    let hosted = hosted("named-twice");
    let (ada, _list, ben, _theirs) = two_sharing(&hosted, &["One"]);
    eventually("Ada seeing Ben", &ada, || {
        shown_on(&ada)[0]
            .people
            .iter()
            .any(|person| person.name == "Ben" && !person.me)
    });
    eventually("Ben's list settling", &ben, || shown_on(&ben)[0].owed == 0);

    ben.shared.named("Ben".to_owned());
    thread::sleep(Duration::from_millis(500));

    assert_eq!(shown_on(&ben)[0].owed, 0);
}

#[test]
fn two_people_editing_at_once_keep_both_edits() {
    let hosted = hosted("merged");
    let (ada, list, ben, theirs) = two_sharing(&hosted, &["One", "Two"]);
    eventually("the songs arriving", &ben, || {
        ben.shelf
            .titles_of(theirs)
            .is_some_and(|titles| titles.len() == 2)
    });

    ada.shelf.edit(list, &["One", "Two", "Ada's"]);
    ben.shelf.edit(theirs, &["Ben's", "One", "Two"]);

    let both = vec![
        "Ben's".to_owned(),
        "One".to_owned(),
        "Two".to_owned(),
        "Ada's".to_owned(),
    ];
    eventually("Ada holding both edits", &ada, || {
        ada.shelf.titles_of(list).as_ref() == Some(&both)
    });
    eventually("Ben holding both edits", &ben, || {
        ben.shelf.titles_of(theirs).as_ref() == Some(&both)
    });
}

#[test]
fn a_rename_reaches_the_other_person_and_a_song_removed_goes_from_both() {
    let hosted = hosted("renamed");
    let (ada, list, ben, theirs) = two_sharing(&hosted, &["One", "Two"]);
    eventually("the songs arriving", &ben, || {
        ben.shelf
            .titles_of(theirs)
            .is_some_and(|titles| titles.len() == 2)
    });

    ada.shelf.renamed(list, "Summer");
    ben.shelf.edit(theirs, &["Two"]);

    eventually("the new name arriving", &ben, || {
        ben.shelf.named(theirs).as_deref() == Some("Summer")
    });
    eventually("the removal arriving", &ada, || {
        ada.shelf.titles_of(list) == Some(vec!["Two".to_owned()])
    });
}

#[test]
fn someone_lacking_a_song_keeps_it_on_the_list_for_everyone_else_and_is_told_of_it() {
    let hosted = hosted("unheld");
    let ada = hosted.person("Ada", Shelf::default());
    let list = ada.shelf.made("Mix", &["One", "Rare", "Two"]);
    let invites = ada.shared.share(list).expect("shared");
    let ben = hosted.person("Ben", Shelf::lacking(&["Rare"]));
    ben.shared
        .join(&code_in(&invites.edit.expect("an edit link")), None)
        .expect("joined");

    eventually("Ben's copy arriving", &ben, || {
        shown_on(&ben).first().and_then(|shown| shown.at).is_some()
    });
    let theirs = local_of(&ben);
    eventually("what Ben holds arriving", &ben, || {
        ben.shelf.titles_of(theirs) == Some(vec!["One".to_owned(), "Two".to_owned()])
    });
    assert_eq!(shown_on(&ben)[0].unheld[0].title, "Rare");

    ben.shelf.edit(theirs, &["One", "Ben's", "Two"]);

    eventually("Ben's edit reaching Ada with Rare kept", &ada, || {
        ada.shelf.titles_of(list)
            == Some(vec![
                "One".to_owned(),
                "Ben's".to_owned(),
                "Rare".to_owned(),
                "Two".to_owned(),
            ])
    });
}

#[test]
fn a_view_link_follows_the_list_and_whatever_is_done_to_the_copy_it_comes_back_as_it_was() {
    let hosted = hosted("viewed");
    let ada = hosted.person("Ada", Shelf::default());
    let list = ada.shelf.made("Mix", &["One"]);
    let invites = ada.shared.share(list).expect("shared");
    eventually("the list reaching the server", &ada, || {
        shown_on(&ada)[0].owed == 0
    });

    let viewer = hosted.person("Viewer", Shelf::default());
    viewer
        .shared
        .join(&code_in(&invites.view), None)
        .expect("joined");
    eventually("the copy arriving", &viewer, || {
        shown_on(&viewer)
            .first()
            .and_then(|shown| shown.at)
            .is_some()
    });
    let copy = local_of(&viewer);
    eventually("its songs arriving", &viewer, || {
        viewer.shelf.titles_of(copy) == Some(vec!["One".to_owned()])
    });
    assert!(!shown_on(&viewer)[0].may_edit);

    viewer.shelf.edit(copy, &["Vandalised"]);
    eventually("the copy put right", &viewer, || {
        viewer.shelf.titles_of(copy) == Some(vec!["One".to_owned()])
    });

    ada.shelf.edit(list, &["One", "Two"]);
    viewer.shared.watch(Some(shown_on(&viewer)[0].space));
    eventually("the edit reaching the viewer", &viewer, || {
        viewer.shelf.titles_of(copy) == Some(vec!["One".to_owned(), "Two".to_owned()])
    });
    assert_eq!(
        ada.shelf.titles_of(list),
        Some(vec!["One".to_owned(), "Two".to_owned()]),
        "the viewer's copy reached the list"
    );
}

#[test]
fn removing_someone_moves_everyone_else_and_leaves_them_only_the_copy_they_held() {
    let hosted = hosted("removed");
    let (ada, list, ben, theirs) = two_sharing(&hosted, &["One"]);
    let cal = hosted.person("Cal", Shelf::default());
    let edit = ada.shared.shown()[0].invites.edit.clone().expect("a link");
    cal.shared.join(&code_in(&edit), None).expect("joined");
    eventually("Cal's copy arriving", &cal, || {
        shown_on(&cal).first().and_then(|shown| shown.at).is_some()
    });
    let cals = local_of(&cal);
    eventually("Ada seeing them both", &ada, || {
        shown_on(&ada)[0].people.len() == 3
    });

    let space = shown_on(&ada)[0].space;
    let removed = shown_on(&ada)[0]
        .people
        .iter()
        .find(|person| person.name == "Ben")
        .map(|person| person.member);
    let fresh = ada.shared.reset(space, removed).expect("reset");
    assert_ne!(fresh.edit.as_deref(), Some(edit.as_str()));

    eventually("Ben finding himself out", &ben, || {
        shown_on(&ben).is_empty()
    });
    assert!(
        !ben.shelf.is_flagged(theirs),
        "Ben's copy stays his own list"
    );
    assert!(ben.shelf.titles_of(theirs).is_some());

    eventually("Cal moving to the new list", &cal, || {
        shown_on(&cal)
            .first()
            .is_some_and(|shown| shown.space != space)
    });

    ada.shelf.edit(list, &["One", "After"]);
    eventually("an edit after the reset reaching Cal", &cal, || {
        cal.shelf.titles_of(cals) == Some(vec!["One".to_owned(), "After".to_owned()])
    });
    assert_eq!(
        ben.shelf.titles_of(theirs),
        Some(vec!["One".to_owned()]),
        "someone removed heard an edit made after"
    );
}

#[test]
fn a_list_left_or_deleted_here_leaves_the_others_their_copy() {
    let hosted = hosted("left");
    let (ada, list, ben, theirs) = two_sharing(&hosted, &["One"]);
    eventually("Ada seeing Ben", &ada, || {
        shown_on(&ada)[0].people.len() == 2
    });

    ben.shelf.removed(theirs);

    eventually("Ben gone from Ada's list", &ada, || {
        shown_on(&ada)[0].people.len() == 1
    });
    eventually("Ben's store letting go", &ben, || shown_on(&ben).is_empty());
    assert!(ada.shelf.titles_of(list).is_some());
}

#[test]
fn a_list_shared_with_no_server_reachable_waits_and_arrives_once_one_is() {
    let folder = env::temp_dir().join(format!(
        "resonate-shared-offline-{}-{}",
        process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    fs::create_dir_all(&folder).expect("a scratch folder");
    let shelf = Arc::new(Shelf::default());
    let list = shelf.made("Mix", &["One"]);
    let shared = Shared::start(
        SharedSettings {
            server: Reached::At("ws://127.0.0.1:9/".to_owned()),
            trusted: None,
            store: folder.join("alone.sqlite"),
            name: String::new(),
        },
        Arc::clone(&shelf) as Arc<dyn SharedCatalog>,
    )
    .expect("started");

    let invites = shared.share(list).expect("sharing needs no connection");
    assert!(invites.edit.is_some());
    let person = Person { shelf, shared };
    eventually("being told the server is away", &person, || {
        shown_on(&person)[0].standing == SharedStanding::Unreached
    });
    assert!(shown_on(&person)[0].owed > 0);
}

#[test]
fn what_is_held_is_shown_the_moment_the_service_starts() {
    let hosted = hosted("started");
    let ada = hosted.person("Ada", Shelf::default());
    let list = ada.shelf.made("Mix", &["One"]);
    ada.shared.share(list).expect("shared");

    drop(ada);
    let again = hosted.person("Ada", Shelf::default());

    assert_eq!(
        again.shared.shown().len(),
        1,
        "the terminal reads this straight after starting, with no wait to lean on"
    );
}

fn the_page_of(hosted: &Hosted) -> String {
    let mut asking = std::net::TcpStream::connect(hosted.address).expect("the server answers");
    asking
        .write_all(b"GET /s/ HTTP/1.1\r\nHost: resonate.test\r\n\r\n")
        .expect("asked");
    let mut answer = String::new();
    asking.read_to_string(&mut answer).expect("answered");

    answer
}

fn the_reader_on(page: &str) -> &str {
    let opening = "<script id=\"reading\">";
    let from = page.find(opening).expect("the page reads the list") + opening.len();
    let to = from + page[from..].find("</script>").expect("the reader ends");

    &page[from..to]
}

fn read_in_a_browser(hosted: &Hosted, reader: &str, code: &str) -> Option<String> {
    let script = hosted.folder.join("reading.js");
    fs::write(
        &script,
        format!(
            "{reader}\nreadShared(process.argv[2], process.argv[3]).then((read) => {{ \
             console.log(JSON.stringify(read)); process.exit(0); }});\n"
        ),
    )
    .expect("the script written");
    let ran = Command::new("node")
        .arg(&script)
        .arg(code)
        .arg(format!("ws://{}/", hosted.address))
        .output()
        .ok()?;
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );

    Some(String::from_utf8_lossy(&ran.stdout).trim().to_owned())
}

fn a_row(title: &str) -> String {
    format!("{{\"title\":\"{title}\",\"artist\":\"Ada\",\"album\":\"Album\",\"ms\":200000}}")
}

#[test]
fn a_browser_given_either_link_opens_the_list_as_it_stands_and_a_link_nobody_shared_is_not_held() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: no node to stand in for a browser");
        return;
    }
    let hosted = hosted("browsed");
    let ada = hosted.person("Ada", Shelf::default());
    let list = ada.shelf.made("Mix", &["One", "Two", "Three"]);
    let invites = ada.shared.share(list).expect("shared");
    eventually("the list reaching the server", &ada, || {
        shown_on(&ada)[0].owed == 0
    });
    ada.shelf.edit(list, &["Three", "One"]);
    ada.shelf.renamed(list, "Road Trip");
    eventually("the edit reaching the server", &ada, || {
        ada.shared.force();
        shown_on(&ada)[0].owed == 0 && shown_on(&ada)[0].name == "Road Trip"
    });
    let page = the_page_of(&hosted);
    let reader = the_reader_on(&page);
    let stands = format!(
        "{{\"name\":\"Road Trip\",\"rows\":[{},{}]}}",
        a_row("Three"),
        a_row("One")
    );

    assert!(page.contains("connect-src 'self'"));
    assert_eq!(
        read_in_a_browser(&hosted, reader, &code_in(&invites.view)).as_deref(),
        Some(stands.as_str())
    );
    assert_eq!(
        read_in_a_browser(
            &hosted,
            reader,
            &code_in(&invites.edit.expect("an edit link"))
        )
        .as_deref(),
        Some(stands.as_str())
    );
    let nobodys = resonate_sync::shared::Access::minted()
        .and_then(|access| access.viewing())
        .expect("a key")
        .code();
    assert_eq!(
        read_in_a_browser(&hosted, reader, &nobodys).as_deref(),
        Some("{\"failed\":\"unknown\"}")
    );
    assert_eq!(
        read_in_a_browser(&hosted, reader, "V-NOT-A-CODE").as_deref(),
        Some("{\"failed\":\"unread\"}")
    );
}

#[test]
fn a_browser_given_the_link_of_a_list_shared_anew_is_told_it_moved() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: no node to stand in for a browser");
        return;
    }
    let hosted = hosted("browsed-moved");
    let ada = hosted.person("Ada", Shelf::default());
    let list = ada.shelf.made("Mix", &["One"]);
    let invites = ada.shared.share(list).expect("shared");
    eventually("the list reaching the server", &ada, || {
        shown_on(&ada)[0].owed == 0
    });
    let space = shown_on(&ada)[0].space;
    ada.shared.reset(space, None).expect("reset");
    eventually("the list moving", &ada, || {
        shown_on(&ada)
            .first()
            .is_some_and(|shown| shown.space != space)
    });
    let page = the_page_of(&hosted);

    assert_eq!(
        read_in_a_browser(&hosted, the_reader_on(&page), &code_in(&invites.view)).as_deref(),
        Some("{\"failed\":\"moved\"}")
    );
}
