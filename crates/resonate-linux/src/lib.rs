#[cfg(any(target_os = "linux", target_os = "android"))]
mod error;
#[cfg(any(target_os = "linux", target_os = "android"))]
mod lease;

#[cfg(any(target_os = "linux", target_os = "android"))]
pub use crate::{
    error::{Error, LeaseOp, Result},
    lease::Lease,
};
