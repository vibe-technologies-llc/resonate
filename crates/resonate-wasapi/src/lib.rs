mod clock;
mod fill;
mod formats;
mod ways;

#[cfg(windows)]
mod backend;
#[cfg(windows)]
mod device;
#[cfg(windows)]
mod priority;
#[cfg(windows)]
mod render;
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
    ways::{Way, endpoint_of, node_of},
};
