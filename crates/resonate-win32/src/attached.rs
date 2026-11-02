use windows::Win32::System::Console::{FreeConsole, GetConsoleProcessList};

use crate::error::{Op, Result, refused};

const THIS_PROCESS_ALONE: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Console {
    None,
    Shared,
    LeftBehind,
}

pub fn leave_a_console_of_its_own() -> Result<Console> {
    let mut attached = [0_u32; 2];
    match unsafe { GetConsoleProcessList(&mut attached) } {
        0 => Ok(Console::None),
        THIS_PROCESS_ALONE => {
            unsafe { FreeConsole() }.map_err(refused(Op::LeaveTheConsole))?;
            Ok(Console::LeftBehind)
        }
        _ => Ok(Console::Shared),
    }
}
