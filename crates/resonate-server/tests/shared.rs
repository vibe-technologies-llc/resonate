use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::Recording;
use resonate_server::{Allowance, Limits, Server, Store};
use resonate_sync::{
    LocalPlaylistId, Refused, Server as Reached,
    shared::{LocalShared, Shared, SharedCatalog, SharedSettings, SharedShown, SharedStanding},
};
use tokio::net::TcpListener;

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
        shelf.held.lock().holds_nothing_named = titles.iter().map(|title| (*title).to_owned()).collect();

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
        "resonate-shared-{name}-{}-{:?}",
        process::id(),
        Instant::now()
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

fn two_sharing(hosted: &Hosted, titles: &[&str]) -> (Person, LocalPlaylistId, Person, LocalPlaylistId) {
    let ada = hosted.person("Ada", Shelf::default());
    let list = ada.shelf.made("Mix", titles);
    let invites = ada.shared.share(list).expect("shared");

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
fn two_people_editing_at_once_keep_both_edits() {
    let hosted = hosted("merged");
    let (ada, list, ben, theirs) = two_sharing(&hosted, &["One", "Two"]);
    eventually("the songs arriving", &ben, || {
        ben.shelf.titles_of(theirs).is_some_and(|titles| titles.len() == 2)
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
        ben.shelf.titles_of(theirs).is_some_and(|titles| titles.len() == 2)
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
        shown_on(&viewer).first().and_then(|shown| shown.at).is_some()
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
    assert!(!ben.shelf.is_flagged(theirs), "Ben's copy stays his own list");
    assert!(ben.shelf.titles_of(theirs).is_some());

    eventually("Cal moving to the new list", &cal, || {
        shown_on(&cal).first().is_some_and(|shown| shown.space != space)
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
        "resonate-shared-offline-{}-{:?}",
        process::id(),
        Instant::now()
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
    let person = Person {
        shelf,
        shared,
    };
    eventually("being told the server is away", &person, || {
        shown_on(&person)[0].standing == SharedStanding::Unreached
    });
    assert!(shown_on(&person)[0].owed > 0);
}
