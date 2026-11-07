mod connection;
mod in_place;
mod playlists;
mod search;
mod tool;

use std::{
    io::{self, Read},
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};
use resonate_core::{MediaLocation, Service, SourceId};
use resonate_fetch::{Patience, Trusted, asking_agent};
use resonate_providers::{
    Choosing, Delivery, Error, Extension, Id, Identity, Obtained, Opened, Opening, Pacing,
    PlaysInPlace, Provider, ProviderOp, Result, Song, THE_SUBSCRIPTION_LAPSED,
};
use ureq::Agent;

use crate::search::Row;
pub use crate::{
    connection::Connection,
    playlists::{Playlist, playlist_id},
    tool::{Cookies, Formats, Tier, Tool, VideoId, stop_the_tools},
};

const YTMUSIC: &str = "ytmusic";
const THE_SERVICE: &str = "https://music.youtube.com";
const ASKED_APART: Duration = Duration::from_secs(2);
const CHECKED_WITH: &str = "9RfVp-GhKfs";
pub(crate) const A_POLL_LASTS: Duration = Duration::from_secs(10 * 60);
const A_TURN_IS_WAITED_ON_FOR: Duration = Duration::from_secs(45);
const A_VIDEO_IS_NAMED_BY: &str = "v=";

#[derive(Default)]
struct OneAtATime {
    taken: Mutex<bool>,
    freed: Condvar,
}

struct Turn(Arc<OneAtATime>);

impl OneAtATime {
    fn turn_within(self: &Arc<Self>, within: Duration) -> Option<Turn> {
        let by = Instant::now() + within;
        let mut taken = self.taken.lock();
        while *taken {
            if self.freed.wait_until(&mut taken, by).timed_out() && *taken {
                return None;
            }
        }
        *taken = true;
        Some(Turn(Arc::clone(self)))
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        *self.0.taken.lock() = false;
        self.0.freed.notify_one();
    }
}

struct Downloading {
    reader: Box<dyn Read + Send>,
    _turn: Turn,
}

impl Read for Downloading {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.reader.read(into)
    }
}

#[derive(Clone, Copy)]
struct AccountChecked {
    at: Instant,
    premium: bool,
}

impl AccountChecked {
    fn still_stands(self) -> bool {
        self.at.elapsed() < A_POLL_LASTS
    }
}

#[derive(Default)]
struct Looked {
    checked: Option<AccountChecked>,
    looking: bool,
}

#[derive(Default)]
struct Account {
    looked: Mutex<Looked>,
    done: Condvar,
}

enum Standing {
    Known(bool),
    Ours(Looking),
}

struct Looking(Arc<Account>);

impl Account {
    fn standing(self: &Arc<Self>) -> Standing {
        let mut looked = self.looked.lock();
        loop {
            if let Some(checked) = looked.checked.filter(|checked| checked.still_stands()) {
                return Standing::Known(checked.premium);
            }
            if !looked.looking {
                looked.looking = true;
                return Standing::Ours(Looking(Arc::clone(self)));
            }
            self.done.wait(&mut looked);
        }
    }
}

impl Looking {
    fn concluded(self, premium: bool) {
        self.0.looked.lock().checked = Some(AccountChecked {
            at: Instant::now(),
            premium,
        });
    }
}

impl Drop for Looking {
    fn drop(&mut self) {
        self.0.looked.lock().looking = false;
        self.0.done.notify_all();
    }
}

fn video_named_by(link: &str) -> Option<VideoId> {
    let (_, query) = link.split_once('?')?;
    query
        .split('#')
        .next()?
        .split('&')
        .find_map(|pair| pair.strip_prefix(A_VIDEO_IS_NAMED_BY))
        .and_then(VideoId::read)
}

pub struct YtMusic {
    source: SourceId,
    music: String,
    tool: Arc<Tool>,
    asking: Agent,
    pacing: Arc<Pacing>,
    client_version: Arc<Mutex<Option<String>>>,
    account: Arc<Account>,
    downloads: Arc<OneAtATime>,
    turn_within: Duration,
}

