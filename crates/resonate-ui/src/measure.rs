use resonate_engine::{DspError, NodeName};
use resonate_eq::{Error as EqError, Response};
use resonate_listen::{Error as ListenError, Hearing, Listening};
use thiserror::Error;

pub trait Measures: Send + Sync {
    fn measure(
        &self,
        through: Option<&NodeName>,
        from: &Listening,
        hearing: &Hearing,
        calibration: Option<&Response>,
    ) -> Result<Response, Unmeasured>;
}

#[derive(Debug, Error)]
pub enum Unmeasured {
    #[error("the sweep could not be played and heard")]
    Heard(#[source] Box<ListenError>),

    #[error("what was heard could not be read as the room's response")]
    Deconvolved(#[source] DspError),

    #[error("the room's response could not be made a measurement")]
    Response(#[source] EqError),
}

impl Unmeasured {
    pub(crate) fn is_silence(&self) -> bool {
        matches!(
            self,
            Self::Heard(heard) if matches!(**heard, ListenError::NothingHeard)
        ) || matches!(self, Self::Deconvolved(DspError::NothingHeard))
    }

    pub(crate) fn is_stopped(&self) -> bool {
        matches!(self, Self::Heard(heard) if matches!(**heard, ListenError::Stopped))
    }

    pub(crate) fn is_a_missing_output(&self) -> bool {
        match self {
            Self::Heard(heard) => matches!(
                **heard,
                ListenError::NoSuchSink { .. } | ListenError::NoOutput
            ),
            Self::Deconvolved(_) | Self::Response(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::*;

    #[test]
    fn an_error_of_the_seam_is_small_enough_to_return() {
        assert!(size_of::<Unmeasured>() <= 128);
    }

    #[test]
    fn silence_is_told_apart_from_a_sound_that_could_not_be_made() {
        let quiet = Unmeasured::Heard(Box::new(ListenError::NothingHeard));
        let deaf = Unmeasured::Deconvolved(DspError::NothingHeard);
        let no_output = Unmeasured::Heard(Box::new(ListenError::NoOutput));

        assert!(quiet.is_silence() && deaf.is_silence());
        assert!(!no_output.is_silence() && no_output.is_a_missing_output());
        assert!(!quiet.is_a_missing_output());
    }

    #[test]
    fn a_sweep_the_listener_stopped_is_told_apart_from_one_that_failed() {
        let stopped = Unmeasured::Heard(Box::new(ListenError::Stopped));
        let quiet = Unmeasured::Heard(Box::new(ListenError::NothingHeard));

        assert!(stopped.is_stopped());
        assert!(!quiet.is_stopped());
        assert!(!stopped.is_silence());
    }
}
