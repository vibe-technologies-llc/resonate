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

pub(crate) const OUTPUT: SinkId = SinkId::new(1);
const SYSTEM_DEFAULT: i32 = 0;

pub(crate) struct Routing {
    requested: AtomicI32,
    actual: AtomicI32,
    follows: AtomicBool,
    offers: Mutex<Offers>,
    probed_for: Mutex<Option<i32>>,
    opened: Mutex<Option<(u64, OutputStream)>>,
    streams_opened: AtomicU64,
    mixer: Mutex<Option<Arc<dyn OutputMixer>>>,
    preferred: Mutex<Option<i32>>,
    announce: Sender<SinkChange>,
    pub(crate) changes: Receiver<SinkChange>,
}

impl Routing {
    pub(crate) fn new() -> Self {
        let (announce, changes) = unbounded();
        Self {
            requested: AtomicI32::new(SYSTEM_DEFAULT),
            actual: AtomicI32::new(SYSTEM_DEFAULT),
            follows: AtomicBool::new(false),
            offers: Mutex::new(Offers::default()),
            probed_for: Mutex::new(None),
            opened: Mutex::new(None),
            streams_opened: AtomicU64::new(0),
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

    pub(crate) fn opened(&self, stream: OutputStream) -> u64 {
        let which = self.streams_opened.fetch_add(1, Ordering::Relaxed);
        *self.opened.lock() = Some((which, stream));
        which
    }

    pub(crate) fn closed(&self, which: u64) {
        let mut opened = self.opened.lock();
        if opened.is_some_and(|(held, _)| held == which) {
            *opened = None;
        }
    }

    pub(crate) fn stream(&self) -> Option<OutputStream> {
        self.opened.lock().map(|(_, stream)| stream)
    }

    pub(crate) fn watch_mixer(&self, mixer: Arc<dyn OutputMixer>) {
        *self.mixer.lock() = Some(mixer);
    }

    pub(crate) fn prefer_bit_perfect(&self, device: i32, spec: StreamSpec) -> bool {
        let Some(format) = OutputFormat::of(spec) else {
            return false;
        };
        let Some(mixer) = self.mixer.lock().clone() else {
            return false;
        };
        let preferred = mixer.prefer_bit_perfect(device, format);
        let given_up = {
            let mut held = self.preferred.lock();
            let before = held.take();
            *held = preferred.then_some(device);
            before.filter(|before| !preferred || *before != device)
        };
        if let Some(before) = given_up {
            mixer.mix_freely(before);
        }
        preferred
    }

    pub(crate) fn mix_freely(&self) {
        let Some(device) = self.preferred.lock().take() else {
            return;
        };
        let mixer = self.mixer.lock().clone();
        if let Some(mixer) = mixer {
            mixer.mix_freely(device);
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
        let _ = self.announce.send(SinkChange::Reformatted(OUTPUT));
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

    #[test]
    fn the_mixer_is_asked_for_bit_perfect_and_freed_when_a_mixed_stream_follows() {
        let routing = Routing::new();
        let mixer = Arc::new(Mixer::default());

        assert!(!routing.prefer_bit_perfect(USB, CD));

        routing.watch_mixer(Arc::new(Arc::clone(&mixer)));
        assert!(routing.prefer_bit_perfect(USB, CD));
        assert!(routing.prefer_bit_perfect(USB, CD));
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

        assert!(!routing.prefer_bit_perfect(USB, CD));
        assert!(!routing.prefer_bit_perfect(
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
}
