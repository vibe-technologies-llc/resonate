use std::num::NonZeroU64;

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::error::{Error, Op, Result, refused};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerformanceCounter {
    pub per_second: NonZeroU64,
}

impl PerformanceCounter {
    pub fn of_this_machine() -> Result<Self> {
        let mut per_second = 0_i64;
        unsafe { QueryPerformanceFrequency(&raw mut per_second) }
            .map_err(refused(Op::ReadCounterFrequency))?;
        let per_second = u64::try_from(per_second)
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(Error::CounterStandsStill)?;
        Ok(Self { per_second })
    }
}

pub fn performance_counter() -> Result<u64> {
    let mut ticks = 0_i64;
    unsafe { QueryPerformanceCounter(&raw mut ticks) }.map_err(refused(Op::ReadCounter))?;
    Ok(u64::try_from(ticks).unwrap_or_default())
}
