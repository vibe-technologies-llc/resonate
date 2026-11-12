use std::time::Duration;

use resonate_codec::Packing;
use resonate_core::{Frames, Gain};

use super::{
    Ahead, Boundary, Engine, Filled, HANDED_ON_WHILE_THE_RING_HOLDS, Handing, Track, Unwrapped,
    Waiting, levelled,
    output::{Output, heard_amplitude},
};
use crate::{
    Event, OutputPlan,
    pipeline::{plan_output, replay_gain_as_planned},
    queue::QueueItem,
    ring::TURNS_WAITING_AT_MOST,
};

impl Engine {
    pub(super) fn take_what_was_decoded(&mut self) {
        if let Some(track) = self.track.as_mut() {
            track.decoding.take_what_came_back();
        }
    }

    pub(super) fn pump(&mut self) {
        self.fill_owed = false;
        if self.failing_past_the_boundary.is_some() {
            self.hand_on_the_leaving_tail();
            return;
        }
        let outcome = {
            let (Some(track), Some(output)) = (self.track.as_mut(), self.output.as_mut()) else {
                return;
            };
            if output.ended {
                return;
            }
            Self::fill(track, output)
        };
        match outcome {
            Ok(Filled::AtAChange) => self.follow_the_change(),
            Ok(Filled::AtTheEnd) => self.end_the_track(),
            Ok(filled) => self.fill_owed = filled == Filled::ForNow,
            Err(error) => self.fail(error),
        }
    }

    fn end_the_track(&mut self) {
        match self.hand_on() {
            Handing::Handed => self.fill_owed = true,
            Handing::Waits => {}
            Handing::Refused => {
                if let Some(output) = self.output.as_mut() {
                    Self::flush(output);
                }
            }
        }
    }

    fn sleeps_at_this_end(&self) -> bool {
        self.sleeps_at_the_end_of(0)
    }

    fn sleeps_at_the_end_of(&self, rows_ahead: usize) -> bool {
        self.sleep.is_some_and(|sleeping| {
            sleeping.ends_the_track()
                || (sleeping.ends_the_queue()
                    && (self.queue.wraps_next_after(rows_ahead)
                        || self.queue.ends_with_the_row_after(rows_ahead)))
        })
    }

    fn may_hand_on(&self) -> bool {
        let Some(output) = self.output.as_ref() else {
            return false;
        };
        self.boundaries.len() < TURNS_WAITING_AT_MOST
            && !self.sleeps_at_the_end_of(self.boundaries.len())
            && !self.rebind_owed
            && self.marks_on_across_a_boundary(output)
            && output.draining.is_none()
            && !output.ended
            && !output.ends_at_a_change
            && !output.sleep_fading
            && !output.rendered_for_another
            && !output.producer.is_discarding()
    }

    fn marks_on_across_a_boundary(&self, output: &Output) -> bool {
        match output.plan.packing {
            Packing::Samples => true,
            Packing::DopMarked(_) => self
                .track
                .as_ref()
                .is_some_and(|track| track.decoding.marker_due().is_some()),
        }
    }

    fn is_settling(&self) -> bool {
        self.output.as_ref().is_some_and(|output| {
            output.producer.is_splicing()
                || output.splice_owed
                || output.settles_into.is_some()
                || output.chain.is_ramping()
        })
    }

    fn waits_while_the_ring_holds(&self) -> Handing {
        let holds = self.output.as_ref().is_some_and(|output| {
            let least =
                Frames::from_duration(HANDED_ON_WHILE_THE_RING_HOLDS, output.plan.stream.rate);
            output.stream.is_none() || output.buffered() as u64 > least.get()
        });
        if holds {
            Handing::Waits
        } else {
            Handing::Refused
        }
    }

    fn plan_on_the_open_stream(&self, unwrapped: &Unwrapped) -> Option<OutputPlan> {
        let output = self.output.as_ref()?;
        if output.plan.stream != output.status.negotiated {
            return None;
        }
        let sink = self.bound_sink()?;
        let replay_gain = levelled(&self.config, &unwrapped.info, unwrapped.hints);
        let wanted = plan_output(unwrapped.decoded(), &sink, &self.config, replay_gain);
        self.keeps_its_stream_for(&wanted).then_some(wanted)
    }

