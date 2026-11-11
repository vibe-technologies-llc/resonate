use crate::error::{Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

const THIS_PROCESS_ALONE: u32 = 1;
const PROCESSES_LISTED: usize = 2;

pub(crate) trait Attachment {
    fn hear_interrupts(&self) -> Raw<()>;
    fn console_processes(&self, listed: &mut [u32]) -> u32;
    fn free_console(&self) -> Raw<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Console {
    None,
    Shared,
    LeftBehind,
}

pub(crate) fn hearing_interrupts(attachment: &impl Attachment) -> Result<()> {
    attachment
        .hear_interrupts()
        .map_err(refused(Op::HearConsoleInterrupts))
}

pub(crate) fn leaving_a_console_of_its_own(attachment: &impl Attachment) -> Result<Console> {
    let mut attached = [0_u32; PROCESSES_LISTED];
    match attachment.console_processes(&mut attached) {
        0 => Ok(Console::None),
        THIS_PROCESS_ALONE => {
            attachment
                .free_console()
                .map_err(refused(Op::LeaveTheConsole))?;
            Ok(Console::LeftBehind)
        }
        _ => Ok(Console::Shared),
    }
}

#[cfg(windows)]
pub fn hear_console_interrupts() -> Result<()> {
    hearing_interrupts(&Windows)
}

#[cfg(windows)]
pub fn leave_a_console_of_its_own() -> Result<Console> {
    leaving_a_console_of_its_own(&Windows)
}

#[cfg(windows)]
mod real {
    use windows::Win32::System::Console::{
        FreeConsole, GetConsoleProcessList, SetConsoleCtrlHandler,
    };

    use super::Attachment;
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    impl Attachment for Windows {
        fn hear_interrupts(&self) -> Raw<()> {
            unsafe { SetConsoleCtrlHandler(None, false) }.map_err(hresult)
        }

        fn console_processes(&self, listed: &mut [u32]) -> u32 {
            unsafe { GetConsoleProcessList(listed) }
        }

        fn free_console(&self) -> Raw<()> {
            unsafe { FreeConsole() }.map_err(hresult)
        }
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        List(usize),
        Free,
        Interrupts,
    }

    struct Fake {
        calls: Mutex<Vec<Call>>,
        processes: u32,
        refusing: bool,
    }

    impl Fake {
        fn with(processes: u32) -> Self {
            Self {
                calls: Mutex::default(),
                processes,
                refusing: false,
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }

        fn answer(&self) -> Raw<()> {
            if self.refusing {
                return Err(HResult(-6));
            }
            Ok(())
        }
    }

    impl Attachment for Fake {
        fn hear_interrupts(&self) -> Raw<()> {
            self.calls.lock().push(Call::Interrupts);
            self.answer()
        }

        fn console_processes(&self, listed: &mut [u32]) -> u32 {
            self.calls.lock().push(Call::List(listed.len()));
            self.processes
        }

        fn free_console(&self) -> Raw<()> {
            self.calls.lock().push(Call::Free);
            self.answer()
        }
    }

    #[test]
    fn a_process_with_no_console_leaves_nothing() {
        let fake = Fake::with(0);

        assert_eq!(
            leaving_a_console_of_its_own(&fake).expect("answered"),
            Console::None
        );
        assert_eq!(fake.calls(), [Call::List(PROCESSES_LISTED)]);
    }

    #[test]
    fn a_console_this_process_is_alone_on_is_freed() {
        let fake = Fake::with(1);

        assert_eq!(
            leaving_a_console_of_its_own(&fake).expect("answered"),
            Console::LeftBehind
        );
        assert_eq!(fake.calls(), [Call::List(PROCESSES_LISTED), Call::Free]);
    }

    #[test]
    fn a_console_shared_with_others_is_not_freed_however_many_share_it() {
        for processes in [2, 3, 40] {
            let fake = Fake::with(processes);

            assert_eq!(
                leaving_a_console_of_its_own(&fake).expect("answered"),
                Console::Shared
            );
            assert_eq!(fake.calls(), [Call::List(PROCESSES_LISTED)]);
        }
    }

    #[test]
    fn a_console_that_will_not_be_freed_is_an_error() {
        let fake = Fake {
            refusing: true,
            ..Fake::with(1)
        };

        assert!(matches!(
            leaving_a_console_of_its_own(&fake),
            Err(Error::Windows {
                op: Op::LeaveTheConsole,
                source: HResult(-6)
            })
        ));
    }

    #[test]
    fn hearing_interrupts_again_names_its_call_when_refused() {
        let fake = Fake {
            refusing: true,
            ..Fake::with(0)
        };

        assert!(matches!(
            hearing_interrupts(&fake),
            Err(Error::Windows {
                op: Op::HearConsoleInterrupts,
                source: HResult(-6)
            })
        ));
        assert_eq!(fake.calls(), [Call::Interrupts]);
    }
}
