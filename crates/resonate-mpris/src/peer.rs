#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpStream},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_core::{PlaylistId, TrackId, ZoneId};
use resonate_engine::{Asleep, Until};

use crate::{
    Described, Error, PipeOp, PlaybackStatus, PlaylistInfo, PlaylistOrder, Reloadable, Result,
    remote::ANSWERS_WITHIN,
    track::{sleep_asked, sleep_timer},
    wire::{Act, Call, Reply, Request, read_frame, write_frame},
};

const LINK_THREAD: &str = "resonate-mpris-link";

pub(crate) trait Peer: Send + Sync {
    fn can_raise(&self) -> Result<bool>;

    fn told(&self, act: Act) -> Result<()>;

    fn seek(&self, micros: i64) -> Result<()>;

    fn set_position(&self, track: TrackId, micros: i64) -> Result<()>;

    fn playback(&self) -> Result<Option<PlaybackStatus>>;

    fn volume(&self) -> Result<f64>;

    fn set_volume(&self, volume: f64) -> Result<()>;

    fn shuffle(&self) -> Result<bool>;

    fn metadata(&self) -> Result<Option<Described>>;

    fn position(&self) -> Result<i64>;

    fn queued(&self) -> Result<Vec<TrackId>>;

    fn play_order(&self) -> Result<Vec<TrackId>>;

    fn described(&self, rows: &[TrackId]) -> Result<Vec<Described>>;

    fn remove_track(&self, track: TrackId) -> Result<()>;

    fn playlists(
        &self,
        order: PlaylistOrder,
        reverse: bool,
        from: u32,
        most: u32,
    ) -> Result<Vec<PlaylistInfo>>;

    fn activate_playlist(&self, playlist: PlaylistId) -> Result<()>;

    fn sleep(&self) -> Result<Option<Asleep>>;

    fn set_sleep(&self, until: Option<Until>) -> Result<()>;

    fn add_tracks(&self, uris: &[String], after: Option<TrackId>, play: bool) -> Result<()>;

    fn relocate(&self, moved: &[(String, String)]) -> Result<()>;

    fn tune(&self, settings: &[(String, String)]) -> Result<()>;

    fn reload(&self, group: Reloadable, settings: &Path) -> Result<()>;

    fn hand_over(&self, to: ZoneId) -> Result<()>;

    fn zone(&self) -> Result<String>;

    fn device(&self) -> Result<String>;

    fn waiting(&self) -> Result<usize>;
}

pub type Sever = Box<dyn FnOnce() + Send>;

pub trait Severable: Read + Write + Send + 'static {
    fn severer(&self) -> io::Result<Sever>;
}

impl Severable for TcpStream {
    fn severer(&self) -> io::Result<Sever> {
        let held = self.try_clone()?;

        Ok(Box::new(move || {
            let _ = held.shutdown(Shutdown::Both);
        }))
    }
}

#[cfg(unix)]
impl Severable for UnixStream {
    fn severer(&self) -> io::Result<Sever> {
        let held = self.try_clone()?;

        Ok(Box::new(move || {
            let _ = held.shutdown(Shutdown::Both);
        }))
    }
}

struct Job {
    request: Vec<u8>,
    reply: Sender<Result<Vec<u8>>>,
    taken: Arc<AtomicBool>,
}

fn first_to_take(taken: &AtomicBool) -> bool {
    !taken.swap(true, Ordering::AcqRel)
}

struct Link {
    jobs: Sender<Job>,
    broken: Arc<AtomicBool>,
    sever: Mutex<Option<Sever>>,
}

impl Link {
    fn over<S: Severable>(stream: S) -> Result<Self> {
        let sever = stream
            .severer()
            .map_err(|source| Error::pipe(PipeOp::Hold, source))?;
        let broken = Arc::new(AtomicBool::new(false));
        let (jobs, queue) = unbounded();

        {
            let broken = Arc::clone(&broken);
            thread::Builder::new()
                .name(LINK_THREAD.to_owned())
                .spawn(move || converse(stream, &queue, &broken))
                .map_err(|source| Error::pipe(PipeOp::Spawn, source))?;
        }

        Ok(Self {
            jobs,
            broken,
            sever: Mutex::new(Some(sever)),
        })
    }

