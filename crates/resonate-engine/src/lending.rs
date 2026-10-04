use std::{mem, thread};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use resonate_codec::{DecodeStatus, Decoder, Delivery, PacketSpan};
use resonate_core::{AudioBuffer, Frames};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    Decoded,
    Ended,
    Awaited,
}

impl From<DecodeStatus> for Block {
    fn from(status: DecodeStatus) -> Self {
        match status {
            DecodeStatus::Decoded => Self::Decoded,
            DecodeStatus::EndOfStream => Self::Ended,
        }
    }
}

#[derive(Debug)]
pub(crate) enum Lost {
    Decode(resonate_codec::Error),
    WorkerStopped,
}

pub(crate) struct Returned {
    decoder: Box<Decoder>,
    block: AudioBuffer,
    status: resonate_codec::Result<DecodeStatus>,
}

struct Lent {
    decoder: Box<Decoder>,
    block: AudioBuffer,
}

struct Worker {
    lending: Sender<Lent>,
    returns: Receiver<Returned>,
}

impl Worker {
    fn start() -> Option<Self> {
        let (lending, lent) = crossbeam_channel::bounded::<Lent>(1);
        let (returning, returns) = crossbeam_channel::bounded(1);
        let started = thread::Builder::new()
            .name("resonate-decode".into())
            .spawn(move || {
                for Lent {
                    mut decoder,
                    mut block,
                } in lent
                {
                    let status = decoder.next_block(&mut block);
                    let returned = Returned {
                        decoder,
                        block,
                        status,
                    };
                    if returning.send(returned).is_err() {
                        return;
                    }
                }
            });
        match started {
            Ok(_) => Some(Self { lending, returns }),
            Err(error) => {
                tracing::warn!(%error, "no thread could be started to decode a source that can stall; it decodes in line");
                None
            }
        }
    }
}

enum Holding {
    Home(Box<Decoder>),
    Lent,
    Back(Returned),
    Gone,
}

pub(crate) struct Decoding {
    holding: Holding,
    worker: Option<Worker>,
    position: Frames,
    last_packet: Option<PacketSpan>,
    owed: Option<Delivery>,
}

impl Decoding {
    pub(crate) fn new(decoder: Decoder) -> Self {
        Self {
            position: decoder.position(),
            last_packet: decoder.last_packet(),
            holding: Holding::Home(Box::new(decoder)),
            worker: None,
            owed: None,
        }
    }

    pub(crate) fn home(&mut self) -> Option<&mut Decoder> {
        match &mut self.holding {
            Holding::Home(decoder) => Some(decoder),
            Holding::Lent | Holding::Back(_) | Holding::Gone => None,
        }
    }

    pub(crate) fn position(&self) -> Frames {
        match &self.holding {
            Holding::Home(decoder) => decoder.position(),
            Holding::Lent | Holding::Back(_) | Holding::Gone => self.position,
        }
    }

    pub(crate) fn last_packet(&self) -> Option<PacketSpan> {
        match &self.holding {
            Holding::Home(decoder) => decoder.last_packet(),
            Holding::Lent | Holding::Back(_) | Holding::Gone => self.last_packet,
        }
    }

    pub(crate) fn deliver(&mut self, delivery: Delivery) {
        match &mut self.holding {
            Holding::Home(decoder) => decoder.deliver(delivery),
            Holding::Lent | Holding::Back(_) | Holding::Gone => self.owed = Some(delivery),
        }
    }

    pub(crate) fn awaited(&self) -> Option<&Receiver<Returned>> {
        match self.holding {
            Holding::Lent => self.worker.as_ref().map(|worker| &worker.returns),
            Holding::Home(_) | Holding::Back(_) | Holding::Gone => None,
        }
    }

    pub(crate) fn take_what_came_back(&mut self) {
        if !matches!(self.holding, Holding::Lent) {
            return;
        }
        let Some(worker) = self.worker.as_ref() else {
            self.holding = Holding::Gone;
            return;
        };
        match worker.returns.try_recv() {
            Ok(returned) => self.holding = Holding::Back(returned),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.holding = Holding::Gone,
        }
    }

    pub(crate) fn next_block(
        &mut self,
        out: &mut AudioBuffer,
        may_stall: bool,
    ) -> Result<Block, Lost> {
        self.take_what_came_back();
        match mem::replace(&mut self.holding, Holding::Gone) {
            Holding::Home(decoder) if may_stall => self.lend(decoder, out),
            Holding::Home(decoder) => self.decoded_in_line(decoder, out),
            Holding::Lent => {
                self.holding = Holding::Lent;
                Ok(Block::Awaited)
            }
            Holding::Back(returned) => self.landed(returned, out),
            Holding::Gone => Err(Lost::WorkerStopped),
        }
    }

    fn lend(&mut self, decoder: Box<Decoder>, out: &mut AudioBuffer) -> Result<Block, Lost> {
        if self.worker.is_none() {
            self.worker = Worker::start();
        }
        let Some(worker) = self.worker.as_ref() else {
            return self.decoded_in_line(decoder, out);
        };

        self.position = decoder.position();
        self.last_packet = decoder.last_packet();
        let block = mem::replace(out, AudioBuffer::empty(out.spec()));
        match worker.lending.send(Lent { decoder, block }) {
            Ok(()) => {
                self.holding = Holding::Lent;
                Ok(Block::Awaited)
            }
            Err(refused) => {
                let Lent { decoder, block } = refused.into_inner();
                *out = block;
                self.worker = None;
                self.decoded_in_line(decoder, out)
            }
        }
    }

    fn decoded_in_line(
        &mut self,
        mut decoder: Box<Decoder>,
        out: &mut AudioBuffer,
    ) -> Result<Block, Lost> {
        let status = decoder.next_block(out);
        self.holding = Holding::Home(decoder);
        status.map(Block::from).map_err(Lost::Decode)
    }

    fn landed(&mut self, returned: Returned, out: &mut AudioBuffer) -> Result<Block, Lost> {
        let Returned {
            mut decoder,
            mut block,
            status,
        } = returned;
        if let Some(delivery) = self.owed.take() {
            decoder.deliver(delivery);
            block.retype(delivery.format);
        }
        *out = block;
        self.holding = Holding::Home(decoder);
        status.map(Block::from).map_err(Lost::Decode)
    }
}
