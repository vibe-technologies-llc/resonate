use std::time::Instant;

use resonate_core::Frames;

use super::{DISCARD_TIMEOUT, Engine, Unanswered};
use crate::{CommandKind, Error, Result};

impl Engine {
    pub(super) fn seek(&mut self, to: Frames) -> Result<()> {
        self.take_the_boundary_back();
        if let Some(opening) = self.opening.as_mut() {
            opening.at = to;
            return Ok(());
        }
        let Some(track) = self.track.as_ref() else {
            return Err(Error::InvalidTransition {
                state: self.transport,
                attempted: CommandKind::Seek,
            });
        };
        if let Some(duration) = track.info.duration
            && to > duration
        {
            return Err(Error::SeekOutOfRange {
                track: track.id,
                requested: to,
                duration,
            });
        }
        if !track.info.is_seekable {
            return self.seek_where_nothing_seeks(to);
        }
        let landed = if self.reuses_stream() {
            self.seek_in_place(to)
        } else {
            self.rebind(Some(to), None)
        };
        if landed.is_ok() {
            self.seeks = self.seeks.stepped();
            self.heard_at_least = None;
            self.lift_the_sleep_fade();
        }
        landed
    }

    fn seek_where_nothing_seeks(&mut self, to: Frames) -> Result<()> {
        let Some(track) = self.track.as_ref() else {
            return Ok(());
        };
        if self.position() == to {
            return Ok(());
        }
        if to == Frames::ZERO {
            self.seeks = self.seeks.stepped();
            return self.start(Frames::ZERO);
        }
        Err(Error::Decode {
            track: track.id,
            source: resonate_codec::Error::NotSeekable {
                location: track.location.clone(),
            },
        })
    }

    pub(super) fn rebind_where_it_stands(&mut self) -> Result<()> {
        let streaming_what_cannot_seek = self.output.is_some()
            && self
                .track
                .as_ref()
                .is_some_and(|track| !track.info.is_seekable);
        if streaming_what_cannot_seek {
            tracing::debug!(
                "a track that cannot seek keeps its stream until it can or the next begins"
            );
            self.rebind_owed = true;
            return Ok(());
        }
        let at = self.position();
        self.rebind(Some(at), None)
    }

    fn reuses_stream(&self) -> bool {
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_ref()) else {
            return false;
        };
        self.playing
            && track.info.is_seekable
            && output.holds_a_live_stream()
            && !output.holds_seconds_downstream()
            && !output.rendered_for_another
    }

    fn seek_in_place(&mut self, to: Frames) -> Result<()> {
        if let Some(track) = self.track.as_mut() {
            track.seek(to)?;
            track.rewind_carry();
            track.restart_profile();
        }

        if let Some(output) = self.output.as_mut() {
            output.chain.reset();
            output.recall.forget();
            output.start_the_arms_over();
            output.recall_spoiled = false;
            output.producer.discard_buffered();
            output.discarding_since = Some(Instant::now());
            if let Some(tapping) = output.tapping.as_ref() {
                tapping.forget();
            }
        }
        Ok(())
    }

    pub(super) fn watch_discard(&mut self) {
        let Some(unanswered) = self.unanswered_discard() else {
            return;
        };
        match unanswered {
            Unanswered::NothingPulling => tracing::debug!(
                "the transport stopped before the graph took the seek; rebuilding the ring around it"
            ),
            Unanswered::GraphStalled => tracing::warn!(
                "the graph did not drain the ring after a seek; rebuilding the stream around it"
            ),
        }
        let at = self.position();
        if let Err(error) = self.rebind_on_a_new_stream(Some(at), None) {
            self.fail(error);
        }
    }

    fn unanswered_discard(&mut self) -> Option<Unanswered> {
        let playing = self.playing;
        let output = self.output.as_mut()?;
        let since = output.discarding_since?;
        if !output.producer.is_discarding() {
            output.discarding_since = None;
            return None;
        }
        if !playing {
            return Some(Unanswered::NothingPulling);
        }
        (since.elapsed() >= DISCARD_TIMEOUT).then_some(Unanswered::GraphStalled)
    }
}
