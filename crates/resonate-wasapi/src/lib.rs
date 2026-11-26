mod clock;
mod fill;
mod formats;
mod opening;
mod render;
mod serving;
mod survey;
mod surveying;
mod ways;

#[cfg(windows)]
mod backend;
#[cfg(windows)]
mod device;
#[cfg(windows)]
mod priority;
#[cfg(windows)]
mod stream;

pub use resonate_audio::{
    Pace, changes_between, gain_of, refusal_of, scalar_of, sink_id_of, watch,
};

#[cfg(windows)]
pub use crate::backend::Wasapi;
pub use crate::{
    clock::{ClockReading, CounterNow},
    fill::{Filled, fill_buffer},
    formats::{
        PROBED_LAYOUTS, PROBED_RATES, Wire, format_of, formats_taken, mask_of, rates_of, wire_of,
    },
    opening::{Initialize, Refusal, Status, initialized, period_of, refused_at_open},
    render::{Buffer, Cycle},
    serving::{Cycled, Feeding, Hardware, Woken, launch, words_of},
    survey::{Endpoint, Endpoints, Mix, Probe, Probed},
    surveying::{Surveys, joined_within},
    ways::{Way, endpoint_of, node_of},
};
