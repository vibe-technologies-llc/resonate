use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use resonate_audio::{SinkChange, SinkId};
use resonate_core::{Frames, SampleRate};

use super::formats::{OutputFormat, OutputMixer, OutputStream};

pub(crate) const OUTPUT: SinkId = SinkId::new(1);
const SHORTEST_DRAIN: Duration = Duration::from_millis(20);
const MILLISECONDS_A_SECOND: f64 = 1000.0;

pub(crate) struct Routing {
    rate: AtomicU32,
    latency: AtomicU64,
    announce: Sender<SinkChange>,
    pub(crate) changes: Receiver<SinkChange>,
    latency_moved: Sender<()>,
    pub(crate) latency_moves: Receiver<()>,
}

impl Routing {
    pub(crate) fn new() -> Self {
        let (announce, changes) = unbounded();
        let (latency_moved, latency_moves) = bounded(1);
        Self {
            rate: AtomicU32::new(SampleRate::HZ_48000.hz()),
            latency: AtomicU64::new(0),
            announce,
            changes,
            latency_moved,
            latency_moves,
        }
    }

    pub(crate) fn select(&self, _: i32) -> bool {
        false
    }

    pub(crate) fn actual(&self) -> i32 {
        0
    }

    pub(crate) fn route_changed(&self, rate: u32, latency_ms: f64) -> bool {
        let Ok(rate) = SampleRate::new(rate) else {
            return false;
        };
        let latency = latency_ms.max(0.0) * f64::from(rate.hz()) / MILLISECONDS_A_SECOND;
        if self.latency.swap(latency as u64, Ordering::Relaxed) != latency as u64 {
            let _ = self.latency_moved.try_send(());
        }
        let moved = self.rate.swap(rate.hz(), Ordering::Relaxed) != rate.hz();
        if moved {
            let _ = self.announce.send(SinkChange::Reformatted(OUTPUT));
        }
        moved
    }

    pub(crate) fn follow_song_rate(&self, _: bool) -> bool {
        false
    }

    pub(crate) fn offer(&self, _: i32, _: &[OutputFormat]) -> bool {
        false
    }

    pub(crate) fn stream(&self) -> Option<OutputStream> {
        None
    }

    pub(crate) fn watch_mixer(&self, _: Arc<dyn OutputMixer>) {}

    pub(crate) fn rate(&self) -> SampleRate {
        SampleRate::new(self.rate.load(Ordering::Relaxed)).unwrap_or(SampleRate::HZ_48000)
    }

    pub(crate) fn latency(&self) -> Frames {
        Frames(self.latency.load(Ordering::Relaxed))
    }
}

pub(crate) struct Drain {
    requested: bool,
    ready_at: Option<Instant>,
}

impl Drain {
    pub(crate) const fn new() -> Self {
        Self {
            requested: false,
            ready_at: None,
        }
    }

    pub(crate) fn request(&mut self) {
        self.requested = true;
    }

    pub(crate) const fn is_requested(&self) -> bool {
        self.requested
    }

