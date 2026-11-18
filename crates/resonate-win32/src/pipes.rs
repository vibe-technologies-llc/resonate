use crate::error::{HResult, Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

pub(crate) trait Disconnecting<Pipe> {
    fn disconnect(&self, pipe: Pipe) -> Raw<()>;
}

pub(crate) trait Cancelling<Pipe> {
    fn cancel(&self, pipe: Pipe) -> Raw<()>;
}

pub(crate) fn disconnected<Pipe>(
    disconnecting: &impl Disconnecting<Pipe>,
    pipe: Pipe,
) -> Result<()> {
    disconnecting
        .disconnect(pipe)
        .map_err(refused(Op::DisconnectPipe))
}

pub(crate) fn cancelled<Pipe>(cancelling: &impl Cancelling<Pipe>, pipe: Pipe) -> Result<()> {
    match cancelling.cancel(pipe) {
        Err(HResult::NOTHING_PENDING) => Ok(()),
        cancelled => cancelled.map_err(refused(Op::CancelPipeIo)),
    }
}

#[cfg(windows)]
pub fn disconnect_pipe(pipe: std::os::windows::io::BorrowedHandle<'_>) -> Result<()> {
    disconnected(&Windows, pipe)
}

#[cfg(windows)]
pub fn cancel_pipe_io(pipe: std::os::windows::io::BorrowedHandle<'_>) -> Result<()> {
    cancelled(&Windows, pipe)
}

#[cfg(windows)]
mod real {
    use std::os::windows::io::{AsRawHandle as _, BorrowedHandle};

    use windows::Win32::{
        Foundation::HANDLE,
        System::{IO::CancelIoEx, Pipes::DisconnectNamedPipe},
    };

    use super::{Cancelling, Disconnecting};
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    impl Disconnecting<BorrowedHandle<'_>> for Windows {
        fn disconnect(&self, pipe: BorrowedHandle<'_>) -> Raw<()> {
            unsafe { DisconnectNamedPipe(HANDLE(pipe.as_raw_handle())) }.map_err(hresult)
        }
    }

    impl Cancelling<BorrowedHandle<'_>> for Windows {
        fn cancel(&self, pipe: BorrowedHandle<'_>) -> Raw<()> {
            unsafe { CancelIoEx(HANDLE(pipe.as_raw_handle()), None) }.map_err(hresult)
        }
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    struct Fake {
        asked: Mutex<Vec<u32>>,
        refusing: bool,
    }

    impl Cancelling<u32> for Fake {
        fn cancel(&self, pipe: u32) -> Raw<()> {
            self.asked.lock().push(pipe);
            if self.refusing {
                return Err(HResult(-7));
            }
            Err(HResult::NOTHING_PENDING)
        }
    }

    impl Disconnecting<u32> for Fake {
        fn disconnect(&self, pipe: u32) -> Raw<()> {
            self.asked.lock().push(pipe);
            if self.refusing {
                return Err(HResult(-7));
            }
            Ok(())
        }
    }

    #[test]
    fn the_pipe_given_is_the_one_disconnected() {
        let fake = Fake {
            asked: Mutex::default(),
            refusing: false,
        };

        disconnected(&fake, 4).expect("disconnected");

        assert_eq!(*fake.asked.lock(), [4]);
    }

    #[test]
    fn a_refusal_names_the_call() {
        let fake = Fake {
            asked: Mutex::default(),
            refusing: true,
        };

        assert!(matches!(
            disconnected(&fake, 1),
            Err(Error::Windows {
                op: Op::DisconnectPipe,
                source: HResult(-7)
            })
        ));
    }

    #[test]
    fn a_pipe_with_nothing_pending_is_cancelled_without_complaint() {
        let fake = Fake {
            asked: Mutex::default(),
            refusing: false,
        };

        cancelled(&fake, 6).expect("nothing to cancel");

        assert_eq!(*fake.asked.lock(), [6]);
    }

    #[test]
    fn a_refused_cancel_names_the_call() {
        let fake = Fake {
            asked: Mutex::default(),
            refusing: true,
        };

        assert!(matches!(
            cancelled(&fake, 1),
            Err(Error::Windows {
                op: Op::CancelPipeIo,
                source: HResult(-7)
            })
        ));
    }
}
