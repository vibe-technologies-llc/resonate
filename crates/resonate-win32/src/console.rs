use std::{
    ffi::{OsStr, OsString, c_void},
    fs::OpenOptions,
    io::{self, PipeReader, PipeWriter},
    os::windows::{
        ffi::OsStrExt as _,
        io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
    },
    path::Path,
};

use windows::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Console::{
            CONSOLE_MODE, COORD, CTRL_BREAK_EVENT, CTRL_C_EVENT, ClosePseudoConsole,
            CreatePseudoConsole, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, GenerateConsoleCtrlEvent,
            GetConsoleMode, HPCON,
        },
        Threading::{
            CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
            InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
            STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};
use windows_core::{PCWSTR, PWSTR};

use crate::{
    error::{Error, Op, Result, refused},
    wide::Wide,
};

const THE_CONSOLES_INPUT: &str = "CONIN$";
const ENDED_BY_THE_TEST: u32 = 1;
const ATTRIBUTES_HELD: u32 = 1;

const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const SPACE: u16 = b' ' as u16;
const EQUALS: u16 = b'=' as u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleEvent {
    Interrupt,
    Break,
}

impl ConsoleEvent {
    const fn code(self) -> u32 {
        match self {
            Self::Interrupt => CTRL_C_EVENT,
            Self::Break => CTRL_BREAK_EVENT,
        }
    }
}

pub fn tell_the_console(event: ConsoleEvent) -> Result<()> {
    unsafe { GenerateConsoleCtrlEvent(event.code(), 0) }.map_err(refused(Op::TellTheConsole))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleMode(pub u32);

impl ConsoleMode {
    pub const fn takes_a_line_at_a_time(self) -> bool {
        let wanted = ENABLE_LINE_INPUT.0 | ENABLE_ECHO_INPUT.0;
        self.0 & wanted == wanted
    }
}

pub fn console_input_mode() -> Result<ConsoleMode> {
    let input = OpenOptions::new()
        .read(true)
        .write(true)
        .open(THE_CONSOLES_INPUT)
        .map_err(|source| Error::Io {
            op: Op::ReadConsoleMode,
            path: THE_CONSOLES_INPUT.into(),
            source,
        })?;
    let mut mode = CONSOLE_MODE(0);
    unsafe { GetConsoleMode(HANDLE(input.as_raw_handle()), &raw mut mode) }
        .map_err(refused(Op::ReadConsoleMode))?;
    Ok(ConsoleMode(mode.0))
}

pub struct PseudoConsole {
    console: HPCON,
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        unsafe { ClosePseudoConsole(self.console) };
    }
}

fn piped() -> Result<(PipeReader, PipeWriter)> {
    io::pipe().map_err(|source| Error::Io {
        op: Op::MakePipe,
        path: THE_CONSOLES_INPUT.into(),
        source,
    })
}

impl PseudoConsole {
    pub fn open(columns: i16, rows: i16) -> Result<(Self, PipeWriter, PipeReader)> {
        let (console_reads, typed) = piped()?;
        let (shown, console_writes) = piped()?;
        let console = unsafe {
            CreatePseudoConsole(
                COORD {
                    X: columns,
                    Y: rows,
                },
                HANDLE(console_reads.as_raw_handle()),
                HANDLE(console_writes.as_raw_handle()),
                0,
            )
        }
        .map_err(refused(Op::OpenPseudoConsole))?;
        Ok((Self { console }, typed, shown))
    }

    pub fn spawn(
        &self,
        program: &Path,
        arguments: &[&OsStr],
        environment: &[(OsString, OsString)],
    ) -> Result<Process> {
        let application = Wide::of_path(program)?;
        let mut line = command_line(program.as_os_str(), arguments);
        let block = environment_block(environment);
        let attributes = Attributes::for_console(self.console)?;

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(u32::MAX);
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        startup.lpAttributeList = attributes.list;

        let mut started = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                application.name(),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                false,
                EXTENDED_STARTUPINFO_PRESENT
                    | CREATE_UNICODE_ENVIRONMENT
                    | CREATE_NEW_PROCESS_GROUP,
                Some(block.as_ptr().cast::<c_void>()),
                PCWSTR::null(),
                &raw const startup.StartupInfo,
                &raw mut started,
            )
        }
        .map_err(refused(Op::StartProcess))?;
        Ok(Process::owning(started))
    }
}

struct Attributes {
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
    _storage: Vec<usize>,
}

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.list) };
    }
}

