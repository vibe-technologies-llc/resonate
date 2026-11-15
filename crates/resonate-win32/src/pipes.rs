use crate::error::{Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

pub(crate) trait Disconnecting<Pipe> {
    fn disconnect(&self, pipe: Pipe) -> Raw<()>;
}

pub(crate) fn disconnected<Pipe>(
    disconnecting: &impl Disconnecting<Pipe>,
    pipe: Pipe,
) -> Result<()> {
    disconnecting
        .disconnect(pipe)
        .map_err(refused(Op::DisconnectPipe))
}

#[cfg(windows)]
pub fn disconnect_pipe(pipe: std::os::windows::io::BorrowedHandle<'_>) -> Result<()> {
    disconnected(&Windows, pipe)
}

#[cfg(windows)]
mod real {
    use std::os::windows::io::{AsRawHandle as _, BorrowedHandle};

    use windows::Win32::{Foundation::HANDLE, System::Pipes::DisconnectNamedPipe};

    use super::Disconnecting;
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    impl Disconnecting<BorrowedHandle<'_>> for Windows {
        fn disconnect(&self, pipe: BorrowedHandle<'_>) -> Raw<()> {
            unsafe { DisconnectNamedPipe(HANDLE(pipe.as_raw_handle())) }.map_err(hresult)
        }
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    struct Fake {
        disconnected: Mutex<Vec<u32>>,
        refusing: bool,
    }

    impl Disconnecting<u32> for Fake {
        fn disconnect(&self, pipe: u32) -> Raw<()> {
            self.disconnected.lock().push(pipe);
            if self.refusing {
                return Err(HResult(-7));
            }
            Ok(())
        }
    }

    #[test]
    fn the_pipe_given_is_the_one_disconnected() {
        let fake = Fake {
            disconnected: Mutex::default(),
            refusing: false,
        };

        disconnected(&fake, 4).expect("disconnected");

        assert_eq!(*fake.disconnected.lock(), [4]);
    }

    #[test]
    fn a_refusal_names_the_call() {
        let fake = Fake {
            disconnected: Mutex::default(),
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
}
