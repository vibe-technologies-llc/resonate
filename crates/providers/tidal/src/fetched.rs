use std::{
    collections::VecDeque,
    io::{self, ErrorKind, Read},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
    vec,
};

use resonate_fetch::{Patience, Ranged, Unfetched};
use ureq::Agent;

const SEGMENTS_AHEAD: usize = 3;
const LARGEST_SEGMENT: u64 = 64 * 1024 * 1024;

fn resumed_after() -> Duration {
    Patience::default().resumed_after
}

fn fetched_whole(agent: Agent, url: String) -> io::Result<Vec<u8>> {
    let piece = Ranged::opened(agent, url, resumed_after()).map_err(Unfetched::into_io)?;
    let mut whole = Vec::new();
    piece.take(LARGEST_SEGMENT + 1).read_to_end(&mut whole)?;
    if whole.len() as u64 > LARGEST_SEGMENT {
        return Err(io::Error::from(ErrorKind::FileTooLarge));
    }
    Ok(whole)
}

type Ahead = Receiver<io::Result<Vec<u8>>>;

pub(crate) struct Fetched {
    agent: Agent,
    left: vec::IntoIter<String>,
    piece: Option<Ranged>,
    ahead: VecDeque<Ahead>,
    held: io::Cursor<Vec<u8>>,
}

impl Fetched {
    pub(crate) fn opened(agent: Agent, urls: Vec<String>) -> Result<Self, Unfetched> {
        let mut left = urls.into_iter();
        let piece = left
            .next()
            .map(|url| Ranged::opened(agent.clone(), url, resumed_after()))
            .transpose()?;
        let mut fetched = Self {
            agent,
            left,
            piece,
            ahead: VecDeque::new(),
            held: io::Cursor::new(Vec::new()),
        };
        fetched.read_ahead();
        Ok(fetched)
    }

    fn read_ahead(&mut self) {
        while self.ahead.len() < SEGMENTS_AHEAD {
            let Some(url) = self.left.next() else {
                return;
            };
            let (told, ahead) = mpsc::sync_channel(1);
            let agent = self.agent.clone();
            let fetching = thread::Builder::new()
                .name("resonate-tidal-segment".to_owned())
                .spawn({
                    let told = told.clone();
                    let url = url.clone();
                    move || {
                        let _ = told.send(fetched_whole(agent, url));
                    }
                });
            if let Err(error) = fetching {
                tracing::debug!(%error, "a Tidal segment is fetched in turn, no thread starting for it");
                let _ = told.send(fetched_whole(self.agent.clone(), url));
            }
            self.ahead.push_back(ahead);
        }
    }

    fn read_the_first(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        let Some(piece) = self.piece.as_mut() else {
            return Ok(None);
        };
        match piece.read(buf)? {
            0 if !buf.is_empty() => {
                self.piece = None;
                Ok(None)
            }
            read => Ok(Some(read)),
        }
    }
}

impl Read for Fetched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(read) = self.read_the_first(buf)? {
            return Ok(read);
        }
        loop {
            let read = self.held.read(buf)?;
            if read > 0 || buf.is_empty() {
                return Ok(read);
            }
            let Some(ahead) = self.ahead.pop_front() else {
                return Ok(0);
            };
            let segment = ahead
                .recv()
                .unwrap_or_else(|_| Err(io::Error::from(ErrorKind::BrokenPipe)))?;
            self.held = io::Cursor::new(segment);
            self.read_ahead();
        }
    }
}
