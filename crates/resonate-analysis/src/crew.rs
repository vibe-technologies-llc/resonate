use std::{
    collections::VecDeque,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};
use rayon::{Scope, ThreadPool, ThreadPoolBuilder};

const SAMPLES_A_SLAB_AT_LEAST: usize = 1 << 12;
const BYTES_QUEUED_AT_MOST: usize = 8 << 20;
const STOPPED_LOOKED_FOR_EVERY: Duration = Duration::from_millis(20);
const CORES_KEPT_FROM_A_PASS: usize = 1;
const BACKGROUND_REST_PER_BUSY: u32 = 3;

pub(crate) trait Noting: Send {
    fn note(&mut self, interleaved: &[f32]);

    fn hears_any_pieces_alike(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Crew {
    AsTheMachineAllows,
    Background,
}

impl Crew {
    pub(crate) fn for_watch(watch: &dyn crate::Watching) -> Self {
        if watch.is_background() {
            Self::Background
        } else {
            Self::AsTheMachineAllows
        }
    }

    fn threads(self) -> usize {
        let cores = thread::available_parallelism().map_or(1, usize::from);
        match self {
            Self::AsTheMachineAllows => cores.saturating_sub(CORES_KEPT_FROM_A_PASS),
            Self::Background => 1,
        }
        .max(1)
    }

    fn pool(self) -> Option<&'static ThreadPool> {
        static WHOLE: OnceLock<Option<ThreadPool>> = OnceLock::new();
        static BACKGROUND: OnceLock<Option<ThreadPool>> = OnceLock::new();
        let (held, name) = match self {
            Self::AsTheMachineAllows => (&WHOLE, "resonate-analyse"),
            Self::Background => (&BACKGROUND, "resonate-studying"),
        };
        held.get_or_init(|| {
            ThreadPoolBuilder::new()
                .num_threads(self.threads())
                .thread_name(move |nth| format!("{name}-{nth}"))
                .build()
                .inspect_err(|error| {
                    tracing::warn!(%error, "an analysis runs on the thread that asked for it");
                })
                .ok()
        })
        .as_ref()
    }

    fn rest(self, began: Instant, stopped: &(dyn Fn() -> bool + Sync)) {
        self.rested_by(began, stopped, thread::sleep);
    }

    fn rested_by(
        self,
        began: Instant,
        stopped: &(dyn Fn() -> bool + Sync),
        mut sleep: impl FnMut(Duration),
    ) {
        if self != Self::Background {
            return;
        }
        let rest = began.elapsed() * BACKGROUND_REST_PER_BUSY;
        let mut rested = Duration::ZERO;
        while let Some(left) = rest.checked_sub(rested).filter(|left| !left.is_zero()) {
            if stopped() {
                break;
            }
            let step = left.min(STOPPED_LOOKED_FOR_EVERY);
            sleep(step);
            rested += step;
        }
    }

    const fn slabs_a_second(self) -> usize {
        match self {
            Self::AsTheMachineAllows => 1,
            Self::Background => 4,
        }
    }

    pub(crate) fn fanned<'stages, T>(
        self,
        samples_a_second: usize,
        stages: Vec<&'stages mut (dyn Noting + 'stages)>,
        stopped: &'stages (dyn Fn() -> bool + Sync),
        walk: impl FnOnce(&mut dyn FnMut(&[f32])) -> T,
    ) -> T {
        let Some(pool) = self.pool() else {
            let mut stages = stages;
            return walk(&mut |samples| {
                for stage in &mut stages {
                    if self == Self::Background {
                        resonate_core::gaming::wait(stopped);
                    }
                    let began = Instant::now();
                    stage.note(samples);
                    self.rest(began, stopped);
                }
            });
        };

        let fan = Fan {
            lanes: stages.into_iter().map(Lane::of).collect(),
            crew: self,
            stopped,
            queued: Arc::new(Queued::default()),
            cancelled: AtomicBool::new(false),
        };
        let samples_a_slab =
            (samples_a_second / self.slabs_a_second()).max(SAMPLES_A_SLAB_AT_LEAST);
        pool.in_place_scope(|scope| {
            let mut gathering = Slab::gathering(Arc::clone(&fan.queued));
            let answer = walk(&mut |samples| {
                if self == Self::Background {
                    resonate_core::gaming::wait(stopped);
                }
                gathering.hold(samples);
                if gathering.samples.len() >= samples_a_slab {
                    let next = Slab::gathering(Arc::clone(&fan.queued));
                    fan.send(std::mem::replace(&mut gathering, next), scope);
                }
            });
            fan.send(gathering, scope);
            fan.settled(stopped);
            answer
        })
    }
}