impl YtMusic {
    pub fn new(tool: Tool) -> Self {
        Self::reaching(tool, THE_SERVICE, Trusted::BuiltInRoots)
    }

    pub fn at(tool: Tool, music: &str) -> Self {
        Self::reaching(tool, music, Trusted::SystemStoreToo)
    }

    fn reaching(tool: Tool, music: &str, trusted: Trusted) -> Self {
        Self {
            source: SourceId::new(YTMUSIC).unwrap_or_else(|_| SourceId::local()),
            music: music.trim().trim_end_matches('/').to_owned(),
            tool: Arc::new(tool),
            asking: asking_agent(Patience::default(), trusted),
            pacing: Arc::new(Pacing::new(ASKED_APART)),
            client_version: Arc::default(),
            account: Arc::default(),
            downloads: Arc::default(),
            turn_within: A_TURN_IS_WAITED_ON_FOR,
        }
    }

    #[must_use]
    pub fn asked_apart(self, apart: Duration) -> Self {
        Self {
            pacing: Arc::new(Pacing::new(apart)),
            ..self
        }
    }

    #[must_use]
    pub fn waiting_for_a_turn_at_most(self, within: Duration) -> Self {
        Self {
            turn_within: within,
            ..self
        }
    }

    pub fn song(&self, video: &VideoId) -> Result<Option<Song>> {
        self.tool.song(video)
    }

    fn key_of(video: &VideoId) -> String {
        format!("track/{}", video.as_str())
    }

    fn found_gone(&self, identity: &Identity, video: &VideoId) -> bool {
        identity.finds_gone(&MediaLocation::new(
            self.source.clone(),
            Self::key_of(video),
        ))
    }

    fn linked(&self, identity: &Identity) -> Option<VideoId> {
        video_named_by(identity.track_on(Service::YoutubeMusic)?)
            .filter(|video| !self.found_gone(identity, video))
    }

    fn listed(&self, identity: &Identity) -> Result<Option<Row>> {
        let mut choosing = Choosing::default();
        let wordings = identity.wordings();
        for words in &wordings {
            for row in self.songs(words)? {
                row.weigh(identity, &mut choosing);
            }
            if choosing.holds_a_taken_listing() {
                break;
            }
        }
        let seen = choosing.seen();
        let taken = choosing
            .ranked()
            .into_iter()
            .find(|(row, _)| !self.found_gone(identity, &row.video));
        match taken {
            Some((row, taken)) => {
                tracing::debug!(
                    video = row.video.as_str(),
                    album = row.album,
                    ?taken,
                    "YouTube Music lists the song"
                );
                Ok(Some(row))
            }
            None => {
                tracing::debug!(
                    title = identity.title,
                    asked = wordings.len(),
                    seen,
                    "YouTube Music lists nothing named as the song"
                );
                Ok(None)
            }
        }
    }

    fn account_still_premium(&self) -> Result<bool> {
        let looking = match self.account.standing() {
            Standing::Known(premium) => return Ok(premium),
            Standing::Ours(looking) => looking,
        };
        let video = VideoId::read(CHECKED_WITH).ok_or(Error::Unreadable {
            provider: self.source.clone(),
            op: ProviderOp::Search,
        })?;
        let premium = self
            .tool
            .formats_standing(&video)?
            .is_none_or(|formats| formats.best.is_some_and(Tier::is_premium));
        looking.concluded(premium);
        Ok(premium)
    }

    fn holds_up(&self, video: &VideoId, tier: Tier) -> Result<()> {
        if tier.is_premium() || !self.tool.holds_cookies() {
            return Ok(());
        }
        if !self.account_still_premium()? {
            return Err(Error::Unwelcome {
                provider: self.source.clone(),
                op: ProviderOp::Search,
                code: THE_SUBSCRIPTION_LAPSED,
            });
        }
        tracing::warn!(
            video = video.as_str(),
            ?tier,
            "this song has no Premium format though the account does, and is taken at the tier it has"
        );
        Ok(())
    }

