#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::num::{NonZeroU32, NonZeroU128};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostClock {
    numer: u32,
    denom: NonZeroU32,
}

impl HostClock {
    pub const NANOSECONDS: Self = Self {
        numer: 1,
        denom: NonZeroU32::MIN,
    };

    pub const fn of(numer: u32, denom: u32) -> Self {
        match NonZeroU32::new(denom) {
            Some(denom) => Self { numer, denom },
            None => Self::NANOSECONDS,
        }
    }

    #[cfg(target_vendor = "apple")]
    pub fn of_this_machine() -> Self {
        let mut info = MachTimebaseInfo::default();
        match unsafe { mach_timebase_info(&raw mut info) } {
            KERN_SUCCESS => Self::of(info.numer, info.denom),
            _ => Self::NANOSECONDS,
        }
    }

    pub fn nanos(self, ticks: u64) -> u64 {
        let nanos = u128::from(ticks).saturating_mul(u128::from(self.numer))
            / NonZeroU128::from(self.denom);
        u64::try_from(nanos).unwrap_or(u64::MAX)
    }
}

#[cfg(target_vendor = "apple")]
const KERN_SUCCESS: i32 = 0;

#[cfg(target_vendor = "apple")]
#[derive(Default)]
#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

#[cfg(target_vendor = "apple")]
unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

#[cfg(target_vendor = "apple")]
pub(crate) fn host_ticks() -> u64 {
    unsafe { mach_absolute_time() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ticks_are_read_in_nanoseconds_through_the_timebase() {
        assert_eq!(HostClock::of(125, 3).nanos(24_000_000), 1_000_000_000);
        assert_eq!(HostClock::NANOSECONDS.nanos(42), 42);
        assert_eq!(HostClock::of(1, 0), HostClock::NANOSECONDS);
    }

    #[test]
    fn ticks_too_many_for_nanoseconds_stand_at_the_most_there_are() {
        assert_eq!(HostClock::of(u32::MAX, 1).nanos(u64::MAX), u64::MAX);
    }
}