    fn exchange(&self, call: Call, request: Vec<u8>) -> Result<Vec<u8>> {
        if self.broken.load(Ordering::Acquire) {
            return Err(severed());
        }

        let (reply, answer) = bounded(1);
        let taken = Arc::new(AtomicBool::new(false));
        let job = Job {
            request,
            reply,
            taken: Arc::clone(&taken),
        };
        if self.jobs.send(job).is_err() {
            self.broken.store(true, Ordering::Release);
            return Err(severed());
        }

        match answer.recv_timeout(ANSWERS_WITHIN) {
            Ok(answered) => answered,
            Err(RecvTimeoutError::Disconnected) => {
                self.broken.store(true, Ordering::Release);
                Err(severed())
            }
            Err(RecvTimeoutError::Timeout) => {
                self.broken.store(true, Ordering::Release);
                if first_to_take(&taken) {
                    return Err(Error::pipe(PipeOp::Write, io::ErrorKind::TimedOut.into()));
                }
                self.cut();
                Err(Error::Unconfirmed { call })
            }
        }
    }

    fn cut(&self) {
        if let Some(sever) = self.sever.lock().take() {
            sever();
        }
    }
}

fn severed() -> Error {
    Error::pipe(PipeOp::Read, io::ErrorKind::BrokenPipe.into())
}

fn converse<S: Read + Write>(mut stream: S, queue: &Receiver<Job>, broken: &AtomicBool) {
    for job in queue {
        if broken.load(Ordering::Acquire) {
            break;
        }
        if !first_to_take(&job.taken) {
            continue;
        }
        let answered = write_frame(&mut stream, &job.request).and_then(|()| {
            read_frame(&mut stream)?
                .ok_or_else(|| Error::pipe(PipeOp::Read, io::ErrorKind::UnexpectedEof.into()))
        });
        let failed = answered.is_err();
        let _ = job.reply.send(answered);
        if failed {
            break;
        }
    }
}

pub(crate) struct PipePeer {
    link: Link,
}

impl PipePeer {
    pub(crate) fn over<S: Severable>(stream: S) -> Result<Self> {
        Ok(Self {
            link: Link::over(stream)?,
        })
    }

    fn ask(&self, request: &Request) -> Result<Reply> {
        let answered = self.link.exchange(request.call(), request.encode())?;

        Ok(Reply::decode(&answered)?)
    }
}

impl Peer for PipePeer {
    fn can_raise(&self) -> Result<bool> {
        self.ask(&Request::CanRaise)?.flag(Call::CanRaise)
    }

    fn told(&self, act: Act) -> Result<()> {
        self.ask(&Request::Told(act))?.done(Call::Told)
    }

    fn seek(&self, micros: i64) -> Result<()> {
        self.ask(&Request::Seek(micros))?.done(Call::Seek)
    }

    fn set_position(&self, track: TrackId, micros: i64) -> Result<()> {
        self.ask(&Request::SetPosition { track, micros })?
            .done(Call::SetPosition)
    }

    fn playback(&self) -> Result<Option<PlaybackStatus>> {
        let status = self.ask(&Request::Playback)?.text(Call::Playback)?;

        Ok(PlaybackStatus::read(&status))
    }

    fn volume(&self) -> Result<f64> {
        self.ask(&Request::Volume)?.real(Call::Volume)
    }

    fn set_volume(&self, volume: f64) -> Result<()> {
        self.ask(&Request::SetVolume(volume))?.done(Call::SetVolume)
    }

    fn shuffle(&self) -> Result<bool> {
        self.ask(&Request::Shuffle)?.flag(Call::Shuffle)
    }

    fn metadata(&self) -> Result<Option<Described>> {
        self.ask(&Request::Metadata)?.row(Call::Metadata)
    }

    fn position(&self) -> Result<i64> {
        self.ask(&Request::Position)?.micros(Call::Position)
    }

    fn queued(&self) -> Result<Vec<TrackId>> {
        self.ask(&Request::Queued)?.tracks(Call::Queued)
    }

    fn play_order(&self) -> Result<Vec<TrackId>> {
        self.ask(&Request::PlayOrder)?.tracks(Call::PlayOrder)
    }

