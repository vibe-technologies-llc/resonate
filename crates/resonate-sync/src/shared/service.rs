use std::{
    collections::{BTreeMap, BTreeSet},
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
    Welcomed,
    key::random_bytes,
    merge::projection_digest,
    quic::{to_the_server, trusted_roots},
    seal::{opened, sealed},
    service::{HOSTED_SERVER, InParts, Oversized, dialled_server, within},
    shared::{
        Access,
        appending::{
            AppendFrame, CompactionAnswered, append_frames, compaction_answered, compaction_pieces,
            owed_sent,
        },
        catalog::{LocalShared, SharedCatalog},
        diff::{Looking, Resolving, edits_seen, written_order},
        invitation::{Invited, link_to, web_link_to},
        moved::{
            LARGEST_SEATS_OPENED, Personal, Seat, access_from, decoded_seats, encoded_seats,
            seats_after,
        },
        op::{
            BUNDLE_OPS_AT_MOST, EntryId, LARGEST_BUNDLE_OPENED, MemberId, SharedList, SharedOp,
            decoded_ops, encoded_ops,
        },
        place::Place,
        store::{Kept, Me, Store},
    },
    wall_nanos,
    web::{Incoming, Outgoing, connect, to_the_web, web_address},
    wire::{
        FromServer, INCARNATION_VERSION, LARGEST_RECORD, PROTOCOL_VERSION, Refusal, SpaceAsked,
        ToServer,
    },
    words,
};

const LOOK_EVERY: Duration = Duration::from_secs(2);

const POLL_EVERY: Duration = Duration::from_secs(5 * 60);

const POLL_WHILE_SHOWN_EVERY: Duration = Duration::from_secs(5);

const FIRST_RETRY: Duration = Duration::from_secs(5);

const LAST_RETRY: Duration = Duration::from_secs(30 * 60);

const COMPACT_PAST_RECORDS: u64 = 512;

const COMPACT_PAST_BYTES: u64 = 4 * 1024 * 1024;

const CLOSES_BEFORE_ANSWER_FOR_TOO_OLD: u32 = 3;