    pub(super) fn hand_on(&mut self) -> Handing {
        if !self.may_hand_on() {
            return Handing::Refused;
        }
        if self.is_settling() {
            return self.waits_while_the_ring_holds();
        }
        let Some(next) = self
            .queue
            .follows_naturally_after(self.boundaries.len())
            .cloned()
        else {
            return Handing::Refused;
        };
        if self
            .ahead
            .as_ref()
            .is_none_or(|ahead| ahead.opening.item != next || ahead.lost)
        {
            self.ahead = Some(Ahead::begin(next, &self.sources));
        }
        let Some(ahead) = self.ahead.as_mut() else {
            return Handing::Refused;
        };
        if !ahead.look() {
            return if ahead.lost {
                Handing::Refused
            } else {
                self.waits_while_the_ring_holds()
            };
        }
        let wanted = match self.ahead.as_ref().and_then(|ahead| ahead.came.as_ref()) {
            Some(Ok(unwrapped)) => self.plan_on_the_open_stream(unwrapped),
            Some(Err(_)) | None => None,
        };
        let (Some(wanted), Some(output)) = (wanted, self.output.as_ref()) else {
            return Handing::Refused;
        };
        let alike = wanted.same_shape_as(&output.plan);
        let carries = output.plan.carries_the_front_into(&wanted);
        if !alike && !output.has_room_for_what_the_chain_holds(carries) {
            return self.waits_while_the_ring_holds();
        }
        let Some(Ahead {
            opening,
            came: Some(Ok(unwrapped)),
            ..
        }) = self.ahead.take()
        else {
            return Handing::Refused;
        };
        self.hand_on_to(&opening.item, unwrapped, wanted, alike || carries);
        Handing::Handed
    }

    fn hand_on_to(
        &mut self,
        item: &QueueItem,
        unwrapped: Unwrapped,
        wanted: OutputPlan,
        holds_the_leaving: bool,
    ) {
        let entering = Track::of(item, unwrapped, &self.sources, &self.config);
        let Some(leaving) = self.track.replace(entering) else {
            return;
        };
        let alike = self
            .output
            .as_ref()
            .is_some_and(|output| wanted.same_shape_as(&output.plan));
        let swapped = if alike {
            self.retune_for_the_entering(wanted);
            Ok(())
        } else {
            self.swap_chain(wanted)
        };
        if let (Some(due), Some(entering)) = (leaving.decoding.marker_due(), self.track.as_mut()) {
            entering.decoding.mark_next_with(due);
        }

        let muted = self.muted;
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let ahead = output.frames_rendered_ahead_of_the_ring();
        let rendered = output.chain.gain_amplitude().unwrap_or(Gain::UNITY.get());
        let turned = output
            .producer
            .turn_at(rendered, heard_amplitude(&output.plan, muted), ahead);
        let held = if holds_the_leaving {
            output.chain.latency_frames().round() as u64
        } else {
            0
        };
        self.boundaries.push_back(Boundary {
            at: output.producer.written().saturating_add(held),
            made: output.made,
            turned,
            leaving,
        });
        if let Err(error) = swapped {
            self.fail(error);
        }
    }

    fn retune_for_the_entering(&mut self, wanted: OutputPlan) {
        let (Some(track), Some(output)) = (self.track.as_mut(), self.output.as_mut()) else {
            return;
        };
        let volume = output.attenuator.leaves(self.config.volume);
        let planned =
            replay_gain_as_planned(track.decoded().packing, &self.config, track.replay_gain);
        output.chain.set_gain(volume, planned);
        if let Some(arms) = output.arms.as_mut() {
            arms.set_gain_of_the_other(volume, planned);
        }
        let delivery = wanted.delivery();
        track.decoding.deliver(delivery);
        track.decoded.retype(delivery.format);
        output.plan = wanted;
    }

    pub(super) fn heard_track(&self) -> Option<&Track> {
        self.boundaries
            .front()
            .map(|boundary| &boundary.leaving)
            .or(self.track.as_ref())
    }

    pub(super) fn reach_the_boundary(&mut self) {
        loop {
            let reached = match (self.boundaries.front(), self.output.as_ref()) {
                (None, _) => return,
                (Some(boundary), Some(output)) if output.made == boundary.made => {
                    output.producer.read() >= boundary.at
                }
                (Some(_), _) => {
                    self.take_the_boundary_back();
                    return;
                }
            };
            if !reached || !self.pass_the_boundary() {
                return;
            }
        }
    }