struct Lane<'stages> {
    stage: Mutex<&'stages mut (dyn Noting + 'stages)>,
    waiting: Mutex<Waiting>,
}

#[derive(Default)]
struct Waiting {
    slabs: VecDeque<Arc<Slab>>,
    draining: bool,
}

impl<'stages> Lane<'stages> {
    fn of(stage: &'stages mut (dyn Noting + 'stages)) -> Self {
        Self {
            stage: Mutex::new(stage),
            waiting: Mutex::new(Waiting::default()),
        }
    }

    fn drain<'scope>(&'scope self, fan: &'scope Fan<'stages>, scope: &Scope<'scope>)
    where
        'stages: 'scope,
    {
        if fan.crew == Crew::Background {
            resonate_core::gaming::wait(fan.stopped);
        }
        if (fan.stopped)() {
            fan.cancelled.store(true, Ordering::Relaxed);
        }
        let mut stage = self.stage.lock();
        let slab = self.waiting.lock().slabs.pop_front();
        if let Some(slab) = slab
            && !fan.cancelled.load(Ordering::Relaxed)
        {
            let began = Instant::now();
            if stage.hears_any_pieces_alike() {
                stage.note(&slab.samples);
            } else {
                for piece in slab.pieces() {
                    stage.note(piece);
                }
            }
            fan.crew.rest(began, fan.stopped);
        }
        drop(stage);

        let mut waiting = self.waiting.lock();
        if waiting.slabs.is_empty() {
            waiting.draining = false;
        } else {
            scope.spawn(move |scope| self.drain(fan, scope));
        }
    }
}

struct Fan<'stages> {
    lanes: Vec<Lane<'stages>>,
    crew: Crew,
    stopped: &'stages (dyn Fn() -> bool + Sync),
    queued: Arc<Queued>,
    cancelled: AtomicBool,
}

impl<'stages> Fan<'stages> {
    fn send<'scope>(&'scope self, mut slab: Slab, scope: &Scope<'scope>)
    where
        'stages: 'scope,
    {
        if slab.ends.is_empty() {
            return;
        }
        self.queued.wait_for_room(slab.bytes());
        slab.charged = true;
        let slab = Arc::new(slab);
        for lane in &self.lanes {
            let mut waiting = lane.waiting.lock();
            waiting.slabs.push_back(Arc::clone(&slab));
            if !waiting.draining {
                waiting.draining = true;
                scope.spawn(move |scope| lane.drain(self, scope));
            }
        }
    }

    fn settled(&self, stopped: &(dyn Fn() -> bool + Sync)) {
        while !self.queued.is_empty_within(STOPPED_LOOKED_FOR_EVERY) {
            if stopped() {
                self.cancelled.store(true, Ordering::Relaxed);
            }
        }
    }
}

#[derive(Default)]
struct Queued {
    bytes: Mutex<usize>,
    freed: Condvar,
}

impl Queued {
    fn wait_for_room(&self, more: usize) {
        let mut bytes = self.bytes.lock();
        while *bytes > 0 && *bytes + more > BYTES_QUEUED_AT_MOST {
            self.freed.wait(&mut bytes);
        }
        *bytes += more;
    }

    fn free(&self, less: usize) {
        *self.bytes.lock() -= less;
        self.freed.notify_all();
    }

    fn is_empty_within(&self, patience: Duration) -> bool {
        let mut bytes = self.bytes.lock();
        if *bytes > 0 {
            self.freed.wait_for(&mut bytes, patience);
        }
        *bytes == 0
    }
}

struct Slab {
    samples: Vec<f32>,
    ends: Vec<usize>,
    queued: Arc<Queued>,
    charged: bool,
}

impl Slab {
    fn gathering(queued: Arc<Queued>) -> Self {
        Self {
            samples: Vec::new(),
            ends: Vec::new(),
            queued,
            charged: false,
        }
    }

    fn hold(&mut self, samples: &[f32]) {
        self.samples.extend_from_slice(samples);
        self.ends.push(self.samples.len());
    }