const RESETS_OVERTAKEN_AT_MOST: usize = 4;

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
        let mut store = Store::open(&settings.store)?;
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
            catalog,
            server: settings.server.address(),
            to_web,
            to_server,
            endpoint: None,
            watched: None,
            revision: None,
            standings: BTreeMap::new(),
            closes_before_answer: BTreeMap::new(),
            servers_before_incarnations: BTreeSet::new(),
            oversized: BTreeMap::new(),
            shown: Arc::clone(&shown),
            held: Arc::clone(&held),
            poll_due: Instant::now(),
            retry_after: FIRST_RETRY,
        };

        running.publish();

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
    catalog: Arc<dyn SharedCatalog>,
    server: Option<String>,
    to_web: Arc<WebConfig>,
    to_server: ClientConfig,
    endpoint: Option<Endpoint>,
    watched: Option<SpaceId>,
    revision: Option<u64>,
    standings: BTreeMap<SpaceId, SharedStanding>,
    closes_before_answer: BTreeMap<String, u32>,
    servers_before_incarnations: BTreeSet<String>,
    oversized: BTreeMap<SpaceId, Oversized>,
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
        let mut clock = Clock::from_last(0);

        let mut stamped = || clock.now(device);

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
            incarnation: None,
            list,
            written,
            records_since: 0,
            bytes_since: 0,
            leaving: false,
            owed: vec![(1, ops)],
            clock,
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

                let kept = &mut self.lists[at];
                let op = who_i_am(&self.name, kept);

                kept.list.apply(&op);
                kept.owe(vec![op]);

                let kept = self.lists[at].clone();

                self.store.keep(&kept)?;
            }
            return Ok(keys.space);
        }

        let may_edit = access.may_edit();
        let mut kept = Kept {
            space: keys.space,
            access,
            server,
            device: DeviceId::minted()?,
            me,
            at: None,
            alias: None,
            seen: None,
            cursor: 0,
            incarnation: None,
            list: SharedList::default(),
            written: Vec::new(),
            records_since: 0,
            bytes_since: 0,
            leaving: false,
            owed: Vec::new(),
            clock: Clock::from_last(0),
        };

        if may_edit {
            let op = who_i_am(&self.name, &mut kept);

            kept.list.apply(&op);
            kept.owe(vec![op]);
        }
        self.store.keep(&kept)?;
        self.lists.push(kept);
        self.standings.insert(keys.space, SharedStanding::Waiting);

        Ok(keys.space)
    }

    fn say_who_i_am(&mut self) {
        for at in 0..self.lists.len() {
            if !self.lists[at].access.may_edit() || self.lists[at].leaving {
                continue;
            }

            let kept = &mut self.lists[at];

            if kept.list.is_named(kept.me.member, &self.name) {
                continue;
            }

            let op = who_i_am(&self.name, kept);

            kept.list.apply(&op);
            kept.owe(vec![op]);

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

        let kept = &mut self.lists[at];
        let op = SharedOp::Left {
            member: kept.me.member,
            stamp: kept.stamped(),
        };

        kept.list.apply(&op);
        kept.leaving = true;
        kept.at = None;
        kept.owe(vec![op]);

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
            clock: &mut kept.clock,
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
            kept.owe(ops);
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
        let name = kept.list.name().map_or_else(
            || words::shared::a_list_with_no_name().to_owned(),
            str::to_owned,
        );

        let written = catalog
            .shared_write(kept.at, &name, &entries, kept.access.may_edit())
            .map_err(Error::Catalog)?;
        let mut doing = Doing {
            catalog: catalog.as_ref(),
            clock: &mut kept.clock,
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
                let moved = head.is_some_and(|(head, _)| head != kept.cursor);
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
            Ok(Ok(None)) => return Err(self.closed_before_answering(address)),
            Ok(Err(_)) | Err(_) => return Err(SharedStanding::Unreached),
        };

        self.closes_before_answer.remove(address);
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

    fn closed_before_answering(&mut self, address: &str) -> SharedStanding {
        let closes = self
            .closes_before_answer
            .entry(address.to_owned())
            .or_default();

        *closes += 1;

        if *closes >= CLOSES_BEFORE_ANSWER_FOR_TOO_OLD {
            SharedStanding::ServerTooOld
        } else {
            SharedStanding::Unreached
        }
    }

    async fn reached(&mut self, server: &str) -> Result<(Outgoing, Incoming)> {
        if let Some(web) = web_address(server) {
            return within(connect(&web, &self.to_web)).await;
        }

        if self.endpoint.is_none() {
            let any = std::net::SocketAddr::from(([0, 0, 0, 0], 0));

            self.endpoint = Some(Endpoint::client(any).map_err(Error::QuicEndpoint)?);
        }

        let Some(endpoint) = self.endpoint.as_ref() else {
            return Err(Error::SharedStopped);
        };
        let (connection, _) = dialled_server(endpoint, &self.to_server, server).await?;
        let (send, recv) = within(async {
            connection
                .open_bi()
                .await
                .map_err(|source| Error::Connection(Box::new(source)))
        })
        .await?;

        Ok((Outgoing::Quic(send), Incoming::Quic(recv)))
    }

    fn share_version(&self, address: &str) -> u16 {
        if self.servers_before_incarnations.contains(address) {
            PROTOCOL_VERSION
        } else {
            INCARNATION_VERSION
        }
    }

    fn falls_back(&mut self, address: &str, version: u16, error: &Error) -> bool {
        let before_incarnations =
            version == INCARNATION_VERSION && matches!(error, Error::Refused(Refusal::Version));

        if before_incarnations {
            tracing::info!(
                server = address,
                "the shared playlists' server tells no incarnation, so a space made anew is told by its head alone"
            );
            self.servers_before_incarnations.insert(address.to_owned());
        }

        before_incarnations
    }

    async fn visited(&mut self, at: usize, address: &str, closed: bool) -> Result<()> {
        if self.visited_saying(at, address, closed).await? == Visit::Done {
            return Ok(());
        }

        let kept = self.lists[at].clone();

        self.store.keep(&kept)?;
        match self.visited_saying(at, address, closed).await? {
            Visit::Done => Ok(()),
            Visit::MadeAnew => Err(Error::SpaceMadeAnew),
        }
    }

    async fn visited_saying(&mut self, at: usize, address: &str, closed: bool) -> Result<Visit> {
        let version = self.share_version(address);

        match self.visited_at(at, address, closed, version).await {
            Err(error) if self.falls_back(address, version, &error) => {
                self.visited_at(at, address, closed, PROTOCOL_VERSION).await
            }
            visited => visited,
        }
    }

    async fn visited_at(
        &mut self,
        at: usize,
        address: &str,
        closed: bool,
        version: u16,
    ) -> Result<Visit> {
        let keys = self.lists[at].access.keys()?;
        let (mut send, mut recv) = self.reached(address).await?;
        let device = self.lists[at].device;
        let after = self.lists[at].cursor;
        let viewing = keys.write_token.is_none();

        let hello = match keys.write_token.clone() {
            Some(token) => ToServer::Share {
                version,
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
            match self.heard_in_a_session(at, &keys, FromServer::decoded(&frame)?, &mut target)? {
                Heard::Read { changed: read } => changed |= read,
                Heard::Closed(sealed) => {
                    heard_closed = Some(sealed);
                    break;
                }
                Heard::MadeAnew => {
                    send.finish().await;
                    return Ok(Visit::MadeAnew);
                }
            }
        }

        if let Some(sealed) = heard_closed {
            send.finish().await;
            return self
                .heard_it_closed(at, &keys, &sealed)
                .map(|_| Visit::Done);
        }

        let mut closed_now = None;

        if !viewing {
            closed_now = self
                .pushed(at, &keys, &mut send, &mut recv, &mut changed)
                .await?;
        }
        if closed_now.is_none() && !viewing {
            self.compacted(at, &keys, &mut send, &mut recv).await?;
            send.send(&ToServer::Bye.encoded()).await?;
        }
        send.finish().await;
        if let Some(sealed) = closed_now {
            return self
                .heard_it_closed(at, &keys, &sealed)
                .map(|_| Visit::Done);
        }

        if self.lists[at].leaving {
            return Ok(Visit::Done);
        }
        if changed || self.lists[at].at.is_none() {
            self.materialised(at)?;
        } else {
            let kept = self.lists[at].clone();

            self.store.keep(&kept)?;
        }

        Ok(Visit::Done)
    }

    fn heard_in_a_session(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        frame: FromServer,
        target: &mut Option<u64>,
    ) -> Result<Heard> {
        let changed = match frame {
            FromServer::Welcome {
                head, incarnation, ..
            } => {
                if self.lists[at].welcomed(head, incarnation) == Welcomed::MadeAnew {
                    tracing::info!(
                        "a shared playlist's space was made anew, so it is read from its start and sent whole"
                    );
                    self.oversized.remove(&keys.space);
                    return Ok(Heard::MadeAnew);
                }
                *target = Some(head);
                false
            }
            FromServer::Snapshot { through, parts } => {
                let mut changed = false;

                for part in &parts {
                    changed |= self.heard_a_record(at, keys, part)?;
                }

                let kept = &mut self.lists[at];

                kept.cursor = through;
                kept.records_since = 0;
                kept.bytes_since = 0;
                changed
            }
            FromServer::Records { entries } => self.heard_records(at, keys, entries)?,
            FromServer::Closed { sealed } => return Ok(Heard::Closed(sealed)),
            FromServer::Refused(refusal) => return Err(Error::Refused(refusal)),
            _ => false,
        };

        Ok(Heard::Read { changed })
    }

    fn heard_records(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        entries: Vec<crate::wire::Record>,
    ) -> Result<bool> {
        let mut changed = false;

        for entry in entries {
            if let Some(blob) = &entry.blob {
                changed |= self.heard_a_record(at, keys, blob)?;
                self.lists[at].bytes_since += blob.len() as u64;
            }

            let kept = &mut self.lists[at];

            kept.cursor = entry.seq;
            kept.records_since += 1;
        }

        Ok(changed)
    }

    fn heard_a_record(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        blob: &[u8],
    ) -> Result<bool> {
        let kept = &mut self.lists[at];

        Ok(heard_sealed(
            &mut kept.list,
            &mut kept.clock,
            keys,
            blob,
            wall_nanos(),
        ))
    }

    async fn pushed(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        send: &mut Outgoing,
        recv: &mut Incoming,
        changed: &mut bool,
    ) -> Result<Option<Vec<u8>>> {
        if self.lists[at].owed.is_empty() {
            return Ok(None);
        }

        let ops: Vec<SharedOp> = self.lists[at]
            .owed
            .iter()
            .flat_map(|(_, ops)| ops.iter().cloned())
            .collect();
        let frames = append_frames(ops.chunks(BUNDLE_OPS_AT_MOST), |bundle, sealed| {
            sealed_within(&keys.sealer, keys.space, bundle, sealed)
        })?;

        for frame in frames {
            if let Some(closed) = self.appended(at, keys, frame, send, recv, changed).await? {
                return Ok(Some(closed));
            }
        }

        Ok(None)
    }

    async fn appended(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        frame: AppendFrame,
        send: &mut Outgoing,
        recv: &mut Incoming,
        changed: &mut bool,
    ) -> Result<Option<Vec<u8>>> {
        send.send(&ToServer::Append { blobs: frame.blobs }.encoded())
            .await?;

        loop {
            let reply = within(async { recv.read().await }).await?;
            let Some(reply) = reply else {
                return Err(Error::ServerSilent);
            };
            match FromServer::decoded(&reply)? {
                FromServer::Appended { .. } => {
                    owed_sent(&mut self.lists[at].owed, frame.ops);
                    return Ok(None);
                }
                FromServer::Records { entries } => {
                    *changed |= self.heard_records(at, keys, entries)?;
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
        let records_since = kept.records_since;

        let waits = self
            .oversized
            .get(&kept.space)
            .is_some_and(|oversized| oversized.waits(records_since));

        if (records_since < COMPACT_PAST_RECORDS && kept.bytes_since < COMPACT_PAST_BYTES) || waits
        {
            return Ok(());
        }

        let through = kept.cursor;
        let ops = kept.list.as_ops();
        let pieces = compaction_pieces(ops.chunks(BUNDLE_OPS_AT_MOST), |bundle, parts| {
            sealed_within(&keys.sealer, keys.space, bundle, parts)
        })?;

        self.oversized.remove(&keys.space);

        let mut in_parts = InParts::default();

        send.send(&in_parts.asked(through, records_since, pieces).encoded())
            .await?;

        loop {
            let frame = within(async { recv.read().await }).await?;
            let Some(frame) = frame else {
                return Err(Error::ServerSilent);
            };
            match compaction_answered(&mut in_parts, records_since, FromServer::decoded(&frame)?) {
                CompactionAnswered::Staging(frames) => {
                    for frame in frames {
                        send.send(&frame.encoded()).await?;
                    }
                }
                CompactionAnswered::Compacted => {
                    let kept = &mut self.lists[at];

                    kept.records_since = 0;
                    kept.bytes_since = 0;
                    return Ok(());
                }
                CompactionAnswered::TakesNoParts(oversized) => {
                    tracing::warn!(
                        records_since,
                        "the shared playlist's server takes no compaction past one frame, so it is tried again once the records since double"
                    );
                    self.oversized.insert(keys.space, oversized);
                    return Ok(());
                }
                CompactionAnswered::SpaceFull(oversized) => {
                    tracing::warn!(
                        records_since,
                        "the shared playlist's space is too full to take a compaction, so it is tried again once the records since double"
                    );
                    self.oversized.insert(keys.space, oversized);
                    return Ok(());
                }
                CompactionAnswered::Refused(refusal) => return Err(Error::Refused(refusal)),
                CompactionAnswered::Unrelated => {}
            }
        }
    }

    fn heard_it_closed(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        sealed_seats: &[u8],
    ) -> Result<Option<SpaceId>> {
        let seats = seats_in(keys, sealed_seats);
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
        kept.device = DeviceId::minted()?;
        kept.incarnation = None;
        kept.owe_everything();

        let kept = kept.clone();

        self.store.keep(&kept)?;
        self.store.forget(old)?;
        self.standings.remove(&old);
        self.standings.insert(keys.space, SharedStanding::Waiting);
        self.poll_due = Instant::now();

        Ok(keys.space)
    }

    async fn reset(&mut self, space: SpaceId, removing: Option<MemberId>) -> Result<Invites> {
        let mut space = space;

        for _ in 0..RESETS_OVERTAKEN_AT_MOST {
            match self.reset_once(space, removing).await? {
                Reset::Again => {}
                Reset::Made(moved) => {
                    let Some(kept) = self.lists.iter().find(|kept| kept.space == moved) else {
                        return Err(Error::Catalog(Refused::Unavailable));
                    };

                    return invites_of(
                        &kept.access,
                        self.named_in_a_link(kept.server.as_ref()).as_deref(),
                    );
                }
                Reset::Overtaken(followed) => space = followed,
            }
        }

        Err(Error::ResetOvertaken)
    }

    async fn reset_once(&mut self, space: SpaceId, removing: Option<MemberId>) -> Result<Reset> {
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
        let Some(token) = old_keys.write_token.clone() else {
            return Err(Error::Catalog(Refused::Unwritable));
        };
        let device = self.lists[at].device;
        let version = self.share_version(&address);

        let (mut send, mut recv) = self.reached(&address).await?;

        send.send(
            &ToServer::Share {
                version,
                space: old_keys.space,
                token,
                viewer: old_keys.read_token.digest(),
                device,
                after: self.lists[at].cursor,
            }
            .encoded(),
        )
        .await?;

        let mut changed = false;

        let caught_up = match self.caught_up(at, &old_keys, &mut recv, &mut changed).await {
            Err(error) if self.falls_back(&address, version, &error) => return Ok(Reset::Again),
            caught_up => caught_up?,
        };

        match caught_up {
            CaughtUp::Level => {}
            CaughtUp::Closed(earlier) => {
                send.finish().await;
                self.written_if(at, changed)?;
                return self.followed(at, &old_keys, &earlier).map(Reset::Overtaken);
            }
            CaughtUp::MadeAnew => {
                send.finish().await;

                let kept = self.lists[at].clone();

                self.store.keep(&kept)?;
                return Ok(Reset::Again);
            }
        }

        let told = sealed(
            &old_keys.sealer,
            old_keys.space,
            &encoded_seats(&self.seats_after(at, removing)?),
        )?;

        send.send(
            &ToServer::Close {
                sealed: told.clone(),
            }
            .encoded(),
        )
        .await?;

        let mut target = None;

        let word = loop {
            let frame = within(async { recv.read().await }).await?;
            let Some(frame) = frame else {
                return Err(Error::ServerSilent);
            };
            match self.heard_in_a_session(
                at,
                &old_keys,
                FromServer::decoded(&frame)?,
                &mut target,
            )? {
                Heard::Read { changed: read } => changed |= read,
                Heard::Closed(word) => break word,
                Heard::MadeAnew => return Err(Error::SpaceMadeAnew),
            }
        };

        send.finish().await;

        self.written_if(at, changed)?;
        if word != told {
            return self.followed(at, &old_keys, &word).map(Reset::Overtaken);
        }
        if let Some(removing) = removing {
            let kept = &mut self.lists[at];
            let stamp = kept.stamped();

            kept.list.apply(&SharedOp::Left {
                member: removing,
                stamp,
            });
        }
        match self.heard_it_closed(at, &old_keys, &word)? {
            Some(moved) => Ok(Reset::Made(moved)),
            None => Err(Error::Catalog(Refused::Unavailable)),
        }
    }

    async fn caught_up(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        recv: &mut Incoming,
        changed: &mut bool,
    ) -> Result<CaughtUp> {
        let mut target = None;
        loop {
            if target.is_some_and(|target| self.lists[at].cursor >= target) {
                return Ok(CaughtUp::Level);
            }

            let frame = within(async { recv.read().await }).await?;
            let Some(frame) = frame else {
                return Err(Error::ServerSilent);
            };
            match self.heard_in_a_session(at, keys, FromServer::decoded(&frame)?, &mut target)? {
                Heard::Read { changed: read } => *changed |= read,
                Heard::Closed(word) => return Ok(CaughtUp::Closed(word)),
                Heard::MadeAnew => return Ok(CaughtUp::MadeAnew),
            }
        }
    }

    fn written_if(&mut self, at: usize, changed: bool) -> Result<()> {
        if changed && !self.lists[at].leaving {
            self.materialised(at)?;
        }

        Ok(())
    }

    fn seats_after(&self, at: usize, removing: Option<MemberId>) -> Result<Vec<Seat>> {
        let kept = &self.lists[at];

        seats_after(
            &kept.list,
            kept.me.member,
            &kept.me.personal,
            removing,
            &Access::minted()?,
        )
    }

    fn followed(
        &mut self,
        at: usize,
        keys: &crate::shared::SharedKeys,
        earlier: &[u8],
    ) -> Result<SpaceId> {
        let seated: BTreeSet<MemberId> = seats_in(keys, earlier)
            .into_iter()
            .map(|seat| seat.member)
            .collect();
        let left_out: Vec<MemberId> = self.lists[at]
            .list
            .people()
            .map(|(member, _)| member)
            .filter(|member| !seated.contains(member))
            .collect();
        let kept = &mut self.lists[at];

        for member in left_out {
            let stamp = kept.stamped();

            kept.list.apply(&SharedOp::Left { member, stamp });
        }

        self.heard_it_closed(at, keys, earlier)?
            .ok_or(Error::Catalog(Refused::Unavailable))
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
                name: kept.list.name().map_or_else(
                    || words::shared::a_list_with_no_name().to_owned(),
                    str::to_owned,
                ),
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

enum Heard {
    Read { changed: bool },
    Closed(Vec<u8>),
    MadeAnew,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    Done,
    MadeAnew,
}

enum CaughtUp {
    Level,
    Closed(Vec<u8>),
    MadeAnew,
}

enum Reset {
    Made(SpaceId),
    Overtaken(SpaceId),
    Again,
}

fn seats_in(keys: &crate::shared::SharedKeys, sealed_seats: &[u8]) -> Vec<Seat> {
    opened(&keys.sealer, keys.space, sealed_seats, LARGEST_SEATS_OPENED)
        .ok()
        .and_then(|plain| decoded_seats(&plain).ok())
        .unwrap_or_default()
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

fn who_i_am(name: &str, kept: &mut Kept) -> SharedOp {
    let stamp = kept.stamped();

    SharedOp::Member {
        member: kept.me.member,
        name: name.to_owned(),
        public: kept.me.personal.public(),
        stamp,
    }
}

fn heard_sealed(
    list: &mut SharedList,
    clock: &mut Clock,
    keys: &crate::shared::SharedKeys,
    blob: &[u8],
    wall: i64,
) -> bool {
    let Ok(plain) = opened(&keys.sealer, keys.space, blob, LARGEST_BUNDLE_OPENED) else {
        tracing::warn!("a shared playlist's record did not open and was passed over");
        return false;
    };
    let Ok(ops) = decoded_ops(&plain) else {
        tracing::warn!("a shared playlist's record did not read and was passed over");
        return false;
    };

    heard_ops(list, clock, &ops, wall)
}

fn heard_ops(list: &mut SharedList, clock: &mut Clock, ops: &[SharedOp], wall: i64) -> bool {
    let mut changed = false;

    for op in ops {
        let stamp = op.stamp();

        if stamp.too_far_ahead_of(wall) {
            tracing::warn!(
                nanos = stamp.nanos,
                wall,
                "a shared playlist's op stamped too far past the wall clock was passed over"
            );
            continue;
        }
        clock.saw(stamp);
        changed |= list.apply(op);
    }

    changed
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{STAMP_AHEAD_OF_THE_WALL_AT_MOST_NANOS, key::DEVICE_BYTES};

    const AN_HOUR_NANOS: i64 = 60 * 60 * 1_000_000_000;

    fn device(of: u8) -> DeviceId {
        DeviceId([of; DEVICE_BYTES])
    }

    fn a_list_of_one_song() -> SharedList {
        let mut list = SharedList::default();

        list.apply(&SharedOp::Added {
            entry: EntryId([1; 16]),
            recording: Recording {
                mbid: None,
                title: "One".to_owned(),
                artist: "Ada".to_owned(),
                album: "Album".to_owned(),
                length: Some(Duration::from_secs(200)),
            },
            place: Place(vec![10]),
            by: None,
            stamp: Stamp {
                nanos: 1,
                device: device(1),
            },
        });

        list
    }

    fn moved_to(place: u8, stamp: Stamp) -> SharedOp {
        SharedOp::Moved {
            entry: EntryId([1; 16]),
            place: Place(vec![place]),
            stamp,
        }
    }

    fn place_of_the_song(list: &SharedList) -> Vec<u8> {
        list.rows()
            .first()
            .map(|row| row.place.0.clone())
            .expect("a row")
    }

    #[test]
    fn a_stamp_far_past_the_wall_clock_is_refused_and_later_moves_still_land() {
        let wall = wall_nanos();
        let mut list = a_list_of_one_song();
        let mut clock = Clock::from_last(0);
        let hostile = moved_to(
            99,
            Stamp {
                nanos: i64::MAX,
                device: device(2),
            },
        );

        let changed = heard_ops(&mut list, &mut clock, &[hostile], wall);
        let first = moved_to(20, clock.now(device(1)));

        list.apply(&first);

        let second = moved_to(30, clock.now(device(1)));

        list.apply(&second);

        assert!(!changed);
        assert!(clock.last() < i64::MAX);
        assert_eq!(place_of_the_song(&list), [30]);
    }

    #[test]
    fn a_stamp_within_a_day_ahead_is_taken_and_moves_only_its_own_lists_clock() {
        let wall = wall_nanos();
        let ahead = wall + AN_HOUR_NANOS;
        let mut heard_ahead = a_list_of_one_song();
        let mut its_clock = Clock::from_last(0);
        let mut another_clock = Clock::from_last(0);
        let from_a_clock_running_fast = moved_to(
            40,
            Stamp {
                nanos: ahead,
                device: device(2),
            },
        );

        let changed = heard_ops(
            &mut heard_ahead,
            &mut its_clock,
            &[from_a_clock_running_fast],
            wall,
        );
        let next_here = its_clock.now(device(1));
        let next_elsewhere = another_clock.now(device(1));

        assert!(changed);
        assert_eq!(place_of_the_song(&heard_ahead), [40]);
        assert!(next_here.nanos > ahead);
        assert!(next_elsewhere.nanos < ahead);
    }

    #[test]
    fn a_snapshot_compacted_with_a_stamp_far_past_the_wall_clock_is_folded_without_it() {
        let wall = wall_nanos();
        let keys = Access::minted()
            .and_then(|access| access.keys())
            .expect("keys");
        let mut compacted = a_list_of_one_song();
        let mut clock = Clock::from_last(0);
        let mut parts = Vec::new();
        let renamed_at_the_end_of_time = SharedOp::Renamed {
            name: "Theirs".to_owned(),
            stamp: Stamp {
                nanos: i64::MAX,
                device: device(2),
            },
        };

        compacted.apply(&renamed_at_the_end_of_time);
        compacted.apply(&moved_to(
            40,
            Stamp {
                nanos: wall,
                device: device(2),
            },
        ));

        sealed_within(&keys.sealer, keys.space, &compacted.as_ops(), &mut parts).expect("sealed");

        let mut folded = SharedList::default();

        let changed = parts.iter().fold(false, |changed, part| {
            heard_sealed(&mut folded, &mut clock, &keys, part, wall) | changed
        });
        let unnamed = folded.name().map(str::to_owned);

        folded.apply(&SharedOp::Renamed {
            name: "Ours".to_owned(),
            stamp: clock.now(device(1)),
        });

        assert!(changed);
        assert_eq!(unnamed, None);
        assert_eq!(place_of_the_song(&folded), [40]);
        assert_eq!(folded.name(), Some("Ours"));
        assert!(clock.last() < i64::MAX);
    }

    #[test]
    fn a_day_ahead_is_the_furthest_a_stamp_is_believed() {
        let wall = 5 * AN_HOUR_NANOS;
        let at = |nanos| Stamp {
            nanos,
            device: device(1),
        };

        assert!(!at(wall + STAMP_AHEAD_OF_THE_WALL_AT_MOST_NANOS).too_far_ahead_of(wall));
        assert!(at(wall + STAMP_AHEAD_OF_THE_WALL_AT_MOST_NANOS + 1).too_far_ahead_of(wall));
    }
}
