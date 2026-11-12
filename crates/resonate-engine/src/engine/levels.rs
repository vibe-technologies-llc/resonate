use std::{mem, sync::Arc};

use resonate_codec::Decoder;

use super::{Engine, Retuning, levelled};
use crate::Result;

impl Engine {
    pub(super) fn re_level(&mut self) -> Result<()> {
        if let Some(track) = self.track.as_mut() {
            track.replay_gain = levelled(&self.config, &track.info, track.hints);
            track.published = None;
            track.measure_where_wanted(&self.sources, &self.config);
        }
        self.retune_heard_past_the_buffer(Retuning::Level)
    }

    pub(super) fn settle_what_was_spooled(&mut self) {
        let Some(track) = self.track.as_mut() else {
            return;
        };
        if track.info.is_seekable {
            return;
        }
        let Some(settled) = track.decoding.home().and_then(Decoder::settle_the_spool) else {
            return;
        };
        let seekable = settled.is_seekable;
        let info = Arc::make_mut(&mut track.info);
        info.is_seekable = seekable;
        info.duration = settled.duration;
        info.playable = settled.playable;
        info.splices = settled.splices.clone();
        track.published = None;
        if !seekable || !mem::take(&mut self.rebind_owed) {
            return;
        }
        if let Err(error) = self.rebind_where_it_stands() {
            self.fail(error);
        }
    }

    pub(super) fn heed_the_measured_peak(&mut self) {
        let heeded = self
            .track
            .as_mut()
            .is_some_and(|track| track.heed_what_was_measured(&self.config));
        if !heeded {
            return;
        }
        if let Err(error) = self.retune() {
            tracing::warn!(%error, "the peak measured for the playing track could not be heeded");
        }
    }
}
