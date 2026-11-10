#[cfg(any(target_vendor = "apple", test))]
mod asbd;
#[cfg(any(target_vendor = "apple", test))]
mod buffers;
#[cfg(any(target_vendor = "apple", test))]
mod clock;
#[cfg(target_os = "macos")]
mod device;
#[cfg(any(target_vendor = "apple", test))]
mod error;
#[cfg(target_os = "macos")]
mod hog;
#[cfg(target_os = "macos")]
mod listening;
#[cfg(any(target_os = "macos", test))]
mod physical;
#[cfg(target_os = "macos")]
mod property;
#[cfg(target_vendor = "apple")]
mod render;
#[cfg(any(target_vendor = "apple", test))]
mod status;
#[cfg(target_vendor = "apple")]
mod toolbox;
#[cfg(any(target_vendor = "apple", test))]
mod unit;
#[cfg(target_os = "macos")]
mod volume;

#[cfg(target_vendor = "apple")]
pub use crate::{
    asbd::{Format, Word},
    error::{Error, Op, Result},
    status::OsStatus,
    toolbox::Unit,
    unit::{Component, Hear, HostTime, Render, Shape},
};
#[cfg(target_os = "macos")]
pub use crate::{
    device::{Device, Offered, RateRange, Stream, Transport},
    hog::{Hog, Pid},
    listening::{Listening, Told, notify_on_the_hals_own_thread, told_of_the_system},
    physical::{Depth, PhysicalFormat},
    property::Direction,
    volume::{Mute, Scalar, Volume},
};
