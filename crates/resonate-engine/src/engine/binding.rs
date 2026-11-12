use std::time::{Duration, Instant};

use resonate_core::{Frames, StreamSpec};

use super::{
    DOWNSTREAM_RESTARTED_AFTER_QUIET_FOR, Engine, ReadBy, Retuning,
    output::{Output, Retire, chosen_sink},
    ring_capacity,
};
use crate::{
    Event, OutputPlan, Result, TransportState,
    pipeline::{Attenuator, plan_for, plan_output},
    ring::Entering,
};

impl Engine {
    pub(super) fn reopen_where_the_stream_moves(&mut self) -> Result<()> {
        if self.keeps_its_stream() {
            return Ok(());
        }
        self.rebind_where_it_stands()
    }

    pub(super) fn rebind_where_the_plan_moves(&mut self, read_by: ReadBy) -> Result<()> {
        if self.keeps_its_plan(read_by) {
            return Ok(());
        }
        self.rebind_where_it_stands()
    }

    fn keeps_its_plan(&self, read_by: ReadBy) -> bool {
        let (Some(output), Some(wanted)) = (self.output.as_ref(), self.plan_wanted_now()) else {
            return false;
        };
        !read_by.reaches_into(&output.plan) && wanted.renders_alike(&output.plan)
    }

    pub(super) fn stays_on_the_bound_device(&self) -> bool {
        let Some(output) = self.output.as_ref() else {
            return false;
        };
        chosen_sink(&self.published.sinks.read(), self.config.sink.as_ref())
            .is_some_and(|sink| sink.id == output.sink && sink.is_named(&output.bound.name))
    }

    pub(super) fn plan_wanted_now(&self) -> Option<OutputPlan> {
        let (track, output) = (self.track.as_ref()?, self.output.as_ref()?);
        if output.plan.stream != output.status.negotiated {
            return None;
        }
        let sink = self.bound_sink()?;
        Some(plan_output(
            track.decoded(),
            &sink,
            &self.config,
            track.replay_gain,
        ))
    }

    fn keeps_its_stream(&self) -> bool {
        self.plan_wanted_now()
            .is_some_and(|wanted| self.keeps_its_stream_for(&wanted))
    }

    pub(super) fn keeps_its_stream_for(&self, wanted: &OutputPlan) -> bool {
        let Some(output) = self.output.as_ref() else {
            return false;
        };
        let capacity = ring_capacity(
            self.config.buffer,
            wanted.stream,
            output.chain.max_process_frames(),
        );
        wanted.stream == output.plan.stream
            && wanted.packing == output.plan.packing
            && capacity.get() as usize == output.capacity
    }

    pub(super) fn rebind(&mut self, at: Option<Frames>, target: Option<StreamSpec>) -> Result<()> {
        self.rebind_with(at, target, Retire::HandingOnItsStream)
    }

    pub(super) fn rebind_on_a_new_stream(
        &mut self,
        at: Option<Frames>,
        target: Option<StreamSpec>,
    ) -> Result<()> {
        self.rebind_with(at, target, Retire::ClosingItsStream)
    }

    fn rebind_with(
        &mut self,
        at: Option<Frames>,
        target: Option<StreamSpec>,
        how: Retire,
    ) -> Result<()> {
        if self.track.is_none() {
            return Ok(());
        }
        let resumes_at = at.unwrap_or_else(|| self.position());
        let at = if self.take_the_boundary_back() {
            Some(resumes_at)
        } else {
            at
        };
        self.retire(how);

        let bound = self.bind(at, target);
        if bound.is_err() && self.track.is_some() {
            self.unbound = Some(resumes_at);
        }
        bound
    }

    fn bind(&mut self, at: Option<Frames>, target: Option<StreamSpec>) -> Result<()> {
        if let Some(track) = self.track.as_mut()
            && let Some(at) = at
            && track.decoding.position() != at
            && track.info.is_seekable
        {
            track.seek(at)?;
        }

        if !self.playing && self.unbound.is_some() {
            let at = at.unwrap_or_else(|| self.position());
            self.unbound = Some(at);
            self.transport = TransportState::Paused;
            return Ok(());
        }

        self.note_sink_changes();
        if self.the_survey_will_answer_for_a_list_known_wrong() {
            self.wait_for_the_survey(at);
            return Ok(());
        }
        let sink = self.select_sink()?;
        let Some(track) = self.track.as_ref() else {
            return Ok(());
        };
        let entering = if self.lockstep.is_some() {
            Entering::InStep
        } else if track.at_a_change && at.is_none() {
            Entering::Whole
        } else if at.unwrap_or_else(|| track.decoding.position()) > Frames::ZERO {
            Entering::FadedIn
        } else {
            Entering::Whole
        };
        let mut output = Output::open(
            track,
            &sink,
            &self.config,
            target,
            &self.published.listening,
            entering,
            self.muted,
        )?;
        if let Some(auditioning) = self.auditioning.as_ref() {
            output.take_up_the_audition(track, &self.config, &auditioning.audition)?;
        }

        let delivery = output.plan.delivery();
        if let Some(track) = self.track.as_mut() {
            track.at_a_change = false;
            track.decoding.deliver(delivery);
            track.rewind_carry();
            track.restart_profile();
        }

        if output.attenuator == Attenuator::Device {
            self.take_the_devices_volume(&sink);
        }
        let status = output.status;
        self.outputs_made = self.outputs_made.wrapping_add(1);
        self.downstream_restart_due = None;
        self.output = Some(Output {
            made: self.outputs_made,
            ..output
        });
        self.transport = TransportState::Loading;
        self.emit(Event::OutputChanged(status));
        Ok(())
    }

    pub(super) fn retune(&mut self) -> Result<()> {
        self.retune_for(Retuning::Level)
    }

    pub(super) fn retune_heard_past_the_buffer(&mut self, retuning: Retuning) -> Result<()> {
        let made = self.outputs_made;
        let heard_otherwise = self.renders_otherwise_for_a_renderer();
        let retuned = self.retune_for(retuning);
        if heard_otherwise && self.outputs_made == made {
            self.owe_a_downstream_restart();
        }
        retuned
    }

    fn renders_otherwise_for_a_renderer(&self) -> bool {
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_ref()) else {
            return false;
        };
        let planned_for = self.bound_sink().unwrap_or_else(|| output.bound.clone());
        output.holds_seconds_downstream()
            && !plan_for(
                track.decoded(),
                &planned_for,
                output.plan.stream,
                &self.config,
                track.replay_gain,
                output.attenuator,
            )
            .renders_alike(&output.plan)
    }

    pub(super) fn owe_a_downstream_restart(&mut self) {
        self.downstream_restart_due = Some(Instant::now() + DOWNSTREAM_RESTARTED_AFTER_QUIET_FOR);
    }

    pub(super) fn downstream_restart_in(&self) -> Option<Duration> {
        self.downstream_restart_due
            .map(|due| due.saturating_duration_since(Instant::now()))
    }

    pub(super) fn restart_what_a_renderer_holds(&mut self) {
        let Some(due) = self.downstream_restart_due else {
            return;
        };
        if Instant::now() < due || !self.boundaries.is_empty() {
            return;
        }
        self.downstream_restart_due = None;
        let holds_seconds = self
            .output
            .as_ref()
            .is_some_and(Output::holds_seconds_downstream);
        if !holds_seconds {
            return;
        }
        if let Err(error) = self.rebind_where_it_stands() {
            tracing::warn!(%error, "a renderer's buffer could not be restarted for the change made");
        }
    }
}
