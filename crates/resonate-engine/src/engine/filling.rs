use std::time::Instant;

use resonate_audio::{LatencyRequest, MediaRole, SinkStream, StreamRequest};

use super::{
    Ahead, CHAIN_BLOCK, Engine, FILLED_IN_ONE_GO, Filled, OPENED_AHEAD_BY, Track, output::Output,
};
use crate::{
    OutputMode, Result, SinkResult, TransportState,
    lending::Block,
    streaming::{Asked, Came, Streaming, close_unwanted},
};

impl Engine {
    pub(super) fn open_ahead(&mut self) {
        if let Some(ahead) = self.ahead.as_mut() {
            ahead.look();
        }
        if !self.playing || self.opening.is_some() {
            return;
        }
        let (Some(track), Some(_)) = (self.track.as_ref(), self.output.as_ref()) else {
            return;
        };
        let near = track.ended
            || track.info.duration.is_some_and(|duration| {
                duration
                    .saturating_sub(track.decoding.position())
                    .to_duration(track.clock())
                    <= OPENED_AHEAD_BY
            });
        if !near {
            return;
        }
        let Some(next) = self.queue.follows_naturally_after(self.boundaries.len()) else {
            self.ahead = None;
            return;
        };
        if self
            .ahead
            .as_ref()
            .is_some_and(|ahead| &ahead.opening.item == next && !ahead.lost)
        {
            return;
        }
        self.ahead = Some(Ahead::begin(next.clone(), &self.sources));
    }

    pub(super) fn follow_the_change(&mut self) {
        self.fill_owed = true;
        let kept = self
            .plan_wanted_now()
            .filter(|wanted| self.keeps_its_stream_for(wanted));
        if let Some(wanted) = kept {
            if let Err(error) = self.swap_chain(wanted) {
                self.fail(error);
            }
            return;
        }

        let (Some(track), Some(output)) = (self.track.as_mut(), self.output.as_mut()) else {
            return;
        };
        track.at_a_change = true;
        if output.stream.is_none() && output.buffered() == 0 {
            if let Err(error) = self.reopen_at_the_change() {
                self.fail(error);
            }
            return;
        }
        output.ends_at_a_change = true;
        Self::flush(output);
    }

    pub(super) fn reopen_at_the_change(&mut self) -> Result<()> {
        tracing::debug!(
            "the source changed its rate or channels; the stream is reshaped or reopened for it"
        );
        self.rebind(None, None)
    }

    pub(super) fn fill(track: &mut Track, output: &mut Output) -> Result<Filled> {
        if output.producer.is_discarding() || output.switch_waits_for_room {
            return Ok(Filled::AsFarAsItGoes);
        }
        if output.draining.is_some() {
            Self::drain(output);
            return Ok(Filled::AsFarAsItGoes);
        }

        let began = Instant::now();
        loop {
            if began.elapsed() >= FILLED_IN_ONE_GO {
                return Ok(Filled::ForNow);
            }
            if track.undecoded() == 0 {
                if track.ended {
                    return Ok(Filled::AtTheEnd);
                }
                track.decoded_at = 0;
                match track.next_block()? {
                    Block::Decoded => {}
                    Block::Ended => {
                        track.ended = true;
                        return Ok(Filled::AtTheEnd);
                    }
                    Block::Reformatted(spec) => {
                        track.sounding = spec;
                        return Ok(Filled::AtAChange);
                    }
                    Block::Awaited => return Ok(Filled::WhenTheSourceAnswers),
                }
                track.sample_packet();
                if track.decoded.frames() == 0 {
                    continue;
                }
            }

            if output.plan.is_transparent() && output.arms.is_none() {
                let written = output.producer.write_from(&track.decoded, track.decoded_at);
                if written == 0 {
                    return Ok(Filled::AsFarAsItGoes);
                }
                output.recall_written(&track.decoded, track.decoded_at, written);
                if let Some(tapping) = output.tapping.as_mut() {
                    tapping.rendered_at(output.producer.rendering());
                    tapping.record(&track.decoded, track.decoded_at, written);
                }
                track.decoded_at = track.decoded_at.saturating_add(written);
            } else {
                if output.producer.free_frames() < output.chain.max_process_frames()
                    || output.waits_for_room_to_reshape()
                {
                    return Ok(Filled::AsFarAsItGoes);
                }
                if !Self::convert(track, output) {
                    return Ok(Filled::AsFarAsItGoes);
                }
            }
        }
    }

    fn convert(track: &mut Track, output: &mut Output) -> bool {
        let channels = track.source().channel_count().get() as usize;
        let frames_in = track.undecoded().min(CHAIN_BLOCK);
        let start = track.decoded_at.saturating_mul(channels);
        let samples = frames_in.saturating_mul(channels);

        let Some(room) = output.widened.get_mut(..samples) else {
            return false;
        };
        let widened = track.decoded.data().widen_into(start, room);
        output.level_the_arms(widened);
        let Some(input) = output.widened.get(..widened) else {
            return false;
        };

        let count = output
            .chain
            .process_recalled(input, &mut output.carrier, &mut output.recall);
        output.render_the_other_arm(
            count.frames_out,
            count.frames_in.min(frames_in),
            track.source(),
        );

        track.decoded_at = track
            .decoded_at
            .saturating_add(count.frames_in.min(frames_in));
        output.stage(count.frames_out);
        let written = output.producer.write(&output.staged);
        if let Some(tapping) = output.tapping.as_mut() {
            tapping.rendered_at(output.producer.rendering());
            tapping.record(&output.staged, 0, written);
        }

        count.frames_in > 0 || count.frames_out > 0
    }

