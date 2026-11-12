#[cfg(any(target_vendor = "apple", test))]
mod asbd;
#[cfg(any(target_vendor = "apple", test))]
mod buffers;
#[cfg(any(target_vendor = "apple", test))]
mod clock;
#[cfg(target_os = "macos")]
mod cloning;
#[cfg(target_os = "macos")]
mod device;
#[cfg(any(target_vendor = "apple", test))]
mod error;
#[cfg(target_os = "macos")]
mod hog;
#[cfg(target_os = "macos")]
mod listening;
#[cfg(target_os = "macos")]
mod now_playing;
#[cfg(target_os = "macos")]
mod path_monitor;
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
    cloning::clone_file,
    device::{Device, OfferedFormat, RateRange, Stream, Transport},
    hog::{Hog, Pid},
    listening::{Listening, Told, notify_on_the_hals_own_thread, told_of_the_system},
    now_playing::{Answer, Asked, Clock, NowPlaying, Offered, Playback, Shown},
    path_monitor::{PathMonitor, Route},
    physical::{Depth, PhysicalFormat},
    property::Direction,
    volume::{Mute, Scalar, Volume},
};
