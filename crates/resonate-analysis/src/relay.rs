use std::{
    mem, panic,
    sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel},
    thread::ScopedJoinHandle,
};

const SAMPLES_A_HANDOFF: usize = 1 << 16;
const HANDOFFS_IN_FLIGHT: usize = 4;

#[derive(Default)]
struct Handoff {
    samples: Vec<f32>,
    ends: Vec<usize>,
}

impl Handoff {
    fn hold(&mut self, piece: &[f32]) {
        self.samples.extend_from_slice(piece);
        self.ends.push(self.samples.len());
    }

    fn pieces(&self) -> impl Iterator<Item = &[f32]> {
        let mut from = 0;
        self.ends.iter().map(move |end| {
            let piece = &self.samples[from..*end];
            from = *end;
            piece
        })
    }

    fn emptied(mut self) -> Self {
        self.samples.clear();
        self.ends.clear();
        self
    }
}

pub(crate) fn joined<T>(handle: ScopedJoinHandle<'_, T>) -> T {
    handle
        .join()
        .unwrap_or_else(|payload| panic::resume_unwind(payload))
}

pub(crate) struct Sending {
    onward: SyncSender<Handoff>,
    spent: Receiver<Handoff>,
    gathering: Handoff,
}

pub(crate) struct Receiving {
    from: Receiver<Handoff>,
    spent: Sender<Handoff>,
}

pub(crate) fn relay() -> (Sending, Receiving) {
    let (onward, from) = sync_channel(HANDOFFS_IN_FLIGHT);
    let (returned, spent) = channel();
    (
        Sending {
            onward,
            spent,
            gathering: Handoff::default(),
        },
        Receiving {
            from,
            spent: returned,
        },
    )
}

impl Sending {
    pub(crate) fn feed(&mut self, piece: &[f32]) {
        self.gathering.hold(piece);
        if self.gathering.samples.len() >= SAMPLES_A_HANDOFF {
            self.send();
        }
    }

    fn send(&mut self) {
        let next = self.spent.try_recv().unwrap_or_default();
        let full = mem::replace(&mut self.gathering, next);
        if self.onward.send(full).is_err() {
            self.gathering = Handoff::default();
        }
    }
}

impl Drop for Sending {
    fn drop(&mut self) {
        if !self.gathering.ends.is_empty() {
            self.send();
        }
    }
}

impl Receiving {
    pub(crate) fn each(self, mut hear: impl FnMut(&[f32])) {
        for handoff in &self.from {
            for piece in handoff.pieces() {
                hear(piece);
            }
            let _ = self.spent.send(handoff.emptied());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    fn pieces() -> Vec<Vec<f32>> {
        (0..3_000_u16)
            .map(|nth| vec![f32::from(nth); 1 + usize::from(nth % 900)])
            .collect()
    }

    #[test]
    fn every_piece_arrives_whole_and_in_order() {
        let (mut sending, receiving) = relay();
        let mut heard = Vec::new();

        thread::scope(|scope| {
            let hearing = scope.spawn(|| {
                receiving.each(|piece| heard.push(piece.to_vec()));
            });
            for piece in pieces() {
                sending.feed(&piece);
            }
            drop(sending);
            hearing.join().expect("a thread");
        });

        assert_eq!(heard, pieces());
    }

    #[test]
    fn a_receiver_that_is_gone_does_not_hold_the_sender_back() {
        let (mut sending, receiving) = relay();
        drop(receiving);

        for piece in pieces() {
            sending.feed(&piece);
        }
    }
}
