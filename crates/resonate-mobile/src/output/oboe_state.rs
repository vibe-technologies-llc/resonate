use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use resonate_audio::{SinkChange, SinkId};
use resonate_core::StreamSpec;

use super::{
    formats::{OutputFormat, OutputMixer, OutputStream},
    oboe_offers::{Offers, Way},
};
use crate::foreign::{Callback, answered};

pub(crate) const OUTPUT: SinkId = SinkId::new(1);
const SYSTEM_DEFAULT: i32 = 0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Standing {
    Waiting,
    Answered,
    Abandoned,
}

pub(crate) struct Claim {
    ticket: u64,
    standing: Mutex<Standing>,
}

impl Claim {
    pub(crate) fn abandon(&self) -> bool {
        let mut standing = self.standing.lock();
        if *standing == Standing::Answered {
            return false;
        }
        *standing = Standing::Abandoned;
        true
    }

    pub(crate) fn abandoned(&self) -> bool {
        *self.standing.lock() == Standing::Abandoned
    }
}

#[derive(Clone, Copy)]
struct Preference {
    device: i32,
    held_by: u64,
}

pub(crate) struct Routing {
    requested: AtomicI32,
    actual: AtomicI32,
    follows: AtomicBool,
    offers: Mutex<Offers>,
    probed_for: Mutex<Option<i32>>,
    opened: Mutex<Option<(u64, OutputStream)>>,
    streams_opened: AtomicU64,
    openings_begun: AtomicU64,
    mixer: Mutex<Option<Arc<dyn OutputMixer>>>,
    preferred: Mutex<Option<Preference>>,
    announce: Sender<SinkChange>,
    pub(crate) changes: Receiver<SinkChange>,
    sink: SinkId,
}

impl Routing {
    pub(crate) fn new() -> Self {
        Self::serving(OUTPUT)
    }

    pub(crate) fn serving(sink: SinkId) -> Self {
        let (announce, changes) = unbounded();
        Self {
            sink,
            requested: AtomicI32::new(SYSTEM_DEFAULT),
            actual: AtomicI32::new(SYSTEM_DEFAULT),
            follows: AtomicBool::new(false),
            offers: Mutex::new(Offers::default()),
            probed_for: Mutex::new(None),
            opened: Mutex::new(None),
            streams_opened: AtomicU64::new(0),
            openings_begun: AtomicU64::new(0),
            mixer: Mutex::new(None),
            preferred: Mutex::new(None),
            announce,
            changes,
        }
    }

    pub(crate) fn select(&self, device: i32) -> bool {
        if self.requested.swap(device, Ordering::Relaxed) != device {
            *self.probed_for.lock() = None;
            self.announced();
        }
        true
    }

    pub(crate) const fn sink(&self) -> SinkId {
        self.sink
    }

    pub(crate) fn requested(&self) -> i32 {
        self.requested.load(Ordering::Relaxed)
    }

    pub(crate) fn actual(&self) -> i32 {
        self.actual.load(Ordering::Relaxed)
    }

    pub(crate) fn opened_on(&self, device: i32) {
        self.actual.store(device, Ordering::Relaxed);
    }

    pub(crate) fn route_changed(&self, _: u32, _: f64) -> bool {
        false
    }

    pub(crate) fn follow_song_rate(&self, follows: bool) -> bool {
        let moved = self.follows.swap(follows, Ordering::Relaxed) != follows;
        if moved {
            self.announced();
        }
        moved
    }

    pub(crate) fn follows(&self) -> bool {
        self.follows.load(Ordering::Relaxed)
    }

    pub(crate) fn offer(&self, device: i32, bit_perfect: &[OutputFormat]) -> bool {
        let offered = Offers::of(device, bit_perfect);
        {
            let mut held = self.offers.lock();
            if held.device == offered.device && held.bit_perfect == offered.bit_perfect {
                return false;
            }
            *held = offered;
        }
        *self.probed_for.lock() = None;
        self.announced();
        true
    }

