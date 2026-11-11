use std::marker::PhantomData;

use crate::error::{Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

type BoundToItsThread = PhantomData<*const ()>;

pub(crate) trait Scheduling {
    type Task: Copy;

    fn join_pro_audio(&self) -> Raw<Self::Task>;
    fn revert(&self, task: Self::Task) -> Raw<()>;
}

pub(crate) struct Joined<T: Scheduling> {
    scheduling: T,
    task: Option<T::Task>,
    thread: BoundToItsThread,
}

impl<T: Scheduling> Joined<T> {
    pub(crate) fn join(scheduling: T) -> Result<Self> {
        let task = scheduling
            .join_pro_audio()
            .map_err(refused(Op::JoinProAudio))?;
        Ok(Self {
            scheduling,
            task: Some(task),
            thread: PhantomData,
        })
    }

    pub(crate) fn leave(mut self) -> Result<()> {
        match self.task.take() {
            Some(task) => self
                .scheduling
                .revert(task)
                .map_err(refused(Op::LeaveProAudio)),
            None => Ok(()),
        }
    }
}

impl<T: Scheduling> Drop for Joined<T> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            let _ = self.scheduling.revert(task);
        }
    }
}

#[cfg(windows)]
pub struct ProAudioClass(Joined<Windows>);

#[cfg(windows)]
impl ProAudioClass {
    pub fn join() -> Result<Self> {
        Joined::join(Windows).map(Self)
    }

    pub fn leave(self) -> Result<()> {
        self.0.leave()
    }
}

#[cfg(windows)]
mod real {
    use windows::Win32::{
        Foundation::HANDLE,
        System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW},
    };
    use windows_core::w;

    use super::Scheduling;
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    impl Scheduling for Windows {
        type Task = HANDLE;

        fn join_pro_audio(&self) -> Raw<HANDLE> {
            let mut task_index = 0_u32;
            unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &raw mut task_index) }
                .map_err(hresult)
        }

        fn revert(&self, task: HANDLE) -> Raw<()> {
            unsafe { AvRevertMmThreadCharacteristics(task) }.map_err(hresult)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Join,
        Revert(u32),
    }

    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<Call>>>,
        refusing_join: bool,
        refusing_revert: bool,
    }

    impl Fake {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }
    }

    impl Scheduling for Fake {
        type Task = u32;

        fn join_pro_audio(&self) -> Raw<u32> {
            self.calls.lock().push(Call::Join);
            if self.refusing_join {
                return Err(HResult(-1));
            }
            Ok(41)
        }

        fn revert(&self, task: u32) -> Raw<()> {
            self.calls.lock().push(Call::Revert(task));
            if self.refusing_revert {
                return Err(HResult(-2));
            }
            Ok(())
        }
    }

    #[test]
    fn a_refused_join_holds_nothing_to_revert() {
        let fake = Fake {
            refusing_join: true,
            ..Fake::default()
        };

        let joined = Joined::join(fake.clone());

        assert!(matches!(
            joined,
            Err(Error::Windows {
                op: Op::JoinProAudio,
                source: HResult(-1)
            })
        ));
        assert_eq!(fake.calls(), [Call::Join]);
    }

    #[test]
    fn dropping_a_joined_thread_reverts_its_task_once() {
        let fake = Fake::default();

        drop(Joined::join(fake.clone()).expect("joined"));

        assert_eq!(fake.calls(), [Call::Join, Call::Revert(41)]);
    }

    #[test]
    fn leaving_reverts_once_and_the_drop_after_it_reverts_nothing() {
        let fake = Fake::default();

        Joined::join(fake.clone())
            .expect("joined")
            .leave()
            .expect("left");

        assert_eq!(fake.calls(), [Call::Join, Call::Revert(41)]);
    }

    #[test]
    fn a_refused_leave_is_told_and_is_not_tried_again_on_drop() {
        let fake = Fake {
            refusing_revert: true,
            ..Fake::default()
        };

        let left = Joined::join(fake.clone()).expect("joined").leave();

        assert!(matches!(
            left,
            Err(Error::Windows {
                op: Op::LeaveProAudio,
                source: HResult(-2)
            })
        ));
        assert_eq!(fake.calls(), [Call::Join, Call::Revert(41)]);
    }
}
