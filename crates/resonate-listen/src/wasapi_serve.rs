use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use resonate_audio::Result;

use crate::{
    recording::Recorder,
    session::{BYTES_PER_FRAME, HEARD_IN_CHANNELS, frames_in, zeroed_if_silent},
};

pub(crate) struct Running {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) lost: Arc<AtomicBool>,
    pub(crate) quiet_is_silence: bool,
}

pub(crate) struct Delivery<'a> {
    pub(crate) window: &'a mut [u8],
    pub(crate) silent: bool,
    pub(crate) discontinuity: bool,
}

pub(crate) trait Device {
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn wait(&mut self);
    fn now(&self) -> Instant;
    fn has_moved(&self) -> bool;
    fn next(&mut self) -> Result<Option<Delivery<'_>>>;
}

pub(crate) fn serve<D: Device>(
    device: &mut D,
    recorder: &mut Recorder,
    running: &Running,
) -> Result<()> {
    if running.stop.load(Ordering::Acquire) {
        return Ok(());
    }
    device.start()?;

    let mut turned = device.now();

    while !running.stop.load(Ordering::Acquire) {
        if device.has_moved() {
            tracing::debug!("the default output moved from under the desktop's loopback");
            running.lost.store(true, Ordering::Release);
            break;
        }

        device.wait();

        let now = device.now();
        let elapsed = frames_in(now.saturating_duration_since(turned));
        let accounted = drained(device, recorder, elapsed)?;

        if accounted == 0 && running.quiet_is_silence {
            recorder.pad(elapsed, HEARD_IN_CHANNELS);
        }
        turned = now;
    }
    device.stop()
}

