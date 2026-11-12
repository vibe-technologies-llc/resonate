use resonate_dsp::Easing;
use smallvec::SmallVec;

use super::{CHAIN_BLOCK, Engine, Retuning, Track, output::heard_amplitude, splicing::Splicing};
use crate::{
    Error, Event, OutputPlan, Result,
    pipeline::{packs_again, plan_for, replay_gain_as_planned},
    ring::{TURNS_WAITING_AT_MOST, Turned},
};

impl Engine {
    pub(super) fn retune_for(&mut self, retuning: Retuning) -> Result<()> {
        self.weigh_whose_volume_it_is();
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_mut()) else {
            return Ok(());
        };
        output.settles_into = None;
        let replay_gain = track.replay_gain;
        let decoded_packing = track.decoded().packing;
        let packs = self
            .published
            .sinks
            .read()
            .iter()
            .find(|sink| sink.is_named(&output.bound.name))
            .is_some_and(|sink| {
                packs_again(
                    track.decoded(),
                    &output.plan,
                    sink,
                    &self.config,
                    replay_gain,
                )
            });
        if packs {
            return self.rebind_where_it_stands();
        }
        let planned_for = self
            .published
            .sinks
            .read()
            .iter()
            .find(|sink| sink.id == output.sink)
            .map_or_else(|| output.bound.clone(), Clone::clone);
        let wanted = plan_for(
            track.decoded(),
            &planned_for,
            output.plan.stream,
            &self.config,
            replay_gain,
            output.attenuator,
        );
        let entering = heard_amplitude(&wanted, self.muted);
        let heard_as_planned = |track: &Track| {
            let plan = plan_for(
                track.decoded(),
                &planned_for,
                output.plan.stream,
                &self.config,
                track.replay_gain,
                output.attenuator,
            );
            heard_amplitude(&plan, self.muted)
        };
        match self.boundaries.front() {
            Some(front) => {
                let leaving = heard_as_planned(&front.leaving);
                let after: SmallVec<[(Turned, f32); TURNS_WAITING_AT_MOST]> = self
                    .boundaries
                    .iter()
                    .zip(
                        self.boundaries
                            .iter()
                            .skip(1)
                            .map(|next| heard_as_planned(&next.leaving))
                            .chain([entering]),
                    )
                    .map(|(boundary, heard)| (boundary.turned, heard))
                    .collect();
                output.producer.hear_across(leaving, &after);
            }
            None => output.producer.hear_at(entering),
        }

        let auditioned = output.arms.is_some();
        if retuning == Retuning::Equaliser
            && !auditioned
            && wanted.equalisation != output.plan.equalisation
        {
            match self.splice_the_back(wanted.clone())? {
                Splicing::Spliced(status) => {
                    self.emit(Event::OutputChanged(status));
                    return Ok(());
                }
                Splicing::Owed => return Ok(()),
                Splicing::Unspliceable => {}
            }
        }
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        if wanted.same_shape_as(&output.plan) {
            let volume = output.attenuator.leaves(self.config.volume);
            let planned = replay_gain_as_planned(decoded_packing, &self.config, replay_gain);
            output.chain.set_gain(volume, planned);
            if let Some(arms) = output.arms.as_mut() {
                arms.set_gain_of_the_other(volume, planned);
            }
            output.note_the_rendered_level();
            if let Some(profile) = wanted.equalisation.as_ref() {
                output.chain.set_equalisation(profile);
                output.chain.ease_equalisation(Easing::Returning);
                if let Some(arms) = output.arms.as_mut() {
                    arms.set_equalisation_of_the_other(profile);
                }
            }
            output.plan = wanted;
            return Ok(());
        }
        if auditioned {
            return self.rebind_where_it_stands();
        }
        if output.plan.becomes_on_the_same_stream(&wanted) {
            return self.reshape(wanted);
        }
        self.rebind_where_it_stands()
    }

    pub(super) fn reshape(&mut self, wanted: OutputPlan) -> Result<()> {
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        let drops_the_equaliser =
            wanted.equalisation.is_none() && output.plan.equalisation.is_some();
        if drops_the_equaliser && self.playing {
            output.chain.ease_equalisation(Easing::Leaving);
        }
        if drops_the_equaliser && output.chain.is_ramping() {
            output.settles_into = Some(wanted);
            return Ok(());
        }
        self.swap_chain(wanted)
    }

    pub(super) fn finish_reshaping(&mut self) {
        let settled = self
            .output
            .as_mut()
            .filter(|output| output.settles_into.is_some() && !output.chain.is_ramping())
            .and_then(|output| output.settles_into.take());
        let Some(wanted) = settled else {
            return;
        };
        if let Err(error) = self.swap_chain(wanted) {
            tracing::warn!(%error, "the chain could not be reshaped once its gain had settled");
        }
    }

    pub(super) fn swap_chain(&mut self, wanted: OutputPlan) -> Result<()> {
        let (Some(track), Some(output)) = (self.track.as_mut(), self.output.as_mut()) else {
            return Ok(());
        };
        let still_filling = output.draining.is_none() && !output.ended;
        let carries = output.plan.carries_the_front_into(&wanted);
        if still_filling && !output.has_room_for_what_the_chain_holds(carries) {
            output.settles_into = Some(wanted);
            return Ok(());
        }
        let front = carries.then(|| output.chain.take_the_front()).flatten();
        if still_filling {
            output.hand_on_what_the_chain_holds();
        }
        let source = track.source();
        let built = match front {
            Some(front) => wanted.build_chain_after(front, &self.config, CHAIN_BLOCK),
            None => wanted.build_chain(source, &self.config, CHAIN_BLOCK),
        };
        let mut chain = built.map_err(|error| Error::Convert {
            track: track.id,
            source_spec: source,
            sink_spec: wanted.stream,
            source: error,
        })?;
        let brings_the_equaliser =
            wanted.equalisation.is_some() && output.plan.equalisation.is_none();
        if brings_the_equaliser && self.playing {
            chain.ease_equalisation(Easing::Entering);
        }

        let delivery = wanted.delivery();
        track.decoding.deliver(delivery);
        track.decoded.retype(delivery.format);
        output
            .widened
            .resize(CHAIN_BLOCK * source.channel_count().get() as usize, 0.0);

        let status = output.take_chain(chain, wanted);
        output.note_the_rendered_level();
        self.emit(Event::OutputChanged(status));
        Ok(())
    }
}