    fn described(&self, rows: &[TrackId]) -> Result<Vec<Described>> {
        self.ask(&Request::Described(rows.to_vec()))?
            .rows(Call::Described)
    }

    fn remove_track(&self, track: TrackId) -> Result<()> {
        self.ask(&Request::RemoveTrack(track))?
            .done(Call::RemoveTrack)
    }

    fn playlists(
        &self,
        order: PlaylistOrder,
        reverse: bool,
        from: u32,
        most: u32,
    ) -> Result<Vec<PlaylistInfo>> {
        self.ask(&Request::Playlists {
            order,
            reverse,
            from,
            most,
        })?
        .listed(Call::Playlists)
    }

    fn activate_playlist(&self, playlist: PlaylistId) -> Result<()> {
        self.ask(&Request::ActivatePlaylist(playlist))?
            .done(Call::ActivatePlaylist)
    }

    fn sleep(&self) -> Result<Option<Asleep>> {
        let (mode, seconds) = self.ask(&Request::Sleep)?.sleeping(Call::Sleep)?;

        Ok(sleep_timer(&mode, seconds))
    }

    fn set_sleep(&self, until: Option<Until>) -> Result<()> {
        let (mode, seconds) = sleep_asked(until);

        self.ask(&Request::SetSleep { mode, seconds })?
            .done(Call::SetSleep)
    }

    fn add_tracks(&self, uris: &[String], after: Option<TrackId>, play: bool) -> Result<()> {
        self.ask(&Request::AddTracks {
            uris: uris.to_vec(),
            after,
            play,
        })?
        .done(Call::AddTracks)
    }

    fn relocate(&self, moved: &[(String, String)]) -> Result<()> {
        self.ask(&Request::Relocate(moved.to_vec()))?
            .done(Call::Relocate)
    }

    fn tune(&self, settings: &[(String, String)]) -> Result<()> {
        self.ask(&Request::Tune(settings.to_vec()))?
            .done(Call::Tune)
    }

    fn reload(&self, group: Reloadable, settings: &Path) -> Result<()> {
        self.ask(&Request::Reload {
            group: group.name().to_owned(),
            settings: settings.to_string_lossy().into_owned(),
        })?
        .done(Call::Reload)
    }

    fn hand_over(&self, to: ZoneId) -> Result<()> {
        self.ask(&Request::HandOver(to.get()))?.done(Call::HandOver)
    }

    fn zone(&self) -> Result<String> {
        self.ask(&Request::Zone)?.text(Call::Zone)
    }

    fn device(&self) -> Result<String> {
        self.ask(&Request::Device)?.text(Call::Device)
    }

    fn waiting(&self) -> Result<usize> {
        let waiting = self.ask(&Request::Waiting)?.count(Call::Waiting)?;

        Ok(usize::try_from(waiting).unwrap_or(usize::MAX))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::wire::WireFault;

    struct Watched {
        stream: UnixStream,
        dropped: Sender<()>,
    }

    impl Read for Watched {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.stream.read(buffer)
        }
    }

