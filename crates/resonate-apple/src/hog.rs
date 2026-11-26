use std::{fmt, process};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pid(i32);

impl Pid {
    pub(crate) const fn of(raw: i32) -> Self {
        Self(raw)
    }

    pub(crate) fn of_this_process() -> Self {
        Self(i32::try_from(process::id()).unwrap_or(i32::MAX))
    }

    #[cfg(target_os = "macos")]
    pub const fn get(self) -> i32 {
        self.0
    }
}

impl fmt::Display for Pid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

pub trait Holds {
    fn holder(&self) -> Result<Option<Pid>>;
    fn toggle(&self) -> Result<()>;
}

#[derive(Debug)]
pub struct Held<H: Holds> {
    device: H,
}

#[cfg(target_os = "macos")]
pub type Hog = Held<crate::device::Device>;

impl<H: Holds> Held<H> {
    pub fn taken(device: H) -> Result<Self> {
        if let Some(by) = device.holder()? {
            return Err(Error::Hogged { by });
        }
        device.toggle()?;
        match device.holder() {
            Ok(Some(by)) if by == Pid::of_this_process() => Ok(Self { device }),
            Ok(Some(by)) => Err(Error::Hogged { by }),
            Ok(None) => Err(Error::HogUntaken),
            Err(error) => {
                Self::let_go(&device);
                Err(error)
            }
        }
    }

    fn let_go(device: &H) {
        if let Err(error) = device.toggle() {
            tracing::warn!(%error, "the device could not be let go of");
        }
    }
}

impl<H: Holds> Drop for Held<H> {
    fn drop(&mut self) {
        match self.device.holder() {
            Ok(Some(by)) if by == Pid::of_this_process() => Self::let_go(&self.device),
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "who holds the device could not be read, so it may stay held");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::{error::Op, status::OsStatus};

    const REFUSED: i32 = -10_851;

    #[derive(Default)]
    struct State {
        holder: Option<Pid>,
        toggles: usize,
        reads: usize,
        reads_refused_from: Option<usize>,
        toggles_refused_from: Option<usize>,
    }

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<State>>);

    impl Fake {
        fn held_by(pid: Pid) -> Self {
            let fake = Self::default();

            fake.0.lock().holder = Some(pid);
            fake
        }

        fn refusing_reads_from(self, nth: usize) -> Self {
            self.0.lock().reads_refused_from = Some(nth);
            self
        }

        fn refusing_toggles_from(self, nth: usize) -> Self {
            self.0.lock().toggles_refused_from = Some(nth);
            self
        }

        fn holder(&self) -> Option<Pid> {
            self.0.lock().holder
        }

        fn toggles(&self) -> usize {
            self.0.lock().toggles
        }
    }

    fn refused(op: Op) -> Error {
        Error::Refused {
            op,
            source: OsStatus::of(REFUSED),
        }
    }

    impl Holds for Fake {
        fn holder(&self) -> Result<Option<Pid>> {
            let mut state = self.0.lock();

            state.reads += 1;
            if state
                .reads_refused_from
                .is_some_and(|from| state.reads >= from)
            {
                return Err(refused(Op::ReadHog));
            }
            Ok(state.holder)
        }

        fn toggle(&self) -> Result<()> {
            let mut state = self.0.lock();

            state.toggles += 1;
            if state
                .toggles_refused_from
                .is_some_and(|from| state.toggles >= from)
            {
                return Err(refused(Op::TakeHog));
            }
            state.holder = match state.holder {
                Some(_) => None,
                None => Some(Pid::of_this_process()),
            };
            Ok(())
        }
    }

    fn refusal_of(taken: Result<Held<Fake>>) -> Error {
        match taken {
            Ok(_) => Error::HogUntaken,
            Err(error) => error,
        }
    }

    #[test]
    fn a_free_device_is_taken_and_let_go_of_on_drop() {
        let fake = Fake::default();

        let hog = Held::taken(fake.clone()).expect("a hog");

        assert_eq!(fake.holder(), Some(Pid::of_this_process()));

        drop(hog);

        assert_eq!(fake.holder(), None);
        assert_eq!(fake.toggles(), 2);
    }

    #[test]
    fn a_device_another_process_holds_is_refused_and_never_toggled() {
        let other = Pid::of(1);
        let fake = Fake::held_by(other);

        let error = refusal_of(Held::taken(fake.clone()));

        assert!(matches!(error, Error::Hogged { by } if by == other));
        assert_eq!(fake.toggles(), 0);
    }

    #[test]
    fn a_hog_whose_verification_cannot_be_read_is_let_go_of() {
        let fake = Fake::default().refusing_reads_from(2);

        let error = refusal_of(Held::taken(fake.clone()));

        assert!(matches!(
            error,
            Error::Refused {
                op: Op::ReadHog,
                ..
            }
        ));
        assert_eq!(fake.toggles(), 2);
        assert_eq!(fake.holder(), None);
    }

    #[test]
    fn a_hog_whose_release_is_refused_after_a_failed_verification_answers_the_read() {
        let fake = Fake::default()
            .refusing_reads_from(2)
            .refusing_toggles_from(2);

        let error = refusal_of(Held::taken(fake.clone()));

        assert!(matches!(
            error,
            Error::Refused {
                op: Op::ReadHog,
                ..
            }
        ));
        assert_eq!(fake.toggles(), 2);
        assert_eq!(fake.holder(), Some(Pid::of_this_process()));
    }

    #[test]
    fn a_toggle_the_system_refused_is_the_error_and_leaves_nothing_to_let_go_of() {
        let fake = Fake::default().refusing_toggles_from(1);

        let error = refusal_of(Held::taken(fake.clone()));

        assert!(matches!(
            error,
            Error::Refused {
                op: Op::TakeHog,
                ..
            }
        ));
        assert_eq!(fake.toggles(), 1);
    }

    #[test]
    fn a_release_refused_on_drop_is_survived() {
        let fake = Fake::default().refusing_toggles_from(2);
        let hog = Held::taken(fake.clone()).expect("a hog");

        drop(hog);

        assert_eq!(fake.toggles(), 2);
        assert_eq!(fake.holder(), Some(Pid::of_this_process()));
    }

    #[test]
    fn a_holder_that_cannot_be_read_on_drop_toggles_nothing() {
        let fake = Fake::default();
        let hog = Held::taken(fake.clone()).expect("a hog");

        fake.0.lock().reads_refused_from = Some(0);

        drop(hog);

        assert_eq!(fake.toggles(), 1);
    }

    #[test]
    fn a_device_nobody_holds_any_more_is_not_toggled_again_on_drop() {
        let fake = Fake::default();
        let hog = Held::taken(fake.clone()).expect("a hog");

        fake.0.lock().holder = None;

        drop(hog);

        assert_eq!(fake.toggles(), 1);
    }
}