fn drained<D: Device>(device: &mut D, recorder: &mut Recorder, elapsed: usize) -> Result<usize> {
    let mut accounted = 0;

    while let Some(delivery) = device.next()? {
        let Delivery {
            window,
            silent,
            discontinuity,
        } = delivery;
        let read = window.len() / BYTES_PER_FRAME;

        if discontinuity {
            let lost = elapsed.saturating_sub(accounted + read);

            recorder.pad(lost, HEARD_IN_CHANNELS);
            accounted += lost;
        }
        zeroed_if_silent(window, silent);
        recorder.admit(window);
        accounted += read;
    }
    Ok(accounted)
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, io, time::Duration};

    use resonate_audio::{Error, Operation};

    use super::*;
    use crate::recording::Recording;

    const TURN: Duration = Duration::from_millis(100);
    const PACKET_FRAMES: usize = 480;
    const TURN_FRAMES: usize = 4_800;

    struct Packet {
        bytes: Vec<u8>,
        silent: bool,
        discontinuity: bool,
    }

    fn packet(value: f32, discontinuity: bool) -> Packet {
        Packet {
            bytes: (0..PACKET_FRAMES * HEARD_IN_CHANNELS)
                .flat_map(|_| value.to_le_bytes())
                .collect(),
            silent: false,
            discontinuity,
        }
    }

    struct Fake {
        stop: Arc<AtomicBool>,
        turns: VecDeque<Vec<Packet>>,
        waiting: VecDeque<Packet>,
        started: usize,
        stopped: usize,
        waited: usize,
        clock: Instant,
        moved: bool,
        refusing_the_start: bool,
        current: Option<Packet>,
    }

    impl Fake {
        fn new(stop: &Arc<AtomicBool>, turns: Vec<Vec<Packet>>) -> Self {
            Self {
                stop: Arc::clone(stop),
                turns: turns.into(),
                waiting: VecDeque::new(),
                started: 0,
                stopped: 0,
                waited: 0,
                clock: Instant::now(),
                moved: false,
                refusing_the_start: false,
                current: None,
            }
        }
    }

    impl Device for Fake {
        fn start(&mut self) -> Result<()> {
            self.started += 1;
            if self.refusing_the_start {
                return Err(Error::Backend {
                    op: Operation::Start,
                    source: io::Error::other("refused"),
                });
            }
            Ok(())
        }

        fn stop(&mut self) -> Result<()> {
            self.stopped += 1;
            Ok(())
        }

        fn wait(&mut self) {
            self.waited += 1;
            self.clock += TURN;
            self.waiting = self.turns.pop_front().unwrap_or_default().into();
            if self.turns.is_empty() {
                self.stop.store(true, Ordering::Release);
            }
        }

        fn now(&self) -> Instant {
            self.clock
        }

        fn has_moved(&self) -> bool {
            self.moved
        }

        fn next(&mut self) -> Result<Option<Delivery<'_>>> {
            self.current = self.waiting.pop_front();
            Ok(self.current.as_mut().map(|packet| Delivery {
                window: &mut packet.bytes,
                silent: packet.silent,
                discontinuity: packet.discontinuity,
            }))
        }
    }

    fn running(stop: &Arc<AtomicBool>, quiet_is_silence: bool) -> Running {
        Running {
            stop: Arc::clone(stop),
            lost: Arc::new(AtomicBool::new(false)),
            quiet_is_silence,
        }
    }

    fn recorded() -> (Arc<Recording>, Recorder) {
        let recording = Recording::holding(10 * TURN_FRAMES * HEARD_IN_CHANNELS);
        let recorder = Recorder::admitting(Arc::clone(&recording));

        (recording, recorder)
    }

    #[test]
    fn a_late_capture_whose_stop_was_set_never_starts_the_client() {
        let stop = Arc::new(AtomicBool::new(true));
        let (recording, mut recorder) = recorded();
        let mut fake = Fake::new(&stop, vec![vec![packet(0.5, false)]]);

        let served = serve(&mut fake, &mut recorder, &running(&stop, false));

        assert!(served.is_ok());
        assert_eq!(fake.started, 0);
        assert_eq!(fake.stopped, 0);
        assert_eq!(fake.waited, 0);
        assert_eq!(recording.filled(), 0);
    }

    #[test]
    fn a_capture_asked_to_stop_after_it_starts_stops_the_client_once() {
        let stop = Arc::new(AtomicBool::new(false));
        let (_recording, mut recorder) = recorded();
        let mut fake = Fake::new(&stop, vec![vec![packet(0.5, false)]]);

        serve(&mut fake, &mut recorder, &running(&stop, false)).expect("served");

        assert_eq!(fake.started, 1);
        assert_eq!(fake.stopped, 1);
    }

    #[test]
    fn a_start_refused_is_the_error_and_the_client_is_not_stopped() {
        let stop = Arc::new(AtomicBool::new(false));
        let (_recording, mut recorder) = recorded();
        let mut fake = Fake::new(&stop, Vec::new());

        fake.refusing_the_start = true;

        let served = serve(&mut fake, &mut recorder, &running(&stop, false));

        assert!(matches!(
            served,
            Err(Error::Backend {
                op: Operation::Start,
                ..
            })
        ));
        assert_eq!(fake.stopped, 0);
    }

    #[test]
    fn a_discontinuity_pads_the_time_the_packet_does_not_account_for_before_it() {
        let stop = Arc::new(AtomicBool::new(false));
        let (recording, mut recorder) = recorded();
        let mut fake = Fake::new(&stop, vec![vec![packet(0.5, true)]]);

        serve(&mut fake, &mut recorder, &running(&stop, false)).expect("served");

        let heard = recording.taken();
        let padded = (TURN_FRAMES - PACKET_FRAMES) * HEARD_IN_CHANNELS;

        assert_eq!(heard.len(), TURN_FRAMES * HEARD_IN_CHANNELS);
        assert!(heard[..padded].iter().all(|sample| *sample == 0.0));
        assert!(heard[padded..].iter().all(|sample| *sample == 0.5));
    }

    #[test]
    fn a_packet_after_no_discontinuity_is_kept_as_it_is_whatever_time_passed() {
        let stop = Arc::new(AtomicBool::new(false));
        let (recording, mut recorder) = recorded();
        let mut fake = Fake::new(&stop, vec![vec![packet(0.5, false)]]);

        serve(&mut fake, &mut recorder, &running(&stop, false)).expect("served");

        assert_eq!(recording.filled(), PACKET_FRAMES * HEARD_IN_CHANNELS);
    }

    #[test]
    fn a_second_discontinuity_in_a_turn_already_accounted_for_pads_nothing() {
        let stop = Arc::new(AtomicBool::new(false));
        let (recording, mut recorder) = recorded();
        let turn = vec![packet(0.25, false), packet(0.5, true), packet(0.75, true)];

        let mut fake = Fake::new(&stop, vec![turn]);

        serve(&mut fake, &mut recorder, &running(&stop, false)).expect("served");

        assert_eq!(
            recording.filled(),
            (TURN_FRAMES + PACKET_FRAMES) * HEARD_IN_CHANNELS
        );
    }

    #[test]
    fn a_loopback_turn_with_nothing_drained_is_silence_of_the_time_that_passed() {
        let stop = Arc::new(AtomicBool::new(false));
        let (recording, mut recorder) = recorded();
        let mut fake = Fake::new(&stop, vec![Vec::new()]);

        serve(&mut fake, &mut recorder, &running(&stop, true)).expect("served");

        assert_eq!(recording.filled(), TURN_FRAMES * HEARD_IN_CHANNELS);
    }

    #[test]
    fn a_microphone_turn_with_nothing_drained_pads_nothing() {
        let stop = Arc::new(AtomicBool::new(false));
        let (recording, mut recorder) = recorded();
        let mut fake = Fake::new(&stop, vec![Vec::new()]);

        serve(&mut fake, &mut recorder, &running(&stop, false)).expect("served");

        assert_eq!(recording.filled(), 0);
    }

    #[test]
    fn a_default_output_that_moved_is_a_loss_and_the_client_is_never_left_running() {
        let stop = Arc::new(AtomicBool::new(false));
        let (_recording, mut recorder) = recorded();
        let mut fake = Fake::new(&stop, vec![vec![packet(0.5, false)]]);
        let running = running(&stop, true);

        fake.moved = true;
        serve(&mut fake, &mut recorder, &running).expect("served");

        assert!(running.lost.load(Ordering::Acquire));
        assert_eq!(fake.started, 1);
        assert_eq!(fake.stopped, 1);
        assert_eq!(fake.waited, 0);
    }
}