    pub(super) fn flush(output: &mut Output) {
        if output.plan.is_transparent() && output.arms.is_none() {
            output.end();
            return;
        }
        let frames = match output
            .chain
            .flush_recalled(&mut output.carrier, &mut output.recall)
        {
            Ok(frames) => frames,
            Err(error) => {
                tracing::error!(%error, "the chain's tail did not fit the carrier; it was dropped");
                output.end();
                return;
            }
        };

        output.flush_the_other_arm(frames);
        output.stage(frames);
        output.draining = Some(0);
        Self::drain(output);
    }

    pub(super) fn drain(output: &mut Output) {
        let Some(written) = output.draining else {
            return;
        };
        let laid = output.producer.write_from(&output.staged, written);
        if let Some(tapping) = output.tapping.as_mut() {
            tapping.rendered_at(output.producer.rendering());
            tapping.record(&output.staged, written, laid);
        }
        let written = written.saturating_add(laid);

        if written >= output.staged.frames() {
            output.draining = None;
            output.end();
        } else {
            output.draining = Some(written);
        }
    }

    pub(super) fn is_opening_a_stream(&self) -> bool {
        self.streaming.as_ref().is_some_and(Streaming::is_in_flight)
    }

    pub(super) fn waits_on_a_stream(&self) -> bool {
        self.is_opening_a_stream()
            || self
                .output
                .as_ref()
                .is_some_and(|output| output.stream.is_none())
    }

    fn let_go_of_a_stream_nothing_will_take(&mut self) {
        let Some(sink) = self.kept.as_ref().map(|kept| kept.sink) else {
            return;
        };
        let taken = self
            .output
            .as_ref()
            .is_some_and(|output| output.stream.is_none() && output.sink == sink);
        if !taken {
            self.close_what_was_kept();
        }
    }

    pub(super) fn promote(&mut self) {
        if let Some((made, opened)) = self.streaming.as_mut().and_then(Streaming::answered) {
            self.land_the_stream(made, opened);
        }
        if self.is_opening_a_stream() {
            return;
        }
        self.let_go_of_a_stream_nothing_will_take();
        let Some(output) = self.output.as_ref() else {
            return;
        };
        if output.stream.is_some() || !output.primed() || self.retiring.is_some() {
            return;
        }

        let request = StreamRequest {
            target: Some(output.sink),
            spec: output.plan.stream,
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: self
                .track
                .as_ref()
                .and_then(|track| track.info.tags.title.clone())
                .unwrap_or_else(|| self.config.app_name.clone()),
            force_graph_rate: self.config.force_graph_rate,
            no_convert: output.plan.mode != OutputMode::Converted,
            realtime: true,
        };
        let made = output.made;

        let Some(consumer) = self
            .output
            .as_mut()
            .and_then(|output| output.consumer.take())
        else {
            return;
        };
        if self.playing {
            self.wake_the_link();
        }
        let kept = self.kept.take();
        if let Some(output) = self.output.as_mut() {
            output.handed = kept.as_ref().map(|kept| kept.handed);
        }

        let asked = Asked {
            made,
            request,
            source: Box::new(consumer),
            kept: kept.map(|kept| Box::new(kept.stream)),
        };
        let refused = match self.streaming.as_mut() {
            Some(streaming) => streaming.ask(asked).err(),
            None => Some(asked),
        };
        if let Some(asked) = refused {
            let (made, opened) = asked.answered_by(self.opener.as_ref());
            self.land_the_stream(made, opened);
        }
    }

    fn land_the_stream(&mut self, made: u64, opened: SinkResult<(SinkStream, Came)>) {
        let waited_on = self
            .output
            .as_ref()
            .is_some_and(|output| output.made == made && output.stream.is_none());
        let (stream, came) = match (opened, waited_on) {
            (Ok(landed), true) => landed,
            (Ok((stream, _)), false) => {
                tracing::debug!("a stream opened for an output that has since gone is closed");
                close_unwanted(stream);
                return;
            }
            (Err(error), true) => {
                let heard = self.heard_position();
                self.output = None;
                self.unbound.get_or_insert(heard);
                self.fail(error.into());
                return;
            }
            (Err(error), false) => {
                tracing::debug!(%error, "a stream for an output that has since gone did not open");
                return;
            }
        };

        let active = match stream.set_active(self.playing) {
            Ok(()) => self.playing,
            Err(error) => {
                tracing::warn!(%error, "the stream would not take its initial active state");
                false
            }
        };
        if let Some(output) = self.output.as_mut() {
            output.stream = Some(stream);
            output.active = active;
            match came {
                Came::Reshaped { heard_while_kept } => {
                    output.take_what_the_stream_was_handed(heard_while_kept);
                }
                Came::Opened => output.handed = None,
            }
        }
        self.transport = if self.playing {
            TransportState::Playing
        } else {
            TransportState::Paused
        };
    }
}
