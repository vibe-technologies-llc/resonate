use resonate_codec::Packing;
use resonate_core::abx::Arm;

use super::{Auditioning, Engine, Retuning, splicing::Splicing};
use crate::{
    EngineConfig, Error, Event, OutputPlan, Result,
    audition::{Audition, Contested, plans_of},
};

impl Engine {
    pub(super) fn settings_as_set(&self) -> EngineConfig {
        let mut set = self.config.clone();
        if let Some(auditioning) = self.auditioning.as_ref() {
            auditioning.before.put_back(&mut set);
        }
        set
    }

    pub(super) fn audition(&mut self, asked: Option<Audition>) -> Result<()> {
        let Some(asked) = asked else {
            return self.end_the_audition();
        };
        let running = self.auditioning.as_ref().map(|running| running.audition);
        match running {
            Some(running)
                if running.contest == asked.contest && running.lowered == asked.lowered =>
            {
                self.hear_the_arm(asked.heard)
            }
            Some(_) | None => self.begin_the_audition(asked),
        }
    }

    fn begin_the_audition(&mut self, asked: Audition) -> Result<()> {
        let set = self.settings_as_set();
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_ref()) else {
            return Err(Error::NothingToAudition);
        };
        if !matches!(output.plan.packing, Packing::Samples) {
            return Err(Error::NothingToAudition);
        }
        let planned_for = self.bound_sink().unwrap_or_else(|| output.bound.clone());
        let (a, b) = plans_of(
            track.decoded(),
            &planned_for,
            output.plan.stream,
            &set,
            track.replay_gain,
            output.attenuator,
            asked.contest,
        );
        if asked.contest.renders_alike(&a, &b) {
            return Err(Error::ArmsAlike);
        }

        let before = Contested::of(&set);
        self.config = set;
        asked.contest.apply(asked.heard, &mut self.config);
        self.auditioning = Some(Auditioning {
            audition: asked,
            before,
        });
        self.rebind_where_it_stands()
    }

    fn hear_the_arm(&mut self, arm: Arm) -> Result<()> {
        let Some(running) = self.auditioning.as_mut() else {
            return Ok(());
        };
        if running.audition.heard == arm {
            return Ok(());
        }
        running.audition.heard = arm;
        let contest = running.audition.contest;
        contest.apply(arm, &mut self.config);
        self.switch_the_arms()
    }

    fn switch_the_arms(&mut self) -> Result<()> {
        let Some(heard) = self
            .auditioning
            .as_ref()
            .map(|running| running.audition.heard)
        else {
            return Ok(());
        };
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        if output
            .arms
            .as_ref()
            .is_some_and(|arms| arms.heard() == heard)
        {
            output.switch_waits_for_room = false;
            return Ok(());
        }
        let downstream = output.holds_seconds_downstream();
        if !self.boundaries.is_empty() && output.arms.is_some() {
            output.switch_owed = true;
            if downstream {
                self.owe_a_downstream_restart();
            }
            return Ok(());
        }
        let switched = output.hear_the_other_arm(self.playing);
        if downstream {
            self.owe_a_downstream_restart();
        }
        match switched {
            Splicing::Spliced(status) => {
                self.emit(Event::OutputChanged(status));
                Ok(())
            }
            Splicing::Owed => Ok(()),
            Splicing::Unspliceable => self.rebind_where_it_stands(),
        }
    }

    pub(super) fn switch_what_is_owed(&mut self) {
        let owed = self
            .output
            .as_ref()
            .is_some_and(|output| output.switch_owed && !output.producer.is_splicing());
        if !owed || !self.boundaries.is_empty() {
            return;
        }
        if let Some(output) = self.output.as_mut() {
            output.switch_owed = false;
        }
        if let Err(error) = self.switch_the_arms() {
            tracing::warn!(%error, "the arm of the listening test owed could not be heard");
        }
    }

    pub(super) fn forget_the_audition(&mut self) {
        let Some(running) = self.auditioning.take() else {
            return;
        };
        running.before.put_back(&mut self.config);
        if let Some(output) = self.output.as_mut() {
            output.let_go_of_the_other_arm();
            output.switch_owed = false;
            output.switch_waits_for_room = false;
        }
    }

    fn end_the_audition(&mut self) -> Result<()> {
        let Some(running) = self.auditioning.as_ref().map(|running| running.audition) else {
            return Ok(());
        };
        let set = self.settings_as_set();
        let as_set = Contested::of(&set);
        let arm_as_set = [Arm::A, Arm::B]
            .into_iter()
            .find(|&arm| Contested::of(&running.contest.applied(arm, &set)) == as_set);
        let Some(arm) = arm_as_set.filter(|&arm| running.lowered.gain_of(arm) >= 1.0) else {
            self.forget_the_audition();
            return self.rebind_where_it_stands();
        };
        self.hear_the_arm(arm)?;
        self.forget_the_audition();
        Ok(())
    }

    pub(super) fn splice_the_back(&mut self, wanted: OutputPlan) -> Result<Splicing> {
        if !self.boundaries.is_empty()
            && let Some(output) = self.output.as_mut()
        {
            output.splice_owed = true;
            return Ok(Splicing::Owed);
        }
        let (Some(track), Some(output)) = (self.track.as_mut(), self.output.as_mut()) else {
            return Ok(Splicing::Unspliceable);
        };
        output.splice_the_back(track, wanted, &self.config)
    }

    pub(super) fn splice_what_is_owed(&mut self) {
        let owed = self
            .output
            .as_ref()
            .is_some_and(|output| output.splice_owed && !output.producer.is_splicing());
        if !owed || !self.boundaries.is_empty() {
            return;
        }
        if let Some(output) = self.output.as_mut() {
            output.splice_owed = false;
        }
        if let Err(error) = self.retune_for(Retuning::Equaliser) {
            tracing::warn!(%error, "the equaliser owed to what the ring held could not be rendered");
        }
    }
}
