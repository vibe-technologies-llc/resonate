use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, mpsc as blocking},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::RwLock;
use quinn::{ClientConfig, Endpoint};
use resonate_core::Recording;
use rustls::ClientConfig as WebConfig;
use tokio::{
    runtime,
    sync::mpsc,
    time::{sleep, timeout},
};

use crate::{
    Clock, DeviceId, Error, HeldShare, LocalPlaylistId, Refused, Result, Server, SpaceId, Stamp,
    key::random_bytes,
    merge::projection_digest,
    quic::{to_the_server, trusted_roots},
    seal::{opened, sealed},
    service::{HOSTED_SERVER, resolve, within},
    shared::{
        Access,
        catalog::{LocalShared, SharedCatalog},
        diff::{Looking, Resolving, edits_seen, written_order},
        invitation::{Invited, link_to, web_link_to},
        moved::{Personal, Seat, access_from, decoded_seats, encoded_seats, seat_for},
        op::{EntryId, MemberId, SharedList, SharedOp, decoded_ops, encoded_ops},
        place::Place,
        store::{Kept, Me, Store},
    },
    web::{Incoming, Outgoing, connect, to_the_web, web_address},
    wire::{FromServer, LARGEST_RECORD, PROTOCOL_VERSION, Refusal, SpaceAsked, ToServer},
};

const A_NAME_FOR_A_LIST_WITH_NONE: &str = "Shared playlist";

const LOOK_EVERY: Duration = Duration::from_secs(2);

const POLL_EVERY: Duration = Duration::from_secs(5 * 60);

const POLL_WHILE_SHOWN_EVERY: Duration = Duration::from_secs(5);

const FIRST_RETRY: Duration = Duration::from_secs(5);

const LAST_RETRY: Duration = Duration::from_secs(30 * 60);

const COMPACT_PAST_RECORDS: u64 = 512;

const COMPACT_PAST_BYTES: u64 = 4 * 1024 * 1024;

const BUNDLE_OPS_AT_MOST: usize = 256;

const LARGEST_BUNDLE: usize = LARGEST_RECORD / 2;

