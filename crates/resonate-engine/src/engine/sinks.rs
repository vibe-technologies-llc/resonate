use std::sync::Arc;

use crossbeam_channel::{Receiver, TryRecvError};
use resonate_audio::SinkInfo;
use resonate_core::{Frames, Volume};

use super::{Engine, Retuning, SINK_TIMEOUT, output::chosen_sink};
use crate::{
    Error, Result, TransportState,
    pipeline::Attenuator,
    surveying::{Surveyed, Surveying},
};

impl Engine {
    pub(super) fn note_sink_changes(&mut self) {
        loop {
            match self.changes.as_ref().map(Receiver::try_recv) {
                Some(Ok(_)) => self.stale_sinks = true,
                Some(Err(TryRecvError::Disconnected)) => {
                    tracing::warn!("the backend stopped announcing sinks; the list is now frozen");
                    self.changes = None;
                    return;
                }
                Some(Err(TryRecvError::Empty)) | None => return,
            }
        }
    }

    pub(super) fn rediscover(&mut self) {
        self.note_sink_changes();
        if let Some(found) = self.surveying.as_mut().and_then(Surveying::answered) {
            self.take_the_survey(found);
        }
        if !self.stale_sinks {
            return;
        }

        match self.surveying.as_mut().filter(|survey| survey.is_running()) {
            Some(surveying) => {
                if surveying.ask() {
                    self.stale_sinks = false;
                }
            }
            None if self.output.is_none() => {
                self.stale_sinks = false;
                let found = self.surveyor.enumerate_sinks(SINK_TIMEOUT);
                self.take_the_survey(found);
            }
            None => {}
        }
    }

    fn take_the_survey(&mut self, found: Surveyed) {
        match found {
            Ok(found) => {
                *self.published.sinks.write() = found.into();
                self.follow_the_devices_volume();
                self.follow_the_sink_it_would_choose();
                self.follow_the_names_of_the_sink();
                self.follow_the_formats_it_was_told_of();
                self.bind_the_row_waiting_for_a_device();
                self.bind_the_row_the_graph_let_go();
            }
            Err(error) if self.graph_lost.is_some() => {
                tracing::debug!(%error, "the graph is not back yet");
                self.graph_still_away = Some(Error::Sink(error));
            }
            Err(error) => tracing::warn!(%error, "the sink list could not be refreshed"),
        }
    }

    pub(super) fn bound_sink(&self) -> Option<SinkInfo> {
        let bound = self.output.as_ref()?.sink;
        self.published
            .sinks
            .read()
            .iter()
            .find(|sink| sink.id == bound)
            .cloned()
    }

    pub(super) fn weigh_whose_volume_it_is(&mut self) {
        let Some(sink) = self.bound_sink() else {
            return;
        };
        let attenuator = Attenuator::of(&self.config, &sink);
        let Some(output) = self.output.as_mut() else {
            return;
        };
        output.status.device_turned = attenuator == Attenuator::Device;
        output.status.device_muted = attenuator.hears_the_mute_of(&sink);
        if output.attenuator == attenuator {
            return;
        }
        output.attenuator = attenuator;
        if attenuator == Attenuator::Device {
            self.take_the_devices_volume(&sink);
        }
    }

    pub(super) fn take_the_devices_volume(&mut self, sink: &SinkInfo) {
        let Some(heard) = sink.port.as_ref().and_then(|port| port.volume) else {
            return;
        };
        self.config.volume = Volume::heard_at(heard);
        self.muted = false;
    }

    pub(super) fn hear_the_volume(&mut self, volume: Volume) {
        self.muted = volume == Volume::MUTE && self.config.volume != Volume::MUTE;
        if !self.muted {
            self.config.volume = volume;
        }
    }

    pub(super) fn heard_volume(&self) -> Volume {
        if self.muted {
            Volume::MUTE
        } else {
            self.config.volume
        }
    }

    fn follow_the_devices_volume(&mut self) {
        let turned_here = self
            .output
            .as_ref()
            .is_some_and(|output| output.attenuator == Attenuator::Device);
        if !turned_here {
            return;
        }
        let Some(sink) = self.bound_sink() else {
            return;
        };
        if let Some(output) = self.output.as_mut() {
            output.status.device_muted = output.attenuator.hears_the_mute_of(&sink);
        }
        let Some(heard) = sink.port.as_ref().and_then(|port| port.volume) else {
            return;
        };
        if self.turned.echo(heard, self.heard_volume().to_gain()) {
            return;
        }
        tracing::debug!(%heard, "the device's volume was turned from elsewhere; the slider follows it");
        self.config.volume = Volume::heard_at(heard);
        self.muted = false;
    }

