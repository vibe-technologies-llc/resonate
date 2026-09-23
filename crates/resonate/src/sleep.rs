use std::{fmt, num::NonZeroU64, time::Duration};

use resonate_engine::{Asleep, Until};
use resonate_mpris::Running;

use crate::{Error, Result, reached};

const END_OF_TRACK: &str = "track";
const END_OF_QUEUE: &str = "queue";
const NO_TIMER: &str = "off";

const SECONDS_A_MINUTE: u64 = 60;
const SECONDS_AN_HOUR: u64 = 60 * SECONDS_A_MINUTE;

pub const WITHOUT_A_SPEC: Sleep = Sleep::In(NonZeroU64::new(30).unwrap());

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Spoken(Box<str>);

impl fmt::Display for Spoken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &*self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sleep {
    In(NonZeroU64),
    EndOfTrack,
    EndOfQueue,
    Off,
}

impl Sleep {
    pub fn read(spoken: &str) -> Result<Self> {
        let trimmed = spoken.trim();
        let refused = || Error::NotASleepTimer {
            given: Spoken(trimmed.into()),
        };

        match trimmed {
            END_OF_TRACK => Ok(Self::EndOfTrack),
            END_OF_QUEUE => Ok(Self::EndOfQueue),
            NO_TIMER => Ok(Self::Off),
            minutes => minutes
                .parse::<NonZeroU64>()
                .map(Self::In)
                .map_err(|_| refused()),
        }
    }

    pub const fn until(self) -> Option<Until> {
        match self {
            Self::In(minutes) => Some(Until::After(Duration::from_secs(
                minutes.get().saturating_mul(SECONDS_A_MINUTE),
            ))),
            Self::EndOfTrack => Some(Until::EndOfTrack),
            Self::EndOfQueue => Some(Until::EndOfQueue),
            Self::Off => None,
        }
    }
}

pub fn set(spoken: &str, player: Option<&str>) -> Result<()> {
    let wanted = Sleep::read(spoken)?;
    let running = reached(player)?;
    running.set_sleep(wanted.until())?;

    println!("{}: {}", running.name(), reads_as(&running)?);
    Ok(())
}

fn reads_as(running: &Running) -> Result<String> {
    Ok(running.sleep()?.map_or_else(
        || "no sleep timer".to_owned(),
        |asleep| format!("sleeping {}", when(asleep)),
    ))
}

fn when(asleep: Asleep) -> String {
    match asleep.until {
        Until::After(_) => match asleep.left {
            Some(left) => format!("in {}", counting_down(left)),
            None => "soon".to_owned(),
        },
        Until::EndOfTrack => "at the end of the track playing".to_owned(),
        Until::EndOfQueue => "at the end of the queue".to_owned(),
    }
}

fn counting_down(left: Duration) -> String {
    let seconds = left.as_secs();

    match (seconds / SECONDS_AN_HOUR, seconds % SECONDS_AN_HOUR) {
        (0, rest) => format!("{}m {}s", rest / SECONDS_A_MINUTE, rest % SECONDS_A_MINUTE),
        (hours, rest) => format!("{hours}h {}m", rest / SECONDS_A_MINUTE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(count: u64) -> Sleep {
        Sleep::In(NonZeroU64::new(count).expect("a timer of no minutes is not one to read"))
    }

    #[test]
    fn a_sleep_spec_reads_every_way_it_can_be_written() {
        assert_eq!(Sleep::read("30").ok(), Some(minutes(30)));
        assert_eq!(Sleep::read(" 90 ").ok(), Some(minutes(90)));
        assert_eq!(Sleep::read("track").ok(), Some(Sleep::EndOfTrack));
        assert_eq!(Sleep::read("queue").ok(), Some(Sleep::EndOfQueue));
        assert_eq!(Sleep::read("off").ok(), Some(Sleep::Off));

        for refused in ["", "soon", "0", "-5", "30m", "Track", "1.5"] {
            assert!(Sleep::read(refused).is_err(), "{refused:?} was read");
        }
    }

    #[test]
    fn a_sleep_spec_that_cannot_be_read_names_what_was_given() {
        let Err(Error::NotASleepTimer { given }) = Sleep::read("  half an hour  ") else {
            panic!("a spec nothing can be made of was read as one");
        };

        assert_eq!(given.to_string(), "\"half an hour\"");
    }

    #[test]
    fn every_spec_but_off_names_a_moment_the_engine_understands() {
        assert_eq!(
            minutes(45).until(),
            Some(Until::After(Duration::from_secs(45 * 60)))
        );
        assert_eq!(Sleep::EndOfTrack.until(), Some(Until::EndOfTrack));
        assert_eq!(Sleep::EndOfQueue.until(), Some(Until::EndOfQueue));
        assert_eq!(Sleep::Off.until(), None);
    }

    #[test]
    fn what_is_left_of_a_timer_reads_in_hours_once_it_is_past_one() {
        assert_eq!(counting_down(Duration::from_secs(59)), "0m 59s");
        assert_eq!(counting_down(Duration::from_secs(1_799)), "29m 59s");
        assert_eq!(counting_down(Duration::from_secs(3_600)), "1h 0m");
        assert_eq!(counting_down(Duration::from_secs(8_130)), "2h 15m");
    }
}