impl Attributes {
    fn for_console(console: HPCON) -> Result<Self> {
        let mut bytes = 0_usize;
        let _ = unsafe {
            InitializeProcThreadAttributeList(None, ATTRIBUTES_HELD, None, &raw mut bytes)
        };
        let mut held = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(held.as_mut_ptr().cast());
        unsafe {
            InitializeProcThreadAttributeList(Some(list), ATTRIBUTES_HELD, None, &raw mut bytes)
        }
        .map_err(refused(Op::MakeAttributes))?;
        let attributes = Self {
            list,
            _storage: held,
        };

        unsafe {
            UpdateProcThreadAttribute(
                attributes.list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(console.0 as *const c_void),
                size_of::<HPCON>(),
                None,
                None,
            )
        }
        .map_err(refused(Op::MakeAttributes))?;
        Ok(attributes)
    }
}

fn quoted_into(line: &mut Vec<u16>, argument: &OsStr) {
    line.push(QUOTE);
    let mut backslashes = 0_usize;
    for unit in argument.encode_wide() {
        match unit {
            BACKSLASH => {
                backslashes += 1;
                continue;
            }
            QUOTE => line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2 + 1)),
            _ => line.extend(std::iter::repeat_n(BACKSLASH, backslashes)),
        }
        line.push(unit);
        backslashes = 0;
    }
    line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
    line.push(QUOTE);
}

fn command_line(program: &OsStr, arguments: &[&OsStr]) -> Vec<u16> {
    let mut line = Vec::new();
    quoted_into(&mut line, program);
    for argument in arguments {
        line.push(SPACE);
        quoted_into(&mut line, argument);
    }
    line.retain(|unit| *unit != 0);
    line.push(0);
    line
}

fn environment_block(environment: &[(OsString, OsString)]) -> Vec<u16> {
    let mut entries: Vec<Vec<u16>> = environment
        .iter()
        .filter_map(|(key, value)| {
            let key: Vec<u16> = key.encode_wide().collect();
            let value: Vec<u16> = value.encode_wide().collect();
            let whole = !key.is_empty() && !key.contains(&0) && !value.contains(&0);
            whole.then(|| key.into_iter().chain([EQUALS]).chain(value).collect())
        })
        .collect();
    entries.sort_by_key(|entry| String::from_utf16_lossy(entry).to_uppercase());

    let mut block: Vec<u16> = entries
        .into_iter()
        .flat_map(|entry| entry.into_iter().chain([0]))
        .collect();
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

pub struct Process {
    handle: OwnedHandle,
    id: u32,
}

impl Process {
    fn owning(started: PROCESS_INFORMATION) -> Self {
        let handle = unsafe { OwnedHandle::from_raw_handle(started.hProcess.0) };
        drop(unsafe { OwnedHandle::from_raw_handle(started.hThread.0) });
        Self {
            handle,
            id: started.dwProcessId,
        }
    }

    pub const fn id(&self) -> u32 {
        self.id
    }

    fn raw(&self) -> HANDLE {
        HANDLE(self.handle.as_raw_handle())
    }

    pub fn exit_code(&self) -> Result<Option<u32>> {
        match unsafe { WaitForSingleObject(self.raw(), 0) } {
            WAIT_TIMEOUT => return Ok(None),
            WAIT_OBJECT_0 => {}
            _ => {
                return Err(refused(Op::WaitForProcess)(
                    windows_core::Error::from_thread(),
                ));
            }
        }
        let mut code = 0_u32;
        unsafe { GetExitCodeProcess(self.raw(), &raw mut code) }
            .map_err(refused(Op::WaitForProcess))?;
        Ok(Some(code))
    }

    pub fn end(&self) -> Result<()> {
        unsafe { TerminateProcess(self.raw(), ENDED_BY_THE_TEST) }.map_err(refused(Op::EndProcess))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(units: &[u16]) -> String {
        String::from_utf16_lossy(units.strip_suffix(&[0]).unwrap_or(units))
    }

    #[test]
    fn an_argument_is_quoted_as_the_runtime_reads_it_back() {
        let line = command_line(
            OsStr::new(r"C:\a b\resonate.exe"),
            &[
                OsStr::new("play"),
                OsStr::new(r#"say "hi"\"#),
                OsStr::new(r"C:\x\"),
            ],
        );

        assert_eq!(
            text(&line),
            r#""C:\a b\resonate.exe" "play" "say \"hi\"\\" "C:\x\\""#
        );
    }

    #[test]
    fn the_environment_is_a_sorted_run_of_pairs_ending_twice() {
        let block = environment_block(&[
            ("b".into(), "2".into()),
            ("A".into(), "1".into()),
            (String::new().into(), "dropped".into()),
        ]);

        assert_eq!(String::from_utf16_lossy(&block), "A=1\0b=2\0\0");
    }
}
