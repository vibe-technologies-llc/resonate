mod agents;
mod failed;
mod ranged;
mod stall;
mod trust;
mod waited;
mod words;

pub use crate::{
    agents::{
        CONNECTED_WITHIN, Patience, Trusted, USER_AGENT, asking_agent, configured,
        downloading_agent,
    },
    failed::{as_io, unreached},
    ranged::{ASKED_LATER, RESUMES_AT_MOST, Ranged, Unfetched, waited_before},
    stall::BrokenOffAfter,
    trust::{certificate_refused, system_and_built_in},
    waited::{retry_after, retry_after_of},
    words::{escaped, is_a_document},
};