    pub(super) fn drop_what_lies_past_the_boundary(&mut self) -> bool {
        let (Some(boundary), Some(output)) = (self.boundaries.back(), self.output.as_mut()) else {
            return false;
        };
        let producer = &mut output.producer;
        let written = producer.written();
        let reachable = output.made == boundary.made
            && producer.read() < boundary.at
            && boundary.at <= written
            && !producer.is_splicing()
            && !producer.is_discarding();
        if reachable {
            producer.splice(boundary.at, written);
        }
        reachable
    }

    pub(super) fn the_leaving_tail_waits_in_the_chain(&self) -> bool {
        let (Some(boundary), Some(output)) = (self.boundaries.back(), self.output.as_ref()) else {
            return false;
        };
        let producer = &output.producer;
        output.made == boundary.made
            && producer.read() < boundary.at
            && producer.written() < boundary.at
            && !producer.is_splicing()
            && !producer.is_discarding()
            && output.chain.max_flush_frames() > 0
            && output.chain.max_flush_frames() <= output.capacity
    }

    fn hand_on_the_leaving_tail(&mut self) {
        let (Some(boundary), Some(output)) = (self.boundaries.back(), self.output.as_mut()) else {
            return;
        };
        if output.producer.written() >= boundary.at
            || output.producer.free_frames() < output.chain.max_flush_frames()
        {
            return;
        }
        output.hand_on_what_the_chain_holds();
        if self.drop_what_lies_past_the_boundary() {
            return;
        }
        if let Some(error) = self.failing_past_the_boundary.take() {
            self.failing(error, Waiting::NoLonger);
        }
    }

    pub(super) fn fail_what_waited_for_the_boundary(&mut self) {
        if !self.boundaries.is_empty() {
            return;
        }
        if let Some(error) = self.failing_past_the_boundary.take() {
            self.fail(error);
        }
    }

    pub(super) fn until_the_boundary(&self) -> Option<Duration> {
        let (boundary, output) = (self.boundaries.front()?, self.output.as_ref()?);
        let frames = boundary.at.saturating_sub(output.producer.read());
        Some(Frames(frames).to_duration(output.plan.stream.rate))
    }

    pub(super) fn pass_every_boundary(&mut self) -> bool {
        while !self.boundaries.is_empty() {
            if !self.pass_the_boundary() {
                return false;
            }
        }
        true
    }

    fn pass_the_boundary(&mut self) -> bool {
        let Some(boundary) = self.boundaries.pop_front() else {
            return false;
        };
        let entering = match self.boundaries.front() {
            Some(next) => Some(next.leaving.id),
            None => self.track.as_ref().map(|track| track.id),
        };
        let Some(entering) = entering else {
            return false;
        };
        let leaving = boundary.leaving.id;
        drop(boundary);
        self.emit(Event::TrackFinished(leaving));

        let sleeps = self.sleeps_at_this_end();
        if self.queue.advance(true).is_none() {
            self.finish_the_queue();
            return false;
        }
        if self.queue.current().map(|item| item.id) != Some(entering) {
            tracing::debug!("the queue moved while the next row was heard ahead; it opens afresh");
            if sleeps {
                self.nods_off();
            }
            let started = self.start(Frames::ZERO);
            if let Err(error) = self.past_what_will_not_open(started) {
                self.fail(error);
            }
            return false;
        }
        if entering == leaving {
            self.seeks = self.seeks.stepped();
        }
        self.heard_at_least = None;
        self.renegotiations = 0;
        self.failures = 0;
        self.fill_owed = true;
        let muted = self.muted;
        if self.boundaries.is_empty()
            && let Some(output) = self.output.as_mut()
        {
            let heard = heard_amplitude(&output.plan, muted);
            output.producer.hear_at(heard);
        }
        self.emit(Event::TrackStarted(entering));
        if sleeps {
            self.sleep = None;
            if let Err(error) = self.pause() {
                tracing::warn!(%error, "the sleep timer could not pause the transport");
            }
        }
        true
    }

    pub(super) fn take_the_boundary_back(&mut self) -> bool {
        if self.boundaries.is_empty() {
            return false;
        }
        let _ = self.heard_position();
        let Some(boundary) = self.boundaries.pop_front() else {
            return false;
        };
        self.boundaries.clear();
        self.failing_past_the_boundary = None;
        self.track = Some(boundary.leaving);
        if let Some(output) = self.output.as_mut() {
            output.rendered_for_another = true;
        }
        true
    }
}
