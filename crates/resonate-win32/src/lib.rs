#[cfg(all(windows, feature = "console"))]
mod console;
#[cfg(windows)]
mod counter;
#[cfg(windows)]
mod endpoint;
#[cfg(windows)]
mod error;
#[cfg(windows)]
mod files;
#[cfg(windows)]
mod identity;
#[cfg(windows)]
mod mmcss;
#[cfg(windows)]
mod security;
#[cfg(windows)]
mod streams;
#[cfg(windows)]
mod wide;

#[cfg(all(windows, feature = "console"))]
pub use crate::console::{
    ConsoleEvent, ConsoleMode, Process, PseudoConsole, console_input_mode, tell_the_console,
};
#[cfg(windows)]
pub use crate::{
    counter::{PerformanceCounter, performance_counter},
    endpoint::{EndpointVolume, Level, VolumeWatch},
    error::{Error, Op, Result},
    files::{FileIdentity, file_identity},
    identity::name_the_process,
    mmcss::ProAudioClass,
    security::carry_security,
    streams::carry_alternate_streams,
};