    pub(crate) fn offered(&self, probe: impl FnOnce(i32) -> Option<StreamSpec>) -> Vec<StreamSpec> {
        let follows = self.follows();
        let requested = self.requested();
        if follows {
            let mut probed_for = self.probed_for.lock();
            if *probed_for != Some(requested) {
                let exclusive = probe(requested);
                self.offers.lock().exclusive = exclusive;
                *probed_for = Some(requested);
            }
        }
        self.usable(requested).offered(follows)
    }

    pub(crate) fn way_for(&self, spec: StreamSpec) -> Way {
        self.usable(self.requested()).way_for(spec, self.follows())
    }

    pub(crate) fn exclusive_refused(&self) {
        self.offers.lock().exclusive = None;
        self.announced();
    }

    pub(crate) fn claim(&self) -> Arc<Claim> {
        Arc::new(Claim {
            ticket: self.openings_begun.fetch_add(1, Ordering::Relaxed),
            standing: Mutex::new(Standing::Waiting),
        })
    }

    pub(crate) fn answer(
        &self,
        claim: &Claim,
        device: i32,
        stream: OutputStream,
        tell: impl FnOnce(),
    ) -> Option<u64> {
        let mut standing = claim.standing.lock();
        if *standing == Standing::Abandoned {
            return None;
        }
        self.opened_on(device);
        let which = self.opened(stream);
        tell();
        *standing = Standing::Answered;
        Some(which)
    }

    pub(crate) fn opened(&self, stream: OutputStream) -> u64 {
        let which = self.streams_opened.fetch_add(1, Ordering::Relaxed);
        *self.opened.lock() = Some((which, stream));
        which
    }

    pub(crate) fn closed(&self, which: u64) {
        let was_current = {
            let mut opened = self.opened.lock();
            let current = opened.is_some_and(|(held, _)| held == which);
            if current {
                *opened = None;
            }
            current
        };
        if was_current {
            self.mix_freely();
        }
    }

    pub(crate) fn stream(&self) -> Option<OutputStream> {
        self.opened.lock().map(|(_, stream)| stream)
    }

    pub(crate) fn watch_mixer(&self, mixer: Arc<dyn OutputMixer>) {
        *self.mixer.lock() = Some(mixer);
    }

    pub(crate) fn prefer_bit_perfect(&self, claim: &Claim, device: i32, spec: StreamSpec) -> bool {
        let Some(format) = OutputFormat::of(spec) else {
            return false;
        };
        let Some(mixer) = self.mixer.lock().clone() else {
            return false;
        };
        let mut held = self.preferred.lock();
        if claim.abandoned() {
            return false;
        }
        let preferred = answered(Callback::PreferBitPerfect, || {
            mixer.prefer_bit_perfect(device, format)
        })
        .unwrap_or(false);
        let before = held.take();
        *held = preferred.then_some(Preference {
            device,
            held_by: claim.ticket,
        });
        if let Some(before) = before.filter(|before| !preferred || before.device != device) {
            answered(Callback::MixFreely, || mixer.mix_freely(before.device));
        }
        preferred
    }

    pub(crate) fn mix_freely(&self) {
        self.given_back(|_| true);
    }

    pub(crate) fn mix_freely_for(&self, claim: &Claim) {
        self.given_back(|_| !claim.abandoned());
    }

    pub(crate) fn give_back(&self, claim: &Claim) {
        self.given_back(|preference| preference.held_by == claim.ticket);
    }

    fn given_back(&self, whose: impl FnOnce(Preference) -> bool) {
        let mut held = self.preferred.lock();
        let Some(preference) = held.filter(|preference| whose(*preference)) else {
            return;
        };
        *held = None;
        let mixer = self.mixer.lock().clone();
        if let Some(mixer) = mixer {
            answered(Callback::MixFreely, || mixer.mix_freely(preference.device));
        }
    }

    fn usable(&self, requested: i32) -> Offers {
        let mut offers = self.offers.lock().clone();
        if requested != SYSTEM_DEFAULT && requested != offers.device {
            offers.bit_perfect.clear();
        }
        offers
    }

