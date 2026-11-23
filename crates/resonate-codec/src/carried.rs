use std::{
    cmp::min,
    io::{self, Read, Seek, SeekFrom},
    mem,
    sync::{Arc, mpsc},
    thread,
    time::Instant,
};

use parking_lot::Mutex;

use crate::source::MediaStream;

struct Asked {
    buffer: Vec<u8>,
    len: usize,
}

enum Answered {
    Read {
        buffer: Vec<u8>,
        read: io::Result<usize>,
    },
    Returned(Box<dyn MediaStream>),
}

pub(crate) struct Carrier(Carrying);

enum Carrying {
    Threaded {
        asks: mpsc::Sender<Asked>,
        answers: Mutex<mpsc::Receiver<Answered>>,
        len: Option<u64>,
        spare: Vec<u8>,
    },
    Inline(Box<dyn MediaStream>),
}

impl Carrier {
    pub(crate) fn over(inner: Box<dyn MediaStream>) -> Self {
        let len = inner.byte_len();
        let (asks, asked) = mpsc::channel();
        let (answer, answers) = mpsc::channel();
        let handed = Arc::new(Mutex::new(Some(inner)));
        let taken = Arc::clone(&handed);

        let spawned = thread::Builder::new()
            .name("resonate-head".to_owned())
            .spawn(move || {
                let Some(inner) = taken.lock().take() else {
                    return;
                };

                drop(taken);
                carry_the_reads(inner, &asked, &answer);
            });

        if let Err(error) = spawned {
            tracing::warn!(%error, "no thread could be started to read a stream's head, so it is read here");
            if let Some(inner) = handed.lock().take() {
                return Self(Carrying::Inline(inner));
            }
        }
        Self(Carrying::Threaded {
            asks,
            answers: Mutex::new(answers),
            len,
            spare: Vec::new(),
        })
    }

    pub(crate) fn read_by(
        &mut self,
        into: &mut [u8],
        deadline: Instant,
    ) -> Option<io::Result<usize>> {
        let (asks, answers, spare) = match &mut self.0 {
            Carrying::Inline(inner) => return Some(inner.read(into)),
            Carrying::Threaded {
                asks,
                answers,
                spare,
                ..
            } => (asks, answers, spare),
        };
        let asked = Asked {
            buffer: mem::take(spare),
            len: into.len(),
        };

        if asks.send(asked).is_err() {
            return Some(Err(gone()));
        }

        let left = deadline.saturating_duration_since(Instant::now());
        match answers.get_mut().recv_timeout(left) {
            Ok(Answered::Read { buffer, read }) => {
                let read = read.map(|read| {
                    let taken = min(read, into.len());

                    into[..taken].copy_from_slice(&buffer[..taken]);
                    taken
                });

                *spare = buffer;
                Some(read)
            }
            Ok(Answered::Returned(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                Some(Err(gone()))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => None,
        }
    }

    pub(crate) fn handed_on(self) -> Box<dyn MediaStream> {
        match self.0 {
            Carrying::Inline(inner) => inner,
            Carrying::Threaded { answers, len, .. } => Box::new(Carried {
                answers,
                landed: Vec::new(),
                at: 0,
                inner: None,
                len,
            }),
        }
    }
}

fn carry_the_reads(
    mut inner: Box<dyn MediaStream>,
    asked: &mpsc::Receiver<Asked>,
    answer: &mpsc::Sender<Answered>,
) {
    while let Ok(Asked { mut buffer, len }) = asked.recv() {
        buffer.resize(len, 0);

        let read = inner.read(&mut buffer);

        if answer.send(Answered::Read { buffer, read }).is_err() {
            return;
        }
    }

    let _ = answer.send(Answered::Returned(inner));
}

fn gone() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "the thread reading a stream's head is gone",
    )
}

struct Carried {
    answers: Mutex<mpsc::Receiver<Answered>>,
    landed: Vec<u8>,
    at: usize,
    inner: Option<Box<dyn MediaStream>>,
    len: Option<u64>,
}

impl Read for Carried {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        loop {
            if let Some(landed) = self.landed.get(self.at..)
                && !landed.is_empty()
            {
                let taken = min(landed.len(), into.len());

                into[..taken].copy_from_slice(&landed[..taken]);
                self.at += taken;
                return Ok(taken);
            }
            if let Some(inner) = self.inner.as_mut() {
                return inner.read(into);
            }

            match self.answers.get_mut().recv() {
                Ok(Answered::Read { mut buffer, read }) => {
                    let read = read?;

                    if read == 0 {
                        return Ok(0);
                    }
                    buffer.truncate(read);
                    self.landed = buffer;
                    self.at = 0;
                }
                Ok(Answered::Returned(inner)) => self.inner = Some(inner),
                Err(mpsc::RecvError) => return Err(gone()),
            }
        }
    }
}

impl Seek for Carried {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this source cannot seek",
        ))
    }
}

impl MediaStream for Carried {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}