    impl Write for Watched {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.stream.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.stream.flush()
        }
    }

    impl Severable for Watched {
        fn severer(&self) -> io::Result<Sever> {
            self.stream.severer()
        }
    }

    impl Drop for Watched {
        fn drop(&mut self) {
            let _ = self.dropped.send(());
        }
    }

    fn answering_with(reply: Vec<u8>) -> PipePeer {
        let (near, mut far) = UnixStream::pair().expect("a pair of sockets");
        thread::spawn(move || {
            while let Ok(Some(_)) = read_frame(&mut far) {
                if write_frame(&mut far, &reply).is_err() {
                    break;
                }
            }
        });

        PipePeer::over(near).expect("a peer")
    }

    #[test]
    fn a_player_that_never_answers_severs_the_link_after_one_wait_and_is_not_read_out_of_turn() {
        let (near, _silent) = UnixStream::pair().expect("a pair of sockets");
        let peer = PipePeer::over(near).expect("a peer");

        let started = Instant::now();
        let first = peer.zone();
        let waited = started.elapsed();
        let second = peer.zone();

        assert!(matches!(
            first,
            Err(Error::Unconfirmed { call: Call::Zone })
        ));
        assert!(waited >= ANSWERS_WITHIN);
        assert!(started.elapsed() < ANSWERS_WITHIN * 2);
        assert!(matches!(
            second,
            Err(Error::Pipe {
                op: PipeOp::Read,
                ..
            })
        ));
    }

    #[test]
    fn a_command_the_player_reads_but_never_answers_is_said_to_have_maybe_landed_and_its_thread_ends()
     {
        let (near, mut far) = UnixStream::pair().expect("a pair of sockets");
        let (dropped, let_go) = bounded(1);
        let (read, heard) = bounded(1);
        thread::spawn(move || {
            let _ = read.send(read_frame(&mut far).is_ok_and(|frame| frame.is_some()));
            let _ = read_frame(&mut far);
        });
        let peer = PipePeer::over(Watched {
            stream: near,
            dropped,
        })
        .expect("a peer");

        let started = Instant::now();
        let asked = peer.told(Act::Next);
        let waited = started.elapsed();
        let released = let_go.recv_timeout(ANSWERS_WITHIN);

        assert_eq!(
            heard.try_recv(),
            Ok(true),
            "the player never read the command"
        );
        assert!(waited < ANSWERS_WITHIN * 2, "{waited:?}");
        assert!(released.is_ok(), "the link's thread still holds the stream");
        let Err(error) = asked else {
            panic!("a command nobody answered was taken as done");
        };
        assert!(
            matches!(error, Error::Unconfirmed { call: Call::Told }),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("may still have been done"),
            "{error}"
        );
    }

    #[test]
    fn a_call_queued_behind_one_that_went_unanswered_is_said_to_have_failed_and_not_to_have_landed()
    {
        let (near, mut far) = UnixStream::pair().expect("a pair of sockets");
        thread::spawn(move || while let Ok(Some(_)) = read_frame(&mut far) {});
        let peer = Arc::new(PipePeer::over(near).expect("a peer"));

        let first = {
            let peer = Arc::clone(&peer);
            thread::spawn(move || peer.told(Act::Next))
        };
        let second = {
            let peer = Arc::clone(&peer);
            thread::spawn(move || peer.told(Act::Previous))
        };
        let answers = [
            first.join().expect("an asker"),
            second.join().expect("an asker"),
        ];

        let unconfirmed = answers
            .iter()
            .filter(|answer| matches!(answer, Err(Error::Unconfirmed { .. })))
            .count();
        let failed = answers
            .iter()
            .filter(|answer| matches!(answer, Err(Error::Pipe { .. })))
            .count();
        assert_eq!((unconfirmed, failed), (1, 1), "{answers:?}");
    }

    #[test]
    fn a_player_that_hangs_up_is_a_closed_pipe_and_not_a_hang() {
        let (near, far) = UnixStream::pair().expect("a pair of sockets");
        drop(far);
        let peer = PipePeer::over(near).expect("a peer");

        let asked = peer.zone();

        assert!(matches!(asked, Err(Error::Pipe { .. })), "{asked:?}");
    }

    #[test]
    fn a_reply_of_a_shape_the_call_is_not_answered_in_is_named_unexpected() {
        let peer = answering_with(Reply::Flag(true).encode());

        let asked = peer.zone();

        assert!(matches!(
            asked,
            Err(Error::Wire(WireFault::Unexpected { call: Call::Zone }))
        ));
    }

    #[test]
    fn a_reply_that_is_not_a_reply_is_named_by_its_tag() {
        let peer = answering_with(vec![250]);

        let asked = peer.zone();

        assert!(matches!(
            asked,
            Err(Error::Wire(WireFault::NoSuchReply { tag: 250 }))
        ));
    }

    #[test]
    fn many_threads_share_one_link_and_each_is_answered_its_own() {
        let peer = Arc::new(answering_with(Reply::Text("Study".to_owned()).encode()));

        let askers: Vec<_> = (0..8)
            .map(|_| {
                let peer = Arc::clone(&peer);
                thread::spawn(move || {
                    (0..50).all(|_| peer.zone().is_ok_and(|zone| zone == "Study"))
                })
            })
            .collect();

        for asker in askers {
            assert!(asker.join().expect("an asker"));
        }
    }
}