    fn announced(&self) {
        let _ = self.announce.send(SinkChange::Reformatted(self.sink));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

    use super::*;
    use crate::output::{
        formats::{OutputEncoding, OutputPath},
        oboe_offers::MIXED,
    };

    const USB: i32 = 31;
    const CD: StreamSpec = StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::S16,
    );
    const NATIVE: StreamSpec = StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Stereo,
        SampleFormat::S32,
    );

    fn cd() -> OutputFormat {
        OutputFormat {
            rate: 44_100,
            encoding: OutputEncoding::Pcm16,
            channels: 2,
        }
    }

    fn announced(routing: &Routing) -> usize {
        routing.changes.try_iter().count()
    }

    #[derive(Default)]
    struct Mixer {
        preferred: Mutex<Vec<(i32, OutputFormat)>>,
        freed: Mutex<Vec<i32>>,
        refuses: bool,
    }

    impl OutputMixer for Arc<Mixer> {
        fn prefer_bit_perfect(&self, device: i32, format: OutputFormat) -> bool {
            self.preferred.lock().push((device, format));
            !self.refuses
        }

        fn mix_freely(&self, device: i32) {
            self.freed.lock().push(device);
        }
    }

    #[test]
    fn a_route_serving_another_zone_announces_its_own_sink() {
        let kitchen = SinkId::new(2);
        let routing = Routing::serving(kitchen);

        routing.select(USB);

        assert_eq!(Routing::new().sink(), OUTPUT);
        assert_eq!(routing.sink(), kitchen);
        assert!(matches!(
            routing.changes.try_iter().collect::<Vec<_>>().as_slice(),
            [SinkChange::Reformatted(sink)] if *sink == kitchen
        ));
    }

    #[test]
    fn the_device_a_stream_opened_on_is_told_and_a_route_report_moves_nothing() {
        let routing = Routing::new();

        assert_eq!(routing.actual(), 0);
        routing.opened_on(USB);

        assert_eq!(routing.actual(), USB);
        assert!(!routing.route_changed(44_100, 20.0));
        assert_eq!(announced(&routing), 0);
    }

    #[test]
    fn a_new_route_offers_the_shared_mixer_alone_and_probes_nothing() {
        let routing = Routing::new();
        let probes = Cell::new(0);

        routing.offer(USB, &[cd()]);

        assert_eq!(
            routing.offered(|_| {
                probes.set(probes.get() + 1);
                Some(NATIVE)
            }),
            vec![MIXED]
        );
        assert_eq!(probes.get(), 0);
        assert_eq!(routing.way_for(CD), Way::Mixed);
    }

    #[test]
    fn following_the_song_probes_the_route_once_and_offers_what_it_found() {
        let routing = Routing::new();
        let probes = Cell::new(0);
        let probe = |device: i32| {
            assert_eq!(device, 0);
            probes.set(probes.get() + 1);
            Some(NATIVE)
        };

        routing.offer(USB, &[cd()]);
        assert!(routing.follow_song_rate(true));
        assert!(!routing.follow_song_rate(true));

        assert_eq!(routing.offered(probe), vec![MIXED, CD, NATIVE]);
        assert_eq!(routing.offered(probe), vec![MIXED, CD, NATIVE]);
        assert_eq!(probes.get(), 1);
        assert_eq!(routing.way_for(CD), Way::BitPerfect { device: USB });
        assert_eq!(routing.way_for(NATIVE), Way::Exclusive);
    }

    #[test]
    fn a_moved_route_or_choice_is_announced_and_probed_again() {
        let routing = Routing::new();
        let probes = Cell::new(0);
        let probe = |_| {
            probes.set(probes.get() + 1);
            None
        };
        routing.follow_song_rate(true);
        routing.offered(probe);
        let _ = announced(&routing);

        assert!(!routing.offer(0, &[]));
        assert_eq!(announced(&routing), 0);

        assert!(routing.offer(USB, &[cd()]));
        assert_eq!(announced(&routing), 1);
        routing.offered(probe);

        assert!(routing.select(USB));
        assert_eq!(announced(&routing), 1);
        routing.offered(probe);

        assert_eq!(probes.get(), 3);
    }

    #[test]
    fn a_bit_perfect_offer_is_not_used_while_another_output_is_chosen() {
        let routing = Routing::new();
        routing.offer(USB, &[cd()]);
        routing.follow_song_rate(true);
        routing.select(2);

        assert_eq!(routing.offered(|_| None), vec![MIXED]);
        assert_eq!(routing.way_for(CD), Way::Mixed);

        routing.select(USB);
        assert_eq!(routing.offered(|_| None), vec![MIXED, CD]);
    }

    #[test]
    fn an_exclusive_stream_refused_is_offered_no_longer_and_not_probed_again() {
        let routing = Routing::new();
        let probes = Cell::new(0);
        let probe = |_| {
            probes.set(probes.get() + 1);
            Some(NATIVE)
        };
        routing.follow_song_rate(true);
        routing.offered(probe);
        let _ = announced(&routing);

        routing.exclusive_refused();

        assert_eq!(announced(&routing), 1);
        assert_eq!(routing.offered(probe), vec![MIXED]);
        assert_eq!(probes.get(), 1);
        assert_eq!(routing.way_for(NATIVE), Way::Mixed);
    }

    struct Throwing;

    impl OutputMixer for Throwing {
        fn prefer_bit_perfect(&self, _: i32, _: OutputFormat) -> bool {
            panic!("the front end threw")
        }

        fn mix_freely(&self, _: i32) {
            panic!("the front end threw")
        }
    }

    #[test]
    fn a_mixer_that_throws_is_taken_as_refusing_and_the_output_thread_goes_on() {
        let routing = Routing::new();

        routing.watch_mixer(Arc::new(Throwing));

        assert!(!routing.prefer_bit_perfect(&routing.claim(), USB, CD));
        routing.mix_freely();
    }

    #[test]
    fn the_mixer_is_asked_for_bit_perfect_and_freed_when_a_mixed_stream_follows() {
        let routing = Routing::new();

        let mixer = Arc::new(Mixer::default());
        let claim = routing.claim();

        assert!(!routing.prefer_bit_perfect(&claim, USB, CD));

        routing.watch_mixer(Arc::new(Arc::clone(&mixer)));
        assert!(routing.prefer_bit_perfect(&claim, USB, CD));
        assert!(routing.prefer_bit_perfect(&claim, USB, CD));
        assert!(mixer.freed.lock().is_empty());

        routing.mix_freely();
        routing.mix_freely();

        assert_eq!(*mixer.preferred.lock(), vec![(USB, cd()), (USB, cd())]);
        assert_eq!(*mixer.freed.lock(), vec![USB]);
    }

    #[test]
    fn a_mixer_that_refuses_is_never_freed_and_a_format_it_cannot_name_is_not_asked() {
        let routing = Routing::new();
        let mixer = Arc::new(Mixer {
            refuses: true,
            ..Mixer::default()
        });
        routing.watch_mixer(Arc::new(Arc::clone(&mixer)));
        let claim = routing.claim();

        assert!(!routing.prefer_bit_perfect(&claim, USB, CD));
        assert!(!routing.prefer_bit_perfect(
            &claim,
            USB,
            StreamSpec::new(
                SampleRate::HZ_96000,
                ChannelLayout::Stereo,
                SampleFormat::S24
            )
        ));
        routing.mix_freely();

        assert_eq!(mixer.preferred.lock().len(), 1);
        assert!(mixer.freed.lock().is_empty());
    }

    #[test]
    fn what_opened_is_told_until_it_closes_and_a_late_close_leaves_the_next_stream_told() {
        let routing = Routing::new();
        let bit_perfect = OutputStream {
            format: cd(),
            path: OutputPath::BitPerfect,
        };
        let mixed = OutputStream {
            format: OutputFormat::of(MIXED).expect("the mixer's format"),
            path: OutputPath::Mixed,
        };

        let first = routing.opened(bit_perfect);
        assert_eq!(routing.stream(), Some(bit_perfect));

        let second = routing.opened(mixed);
        routing.closed(first);
        assert_eq!(routing.stream(), Some(mixed));

        routing.closed(second);
        assert_eq!(routing.stream(), None);
    }

    #[test]
    fn the_stream_that_held_the_mixer_bit_perfect_gives_it_back_as_it_closes_and_a_late_close_does_not()
     {
        let routing = Routing::new();
        let mixer = Arc::new(Mixer::default());
        routing.watch_mixer(Arc::new(Arc::clone(&mixer)));
        let bit_perfect = OutputStream {
            format: cd(),
            path: OutputPath::BitPerfect,
        };

        assert!(routing.prefer_bit_perfect(&routing.claim(), USB, CD));
        let first = routing.opened(bit_perfect);
        let second = routing.opened(bit_perfect);
        routing.closed(first);

        assert!(mixer.freed.lock().is_empty());

        routing.closed(second);

        assert_eq!(*mixer.freed.lock(), vec![USB]);
    }

    fn bit_perfect() -> OutputStream {
        OutputStream {
            format: cd(),
            path: OutputPath::BitPerfect,
        }
    }

    fn watched(routing: &Routing) -> Arc<Mixer> {
        let mixer = Arc::new(Mixer::default());
        routing.watch_mixer(Arc::new(Arc::clone(&mixer)));
        mixer
    }

    #[test]
    fn an_opening_abandoned_before_it_answers_leaves_the_device_the_stream_and_the_mixer_as_the_next_one_set_them()
     {
        let routing = Routing::new();
        let mixer = watched(&routing);
        let late = routing.claim();
        let next = routing.claim();

        assert!(late.abandon());
        assert!(routing.prefer_bit_perfect(&next, USB, CD));
        let told = routing.answer(&next, USB, bit_perfect(), || ());

        let mut said = false;
        let late_told = routing.answer(&late, 2, bit_perfect(), || said = true);
        routing.mix_freely_for(&late);
        routing.give_back(&late);

        assert!(told.is_some());
        assert_eq!(late_told, None);
        assert!(!said);
        assert_eq!(routing.actual(), USB);
        assert_eq!(routing.stream(), Some(bit_perfect()));
        assert!(mixer.freed.lock().is_empty());
    }

    #[test]
    fn an_abandoned_opening_never_asks_the_mixer_for_bit_perfect() {
        let routing = Routing::new();
        let mixer = watched(&routing);
        let claim = routing.claim();

        claim.abandon();

        assert!(!routing.prefer_bit_perfect(&claim, USB, CD));
        assert!(mixer.preferred.lock().is_empty());
    }

    #[test]
    fn an_opening_that_answered_is_not_abandoned_by_a_wait_that_ends_after() {
        let routing = Routing::new();
        let claim = routing.claim();

        let told = routing.answer(&claim, USB, bit_perfect(), || ());

        assert!(told.is_some());
        assert!(!claim.abandon());
        assert!(!claim.abandoned());
    }

    #[test]
    fn a_preference_granted_to_an_opening_that_failed_is_given_back() {
        let routing = Routing::new();
        let mixer = watched(&routing);
        let claim = routing.claim();

        assert!(routing.prefer_bit_perfect(&claim, USB, CD));
        routing.give_back(&claim);
        routing.give_back(&claim);

        assert_eq!(*mixer.freed.lock(), vec![USB]);
        assert_eq!(routing.stream(), None);
    }

    #[test]
    fn a_preference_another_opening_took_over_is_not_given_back_by_the_one_before() {
        let routing = Routing::new();
        let mixer = watched(&routing);
        let first = routing.claim();
        let second = routing.claim();

        assert!(routing.prefer_bit_perfect(&first, USB, CD));
        assert!(routing.prefer_bit_perfect(&second, USB, CD));
        routing.give_back(&first);

        assert!(mixer.freed.lock().is_empty());

        routing.give_back(&second);

        assert_eq!(*mixer.freed.lock(), vec![USB]);
    }
}
