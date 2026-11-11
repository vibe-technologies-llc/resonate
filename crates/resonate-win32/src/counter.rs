use std::num::NonZeroU64;

use crate::error::{Error, Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

pub(crate) trait Counting {
    fn frequency(&self) -> Raw<i64>;
    fn ticks(&self) -> Raw<i64>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerformanceCounter {
    pub per_second: NonZeroU64,
}

impl PerformanceCounter {
    pub(crate) fn read_from(counting: &impl Counting) -> Result<Self> {
        let per_second = counting
            .frequency()
            .map_err(refused(Op::ReadCounterFrequency))?;
        let per_second = u64::try_from(per_second)
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(Error::CounterStandsStill)?;
        Ok(Self { per_second })
    }
}

pub(crate) fn ticks_of(counting: &impl Counting) -> Result<u64> {
    let ticks = counting.ticks().map_err(refused(Op::ReadCounter))?;
    Ok(u64::try_from(ticks).unwrap_or_default())
}

#[cfg(windows)]
impl PerformanceCounter {
    pub fn of_this_machine() -> Result<Self> {
        Self::read_from(&Windows)
    }
}

#[cfg(windows)]
pub fn performance_counter() -> Result<u64> {
    ticks_of(&Windows)
}

#[cfg(windows)]
mod real {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

    use super::Counting;
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    impl Counting for Windows {
        fn frequency(&self) -> Raw<i64> {
            let mut per_second = 0_i64;
            unsafe { QueryPerformanceFrequency(&raw mut per_second) }.map_err(hresult)?;
            Ok(per_second)
        }

        fn ticks(&self) -> Raw<i64> {
            let mut ticks = 0_i64;
            unsafe { QueryPerformanceCounter(&raw mut ticks) }.map_err(hresult)?;
            Ok(ticks)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HResult;

    struct Fake {
        frequency: Raw<i64>,
        ticks: Raw<i64>,
    }

    impl Counting for Fake {
        fn frequency(&self) -> Raw<i64> {
            self.frequency
        }

        fn ticks(&self) -> Raw<i64> {
            self.ticks
        }
    }

    fn counting(frequency: i64, ticks: i64) -> Fake {
        Fake {
            frequency: Ok(frequency),
            ticks: Ok(ticks),
        }
    }

    #[test]
    fn a_frequency_names_the_ticks_a_second() {
        let counter = PerformanceCounter::read_from(&counting(10_000_000, 0)).expect("a counter");

        assert_eq!(counter.per_second.get(), 10_000_000);
    }

    #[test]
    fn a_counter_that_stands_still_or_runs_backwards_is_refused() {
        for frequency in [0, -1] {
            let read = PerformanceCounter::read_from(&counting(frequency, 0));

            assert!(
                matches!(read, Err(Error::CounterStandsStill)),
                "{frequency}"
            );
        }
    }

    #[test]
    fn a_refused_frequency_names_its_call() {
        let fake = Fake {
            frequency: Err(HResult(-3)),
            ticks: Ok(0),
        };

        assert!(matches!(
            PerformanceCounter::read_from(&fake),
            Err(Error::Windows {
                op: Op::ReadCounterFrequency,
                source: HResult(-3)
            })
        ));
    }

    #[test]
    fn ticks_are_read_and_a_negative_count_is_none() {
        assert_eq!(ticks_of(&counting(1, 12_345)).expect("ticks"), 12_345);
        assert_eq!(ticks_of(&counting(1, -7)).expect("ticks"), 0);
    }

    #[test]
    fn a_refused_tick_names_its_call() {
        let fake = Fake {
            frequency: Ok(1),
            ticks: Err(HResult(-4)),
        };

        assert!(matches!(
            ticks_of(&fake),
            Err(Error::Windows {
                op: Op::ReadCounter,
                source: HResult(-4)
            })
        ));
    }
}