#[derive(Clone, Debug, Default)]
pub struct SharedSettings {
    pub server: Server,
    pub trusted: Option<PathBuf>,
    pub store: PathBuf,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedStanding {
    Waiting,
    Reached,
    Unreached,
    ServerTooOld,
    TurnedAway(Refusal),
    NoServer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invites {
    pub edit: Option<String>,
    pub view: String,
    pub edit_page: Option<String>,
    pub view_page: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonShown {
    pub member: MemberId,
    pub name: String,
    pub me: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedShown {
    pub space: SpaceId,
    pub at: Option<LocalPlaylistId>,
    pub name: String,
    pub may_edit: bool,
    pub invites: Invites,
    pub people: Vec<PersonShown>,
    pub unheld: Vec<Recording>,
    pub standing: SharedStanding,
    pub owed: usize,
}

enum Work {
    Share {
        at: LocalPlaylistId,
        answer: blocking::Sender<Result<Invites>>,
    },
    Join {
        code: String,
        server: Option<String>,
        answer: blocking::Sender<Result<SpaceId>>,
    },
    Leave {
        space: SpaceId,
        answer: blocking::Sender<Result<()>>,
    },
    Reset {
        space: SpaceId,
        removing: Option<MemberId>,
        answer: blocking::Sender<Result<Invites>>,
    },
    Adopt(HeldShare),
    DropCode(String),
    Watch(Option<SpaceId>),
    Named(String),
    Force,
    Stop,
}

pub struct Shared {
    work: mpsc::UnboundedSender<Work>,
    shown: Arc<RwLock<Vec<SharedShown>>>,
    held: Arc<RwLock<Vec<HeldShare>>>,
    thread: Option<JoinHandle<()>>,
}

impl Shared {
    pub fn start(settings: SharedSettings, catalog: Arc<dyn SharedCatalog>) -> Result<Self> {
        let store = Store::open(&settings.store)?;
        let roots = trusted_roots(settings.trusted.as_deref())?;
        let to_web = to_the_web(roots.clone())?;
        let to_server = to_the_server(roots)?;
        let me = store.me()?;
        let lists = store.lists()?;
        let shown = Arc::new(RwLock::new(Vec::new()));
        let held = Arc::new(RwLock::new(held_of(&lists)));
        let (work, asked) = mpsc::unbounded_channel();

        let running = Running {
            store,
            lists,
            me,
            name: settings.name,
            clock: Clock::from_last(0),
            catalog,
            server: settings.server.address(),
            to_web,
            to_server,
            endpoint: None,
            watched: None,
            revision: None,
            standings: BTreeMap::new(),
            shown: Arc::clone(&shown),
            held: Arc::clone(&held),
            poll_due: Instant::now(),
            retry_after: FIRST_RETRY,
        };
        let thread = thread::Builder::new()
            .name("resonate-shared".to_owned())
            .spawn(move || {
                let built = runtime::Builder::new_current_thread().enable_all().build();
                match built {
                    Ok(runtime) => runtime.block_on(running.run(asked)),
                    Err(error) => tracing::error!(%error, "shared playlists could not start"),
                }
            })
            .map_err(Error::Thread)?;

        Ok(Self {
            work,
            shown,
            held,
            thread: Some(thread),
        })
    }

    pub fn share(&self, at: LocalPlaylistId) -> Result<Invites> {
        self.asked(|answer| Work::Share { at, answer })
    }

    pub fn join(&self, code: &str, server: Option<String>) -> Result<SpaceId> {
        let code = code.to_owned();

        self.asked(|answer| Work::Join {
            code,
            server,
            answer,
        })
    }

    pub fn leave(&self, space: SpaceId) -> Result<()> {
        self.asked(|answer| Work::Leave { space, answer })
    }

    pub fn reset(&self, space: SpaceId, removing: Option<MemberId>) -> Result<Invites> {
        self.asked(|answer| Work::Reset {
            space,
            removing,
            answer,
        })
    }

    pub fn held(&self) -> Vec<HeldShare> {
        self.held.read().clone()
    }

    pub fn adopt(&self, share: HeldShare) {
        let _ = self.work.send(Work::Adopt(share));
    }

    pub fn drop_code(&self, code: String) {
        let _ = self.work.send(Work::DropCode(code));
    }

    pub fn watch(&self, space: Option<SpaceId>) {
        let _ = self.work.send(Work::Watch(space));
    }

    pub fn named(&self, name: String) {
        let _ = self.work.send(Work::Named(name));
    }

    pub fn force(&self) {
        let _ = self.work.send(Work::Force);
    }

    pub fn shown(&self) -> Vec<SharedShown> {
        self.shown.read().clone()
    }

    fn asked<T>(&self, work: impl FnOnce(blocking::Sender<Result<T>>) -> Work) -> Result<T> {
        let (answer, answered) = blocking::channel();
        self.work
            .send(work(answer))
            .map_err(|_| Error::SharedStopped)?;

        answered.recv().map_err(|_| Error::SharedStopped)?
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        let _ = self.work.send(Work::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Running {
    store: Store,
    lists: Vec<Kept>,
    me: Me,
    name: String,
    clock: Clock,
    catalog: Arc<dyn SharedCatalog>,
    server: Option<String>,
    to_web: Arc<WebConfig>,
    to_server: ClientConfig,
    endpoint: Option<Endpoint>,
    watched: Option<SpaceId>,
    revision: Option<u64>,
    standings: BTreeMap<SpaceId, SharedStanding>,
    shown: Arc<RwLock<Vec<SharedShown>>>,
    held: Arc<RwLock<Vec<HeldShare>>>,
    poll_due: Instant,
    retry_after: Duration,
}

struct Doing<'a> {
    catalog: &'a dyn SharedCatalog,
    clock: &'a mut Clock,
    device: DeviceId,
}

impl Resolving for Doing<'_> {
    fn held_as(&mut self, recording: &Recording) -> Result<Option<Recording>> {
        self.catalog
            .shared_held_as(recording)
            .map_err(Error::Catalog)
    }

    fn minted(&mut self) -> Result<EntryId> {
        random_bytes().map(EntryId)
    }

    fn stamped(&mut self) -> Stamp {
        self.clock.now(self.device)
    }
}

pub fn web_link(to: Invited, code: &str, server: Option<&str>) -> Option<String> {
    web_link_to(to, code, server.unwrap_or(HOSTED_SERVER))
}

fn invites_of(access: &Access, server: Option<&str>) -> Result<Invites> {
    let view = access.viewing()?;
    let edit = access.may_edit().then(|| access.code());

    Ok(Invites {
        edit: edit
            .as_deref()
            .map(|code| link_to(Invited::Playlist, code, server)),
        view: link_to(Invited::Playlist, &view.code(), server),
        edit_page: edit
            .as_deref()
            .and_then(|code| web_link(Invited::Playlist, code, server)),
        view_page: web_link(Invited::Playlist, &view.code(), server),
    })
}

fn digest_of(local: &LocalShared) -> Vec<u8> {
    projection_digest(&local.name, &local.entries).to_vec()
}

impl Running {
    async fn run(mut self, mut asked: mpsc::UnboundedReceiver<Work>) {
        self.publish();
        loop {
            let wait = self.poll_due.saturating_duration_since(Instant::now());
            tokio::select! {
                work = asked.recv() => {
                    let Some(work) = work else {
                        return;
                    };
                    if !self.worked(work).await {
                        return;
                    }
                }
                () = sleep(wait.min(LOOK_EVERY)) => {}
            }

            if let Err(error) = self.look() {
                tracing::warn!(%error, "the shared playlists could not be looked at");
            }
            if Instant::now() >= self.poll_due {
                self.exchange().await;
            }
            self.publish();
        }
    }

    async fn worked(&mut self, work: Work) -> bool {
        match work {
            Work::Share { at, answer } => {
                let shared = self.share(at);
                self.publish();
                let _ = answer.send(shared);
                self.poll_due = Instant::now();
            }
            Work::Join {
                code,
                server,
                answer,
            } => {
                let joined = self.join(&code, server);
                self.poll_due = Instant::now();
                if joined.is_ok() {
                    self.exchange().await;
                }
                self.publish();
                let _ = answer.send(joined);
            }
            Work::Leave { space, answer } => {
                let left = self.leave(space);
                self.publish();
                let _ = answer.send(left);
                self.poll_due = Instant::now();
            }
            Work::Reset {
                space,
                removing,
                answer,
            } => {
                let reset = self.reset(space, removing).await;
                self.publish();
                let _ = answer.send(reset);
                self.poll_due = Instant::now();
            }
            Work::Adopt(share) => {
                if let Err(error) = self.adopted(&share) {
                    tracing::warn!(%error, "a shared playlist from another device could not be taken up");
                }
                self.poll_due = Instant::now();
            }
            Work::DropCode(code) => {
                let held = self
                    .lists
                    .iter()
                    .find(|kept| kept.access.code() == code)
                    .map(|kept| kept.space);
                if let Some(space) = held
                    && let Err(error) = self.leave(space)
                {
                    tracing::warn!(%error, "a shared playlist left on another device stayed here");
                }
            }
            Work::Watch(space) => {
                self.watched = space;
                self.poll_due = Instant::now();
            }
            Work::Named(name) => {
                self.name = name;
                self.say_who_i_am();
                self.poll_due = Instant::now();
            }
            Work::Force => {
                self.retry_after = FIRST_RETRY;
                self.poll_due = Instant::now();
            }
            Work::Stop => return false,
        }

        true
    }

    fn share(&mut self, at: LocalPlaylistId) -> Result<Invites> {
        if self.server.is_none() {
            return Err(Error::SharedNeedsAServer);
        }
        if let Some(kept) = self.lists.iter().find(|kept| kept.at == Some(at)) {
            return invites_of(
                &kept.access,
                self.named_in_a_link(kept.server.as_ref()).as_deref(),
            );
        }

        let local = self
            .catalog
            .shared_flag(at, true)
            .map_err(Error::Catalog)?
            .ok_or(Error::Catalog(Refused::NotHere))?;
        let access = Access::minted()?;
        let keys = access.keys()?;
        let device = DeviceId::minted()?;
        let me = self.me.clone();
        let mut stamped = || self.clock.now(device);

        let mut ops = vec![SharedOp::Renamed {
            name: local.name.clone(),
            stamp: stamped(),
        }];
        ops.push(SharedOp::Member {
            member: me.member,
            name: self.name.clone(),
            public: me.personal.public(),
            stamp: stamped(),
        });
        let mut written = Vec::with_capacity(local.entries.len());
        let mut place: Option<Place> = None;
        for recording in local.entries.iter().flatten() {
            let entry = EntryId(random_bytes()?);
            let here = place.as_ref().map_or_else(Place::first, Place::after);
            ops.push(SharedOp::Added {
                entry,
                recording: recording.clone(),
                place: here.clone(),
                by: Some(me.member),
                stamp: stamped(),
            });
            written.push(entry);
            place = Some(here);
        }

        let mut list = SharedList::default();
        for op in &ops {
            list.apply(op);
        }
        let kept = Kept {
            space: keys.space,
            access: access.clone(),
            server: None,
            device,
            me,
            at: Some(local.at),
            alias: Some(local.name.clone()),
            seen: Some(digest_of(&local)),
            cursor: 0,
            list,
            written,
            records_since: 0,
            bytes_since: 0,
            leaving: false,
            owed: vec![(1, ops)],
        };
        self.store.keep(&kept)?;
        self.lists.push(kept);
        self.standings.insert(keys.space, SharedStanding::Waiting);

        invites_of(&access, self.named_in_a_link(None).as_deref())
    }

    fn adopted(&mut self, share: &HeldShare) -> Result<()> {
        let me = Me {
            member: MemberId(share.member),
            personal: Personal::from_bytes(share.personal),
        };
        let space = self.joined_as(&share.code, share.server.clone(), me)?;
        if let Some(at) = share.at
            && let Some(position) = self.lists.iter().position(|kept| kept.space == space)
            && self.lists[position].at.is_none()
        {
            let editable = self.lists[position].access.may_edit();
            if self
                .catalog
                .shared_flag(at, editable)
                .map_err(Error::Catalog)?
                .is_some()
            {
                self.lists[position].at = Some(at);
                let kept = self.lists[position].clone();
                self.store.keep(&kept)?;
            }
        }

        Ok(())
    }

    fn join(&mut self, code: &str, server: Option<String>) -> Result<SpaceId> {
        let me = self.me.clone();

        self.joined_as(code, server, me)
    }

    fn joined_as(&mut self, code: &str, server: Option<String>, me: Me) -> Result<SpaceId> {
        if self.server.is_none() && server.is_none() {
            return Err(Error::SharedNeedsAServer);
        }
        let access = Access::from_code(code)?;
        let keys = access.keys()?;
        let server = server.filter(|server| self.server.as_deref() != Some(server.as_str()));

        if let Some(at) = self.lists.iter().position(|kept| kept.space == keys.space) {
            if access.may_edit() && !self.lists[at].access.may_edit() {
                self.lists[at].access = access;
                let device = self.lists[at].device;
                let me = self.lists[at].me.clone();
                let ops = vec![self.who_i_am_on(device, &me)];
                let kept = &mut self.lists[at];
                for op in &ops {
                    kept.list.apply(op);
                }
                owed_more(kept, ops);
                let kept = self.lists[at].clone();
                self.store.keep(&kept)?;
            }
            return Ok(keys.space);
        }

        let device = DeviceId::minted()?;
        let ops = if access.may_edit() {
            vec![self.who_i_am_on(device, &me)]
        } else {
            Vec::new()
        };
        let mut list = SharedList::default();
        for op in &ops {
            list.apply(op);
        }
        let mut kept = Kept {
            space: keys.space,
            access,
            server,
            device,
            me,
            at: None,
            alias: None,
            seen: None,
            cursor: 0,
            list,
            written: Vec::new(),
            records_since: 0,
            bytes_since: 0,
            leaving: false,
            owed: Vec::new(),
        };
        if !ops.is_empty() {
            owed_more(&mut kept, ops);
        }
        self.store.keep(&kept)?;
        self.lists.push(kept);
        self.standings.insert(keys.space, SharedStanding::Waiting);

        Ok(keys.space)
    }

    fn who_i_am_on(&mut self, device: DeviceId, me: &Me) -> SharedOp {
        SharedOp::Member {
            member: me.member,
            name: self.name.clone(),
            public: me.personal.public(),
            stamp: self.clock.now(device),
        }
    }

    fn say_who_i_am(&mut self) {
        for at in 0..self.lists.len() {
            if !self.lists[at].access.may_edit() || self.lists[at].leaving {
                continue;
            }
            let device = self.lists[at].device;
            let me = self.lists[at].me.clone();
            let op = self.who_i_am_on(device, &me);
            let kept = &mut self.lists[at];
            kept.list.apply(&op);
            owed_more(kept, vec![op]);
            let kept = kept.clone();
            if let Err(error) = self.store.keep(&kept) {
                tracing::warn!(%error, "a name could not be kept");
            }
        }
    }

    fn leave(&mut self, space: SpaceId) -> Result<()> {
        let Some(at) = self.lists.iter().position(|kept| kept.space == space) else {
            return Ok(());
        };
        if let Some(local) = self.lists[at].at {
            self.catalog.shared_unflag(local).map_err(Error::Catalog)?;
        }

        self.leaving_at(at)
    }

    fn leaving_at(&mut self, at: usize) -> Result<()> {
        if !self.lists[at].access.may_edit() {
            let space = self.lists[at].space;
            self.store.forget(space)?;
            self.lists.remove(at);
            self.standings.remove(&space);

            return Ok(());
        }

        let stamp = self.clock.now(self.lists[at].device);
        let member = self.lists[at].me.member;
        let kept = &mut self.lists[at];
        let op = SharedOp::Left { member, stamp };
        kept.list.apply(&op);
        kept.leaving = true;
        kept.at = None;
        owed_more(kept, vec![op]);
        let kept = kept.clone();

        self.store.keep(&kept)
    }

    fn look(&mut self) -> Result<()> {
        let revision = self.catalog.shared_revision();
        if self.revision == Some(revision) {
            return Ok(());
        }
        self.revision = Some(revision);

        let mut any_moved = false;
        for at in (0..self.lists.len()).rev() {
            let Some(local) = self.lists[at].at else {
                continue;
            };
            let Some(listed) = self.catalog.shared_read(local).map_err(Error::Catalog)? else {
                self.leaving_at(at)?;
                any_moved = true;
                continue;
            };
            let digest = digest_of(&listed);
            if self.lists[at].seen.as_deref() == Some(digest.as_slice()) {
                continue;
            }

            if self.lists[at].access.may_edit() {
                self.edited(at, &listed, digest)?;
                any_moved = true;
            } else {
                self.materialised(at)?;
            }
        }
        if any_moved {
            self.poll_due = Instant::now();
        }
        self.revision = Some(self.catalog.shared_revision());

        Ok(())
    }

    fn edited(&mut self, at: usize, listed: &LocalShared, digest: Vec<u8>) -> Result<()> {
        let device = self.lists[at].device;
        let member = self.lists[at].me.member;
        let catalog = Arc::clone(&self.catalog);
        let kept = &mut self.lists[at];
        let mut doing = Doing {
            catalog: catalog.as_ref(),
            clock: &mut self.clock,
            device,
        };

        let mut ops = edits_seen(
            &Looking {
                list: &kept.list,
                written: &kept.written,
                listed: &listed.entries,
                by: Some(member),
            },
            &mut doing,
        )?;
        if kept.alias.as_deref() != Some(listed.name.as_str())
            && kept.list.name() != Some(listed.name.as_str())
        {
            ops.push(SharedOp::Renamed {
                name: listed.name.clone(),
                stamp: doing.stamped(),
            });
            kept.alias = Some(listed.name.clone());
        }
        for op in &ops {
            kept.list.apply(op);
        }
        kept.written = written_order(&kept.list.rows(), &listed.entries, &mut doing)?;
        kept.seen = Some(digest);
        if !ops.is_empty() {
            owed_more(kept, ops);
        }
        let kept = kept.clone();

        self.store.keep(&kept)
    }

    fn materialised(&mut self, at: usize) -> Result<()> {
        let device = self.lists[at].device;
        let catalog = Arc::clone(&self.catalog);
        let kept = &mut self.lists[at];
        let rows = kept.list.rows();
        let entries: Vec<Recording> = rows.iter().map(|row| row.recording.clone()).collect();
        let name = kept
            .list
            .name()
            .unwrap_or(A_NAME_FOR_A_LIST_WITH_NONE)
            .to_owned();

        let written = catalog
            .shared_write(kept.at, &name, &entries, kept.access.may_edit())
            .map_err(Error::Catalog)?;
        let mut doing = Doing {
            catalog: catalog.as_ref(),
            clock: &mut self.clock,
            device,
        };
        kept.written = written_order(&rows, &written.entries, &mut doing)?;
        kept.at = Some(written.at);
        kept.alias = Some(written.name.clone());
        kept.seen = Some(digest_of(&written));
        let kept = kept.clone();
        self.store.keep(&kept)?;
        self.revision = None;

        Ok(())
    }

    fn address_of(&self, kept: &Kept) -> Option<String> {
        kept.server.clone().or_else(|| self.server.clone())
    }

    async fn exchange(&mut self) {
        let every = if self.watched.is_some() {
            POLL_WHILE_SHOWN_EVERY
        } else {
            POLL_EVERY
        };
        let mut all_reached = true;

        let servers: Vec<Option<String>> = {
            let mut servers: Vec<Option<String>> =
                self.lists.iter().map(|kept| kept.server.clone()).collect();
            servers.sort();
            servers.dedup();
            servers
        };
        for server in servers {
            let address = server.clone().or_else(|| self.server.clone());
            let on_it: Vec<usize> = (0..self.lists.len())
                .filter(|at| self.lists[*at].server == server)
                .collect();
            let Some(address) = address else {
                for at in on_it {
                    self.standings
                        .insert(self.lists[at].space, SharedStanding::NoServer);
                }
                continue;
            };

            let heads = self.heads(&address, &on_it).await;
            let heads = match heads {
                Ok(heads) => heads,
                Err(standing) => {
                    all_reached = false;
                    for at in on_it {
                        self.standings.insert(self.lists[at].space, standing);
                    }
                    continue;
                }
            };

            let spaces: Vec<SpaceId> = on_it.iter().map(|at| self.lists[*at].space).collect();
            for space in spaces {
                let Some(at) = self.lists.iter().position(|kept| kept.space == space) else {
                    continue;
                };
                let head = heads.get(&space).copied();
                let kept = &self.lists[at];
                let moved = head.is_some_and(|(head, _)| head > kept.cursor);
                let closed = head.is_some_and(|(_, closed)| closed);
                let unknown_to_it = head.is_none() && kept.access.may_edit();
                let first_read = kept.at.is_none() && !kept.leaving;
                if !(moved || closed || unknown_to_it || first_read || !kept.owed.is_empty()) {
                    self.standings.insert(space, SharedStanding::Reached);
                    continue;
                }

                match self.visited(at, &address, closed).await {
                    Ok(()) => {
                        self.standings.insert(space, SharedStanding::Reached);
                    }
                    Err(Error::Refused(refusal)) => {
                        self.standings
                            .insert(space, SharedStanding::TurnedAway(refusal));
                    }
                    Err(error) => {
                        tracing::debug!(%error, "a shared playlist could not be read");
                        all_reached = false;
                        self.standings.insert(space, SharedStanding::Unreached);
                    }
                }
            }
        }

        self.lists.retain(|kept| {
            let done = kept.leaving && kept.owed.is_empty();
            if done && let Err(error) = self.store.forget(kept.space) {
                tracing::warn!(%error, "a shared playlist left could not be forgotten");
            }

            !done
        });

        if all_reached {
            self.retry_after = FIRST_RETRY;
            self.poll_due = Instant::now() + every;
        } else {
            let wait = if self.watched.is_some() {
                POLL_WHILE_SHOWN_EVERY.max(FIRST_RETRY)
            } else {
                self.retry_after
            };
            self.retry_after = (self.retry_after * 2).min(LAST_RETRY);
            self.poll_due = Instant::now() + wait;
        }
    }

    async fn heads(
        &mut self,
        address: &str,
        on_it: &[usize],
    ) -> std::result::Result<BTreeMap<SpaceId, (u64, bool)>, SharedStanding> {
        let mut asked = Vec::with_capacity(on_it.len());
        for at in on_it {
            let Ok(keys) = self.lists[*at].access.keys() else {
                continue;
            };
            asked.push(SpaceAsked {
                space: keys.space,
                token: keys.write_token.unwrap_or(keys.read_token),
            });
        }

        let (mut send, mut recv) = self
            .reached(address)
            .await
            .map_err(|_| SharedStanding::Unreached)?;
        send.send(
            &ToServer::Heads {
                version: PROTOCOL_VERSION,
                asked,
            }
            .encoded(),
        )
        .await
        .map_err(|_| SharedStanding::Unreached)?;

        let answer = timeout(FIRST_RETRY * 3, recv.read()).await;
        let frame = match answer {
            Ok(Ok(Some(frame))) => frame,
            Ok(Ok(None)) => return Err(SharedStanding::ServerTooOld),
            Ok(Err(_)) | Err(_) => return Err(SharedStanding::Unreached),
        };
        send.finish().await;

        match FromServer::decoded(&frame) {
            Ok(FromServer::Heads { heads }) => Ok(heads
                .into_iter()
                .map(|head| (head.space, (head.head, head.closed)))
                .collect()),
            Ok(FromServer::Refused(refusal)) => Err(SharedStanding::TurnedAway(refusal)),
            Ok(_) | Err(_) => Err(SharedStanding::ServerTooOld),
        }
    }

    async fn reached(&mut self, server: &str) -> Result<(Outgoing, Incoming)> {
        if let Some(web) = web_address(server) {
            return within(connect(&web, &self.to_web)).await;
        }

        let (address, host) = within(resolve(server)).await?;
        if self.endpoint.is_none() {
            let any = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
            self.endpoint = Some(Endpoint::client(any).map_err(Error::QuicEndpoint)?);
        }
        let Some(endpoint) = self.endpoint.as_ref() else {
            return Err(Error::SharedStopped);
        };
        let connecting = endpoint
            .connect_with(self.to_server.clone(), address, &host)
            .map_err(|source| Error::Connect(Box::new(source)))?;
        let connection = within(async {
            connecting
                .await
                .map_err(|source| Error::Connection(Box::new(source)))
        })
        .await?;
        let (send, recv) = within(async {
            connection
                .open_bi()
                .await
                .map_err(|source| Error::Connection(Box::new(source)))
        })
        .await?;

        Ok((Outgoing::Quic(send), Incoming::Quic(recv)))
    }

    async fn visited(&mut self, at: usize, address: &str, closed: bool) -> Result<()> {
        let keys = self.lists[at].access.keys()?;
        let (mut send, mut recv) = self.reached(address).await?;
        let device = self.lists[at].device;
        let after = self.lists[at].cursor;
        let viewing = keys.write_token.is_none();

        let hello = match keys.write_token.clone() {
            Some(token) => ToServer::Share {
                version: PROTOCOL_VERSION,
                space: keys.space,
                token,
                viewer: keys.read_token.digest(),
                device,
                after,
            },
            None => ToServer::View {
                version: PROTOCOL_VERSION,
                space: keys.space,
                token: keys.read_token.clone(),
                device: DeviceId::minted()?,
                after,
            },
        };
        send.send(&hello.encoded()).await?;

        let mut changed = false;
        let mut target = None;
        let mut heard_closed = None;
        loop {
            if let Some(target) = target
                && self.lists[at].cursor >= target
                && (!closed || heard_closed.is_some())
                && !viewing
            {
                break;
            }
            let frame = within(async { recv.read().await }).await?;
            let Some(frame) = frame else {
                break;
            };
            match FromServer::decoded(&frame)? {
                FromServer::Welcome { head, .. } => {
                    if head < self.lists[at].cursor {
                        let kept = &mut self.lists[at];
                        kept.cursor = 0;
                        let whole = kept.list.as_ops();
                        kept.owed.clear();
                        owed_more(kept, whole);
                    }
                    target = Some(head);
                }
                FromServer::Snapshot { through, parts } => {
                    for part in &parts {
                        changed |= self.heard_a_record(at, &keys, part)?;
                    }
                    let kept = &mut self.lists[at];
                    kept.cursor = through;
                    kept.records_since = 0;
                    kept.bytes_since = 0;
                }
                FromServer::Records { entries } => {
                    for entry in entries {
                        if let Some(blob) = &entry.blob {
                            changed |= self.heard_a_record(at, &keys, blob)?;
                            self.lists[at].bytes_since += blob.len() as u64;
                        }
                        let kept = &mut self.lists[at];
                        kept.cursor = entry.seq;
                        kept.records_since += 1;
                    }
                }
                FromServer::Closed { sealed } => {
                    heard_closed = Some(sealed);
                    break;
                }
                FromServer::Refused(refusal) => return Err(Error::Refused(refusal)),
                _ => {}
            }
        }

        if let Some(sealed) = heard_closed {
            send.finish().await;
            return self.heard_it_closed(at, &keys, &sealed).map(drop);
        }

        let mut closed_now = None;
        if !viewing {
            closed_now = self.pushed(at, &keys, &mut send, &mut recv).await?;
        }
        if closed_now.is_none() && !viewing {
            self.compacted(at, &keys, &mut send, &mut recv).await?;
            send.send(&ToServer::Bye.encoded()).await?;
        }
        send.finish().await;
        if let Some(sealed) = closed_now {
            return self.heard_it_closed(at, &keys, &sealed).map(drop);
        }

        if self.lists[at].leaving {
            return Ok(());
        }
        if changed || self.lists[at].at.is_none() {
            self.materialised(at)?;
        } else {
            let kept = self.lists[at].clone();
            self.store.keep(&kept)?;
        }

        Ok(())
    }

    fn heard_a_record(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        blob: &[u8],
    ) -> Result<bool> {
        let Ok(plain) = opened(&keys.sealer, keys.space, blob) else {
            tracing::warn!("a shared playlist's record did not open and was passed over");
            return Ok(false);
        };
        let Ok(ops) = decoded_ops(&plain) else {
            tracing::warn!("a shared playlist's record did not read and was passed over");
            return Ok(false);
        };

        let kept = &mut self.lists[at];
        let before = kept.list.clone();
        for op in &ops {
            if let Some(stamp) = stamp_of(op) {
                self.clock.saw(stamp);
            }
            kept.list.apply(op);
        }

        Ok(kept.list != before)
    }

    async fn pushed(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        send: &mut Outgoing,
        recv: &mut Incoming,
    ) -> Result<Option<Vec<u8>>> {
        if self.lists[at].owed.is_empty() {
            return Ok(None);
        }

        let ops: Vec<SharedOp> = self.lists[at]
            .owed
            .iter()
            .flat_map(|(_, ops)| ops.iter().cloned())
            .collect();
        let mut blobs = Vec::new();
        for bundle in ops.chunks(BUNDLE_OPS_AT_MOST) {
            sealed_within(&keys.sealer, keys.space, bundle, &mut blobs)?;
        }
        send.send(&ToServer::Append { blobs }.encoded()).await?;

        loop {
            let frame = within(async { recv.read().await }).await?;
            let Some(frame) = frame else {
                return Err(Error::ServerSilent);
            };
            match FromServer::decoded(&frame)? {
                FromServer::Appended { .. } => {
                    self.lists[at].owed.clear();
                    return Ok(None);
                }
                FromServer::Records { entries } => {
                    let kept = &mut self.lists[at];
                    for entry in entries {
                        kept.cursor = entry.seq;
                        kept.records_since += 1;
                    }
                }
                FromServer::Closed { sealed } => return Ok(Some(sealed)),
                FromServer::Refused(refusal) => return Err(Error::Refused(refusal)),
                _ => {}
            }
        }
    }

    async fn compacted(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        send: &mut Outgoing,
        recv: &mut Incoming,
    ) -> Result<()> {
        let kept = &self.lists[at];
        if kept.records_since < COMPACT_PAST_RECORDS && kept.bytes_since < COMPACT_PAST_BYTES {
            return Ok(());
        }

        let through = kept.cursor;
        let ops = kept.list.as_ops();
        let mut parts = Vec::new();
        for bundle in ops.chunks(BUNDLE_OPS_AT_MOST) {
            sealed_within(&keys.sealer, keys.space, bundle, &mut parts)?;
        }
        send.send(&ToServer::Compact { through, parts }.encoded())
            .await?;

        loop {
            let frame = within(async { recv.read().await }).await?;
            let Some(frame) = frame else {
                return Err(Error::ServerSilent);
            };
            match FromServer::decoded(&frame)? {
                FromServer::Compacted { .. } | FromServer::Refused(Refusal::Stale) => {
                    let kept = &mut self.lists[at];
                    kept.records_since = 0;
                    kept.bytes_since = 0;
                    return Ok(());
                }
                FromServer::Refused(refusal) => return Err(Error::Refused(refusal)),
                _ => {}
            }
        }
    }

    fn heard_it_closed(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        sealed_seats: &[u8],
    ) -> Result<Option<SpaceId>> {
        let seats = opened(&keys.sealer, keys.space, sealed_seats)
            .ok()
            .and_then(|plain| decoded_seats(&plain).ok())
            .unwrap_or_default();
        let me = self.lists[at].me.clone();
        let mine = seats.iter().find(|seat| seat.member == me.member);

        if let Some(seat) = mine
            && let Ok(access) = access_from(seat, &me.personal)
        {
            return self.moved_to(at, access).map(Some);
        }

        let kept = self.lists.remove(at);
        if let Some(local) = kept.at {
            self.catalog.shared_unflag(local).map_err(Error::Catalog)?;
        }
        self.store.forget(kept.space)?;
        self.standings.remove(&kept.space);

        Ok(None)
    }

    fn moved_to(&mut self, at: usize, access: Access) -> Result<SpaceId> {
        let keys = access.keys()?;
        let old = self.lists[at].space;
        let kept = &mut self.lists[at];
        kept.access = access;
        kept.space = keys.space;
        kept.cursor = 0;
        kept.device = DeviceId::minted()?;
        kept.records_since = 0;
        kept.bytes_since = 0;
        let whole = kept.list.as_ops();
        kept.owed.clear();
        owed_more(kept, whole);
        let kept = kept.clone();

        self.store.keep(&kept)?;
        self.store.forget(old)?;
        self.standings.remove(&old);
        self.standings.insert(keys.space, SharedStanding::Waiting);
        self.poll_due = Instant::now();

        Ok(keys.space)
    }

    async fn reset(&mut self, space: SpaceId, removing: Option<MemberId>) -> Result<Invites> {
        let Some(at) = self.lists.iter().position(|kept| kept.space == space) else {
            return Err(Error::Catalog(Refused::NotHere));
        };
        if !self.lists[at].access.may_edit() {
            return Err(Error::Catalog(Refused::Unwritable));
        }
        let address = self
            .address_of(&self.lists[at])
            .ok_or(Error::SharedNeedsAServer)?;

        let old_keys = self.lists[at].access.keys()?;
        let fresh = Access::minted()?;
        let mut seats: Vec<Seat> = Vec::new();
        for (member, person) in self.lists[at].list.people() {
            if Some(member) != removing {
                seats.push(seat_for(member, &person.public, &fresh)?);
            }
        }
        if !seats.iter().any(|seat| seat.member == self.me.member) {
            seats.push(seat_for(
                self.me.member,
                &self.me.personal.public(),
                &fresh,
            )?);
        }
        let told = sealed(&old_keys.sealer, old_keys.space, &encoded_seats(&seats))?;

        let (mut send, mut recv) = self.reached(&address).await?;
        let device = self.lists[at].device;
        let after = self.lists[at].cursor;
        let Some(token) = old_keys.write_token.clone() else {
            return Err(Error::Catalog(Refused::Unwritable));
        };
        send.send(
            &ToServer::Share {
                version: PROTOCOL_VERSION,
                space: old_keys.space,
                token,
                viewer: old_keys.read_token.digest(),
                device,
                after,
            }
            .encoded(),
        )
        .await?;
        send.send(&ToServer::Close { sealed: told }.encoded())
            .await?;

        let kept_word = loop {
            let frame = within(async { recv.read().await }).await?;
            let Some(frame) = frame else {
                return Err(Error::ServerTooOld);
            };
            match FromServer::decoded(&frame)? {
                FromServer::Closed { sealed } => break sealed,
                FromServer::Refused(refusal) => return Err(Error::Refused(refusal)),
                _ => {}
            }
        };
        send.finish().await;

        if let Some(removing) = removing {
            let stamp = self.clock.now(device);
            let op = SharedOp::Left {
                member: removing,
                stamp,
            };
            self.lists[at].list.apply(&op);
        }
        let Some(moved) = self.heard_it_closed(at, &old_keys, &kept_word)? else {
            return Err(Error::Catalog(Refused::Unavailable));
        };
        let Some(kept) = self.lists.iter().find(|kept| kept.space == moved) else {
            return Err(Error::Catalog(Refused::Unavailable));
        };

        invites_of(
            &kept.access,
            self.named_in_a_link(kept.server.as_ref()).as_deref(),
        )
    }

    fn named_in_a_link(&self, kept_on: Option<&String>) -> Option<String> {
        kept_on
            .or(self.server.as_ref())
            .filter(|server| server.as_str() != HOSTED_SERVER)
            .cloned()
    }

    fn publish(&self) {
        let mut shown = Vec::with_capacity(self.lists.len());
        for kept in self.lists.iter().filter(|kept| !kept.leaving) {
            let named = self.named_in_a_link(kept.server.as_ref());
            let Ok(invites) = invites_of(&kept.access, named.as_deref()) else {
                continue;
            };
            let written: std::collections::BTreeSet<EntryId> =
                kept.written.iter().copied().collect();
            let rows = kept.list.rows();

            shown.push(SharedShown {
                space: kept.space,
                at: kept.at,
                name: kept
                    .list
                    .name()
                    .unwrap_or(A_NAME_FOR_A_LIST_WITH_NONE)
                    .to_owned(),
                may_edit: kept.access.may_edit(),
                invites,
                people: kept
                    .list
                    .people()
                    .map(|(member, person)| PersonShown {
                        member,
                        name: person.name.clone(),
                        me: member == kept.me.member,
                    })
                    .collect(),
                unheld: rows
                    .into_iter()
                    .filter(|row| !written.contains(&row.entry))
                    .map(|row| row.recording)
                    .collect(),
                standing: self
                    .standings
                    .get(&kept.space)
                    .copied()
                    .unwrap_or(SharedStanding::Waiting),
                owed: kept.owed.iter().map(|(_, ops)| ops.len()).sum(),
            });
        }

        *self.shown.write() = shown;
        *self.held.write() = held_of(&self.lists);
    }
}

fn held_of(lists: &[Kept]) -> Vec<HeldShare> {
    lists
        .iter()
        .filter(|kept| !kept.leaving)
        .map(|kept| HeldShare {
            code: kept.access.code(),
            server: kept.server.clone(),
            member: kept.me.member.0,
            personal: kept.me.personal.to_bytes(),
            at: kept.at,
        })
        .collect()
}

fn owed_more(kept: &mut Kept, ops: Vec<SharedOp>) {
    let seq = kept.owed.last().map_or(1, |(seq, _)| seq + 1);
    kept.owed.push((seq, ops));
}

const fn stamp_of(op: &SharedOp) -> Option<Stamp> {
    match op {
        SharedOp::Added { stamp, .. }
        | SharedOp::Moved { stamp, .. }
        | SharedOp::Removed { stamp, .. }
        | SharedOp::Renamed { stamp, .. }
        | SharedOp::Member { stamp, .. }
        | SharedOp::Left { stamp, .. } => Some(*stamp),
    }
}

fn sealed_within(
    sealer: &ring::aead::LessSafeKey,
    space: SpaceId,
    ops: &[SharedOp],
    blobs: &mut Vec<Vec<u8>>,
) -> Result<()> {
    let plain = encoded_ops(ops);
    let blob = sealed(sealer, space, &plain)?;
    if blob.len() <= LARGEST_BUNDLE || ops.len() <= 1 {
        blobs.push(blob);
        return Ok(());
    }

    let (first, second) = ops.split_at(ops.len() / 2);
    sealed_within(sealer, space, first, blobs)?;
    sealed_within(sealer, space, second, blobs)
}