    pub(crate) fn settled(&mut self, empty: bool, latency: Duration, now: Instant) -> bool {
        if !(self.requested && empty) {
            self.ready_at = None;
            return false;
        }
        let ready_at = *self
            .ready_at
            .get_or_insert_with(|| now + latency.max(SHORTEST_DRAIN));
        if now < ready_at {
            return false;
        }
        self.requested = false;
        self.ready_at = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Unmixed;

    impl OutputMixer for Unmixed {
        fn prefer_bit_perfect(&self, _: i32, _: OutputFormat) -> bool {
            false
        }

        fn mix_freely(&self, _: i32) {}
    }

    fn changes_waiting(routing: &Routing) -> Vec<SinkChange> {
        routing.changes.try_iter().collect()
    }

    #[test]
    fn a_new_route_is_forty_eight_kilohertz_with_no_latency() {
        let routing = Routing::new();

        assert_eq!(routing.rate(), SampleRate::HZ_48000);
        assert_eq!(routing.latency(), Frames::ZERO);
        assert!(changes_waiting(&routing).is_empty());
    }

    #[test]
    fn a_moved_rate_is_announced_once_and_an_unmoved_one_is_not() {
        let routing = Routing::new();

        assert!(routing.route_changed(44_100, 0.0));
        assert_eq!(routing.rate(), SampleRate::HZ_44100);
        assert!(!routing.route_changed(44_100, 0.0));

        let changes = changes_waiting(&routing);
        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], SinkChange::Reformatted(id) if id == OUTPUT));
    }

    #[test]
    fn a_rate_nobody_can_play_is_refused_and_changes_nothing() {
        let routing = Routing::new();

        assert!(!routing.route_changed(0, 20.0));

        assert_eq!(routing.rate(), SampleRate::HZ_48000);
        assert_eq!(routing.latency(), Frames::ZERO);
        assert!(changes_waiting(&routing).is_empty());
        assert!(routing.latency_moves.try_recv().is_err());
    }

    #[test]
    fn latency_is_kept_in_frames_at_the_reported_rate() {
        let routing = Routing::new();

        routing.route_changed(48_000, 20.0);
        assert_eq!(routing.latency(), Frames(960));

        routing.route_changed(44_100, 10.0);
        assert_eq!(routing.latency(), Frames(441));
    }

    #[test]
    fn a_negative_latency_reads_as_none() {
        let routing = Routing::new();

        routing.route_changed(48_000, -5.0);

        assert_eq!(routing.latency(), Frames::ZERO);
    }

    #[test]
    fn a_moved_latency_wakes_the_output_thread_once_however_often_it_moves() {
        let routing = Routing::new();

        routing.route_changed(48_000, 10.0);
        routing.route_changed(48_000, 20.0);
        routing.route_changed(48_000, 30.0);

        assert!(routing.latency_moves.try_recv().is_ok());
        assert!(routing.latency_moves.try_recv().is_err());
    }

    #[test]
    fn an_unmoved_latency_wakes_nothing() {
        let routing = Routing::new();

        routing.route_changed(48_000, 10.0);
        let _ = routing.latency_moves.try_recv();
        routing.route_changed(48_000, 10.0);

        assert!(routing.latency_moves.try_recv().is_err());
    }

    #[test]
    fn this_route_leaves_the_songs_rate_and_its_formats_to_the_session() {
        let routing = Routing::new();

        assert!(!routing.follow_song_rate(true));
        assert!(!routing.offer(7, &[]));
        routing.watch_mixer(Arc::new(Unmixed));
        assert_eq!(routing.stream(), None);
        assert!(changes_waiting(&routing).is_empty());
    }

    #[test]
    fn this_route_offers_no_selection() {
        let routing = Routing::new();

        assert!(!routing.select(7));
        assert_eq!(routing.actual(), 0);
    }

    #[test]
    fn a_drain_waits_the_latency_after_the_ring_empties_and_then_settles_once() {
        let mut drain = Drain::new();
        let start = Instant::now();
        let latency = Duration::from_millis(100);

        drain.request();

        assert!(!drain.settled(true, latency, start));
        assert!(!drain.settled(true, latency, start + Duration::from_millis(99)));
        assert!(drain.settled(true, latency, start + latency));
        assert!(!drain.is_requested());
        assert!(!drain.settled(true, latency, start + latency * 2));
    }

    #[test]
    fn a_drain_never_waits_less_than_the_shortest() {
        let mut drain = Drain::new();
        let start = Instant::now();

        drain.request();

        assert!(!drain.settled(true, Duration::ZERO, start));
        assert!(!drain.settled(true, Duration::ZERO, start + Duration::from_millis(19)));
        assert!(drain.settled(true, Duration::ZERO, start + SHORTEST_DRAIN));
    }

    #[test]
    fn a_source_that_fills_again_restarts_the_wait() {
        let mut drain = Drain::new();
        let start = Instant::now();
        let latency = Duration::from_millis(100);

        drain.request();
        assert!(!drain.settled(true, latency, start));
        assert!(!drain.settled(false, latency, start + Duration::from_millis(90)));

        assert!(!drain.settled(true, latency, start + Duration::from_millis(120)));
        assert!(!drain.settled(true, latency, start + Duration::from_millis(200)));
        assert!(drain.settled(true, latency, start + Duration::from_millis(220)));
    }

    #[test]
    fn nothing_settles_that_was_never_asked_to_drain() {
        let mut drain = Drain::new();
        let start = Instant::now();

        assert!(!drain.settled(true, Duration::ZERO, start));
        assert!(!drain.settled(true, Duration::ZERO, start + Duration::from_secs(60)));
        assert!(!drain.is_requested());
    }
}