    fn opening(&self, video: VideoId, tier: Tier, length: Option<u64>) -> Opening {
        let tool = Arc::clone(&self.tool);
        let downloads = Arc::clone(&self.downloads);
        let within = self.turn_within;
        let provider = self.source.clone();
        Opening::new(move || {
            let turn = downloads.turn_within(within).ok_or(Error::Busy {
                provider,
                op: ProviderOp::Download,
            })?;
            Ok(match tool.download(&video, tier)? {
                Opened::Reading { reader, .. } => Opened::Reading {
                    reader: Box::new(Downloading {
                        reader,
                        _turn: turn,
                    }),
                    length,
                },
                Opened::Gone => Opened::Gone,
            })
        })
    }
}

impl Provider for YtMusic {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn last_resort(&self) -> bool {
        true
    }

    fn picture(&self, address: &str) -> Result<Vec<u8>> {
        let cover = Id::new(address).ok_or_else(|| Error::Unreadable {
            provider: self.source.clone(),
            op: ProviderOp::Cover,
        })?;
        PlaysInPlace::cover(self, &cover, 0)
    }

    fn find(&self, identity: &Identity) -> Result<Obtained> {
        let video = match self.linked(identity) {
            Some(video) => video,
            None if !identity.may_be_listed() => return Ok(Obtained::Nothing),
            None => match self.listed(identity)? {
                Some(row) => row.video,
                None => return Ok(Obtained::Nothing),
            },
        };
        let formats = self.tool.formats(&video)?;
        let Some(tier) = formats.best else {
            tracing::debug!(
                video = video.as_str(),
                "YouTube Music offers this song in no format wanted"
            );
            return Ok(Obtained::Nothing);
        };
        self.holds_up(&video, tier)?;
        tracing::info!(
            video = video.as_str(),
            title = identity.title,
            ?tier,
            "YouTube Music offers the song"
        );

        Ok(Obtained::Found(Delivery::Stream {
            key: Self::key_of(&video).into_boxed_str(),
            extension: Extension::new(tier.extension())?,
            opening: self.opening(video, tier, formats.length),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn an_account_checked_stands_for_a_poll_and_no_longer() {
        let checked_ago = |ago: Duration| AccountChecked {
            at: Instant::now()
                .checked_sub(ago)
                .expect("a moment within the clock"),
            premium: true,
        };

        assert!(checked_ago(Duration::ZERO).still_stands());
        assert!(!checked_ago(A_POLL_LASTS).still_stands());
    }

    #[test]
    fn a_lane_waits_on_the_look_in_hand_and_takes_its_answer_without_looking_again() {
        let account = Arc::new(Account::default());
        let Standing::Ours(looking) = account.standing() else {
            panic!("the first lane looks");
        };

        let known = thread::scope(|scope| {
            let waiting =
                scope.spawn(|| matches!(account.standing(), Standing::Known(premium) if premium));
            thread::sleep(Duration::from_millis(100));
            looking.concluded(true);
            waiting.join().expect("the waiting lane")
        });

        assert!(known);
    }

    #[test]
    fn a_lane_waiting_on_a_look_that_failed_looks_itself() {
        let account = Arc::new(Account::default());
        let Standing::Ours(looking) = account.standing() else {
            panic!("the first lane looks");
        };

        let looks_itself = thread::scope(|scope| {
            let waiting = scope.spawn(|| matches!(account.standing(), Standing::Ours(_)));
            thread::sleep(Duration::from_millis(100));
            drop(looking);
            waiting.join().expect("the waiting lane")
        });

        assert!(looks_itself);
    }

    #[test]
    fn the_service_is_asked_unless_another_is_named_and_a_trailing_slash_is_dropped() {
        let named = YtMusic::at(Tool::new(None, Cookies::None), " http://music.local/ ");

        assert_eq!(named.music, "http://music.local");
        assert_eq!(
            YtMusic::new(Tool::new(None, Cookies::None)).music,
            THE_SERVICE
        );
    }

    #[test]
    fn the_song_the_account_is_checked_with_reads_as_a_video() {
        assert!(VideoId::read(CHECKED_WITH).is_some());
    }
}
