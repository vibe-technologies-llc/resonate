#[cfg(any(windows, test))]
mod attached;
#[cfg(any(all(windows, feature = "console"), test))]
mod console;
#[cfg(any(windows, test))]
mod counter;
#[cfg(any(windows, test))]
mod endpoint;
#[cfg(any(windows, test))]
mod error;
#[cfg(any(windows, test))]
mod files;
#[cfg(any(windows, test))]
mod gaming;
#[cfg(any(windows, test))]
mod identity;
#[cfg(any(windows, test))]
mod mmcss;
#[cfg(any(windows, test))]
mod security;
#[cfg(any(windows, test))]
mod streams;
#[cfg(windows)]
mod system;
#[cfg(any(windows, test))]
mod wide;

#[cfg(all(windows, feature = "console"))]
pub use crate::console::{
    ConsoleEvent, ConsoleMode, Process, PseudoConsole, console_input_mode, tell_the_console,
};
#[cfg(windows)]
pub use crate::{
    attached::{Console, hear_console_interrupts, leave_a_console_of_its_own},
    counter::{PerformanceCounter, performance_counter},
    endpoint::{EndpointVolume, Level, VolumeWatch},
    error::{Error, HResult, Op, Result},
    files::{FileIdentity, file_identity},
    gaming::GameModeWatch,
    identity::name_the_process,
    mmcss::ProAudioClass,
    security::carry_security,
    streams::carry_alternate_streams,
};