    fn bytes(&self) -> usize {
        self.samples.len() * size_of::<f32>()
    }

    fn pieces(&self) -> impl Iterator<Item = &[f32]> {
        let mut from = 0;
        self.ends.iter().map(move |end| {
            let piece = &self.samples[from..*end];
            from = *end;
            piece
        })
    }
}

impl Drop for Slab {
    fn drop(&mut self) {
        if self.charged {
            self.queued.free(self.bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = 88_200;

    struct Gathered(Vec<Vec<f32>>);

    impl Noting for Gathered {
        fn note(&mut self, interleaved: &[f32]) {
            self.0.push(interleaved.to_vec());
        }
    }

    fn pieces() -> Vec<Vec<f32>> {
        (0..4_000_u16)
            .map(|nth| vec![f32::from(nth); 1 + usize::from(nth % 700)])
            .collect()
    }

    #[test]
    fn background_uses_one_thread_and_rests_between_slabs() {
        assert_eq!(Crew::Background.threads(), 1);
        let began = Instant::now();
        let busy = began - Duration::from_millis(10);
        let mut steps = Vec::new();

        Crew::Background.rested_by(busy, &|| false, |step| steps.push(step));

        assert!(steps.iter().sum::<Duration>() >= Duration::from_millis(30));
        assert!(steps.iter().all(|step| *step <= STOPPED_LOOKED_FOR_EVERY));
        assert!(steps.len() >= 2);
    }

    #[test]
    fn cancellation_skips_background_rest() {
        let began = Instant::now();
        let mut slept = 0;
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let stopped = || {
            asked.fetch_add(1, Ordering::Relaxed);
            true
        };

        Crew::Background.rested_by(began - Duration::from_secs(60), &stopped, |_| slept += 1);

        assert_eq!(slept, 0);
        assert_eq!(asked.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_rest_is_slept_in_steps_and_stopping_between_them_ends_it() {
        let began = Instant::now();
        let stops_after_two = std::sync::atomic::AtomicUsize::new(0);
        let stopped = || stops_after_two.fetch_add(1, Ordering::Relaxed) >= 2;
        let mut slept = 0;

        Crew::Background.rested_by(began - Duration::from_secs(60), &stopped, |_| slept += 1);

        assert_eq!(slept, 2);
    }

    #[test]
    fn the_machines_own_crew_never_rests() {
        let began = Instant::now();
        let mut slept = 0;

        Crew::AsTheMachineAllows
            .rested_by(began - Duration::from_secs(60), &|| false, |_| slept += 1);

        assert_eq!(slept, 0);
    }

    #[test]
    fn every_stage_hears_every_piece_in_order_whatever_the_crew() {
        for crew in [Crew::AsTheMachineAllows, Crew::Background] {
            let (mut first, mut second) = (Gathered(Vec::new()), Gathered(Vec::new()));

            crew.fanned(RATE, vec![&mut first, &mut second], &|| false, |feed| {
                for piece in pieces() {
                    feed(&piece);
                }
            });

            assert_eq!(first.0, pieces());
            assert_eq!(second.0, pieces());
        }
    }

    #[test]
    fn a_stage_that_never_keeps_up_holds_the_walk_back_and_still_hears_all_of_it() {
        struct Slow(usize);

        impl Noting for Slow {
            fn note(&mut self, interleaved: &[f32]) {
                thread::sleep(Duration::from_micros(10));
                self.0 += interleaved.len();
            }
        }

        let mut slow = Slow(0);
        let piece = vec![0.0_f32; 1 << 18];

        Crew::Background.fanned(RATE, vec![&mut slow], &|| false, |feed| {
            for _ in 0..64 {
                feed(&piece);
            }
        });

        assert_eq!(slow.0, 64 * piece.len());
    }

    #[test]
    fn a_stop_noticed_while_settling_leaves_what_was_queued_unheard() {
        struct Gated(usize);

        impl Noting for Gated {
            fn note(&mut self, _: &[f32]) {
                thread::sleep(Duration::from_millis(30));
                self.0 += 1;
            }
        }

        let mut gated = Gated(0);
        let piece = vec![0.0_f32; RATE];

        Crew::AsTheMachineAllows.fanned(RATE, vec![&mut gated], &|| true, |feed| {
            for _ in 0..32 {
                feed(&piece);
            }
        });

        assert!(gated.0 < 32, "{}", gated.0);
    }
}