    pub(super) fn turn_the_device(&mut self) {
        let Some(output) = self
            .output
            .as_ref()
            .filter(|output| output.attenuator == Attenuator::Device)
        else {
            return;
        };
        let gain = self.heard_volume().to_gain();
        if let Err(error) = self.backend.set_device_volume(output.sink, gain) {
            tracing::warn!(%error, "the device's volume could not be turned");
            return;
        }
        self.turned.sent(gain);
    }

    pub(super) fn mute_the_device(&self, muted: bool) -> Result<()> {
        let Some(output) = self
            .output
            .as_ref()
            .filter(|output| output.attenuator == Attenuator::Device)
        else {
            tracing::debug!("no device turns the volume here, so it has no mute to set");
            return Ok(());
        };
        Ok(self.backend.set_device_mute(output.sink, muted)?)
    }

    fn follow_the_names_of_the_sink(&mut self) {
        let Some(current) = self.bound_sink() else {
            return;
        };
        let Some(output) = self.output.as_mut() else {
            return;
        };
        if output.bound.name == current.name && output.bound.aliases == current.aliases {
            return;
        }

        let equaliser = &self.config.equaliser;
        let heard_before = equaliser.for_sink(&output.bound).map(Arc::as_ptr);
        let heard_now = equaliser.for_sink(&current).map(Arc::as_ptr);
        output.bound.name = current.name;
        output.bound.aliases = current.aliases;
        if heard_before == heard_now {
            return;
        }
        if let Err(error) = self.retune_for(Retuning::Equaliser) {
            tracing::warn!(%error, "the equaliser a renamed sink is bound to could not be rendered");
        }
    }

    fn follow_the_sink_it_would_choose(&mut self) {
        let Some(bound) = self.output.as_ref().map(|output| output.sink) else {
            return;
        };
        let chosen = chosen_sink(&self.published.sinks.read(), self.config.sink.as_ref())
            .map(|sink| sink.id);
        if chosen.is_none_or(|chosen| chosen == bound) {
            return;
        }

        tracing::info!(
            from = %bound,
            "the device this build would play on moved; the stream follows it"
        );
        let at = self.position();
        if let Err(error) = self.rebind(Some(at), None) {
            self.fail(error);
        }
    }

    fn follow_the_formats_it_was_told_of(&mut self) {
        if !self.formats_owed || self.stale_sinks {
            return;
        }
        self.formats_owed = false;
        if self.output.is_none() {
            return;
        }
        if let Err(error) = self.reopen_where_the_stream_moves() {
            self.fail(error);
        }
    }

    pub(super) fn select_sink(&mut self) -> Result<SinkInfo> {
        if self.sinks_are_stale() && !self.the_survey_answers_for_the_list() {
            self.stale_sinks = false;
            match self.surveyor.enumerate_sinks(SINK_TIMEOUT) {
                Ok(found) => *self.published.sinks.write() = found.into(),
                Err(error) if self.published.sinks.read().is_empty() => return Err(error.into()),
                Err(error) => {
                    tracing::warn!(%error, "the sink list could not be refreshed; binding to what was last published");
                }
            }
        }

        chosen_sink(&self.published.sinks.read(), self.config.sink.as_ref())
            .cloned()
            .ok_or(Error::Sink(resonate_audio::Error::NoSink))
    }

    pub(super) fn the_survey_runs(&self) -> bool {
        self.surveying.as_ref().is_some_and(Surveying::is_running)
    }

    fn the_survey_answers_for_the_list(&self) -> bool {
        self.the_survey_runs() && self.changes.is_some()
    }

    pub(super) fn the_survey_will_answer_for_a_list_known_wrong(&self) -> bool {
        let asked = self.surveying.as_ref().is_some_and(Surveying::is_asked);
        (self.stale_sinks || asked)
            && self.the_survey_answers_for_the_list()
            && (self.waiting_for_a_device
                || self.graph_lost.is_some()
                || self.published.sinks.read().is_empty())
    }

    pub(super) fn wait_for_the_survey(&mut self, at: Option<Frames>) {
        tracing::debug!("the sink list is known wrong; the row is bound once the survey answers");
        let at = at.unwrap_or_else(|| self.position());
        self.unbound = Some(at);
        self.transport = if self.playing {
            TransportState::Loading
        } else {
            TransportState::Paused
        };
        if self.graph_lost.is_none() {
            self.waiting_for_a_device = true;
        }
    }

    fn sinks_are_stale(&self) -> bool {
        self.stale_sinks || self.changes.is_none() || self.published.sinks.read().is_empty()
    }
}
