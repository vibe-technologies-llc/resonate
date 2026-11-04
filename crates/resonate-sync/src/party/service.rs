use std::{
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use parking_lot::RwLock;
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
        said::{PartyNow, PartySaid, PartySong},
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

enum Asked {
    Now(PartyNow),
    Ask(PartySong),
    Kick(DeviceId),
    TakeAsk(usize),
    Named(String),
    Leave,
}

pub struct Party {
    asked: mpsc::UnboundedSender<Asked>,
    shown: Arc<RwLock<PartyShown>>,
    clock: Arc<RwLock<ServerClock>>,
    thread: Option<JoinHandle<()>>,
}

impl Party {
    pub fn host(settings: PartySettings) -> Result<Self> {
        let key = PartyKey::minted()?;
        let proves_the_host = Token(random_bytes()?);

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

    pub fn take_a_request(&self, at: usize) {
        let _ = self.asked.send(Asked::TakeAsk(at));
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
                Err(Error::ServerTooOld) => {
                    self.shown.write().reach = PartyReach::ServerTooOld;
                    self.wait_until_left(&mut hearing).await;
                    return;
                }
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
            Asked::Ask(_) | Asked::Kick(_) | Asked::TakeAsk(_) | Asked::Leave => {}
        }
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

    async fn tell_everyone_who_is_here(&self, send: &mut Outgoing) -> Result<()> {
        if self.host.is_none() {
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
            }
            .encoded(),
        )
        .await?;

        let first = within(recv.read()).await?;
        match first.as_deref().map(FromServer::decoded).transpose()? {
            Some(FromServer::RoomWelcome { members, host }) => {
                self.members = members;
                self.host_id = Some(host);
            }
            Some(FromServer::Refused(Refusal::NoSuchRoom)) => {
                return Ok(Ended::Over(PartyReach::NoSuchParty));
            }
            Some(FromServer::Refused(Refusal::Token)) => {
                return Ok(Ended::Over(PartyReach::TurnedAway));
            }
            Some(FromServer::Refused(_)) => return Ok(Ended::Lost),
            Some(_) => return Ok(Ended::Lost),
            None => return Err(Error::ServerTooOld),
        }

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
                    match self.asked(asked, &mut send).await {
                        Ok(None) => {}
                        Ok(Some(ended)) => break ended,
                        Err(_) => break Ended::Lost,
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
            Asked::TakeAsk(at) => {
                let mut shown = self.shown.write();
                if at < shown.asked_for.len() {
                    shown.asked_for.remove(at);
                }
            }
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
                let plain = opened(&self.keys.sealer, self.room(), &sealed)?;
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
