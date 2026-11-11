use std::{
    collections::VecDeque,
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use parking_lot::{Mutex, RwLock};
use quinn::{ClientConfig, Endpoint};
use tokio::{
    runtime,
    sync::mpsc,
    time::{Instant, interval, sleep},
};

use crate::{
    DeviceId, Error, Result, SpaceId, Token,
    key::random_bytes,
    party::{
        clock::ServerClock,
        key::{PartyKey, PartyKeys},
        said::{LARGEST_SAID_OPENED, PartyNow, PartySaid, PartySong},
    },
    quic::{to_the_server, trusted_roots},
    seal::{opened, sealed},
    service::{resolve, within},
    web::{Incoming, Outgoing, connect, to_the_web, web_address},
    wire::{FromServer, PROTOCOL_VERSION, Refusal, ToServer},
};

const PINGS_EVERY: Duration = Duration::from_secs(1);

const FIRST_RETRY: Duration = Duration::from_secs(1);

const LAST_RETRY: Duration = Duration::from_secs(15);

const HOST_NAME_FOR_NO_NAME: &str = "";

const WANTS_KEPT_AT_MOST: usize = 32;

const SAID_WHILE_AWAY_AT_MOST: usize = 16;

const PEOPLE_TOLD_AT_MOST_EVERY: Duration = Duration::from_secs(3);

const CLOSES_BEFORE_WELCOME_FOR_TOO_OLD: u32 = 3;

pub struct PartySettings {
    pub server: String,
    pub trusted: Option<std::path::PathBuf>,
    pub name: String,
    pub now_nanos: Arc<dyn Fn() -> u64 + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartyReach {
    Connecting,
    Here,
    Reconnecting,
    Ended,
    NoSuchParty,
    TurnedAway,
    ServerTooOld,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyPerson {
    pub member: DeviceId,
    pub name: String,
    pub host: bool,
    pub me: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyWantingASong {
    pub from: DeviceId,
    pub song: PartySong,
}

const REQUEST_ID_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId {
    from: DeviceId,
    row: u64,
    digest: u64,
}

impl RequestId {
    pub fn to_bytes(self) -> [u8; REQUEST_ID_BYTES] {
        let mut bytes = [0; REQUEST_ID_BYTES];
        bytes[..16].copy_from_slice(&self.from.0);
        bytes[16..24].copy_from_slice(&self.row.to_be_bytes());
        bytes[24..].copy_from_slice(&self.digest.to_be_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; REQUEST_ID_BYTES] = bytes.try_into().ok()?;
        let (from, rest) = bytes.split_first_chunk::<16>()?;
        let (row, digest) = rest.split_first_chunk::<8>()?;
        Some(Self {
            from: DeviceId(*from),
            row: u64::from_be_bytes(*row),
            digest: u64::from_be_bytes(digest.try_into().ok()?),
        })
    }
}

impl PartyWantingASong {
    pub fn id(&self) -> RequestId {
        let mut hasher = DefaultHasher::new();
        let recording = &self.song.recording;
        recording.title.hash(&mut hasher);
        recording.artist.hash(&mut hasher);
        recording.album.hash(&mut hasher);
        self.song.isrc.hash(&mut hasher);

        RequestId {
            from: self.from,
            row: self.song.row,
            digest: hasher.finish(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PartyShown {
    pub hosting: bool,
    pub code: String,
    pub link: String,
    pub page: Option<String>,
    pub me: DeviceId,
    pub reach: PartyReach,
    pub people: Vec<PartyPerson>,
    pub now: Option<PartyNow>,
    pub asked_for: Vec<PartyWantingASong>,
    pub round_trip: Option<Duration>,
}

#[derive(Clone)]
enum Asked {
    Now(PartyNow),
    Ask(PartySong),
    Kick(DeviceId),
    TakeAsk(PartyWantingASong),
    Named(String),
    Leave,
}

pub struct Party {
    asked: mpsc::UnboundedSender<Asked>,
    shown: Arc<RwLock<PartyShown>>,
    clock: Arc<RwLock<ServerClock>>,
    claimed: Mutex<Vec<PartyWantingASong>>,
    thread: Option<JoinHandle<()>>,
}

fn taking_ask(shown: &RwLock<PartyShown>, request: &PartyWantingASong) {
    let mut shown = shown.write();
    if let Some(at) = shown.asked_for.iter().position(|asked| asked == request) {
        shown.asked_for.remove(at);
    }
}

pub struct RequestClaim<'a> {
    party: &'a Party,
    request: PartyWantingASong,
}

impl Drop for RequestClaim<'_> {
    fn drop(&mut self) {
        self.party
            .claimed
            .lock()
            .retain(|claimed| *claimed != self.request);
    }
}

impl Party {
    pub fn host(settings: PartySettings) -> Result<Self> {
        let proves_the_host = Token(random_bytes()?);
        let key = PartyKey::hosted_by(&proves_the_host)?;

        Self::begin(settings, key, Some(proves_the_host))
    }

    pub fn join(settings: PartySettings, code: &str) -> Result<Self> {
        let key = PartyKey::from_code(code)?;

        Self::begin(settings, key, None)
    }

    fn begin(settings: PartySettings, key: PartyKey, host: Option<Token>) -> Result<Self> {
        let me = DeviceId::minted()?;
        let hosted_elsewhere =
            (settings.server != crate::HOSTED_SERVER).then(|| settings.server.clone());
        let shown = Arc::new(RwLock::new(PartyShown {
            hosting: host.is_some(),
            code: key.code(),
            link: key.link(hosted_elsewhere.as_deref()),
            page: key.web_link(hosted_elsewhere.as_deref()),
            me,
            reach: PartyReach::Connecting,
            people: Vec::new(),
            now: None,
            asked_for: Vec::new(),
            round_trip: None,
        }));
        let clock = Arc::new(RwLock::new(ServerClock::default()));
        let (asked, hearing) = mpsc::unbounded_channel();
        let roots = trusted_roots(settings.trusted.as_deref())?;
        let running = Running {
            keys: key.keys()?,
            me,
            seat: Token(random_bytes()?),
            host,
            name: settings.name,
            server: settings.server,
            to_web: to_the_web(roots.clone())?,
            to_server: to_the_server(roots)?,
            endpoint: None,
            now_nanos: settings.now_nanos,
            shown: Arc::clone(&shown),
            clock: Arc::clone(&clock),
            names: Vec::new(),
            members: Vec::new(),
            host_id: None,
            told: None,
            waiting: VecDeque::new(),
            closes_before_welcome: 0,
            people_told: PeopleTold::default(),
        };
        let thread = thread::Builder::new()
            .name("resonate-party".to_owned())
            .spawn(move || {
                let built = runtime::Builder::new_current_thread().enable_all().build();
                match built {
                    Ok(runtime) => runtime.block_on(running.run(hearing)),
                    Err(error) => tracing::error!(%error, "a party could not start"),
                }
            })
            .map_err(Error::Thread)?;

        Ok(Self {
            asked,
            shown,
            clock,
            claimed: Mutex::new(Vec::new()),
            thread: Some(thread),
        })
    }

    pub fn shown(&self) -> PartyShown {
        self.shown.read().clone()
    }

    pub fn to_server(&self, here: u64) -> Option<u64> {
        self.clock.read().to_server(here)
    }

    pub fn from_server(&self, there: u64) -> Option<u64> {
        self.clock.read().from_server(there)
    }

    pub fn tell(&self, now: PartyNow) {
        let _ = self.asked.send(Asked::Now(now));
    }

    pub fn ask_for(&self, song: PartySong) {
        let _ = self.asked.send(Asked::Ask(song));
    }

    pub fn claim_a_request(&self, request: &PartyWantingASong) -> Option<RequestClaim<'_>> {
        let mut claimed = self.claimed.lock();
        if claimed.contains(request) {
            return None;
        }
        claimed.push(request.clone());

        Some(RequestClaim {
            party: self,
            request: request.clone(),
        })
    }

    pub fn take_a_request(&self, request: PartyWantingASong) {
        taking_ask(&self.shown, &request);
        let _ = self.asked.send(Asked::TakeAsk(request));
    }

    pub fn turn_out(&self, member: DeviceId) {
        let _ = self.asked.send(Asked::Kick(member));
    }

    pub fn named(&self, name: String) {
        let _ = self.asked.send(Asked::Named(name));
    }

    pub fn leave(&mut self) {
        let _ = self.asked.send(Asked::Leave);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Party {
    fn drop(&mut self) {
        self.leave();
    }
}

struct Running {
    keys: PartyKeys,
    me: DeviceId,
    seat: Token,
    host: Option<Token>,
    name: String,
    server: String,
    to_web: Arc<rustls::ClientConfig>,
    to_server: ClientConfig,
    endpoint: Option<Endpoint>,
    now_nanos: Arc<dyn Fn() -> u64 + Send + Sync>,
    shown: Arc<RwLock<PartyShown>>,
    clock: Arc<RwLock<ServerClock>>,
    names: Vec<(DeviceId, String)>,
    members: Vec<DeviceId>,
    host_id: Option<DeviceId>,
    told: Option<PartyNow>,
    waiting: VecDeque<Asked>,
    closes_before_welcome: u32,
    people_told: PeopleTold,
}

#[derive(Default)]
struct PeopleTold {
    last: Option<(Instant, Vec<(DeviceId, String)>)>,
    owed: bool,
}

impl PeopleTold {
    fn due_to_tell(&mut self, names: &[(DeviceId, String)], now: Instant) -> bool {
        match &self.last {
            Some((_, told)) if told == names => {
                self.owed = false;
                return false;
            }
            Some((at, _)) if now.saturating_duration_since(*at) < PEOPLE_TOLD_AT_MOST_EVERY => {
                self.owed = true;
                return false;
            }
            _ => {}
        }

        self.last = Some((now, names.to_vec()));
        self.owed = false;
        true
    }
}

enum Ended {
    Lost,
    Over(PartyReach),
    Left,
}

impl Running {
    async fn run(mut self, mut hearing: mpsc::UnboundedReceiver<Asked>) {
        let mut retry = FIRST_RETRY;

        loop {
            let began = Instant::now();
            match self.session(&mut hearing).await {
                Ok(Ended::Left) => return,
                Ok(Ended::Over(reach)) => {
                    self.shown.write().reach = reach;
                    self.wait_until_left(&mut hearing).await;
                    return;
                }
                Ok(Ended::Lost) => {}
                Err(error) => tracing::debug!(%error, "a party lost its server"),
            }

            self.shown.write().reach = PartyReach::Reconnecting;
            if began.elapsed() > LAST_RETRY {
                retry = FIRST_RETRY;
            }
            let until = sleep(retry);
            tokio::pin!(until);
            loop {
                tokio::select! {
                    () = &mut until => break,
                    asked = hearing.recv() => {
                        match asked {
                            None | Some(Asked::Leave) => return,
                            Some(asked) => self.remember(asked),
                        }
                    }
                }
            }
            retry = (retry * 2).min(LAST_RETRY);
        }
    }

    async fn wait_until_left(&self, hearing: &mut mpsc::UnboundedReceiver<Asked>) {
        while let Some(asked) = hearing.recv().await {
            if matches!(asked, Asked::Leave) {
                return;
            }
        }
    }

    fn remember(&mut self, asked: Asked) {
        match asked {
            Asked::Now(now) => self.told = Some(now),
            Asked::Named(name) => self.name = name,
            Asked::TakeAsk(request) => self.take_ask(&request),
            Asked::Ask(_) | Asked::Kick(_) => self.keep_for_the_welcome(asked),
            Asked::Leave => {}
        }
    }

    fn keep_for_the_welcome(&mut self, asked: Asked) {
        if self.waiting.len() == SAID_WHILE_AWAY_AT_MOST {
            self.waiting.pop_front();
        }
        self.waiting.push_back(asked);
    }

    fn take_ask(&self, request: &PartyWantingASong) {
        taking_ask(&self.shown, request);
    }

    async fn send_what_waited(&mut self, send: &mut Outgoing) -> Result<()> {
        while let Some(asked) = self.waiting.pop_front() {
            if let Err(error) = self.asked(asked.clone(), send).await {
                self.waiting.push_front(asked);
                return Err(error);
            }
        }

        Ok(())
    }

    async fn reached(&mut self) -> Result<(Outgoing, Incoming)> {
        if let Some(web) = web_address(&self.server) {
            return within(connect(&web, &self.to_web)).await;
        }

        let (address, host) = within(resolve(&self.server)).await?;
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

    fn room(&self) -> SpaceId {
        self.keys.room
    }

    async fn say(&self, send: &mut Outgoing, to: Option<DeviceId>, said: &PartySaid) -> Result<()> {
        let sealed = sealed(&self.keys.sealer, self.room(), &said.encoded())?;

        send.send(&ToServer::RoomSay { to, sealed }.encoded()).await
    }

    fn publish_people(&self) {
        let people = self
            .members
            .iter()
            .map(|member| PartyPerson {
                member: *member,
                name: self
                    .names
                    .iter()
                    .find(|(named, _)| named == member)
                    .map_or(HOST_NAME_FOR_NO_NAME, |(_, name)| name.as_str())
                    .to_owned(),
                host: self.host_id == Some(*member),
                me: *member == self.me,
            })
            .collect();

        self.shown.write().people = people;
    }

    async fn tell_everyone_who_is_here(&mut self, send: &mut Outgoing) -> Result<()> {
        if self.host.is_none() || !self.people_told.due_to_tell(&self.names, Instant::now()) {
            return Ok(());
        }

        self.say(send, None, &PartySaid::People(self.names.clone()))
            .await
    }

    fn set_own_name(&mut self) {
        self.names.retain(|(member, _)| *member != self.me);
        self.names.push((self.me, self.name.clone()));
    }

    async fn session(&mut self, hearing: &mut mpsc::UnboundedReceiver<Asked>) -> Result<Ended> {
        let (mut send, mut recv) = self.reached().await?;

        send.send(
            &ToServer::Room {
                version: PROTOCOL_VERSION,
                room: self.room(),
                token: self.keys.token.clone(),
                host: self.host.clone(),
                member: self.me,
                seat: Some(self.seat.clone()),
            }
            .encoded(),
        )
        .await?;

        let first = within(recv.read()).await?;
        match first.as_deref().map(FromServer::decoded).transpose()? {
            Some(FromServer::RoomWelcome { members, host }) => {
                self.members = members;
                if !self.members.contains(&self.me) {
                    self.members.insert(0, self.me);
                }
                self.host_id = Some(host);
                self.closes_before_welcome = 0;
            }
            Some(FromServer::Refused(Refusal::NoSuchRoom)) => {
                return Ok(Ended::Over(PartyReach::NoSuchParty));
            }
            Some(FromServer::Refused(Refusal::Token)) => {
                return Ok(Ended::Over(PartyReach::TurnedAway));
            }
            Some(FromServer::Refused(_)) => return Ok(Ended::Lost),
            Some(_) => return Ok(Ended::Lost),
            None => {
                self.closes_before_welcome += 1;
                return Ok(
                    if self.closes_before_welcome >= CLOSES_BEFORE_WELCOME_FOR_TOO_OLD {
                        Ended::Over(PartyReach::ServerTooOld)
                    } else {
                        Ended::Lost
                    },
                );
            }
        }

        self.people_told = PeopleTold::default();
        self.set_own_name();
        self.shown.write().reach = PartyReach::Here;
        self.publish_people();

        if self.host.is_some() {
            self.tell_everyone_who_is_here(&mut send).await?;
            if let Some(told) = self.told.clone() {
                self.say(&mut send, None, &PartySaid::Now(told)).await?;
            }
        } else if let Some(host) = self.host_id {
            let hello = PartySaid::Hello {
                name: self.name.clone(),
            };
            self.say(&mut send, Some(host), &hello).await?;
        }
        self.send_what_waited(&mut send).await?;

        let mut pings = interval(PINGS_EVERY);
        let (frames_in, mut frames) = mpsc::unbounded_channel();
        let reading = tokio::spawn(async move {
            loop {
                match recv.read().await {
                    Ok(Some(frame)) => {
                        if frames_in.send(frame).is_err() {
                            return;
                        }
                    }
                    Ok(None) | Err(_) => return,
                }
            }
        });

        let ended = loop {
            tokio::select! {
                _ = pings.tick() => {
                    let nonce = (self.now_nanos)();
                    if send.send(&ToServer::RoomPing { nonce }.encoded()).await.is_err() {
                        break Ended::Lost;
                    }
                    if self.people_told.owed
                        && self.tell_everyone_who_is_here(&mut send).await.is_err()
                    {
                        break Ended::Lost;
                    }
                }
                frame = frames.recv() => {
                    let Some(frame) = frame else {
                        break Ended::Lost;
                    };
                    match self.heard(&frame, &mut send).await {
                        Ok(None) => {}
                        Ok(Some(ended)) => break ended,
                        Err(error) => {
                            tracing::debug!(%error, "a party's frame could not be read");
                        }
                    }
                }
                asked = hearing.recv() => {
                    let Some(asked) = asked else {
                        break Ended::Left;
                    };
                    let again = matches!(asked, Asked::Ask(_) | Asked::Kick(_)).then(|| asked.clone());
                    match self.asked(asked, &mut send).await {
                        Ok(None) => {}
                        Ok(Some(ended)) => break ended,
                        Err(_) => {
                            if let Some(again) = again {
                                self.keep_for_the_welcome(again);
                            }
                            break Ended::Lost;
                        }
                    }
                }
            }
        };
        reading.abort();
        send.finish().await;

        Ok(ended)
    }

    async fn asked(&mut self, asked: Asked, send: &mut Outgoing) -> Result<Option<Ended>> {
        match asked {
            Asked::Leave => return Ok(Some(Ended::Left)),
            Asked::Now(now) => {
                if self.host.is_some() {
                    self.say(send, None, &PartySaid::Now(now.clone())).await?;
                    self.shown.write().now = Some(now.clone());
                    self.told = Some(now);
                }
            }
            Asked::Ask(song) => {
                if let (None, Some(host)) = (&self.host, self.host_id) {
                    self.say(send, Some(host), &PartySaid::Ask(song)).await?;
                }
            }
            Asked::TakeAsk(request) => self.take_ask(&request),
            Asked::Kick(member) => {
                if self.host.is_some() {
                    send.send(&ToServer::RoomKick { member }.encoded()).await?;
                }
            }
            Asked::Named(name) => {
                self.name = name;
                self.set_own_name();
                self.publish_people();
                if self.host.is_some() {
                    self.tell_everyone_who_is_here(send).await?;
                } else if let Some(host) = self.host_id {
                    let hello = PartySaid::Hello {
                        name: self.name.clone(),
                    };
                    self.say(send, Some(host), &hello).await?;
                }
            }
        }

        Ok(None)
    }

    async fn heard(&mut self, frame: &[u8], send: &mut Outgoing) -> Result<Option<Ended>> {
        match FromServer::decoded(frame)? {
            FromServer::RoomPong { nonce, at_nanos } => {
                let heard_at = (self.now_nanos)();
                let mut clock = self.clock.write();
                clock.answered(nonce, at_nanos, heard_at);
                self.shown.write().round_trip = clock.round_trip();
            }
            FromServer::RoomArrived { member } => {
                if !self.members.contains(&member) {
                    self.members.push(member);
                }
                self.publish_people();
                if self.host.is_some() {
                    self.tell_everyone_who_is_here(send).await?;
                    if let Some(told) = self.told.clone() {
                        self.say(send, Some(member), &PartySaid::Now(told)).await?;
                    }
                }
            }
            FromServer::RoomLeft { member } => {
                self.members.retain(|one| *one != member);
                self.names.retain(|(named, _)| *named != member);
                self.publish_people();
                if self.host.is_some() {
                    self.tell_everyone_who_is_here(send).await?;
                }
            }
            FromServer::RoomEnded => return Ok(Some(Ended::Over(PartyReach::Ended))),
            FromServer::RoomHeard { from, sealed } => {
                let plain = opened(&self.keys.sealer, self.room(), &sealed, LARGEST_SAID_OPENED)?;
                self.said_to_me(from, PartySaid::decoded(&plain)?, send)
                    .await?;
            }
            _ => {}
        }

        Ok(None)
    }

    async fn said_to_me(
        &mut self,
        from: DeviceId,
        said: PartySaid,
        send: &mut Outgoing,
    ) -> Result<()> {
        let from_the_host = self.host_id == Some(from);
        let hosting = self.host.is_some();

        match said {
            PartySaid::Hello { name } if hosting => {
                self.names.retain(|(member, _)| *member != from);
                self.names.push((from, name));
                self.publish_people();
                self.tell_everyone_who_is_here(send).await?;
            }
            PartySaid::People(people) if from_the_host => {
                self.names = people;
                self.publish_people();
            }
            PartySaid::Now(now) if from_the_host => {
                self.shown.write().now = Some(now);
            }
            PartySaid::Ask(song) if hosting => {
                let mut shown = self.shown.write();
                if shown.asked_for.len() < WANTS_KEPT_AT_MOST {
                    shown.asked_for.push(PartyWantingASong { from, song });
                }
            }
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(of: u8, name: &str) -> (DeviceId, String) {
        (DeviceId([of; 16]), name.to_owned())
    }

    fn asked(from: u8, row: u64, title: &str) -> PartyWantingASong {
        PartyWantingASong {
            from: DeviceId([from; 16]),
            song: PartySong {
                row,
                recording: resonate_core::Recording {
                    mbid: None,
                    title: title.to_owned(),
                    artist: "Artist".to_owned(),
                    album: "Album".to_owned(),
                    length: None,
                },
                isrc: None,
            },
        }
    }

    #[test]
    fn a_request_is_named_by_who_asked_what_and_the_name_reads_back_from_its_bytes() {
        let first = asked(1, 4, "Echoes");
        let same_row_other_song = asked(1, 4, "Meddle");
        let other_guest = asked(2, 4, "Echoes");
        let other_row = asked(1, 5, "Echoes");

        assert_eq!(first.id(), asked(1, 4, "Echoes").id());
        assert_ne!(first.id(), same_row_other_song.id());
        assert_ne!(first.id(), other_guest.id());
        assert_ne!(first.id(), other_row.id());
        assert_eq!(
            RequestId::from_bytes(&first.id().to_bytes()),
            Some(first.id())
        );
        assert_eq!(RequestId::from_bytes(&[0; 31]), None);
    }

    #[test]
    fn people_are_told_again_only_where_a_name_changed_and_at_most_every_few_seconds() {
        let start = Instant::now();
        let ada = vec![named(1, "Ada")];
        let ada_and_bo = vec![named(1, "Ada"), named(2, "Bo")];
        let ada_and_cy = vec![named(1, "Ada"), named(2, "Cy")];
        let mut told = PeopleTold::default();

        assert!(told.due_to_tell(&ada, start));
        assert!(!told.due_to_tell(&ada, start + Duration::from_millis(10)));
        assert!(!told.owed);

        assert!(!told.due_to_tell(&ada_and_bo, start + Duration::from_secs(1)));
        assert!(told.owed);
        assert!(!told.due_to_tell(&ada_and_cy, start + Duration::from_secs(2)));
        assert!(told.owed);

        assert!(told.due_to_tell(
            &ada_and_cy,
            start + PEOPLE_TOLD_AT_MOST_EVERY + Duration::from_millis(1)
        ));
        assert!(!told.owed);
        assert!(!told.due_to_tell(&ada_and_cy, start + Duration::from_secs(60)));
    }

    #[test]
    fn a_guest_repeating_hello_under_the_same_name_is_never_told_to_the_room_twice() {
        let start = Instant::now();
        let names = vec![named(1, "Ada"), named(2, "Bo")];
        let mut told = PeopleTold::default();
        let mut sent = 0;

        for second in 0..60 {
            if told.due_to_tell(&names, start + Duration::from_secs(second)) {
                sent += 1;
            }
        }

        assert_eq!(sent, 1);
    }
}
