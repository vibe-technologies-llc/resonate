#[cfg(any(target_vendor = "apple", test))]
mod asbd;
#[cfg(any(target_vendor = "apple", test))]
mod buffers;
#[cfg(any(target_vendor = "apple", test))]
mod clock;
#[cfg(any(target_vendor = "apple", test))]
mod error;
#[cfg(target_vendor = "apple")]
mod render;
#[cfg(any(target_vendor = "apple", test))]
mod status;
#[cfg(target_vendor = "apple")]
mod toolbox;
#[cfg(any(target_vendor = "apple", test))]
mod unit;

#[cfg(target_vendor = "apple")]
pub use crate::{
    asbd::{Format, Word},
    error::{Error, Op, Result},
    status::OsStatus,
    toolbox::Unit,
    unit::{Component, Hear, HostTime, Render, Shape},
};
