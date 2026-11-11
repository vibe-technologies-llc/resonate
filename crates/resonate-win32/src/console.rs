use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, PipeReader, PipeWriter},
    path::Path,
};

#[cfg(all(windows, feature = "console"))]
use crate::system::Windows;
use crate::{
    error::{Error, Op, Raw, Result, refused},
    wide::{Wide, units_of},
};

const THE_CONSOLES_INPUT: &str = "CONIN$";
const ENDED_BY_THE_TEST: u32 = 1;
const ATTRIBUTES_HELD: u32 = 1;
const ENABLE_LINE_INPUT: u32 = 0x0002;
const ENABLE_ECHO_INPUT: u32 = 0x0004;

const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const SPACE: u16 = b' ' as u16;
const EQUALS: u16 = b'=' as u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleEvent {
    Interrupt,
    Break,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Started<H> {
    pub(crate) process: H,
    pub(crate) thread: H,
    pub(crate) id: u32,
}

pub(crate) trait Terminal: Clone {
    type Console: Copy;
    type List: Copy;
    type Handle: Copy;

    fn pipe(&self) -> io::Result<(PipeReader, PipeWriter)>;
    fn create_pseudo_console(
        &self,
        columns: i16,
        rows: i16,
        input: &PipeReader,
        output: &PipeWriter,
    ) -> Raw<Self::Console>;
    fn close_pseudo_console(&self, console: Self::Console);
    fn attribute_list_bytes(&self, count: u32) -> usize;
    fn initialize_attribute_list(
        &self,
        storage: &mut [usize],
        count: u32,
        bytes: usize,
    ) -> Raw<Self::List>;
    fn attach_pseudo_console(&self, list: Self::List, console: Self::Console) -> Raw<()>;
    fn delete_attribute_list(&self, list: Self::List);
    fn create_process(
        &self,
        application: &Wide,
        line: &mut [u16],
        environment: &[u16],
        attributes: Self::List,
    ) -> Raw<Started<Self::Handle>>;
    fn close_handle(&self, handle: Self::Handle);
    fn has_exited(&self, process: Self::Handle) -> Raw<bool>;
    fn exit_code(&self, process: Self::Handle) -> Raw<u32>;
    fn terminate(&self, process: Self::Handle, code: u32) -> Raw<()>;
    fn tell(&self, event: ConsoleEvent) -> Raw<()>;
    fn console_input(&self) -> io::Result<File>;
    fn console_mode(&self, input: &File) -> Raw<u32>;
}

pub(crate) fn telling_the_console(terminal: &impl Terminal, event: ConsoleEvent) -> Result<()> {
    terminal.tell(event).map_err(refused(Op::TellTheConsole))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleMode(pub u32);

impl ConsoleMode {
    pub const fn takes_a_line_at_a_time(self) -> bool {
        let wanted = ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT;
        self.0 & wanted == wanted
    }
}

pub(crate) fn mode_of_the_input(terminal: &impl Terminal) -> Result<ConsoleMode> {
    let input = terminal.console_input().map_err(|source| Error::Io {
        op: Op::ReadConsoleMode,
        path: THE_CONSOLES_INPUT.into(),
        source,
    })?;
    let mode = terminal
        .console_mode(&input)
        .map_err(refused(Op::ReadConsoleMode))?;
    Ok(ConsoleMode(mode))
}

fn piped(terminal: &impl Terminal) -> Result<(PipeReader, PipeWriter)> {
    terminal.pipe().map_err(|source| Error::Io {
        op: Op::MakePipe,
        path: THE_CONSOLES_INPUT.into(),
        source,
    })
}

pub(crate) struct Pseudo<T: Terminal> {
    terminal: T,
    console: T::Console,
}

impl<T: Terminal> Drop for Pseudo<T> {
    fn drop(&mut self) {
        self.terminal.close_pseudo_console(self.console);
    }
}

impl<T: Terminal> Pseudo<T> {
    pub(crate) fn open(
        terminal: T,
        columns: i16,
        rows: i16,
    ) -> Result<(Self, PipeWriter, PipeReader)> {
        let (console_reads, typed) = piped(&terminal)?;
        let (shown, console_writes) = piped(&terminal)?;
        let console = terminal
            .create_pseudo_console(columns, rows, &console_reads, &console_writes)
            .map_err(refused(Op::OpenPseudoConsole))?;
        Ok((Self { terminal, console }, typed, shown))
    }

    pub(crate) fn spawn(
        &self,
        program: &Path,
        arguments: &[&OsStr],
        environment: &[(OsString, OsString)],
    ) -> Result<Spawned<T>> {
        let application = Wide::of_path(program)?;
        let mut line = command_line(program.as_os_str(), arguments);
        let block = environment_block(environment);
        let attributes = Attributes::for_console(&self.terminal, self.console)?;

        let started = self
            .terminal
            .create_process(&application, &mut line, &block, attributes.list)
            .map_err(refused(Op::StartProcess))?;
        Ok(Spawned::owning(self.terminal.clone(), started))
    }
}

struct Attributes<T: Terminal> {
    terminal: T,
    list: T::List,
    _storage: Vec<usize>,
}

impl<T: Terminal> Drop for Attributes<T> {
    fn drop(&mut self) {
        self.terminal.delete_attribute_list(self.list);
    }
}

impl<T: Terminal> Attributes<T> {
    fn for_console(terminal: &T, console: T::Console) -> Result<Self> {
        let bytes = terminal.attribute_list_bytes(ATTRIBUTES_HELD);
        let mut held = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
        let list = terminal
            .initialize_attribute_list(&mut held, ATTRIBUTES_HELD, bytes)
            .map_err(refused(Op::MakeAttributes))?;
        let attributes = Self {
            terminal: terminal.clone(),
            list,
            _storage: held,
        };

        terminal
            .attach_pseudo_console(attributes.list, console)
            .map_err(refused(Op::MakeAttributes))?;
        Ok(attributes)
    }
}

fn quoted_into(line: &mut Vec<u16>, argument: &OsStr) {
    line.push(QUOTE);
    let mut backslashes = 0_usize;
    for unit in units_of(argument) {
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
            let key = units_of(key);
            let value = units_of(value);
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

pub(crate) struct Spawned<T: Terminal> {
    terminal: T,
    handle: T::Handle,
    id: u32,
}

impl<T: Terminal> Drop for Spawned<T> {
    fn drop(&mut self) {
        self.terminal.close_handle(self.handle);
    }
}

impl<T: Terminal> Spawned<T> {
    fn owning(terminal: T, started: Started<T::Handle>) -> Self {
        terminal.close_handle(started.thread);
        Self {
            terminal,
            handle: started.process,
            id: started.id,
        }
    }

    pub(crate) const fn id(&self) -> u32 {
        self.id
    }

    pub(crate) fn exit_code(&self) -> Result<Option<u32>> {
        let exited = self
            .terminal
            .has_exited(self.handle)
            .map_err(refused(Op::WaitForProcess))?;
        if !exited {
            return Ok(None);
        }
        let code = self
            .terminal
            .exit_code(self.handle)
            .map_err(refused(Op::WaitForProcess))?;
        Ok(Some(code))
    }

    pub(crate) fn end(&self) -> Result<()> {
        self.terminal
            .terminate(self.handle, ENDED_BY_THE_TEST)
            .map_err(refused(Op::EndProcess))
    }
}

#[cfg(all(windows, feature = "console"))]
pub fn tell_the_console(event: ConsoleEvent) -> Result<()> {
    telling_the_console(&Windows, event)
}

#[cfg(all(windows, feature = "console"))]
pub fn console_input_mode() -> Result<ConsoleMode> {
    mode_of_the_input(&Windows)
}

#[cfg(all(windows, feature = "console"))]
pub struct PseudoConsole(Pseudo<Windows>);

#[cfg(all(windows, feature = "console"))]
impl PseudoConsole {
    pub fn open(columns: i16, rows: i16) -> Result<(Self, PipeWriter, PipeReader)> {
        let (console, typed, shown) = Pseudo::open(Windows, columns, rows)?;
        Ok((Self(console), typed, shown))
    }

    pub fn spawn(
        &self,
        program: &Path,
        arguments: &[&OsStr],
        environment: &[(OsString, OsString)],
    ) -> Result<Process> {
        self.0.spawn(program, arguments, environment).map(Process)
    }
}

#[cfg(all(windows, feature = "console"))]
pub struct Process(Spawned<Windows>);

#[cfg(all(windows, feature = "console"))]
impl Process {
    pub const fn id(&self) -> u32 {
        self.0.id()
    }

    pub fn exit_code(&self) -> Result<Option<u32>> {
        self.0.exit_code()
    }

    pub fn end(&self) -> Result<()> {
        self.0.end()
    }
}

#[cfg(all(windows, feature = "console"))]
mod real {
    use std::{
        ffi::c_void,
        fs::{File, OpenOptions},
        io::{self, PipeReader, PipeWriter},
        os::windows::io::AsRawHandle as _,
    };

    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::{
            Console::{
                CONSOLE_MODE, COORD, CTRL_BREAK_EVENT, CTRL_C_EVENT, ClosePseudoConsole,
                CreatePseudoConsole, GenerateConsoleCtrlEvent, GetConsoleMode, HPCON,
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

    use super::{ConsoleEvent, Started, THE_CONSOLES_INPUT, Terminal};
    use crate::{
        error::Raw,
        system::{Windows, handle_of, hresult, name_of, raw_of},
        wide::Wide,
    };

    impl ConsoleEvent {
        const fn code(self) -> u32 {
            match self {
                Self::Interrupt => CTRL_C_EVENT,
                Self::Break => CTRL_BREAK_EVENT,
            }
        }
    }

    impl Terminal for Windows {
        type Console = HPCON;
        type List = LPPROC_THREAD_ATTRIBUTE_LIST;
        type Handle = usize;

        fn pipe(&self) -> io::Result<(PipeReader, PipeWriter)> {
            io::pipe()
        }

        fn create_pseudo_console(
            &self,
            columns: i16,
            rows: i16,
            input: &PipeReader,
            output: &PipeWriter,
        ) -> Raw<HPCON> {
            unsafe {
                CreatePseudoConsole(
                    COORD {
                        X: columns,
                        Y: rows,
                    },
                    HANDLE(input.as_raw_handle()),
                    HANDLE(output.as_raw_handle()),
                    0,
                )
            }
            .map_err(hresult)
        }

        fn close_pseudo_console(&self, console: HPCON) {
            unsafe { ClosePseudoConsole(console) };
        }

        fn attribute_list_bytes(&self, count: u32) -> usize {
            let mut bytes = 0_usize;
            let _ = unsafe { InitializeProcThreadAttributeList(None, count, None, &raw mut bytes) };
            bytes
        }

        fn initialize_attribute_list(
            &self,
            storage: &mut [usize],
            count: u32,
            bytes: usize,
        ) -> Raw<LPPROC_THREAD_ATTRIBUTE_LIST> {
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
            let mut bytes = bytes;
            unsafe { InitializeProcThreadAttributeList(Some(list), count, None, &raw mut bytes) }
                .map_err(hresult)?;
            Ok(list)
        }

        fn attach_pseudo_console(
            &self,
            list: LPPROC_THREAD_ATTRIBUTE_LIST,
            console: HPCON,
        ) -> Raw<()> {
            unsafe {
                UpdateProcThreadAttribute(
                    list,
                    0,
                    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                    Some(console.0 as *const c_void),
                    size_of::<HPCON>(),
                    None,
                    None,
                )
            }
            .map_err(hresult)
        }

        fn delete_attribute_list(&self, list: LPPROC_THREAD_ATTRIBUTE_LIST) {
            unsafe { DeleteProcThreadAttributeList(list) };
        }

        fn create_process(
            &self,
            application: &Wide,
            line: &mut [u16],
            environment: &[u16],
            attributes: LPPROC_THREAD_ATTRIBUTE_LIST,
        ) -> Raw<Started<usize>> {
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(u32::MAX);
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
            startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
            startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
            startup.lpAttributeList = attributes;

            let mut started = PROCESS_INFORMATION::default();
            unsafe {
                CreateProcessW(
                    name_of(application),
                    Some(PWSTR(line.as_mut_ptr())),
                    None,
                    None,
                    false,
                    EXTENDED_STARTUPINFO_PRESENT
                        | CREATE_UNICODE_ENVIRONMENT
                        | CREATE_NEW_PROCESS_GROUP,
                    Some(environment.as_ptr().cast::<c_void>()),
                    PCWSTR::null(),
                    &raw const startup.StartupInfo,
                    &raw mut started,
                )
            }
            .map_err(hresult)?;
            Ok(Started {
                process: raw_of(started.hProcess),
                thread: raw_of(started.hThread),
                id: started.dwProcessId,
            })
        }

        fn close_handle(&self, handle: usize) {
            let _ = unsafe { CloseHandle(handle_of(handle)) };
        }

        fn has_exited(&self, process: usize) -> Raw<bool> {
            match unsafe { WaitForSingleObject(handle_of(process), 0) } {
                WAIT_TIMEOUT => Ok(false),
                WAIT_OBJECT_0 => Ok(true),
                _ => Err(hresult(windows_core::Error::from_thread())),
            }
        }

        fn exit_code(&self, process: usize) -> Raw<u32> {
            let mut code = 0_u32;
            unsafe { GetExitCodeProcess(handle_of(process), &raw mut code) }.map_err(hresult)?;
            Ok(code)
        }

        fn terminate(&self, process: usize, code: u32) -> Raw<()> {
            unsafe { TerminateProcess(handle_of(process), code) }.map_err(hresult)
        }

        fn tell(&self, event: ConsoleEvent) -> Raw<()> {
            unsafe { GenerateConsoleCtrlEvent(event.code(), 0) }.map_err(hresult)
        }

        fn console_input(&self) -> io::Result<File> {
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(THE_CONSOLES_INPUT)
        }

        fn console_mode(&self, input: &File) -> Raw<u32> {
            let mut mode = CONSOLE_MODE(0);
            unsafe { GetConsoleMode(HANDLE(input.as_raw_handle()), &raw mut mode) }
                .map_err(hresult)?;
            Ok(mode.0)
        }
    }

    #[cfg(test)]
    mod tests {
        use windows::Win32::System::Console::{ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT};

        use super::super::{ENABLE_ECHO_INPUT as ECHO, ENABLE_LINE_INPUT as LINE};

        #[test]
        fn the_line_input_flags_are_the_systems() {
            assert_eq!(LINE, ENABLE_LINE_INPUT.0);
            assert_eq!(ECHO, ENABLE_ECHO_INPUT.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use parking_lot::Mutex;

    use super::*;
    use crate::error::HResult;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Pipe,
        CreateConsole(i16, i16),
        CloseConsole(u32),
        ListBytes(u32),
        InitializeList {
            slots: usize,
            count: u32,
            bytes: usize,
        },
        Attach(u32, u32),
        DeleteList(u32),
        CreateProcess {
            application: Vec<u16>,
            line: Vec<u16>,
            environment: Vec<u16>,
            list: u32,
        },
        CloseHandle(u32),
        HasExited(u32),
        ExitCode(u32),
        Terminate(u32, u32),
        Tell(ConsoleEvent),
        ConsoleInput,
        ConsoleMode,
    }

    const CONSOLE: u32 = 70;
    const LIST: u32 = 80;
    const PROCESS: u32 = 91;
    const THREAD: u32 = 92;

    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<Call>>>,
        refusing: Option<&'static str>,
        pipes_made: Arc<Mutex<usize>>,
        pipe_fails_at: Option<usize>,
        list_bytes: usize,
        exited: bool,
        mode: u32,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                list_bytes: 60,
                ..Self::default()
            }
        }

        fn refusing(what: &'static str) -> Self {
            Self {
                refusing: Some(what),
                ..Self::new()
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }

        fn log(&self, call: Call) {
            self.calls.lock().push(call);
        }

        fn answer(&self, what: &'static str) -> Raw<()> {
            if self.refusing == Some(what) {
                return Err(HResult(-41));
            }
            Ok(())
        }
    }

    impl Terminal for Fake {
        type Console = u32;
        type List = u32;
        type Handle = u32;

        fn pipe(&self) -> io::Result<(PipeReader, PipeWriter)> {
            self.log(Call::Pipe);
            let mut made = self.pipes_made.lock();
            *made += 1;
            if self.pipe_fails_at == Some(*made) {
                return Err(io::Error::other("no more pipes"));
            }
            io::pipe()
        }

        fn create_pseudo_console(
            &self,
            columns: i16,
            rows: i16,
            _: &PipeReader,
            _: &PipeWriter,
        ) -> Raw<u32> {
            self.log(Call::CreateConsole(columns, rows));
            self.answer("console")?;
            Ok(CONSOLE)
        }

        fn close_pseudo_console(&self, console: u32) {
            self.log(Call::CloseConsole(console));
        }

        fn attribute_list_bytes(&self, count: u32) -> usize {
            self.log(Call::ListBytes(count));
            self.list_bytes
        }

        fn initialize_attribute_list(
            &self,
            storage: &mut [usize],
            count: u32,
            bytes: usize,
        ) -> Raw<u32> {
            self.log(Call::InitializeList {
                slots: storage.len(),
                count,
                bytes,
            });
            self.answer("initialize")?;
            Ok(LIST)
        }

        fn attach_pseudo_console(&self, list: u32, console: u32) -> Raw<()> {
            self.log(Call::Attach(list, console));
            self.answer("attach")
        }

        fn delete_attribute_list(&self, list: u32) {
            self.log(Call::DeleteList(list));
        }

        fn create_process(
            &self,
            application: &Wide,
            line: &mut [u16],
            environment: &[u16],
            attributes: u32,
        ) -> Raw<Started<u32>> {
            self.log(Call::CreateProcess {
                application: application.units().to_vec(),
                line: line.to_vec(),
                environment: environment.to_vec(),
                list: attributes,
            });
            self.answer("process")?;
            Ok(Started {
                process: PROCESS,
                thread: THREAD,
                id: 4242,
            })
        }

        fn close_handle(&self, handle: u32) {
            self.log(Call::CloseHandle(handle));
        }

        fn has_exited(&self, process: u32) -> Raw<bool> {
            self.log(Call::HasExited(process));
            self.answer("wait")?;
            Ok(self.exited)
        }

        fn exit_code(&self, process: u32) -> Raw<u32> {
            self.log(Call::ExitCode(process));
            self.answer("code")?;
            Ok(0xC000_013A)
        }

        fn terminate(&self, process: u32, code: u32) -> Raw<()> {
            self.log(Call::Terminate(process, code));
            self.answer("terminate")
        }

        fn tell(&self, event: ConsoleEvent) -> Raw<()> {
            self.log(Call::Tell(event));
            self.answer("tell")
        }

        fn console_input(&self) -> io::Result<File> {
            self.log(Call::ConsoleInput);
            if self.refusing == Some("input") {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            File::open(std::env::current_exe()?)
        }

        fn console_mode(&self, _: &File) -> Raw<u32> {
            self.log(Call::ConsoleMode);
            self.answer("mode")?;
            Ok(self.mode)
        }
    }

    fn text(units: &[u16]) -> String {
        String::from_utf16_lossy(units.strip_suffix(&[0]).unwrap_or(units))
    }

    fn opened(fake: &Fake) -> Pseudo<Fake> {
        let (console, _typed, _shown) = Pseudo::open(fake.clone(), 200, 50).expect("opened");
        fake.calls.lock().clear();
        console
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

    #[test]
    fn an_empty_environment_is_still_two_nuls_and_a_nul_inside_one_drops_its_pair() {
        assert_eq!(environment_block(&[]), [0, 0]);
        assert_eq!(
            environment_block(&[("K".into(), "a\0b".into()), ("L".into(), "v".into())]),
            units_of(OsStr::new("L=v\0\0")),
        );
    }

    #[test]
    fn a_command_line_never_holds_a_nul_before_its_own() {
        let line = command_line(OsStr::new("p"), &[OsStr::new("a\0b")]);

        assert_eq!(line.iter().filter(|unit| **unit == 0).count(), 1);
        assert_eq!(line.last(), Some(&0));
    }

    #[test]
    fn a_console_is_made_from_two_pipes_then_closed_once_when_dropped() {
        let fake = Fake::new();

        drop(Pseudo::open(fake.clone(), 120, 30).expect("opened"));

        assert_eq!(
            fake.calls(),
            [
                Call::Pipe,
                Call::Pipe,
                Call::CreateConsole(120, 30),
                Call::CloseConsole(CONSOLE)
            ]
        );
    }

    #[test]
    fn a_pipe_that_cannot_be_made_stops_the_open_before_any_console_exists() {
        for failing in [1, 2] {
            let fake = Fake {
                pipe_fails_at: Some(failing),
                ..Fake::new()
            };

            let opened = Pseudo::open(fake.clone(), 80, 24);

            assert!(
                matches!(
                    opened,
                    Err(Error::Io {
                        op: Op::MakePipe,
                        ..
                    })
                ),
                "{failing}"
            );
            assert!(
                !fake
                    .calls()
                    .iter()
                    .any(|call| matches!(call, Call::CreateConsole(..)))
            );
            assert!(
                !fake
                    .calls()
                    .iter()
                    .any(|call| matches!(call, Call::CloseConsole(..)))
            );
        }
    }

    #[test]
    fn a_console_the_system_refuses_is_named_and_is_never_closed() {
        let fake = Fake::refusing("console");

        let opened = Pseudo::open(fake.clone(), 80, 24);

        assert!(matches!(
            opened,
            Err(Error::Windows {
                op: Op::OpenPseudoConsole,
                source: HResult(-41)
            })
        ));
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, Call::CloseConsole(..)))
        );
    }

    #[test]
    fn a_spawn_sizes_the_attribute_list_from_what_the_system_asked_for_and_closes_the_thread_handle()
     {
        let fake = Fake::new();
        let console = opened(&fake);

        let process = console
            .spawn(
                Path::new("p.exe"),
                &[OsStr::new("x")],
                &[("K".into(), "v".into())],
            )
            .expect("spawned");

        assert_eq!(process.id(), 4242);
        assert_eq!(
            fake.calls(),
            [
                Call::ListBytes(ATTRIBUTES_HELD),
                Call::InitializeList {
                    slots: 60_usize.div_ceil(size_of::<usize>()),
                    count: ATTRIBUTES_HELD,
                    bytes: 60
                },
                Call::Attach(LIST, CONSOLE),
                Call::CreateProcess {
                    application: units_of(OsStr::new("p.exe"))
                        .into_iter()
                        .chain([0])
                        .collect(),
                    line: units_of(OsStr::new("\"p.exe\" \"x\""))
                        .into_iter()
                        .chain([0])
                        .collect(),
                    environment: units_of(OsStr::new("K=v\0\0")),
                    list: LIST
                },
                Call::CloseHandle(THREAD),
                Call::DeleteList(LIST)
            ]
        );
    }

    #[test]
    fn a_spawned_process_keeps_its_own_handle_and_closes_it_once_when_dropped() {
        let fake = Fake::new();
        let console = opened(&fake);
        let process = console.spawn(Path::new("p"), &[], &[]).expect("spawned");
        fake.calls.lock().clear();

        drop(process);

        assert_eq!(fake.calls(), [Call::CloseHandle(PROCESS)]);
    }

    #[test]
    fn an_attribute_list_that_will_not_start_is_not_deleted_and_no_process_is_made() {
        let fake = Fake::refusing("initialize");
        let console = opened(&fake);

        let spawned = console.spawn(Path::new("p"), &[], &[]);

        assert!(matches!(
            spawned,
            Err(Error::Windows {
                op: Op::MakeAttributes,
                source: HResult(-41)
            })
        ));
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, Call::DeleteList(..) | Call::CreateProcess { .. }))
        );
    }

    #[test]
    fn an_attribute_that_will_not_attach_still_deletes_the_list_and_makes_no_process() {
        let fake = Fake::refusing("attach");
        let console = opened(&fake);

        let spawned = console.spawn(Path::new("p"), &[], &[]);

        assert!(matches!(
            spawned,
            Err(Error::Windows {
                op: Op::MakeAttributes,
                ..
            })
        ));
        assert_eq!(fake.calls().last(), Some(&Call::DeleteList(LIST)));
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, Call::CreateProcess { .. }))
        );
    }

    #[test]
    fn a_process_the_system_will_not_start_names_the_step_and_still_deletes_the_list() {
        let fake = Fake::refusing("process");
        let console = opened(&fake);

        let spawned = console.spawn(Path::new("p"), &[], &[]);

        assert!(matches!(
            spawned,
            Err(Error::Windows {
                op: Op::StartProcess,
                source: HResult(-41)
            })
        ));
        assert_eq!(fake.calls().last(), Some(&Call::DeleteList(LIST)));
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, Call::CloseHandle(..)))
        );
    }

    #[test]
    fn a_program_name_holding_a_nul_is_refused_before_the_system_is_asked_anything() {
        let fake = Fake::new();
        let console = opened(&fake);

        let spawned = console.spawn(&PathBuf::from("p\0"), &[], &[]);

        assert!(matches!(spawned, Err(Error::NulInName { .. })));
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_process_still_running_has_no_exit_code_and_one_that_ended_reads_it() {
        let running = Fake::new();
        let ended = Fake {
            exited: true,
            ..Fake::new()
        };

        let still = opened(&running)
            .spawn(Path::new("p"), &[], &[])
            .expect("spawned");
        let over = opened(&ended)
            .spawn(Path::new("p"), &[], &[])
            .expect("spawned");

        assert_eq!(still.exit_code().expect("polled"), None);
        assert_eq!(over.exit_code().expect("polled"), Some(0xC000_013A));
        assert!(
            !running
                .calls()
                .iter()
                .any(|call| matches!(call, Call::ExitCode(..)))
        );
    }

    #[test]
    fn a_refused_wait_or_exit_code_names_the_wait() {
        for what in ["wait", "code"] {
            let fake = Fake {
                exited: true,
                ..Fake::refusing(what)
            };
            let console = opened(&Fake::new());
            drop(console);
            let process = Spawned::owning(
                fake.clone(),
                Started {
                    process: PROCESS,
                    thread: THREAD,
                    id: 1,
                },
            );

            let polled = process.exit_code();

            assert!(
                matches!(
                    polled,
                    Err(Error::Windows {
                        op: Op::WaitForProcess,
                        ..
                    })
                ),
                "{what}"
            );
        }
    }

    #[test]
    fn ending_a_process_terminates_it_with_the_test_code() {
        let fake = Fake::new();
        let process = Spawned::owning(
            fake.clone(),
            Started {
                process: PROCESS,
                thread: THREAD,
                id: 1,
            },
        );
        fake.calls.lock().clear();

        process.end().expect("ended");

        assert_eq!(fake.calls(), [Call::Terminate(PROCESS, ENDED_BY_THE_TEST)]);
    }

    #[test]
    fn a_refused_termination_names_itself() {
        let fake = Fake::refusing("terminate");
        let process = Spawned::owning(
            fake,
            Started {
                process: PROCESS,
                thread: THREAD,
                id: 1,
            },
        );

        assert!(matches!(
            process.end(),
            Err(Error::Windows {
                op: Op::EndProcess,
                ..
            })
        ));
    }

    #[test]
    fn the_console_is_told_each_event_and_a_refusal_names_the_call() {
        let fake = Fake::new();

        telling_the_console(&fake, ConsoleEvent::Interrupt).expect("told");
        telling_the_console(&fake, ConsoleEvent::Break).expect("told");

        assert_eq!(
            fake.calls(),
            [
                Call::Tell(ConsoleEvent::Interrupt),
                Call::Tell(ConsoleEvent::Break)
            ]
        );
        assert!(matches!(
            telling_the_console(&Fake::refusing("tell"), ConsoleEvent::Break),
            Err(Error::Windows {
                op: Op::TellTheConsole,
                ..
            })
        ));
    }

    #[test]
    fn the_input_mode_reads_the_mode_of_the_opened_input() {
        let fake = Fake {
            mode: ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | 0x10,
            ..Fake::new()
        };

        let mode = mode_of_the_input(&fake).expect("a mode");

        assert!(mode.takes_a_line_at_a_time());
        assert_eq!(fake.calls(), [Call::ConsoleInput, Call::ConsoleMode]);
        assert!(!ConsoleMode(ENABLE_LINE_INPUT).takes_a_line_at_a_time());
        assert!(!ConsoleMode(ENABLE_ECHO_INPUT).takes_a_line_at_a_time());
    }

    #[test]
    fn an_input_that_will_not_open_is_an_io_error_and_a_refused_mode_names_the_read() {
        assert!(matches!(
            mode_of_the_input(&Fake::refusing("input")),
            Err(Error::Io {
                op: Op::ReadConsoleMode,
                ..
            })
        ));
        assert!(matches!(
            mode_of_the_input(&Fake::refusing("mode")),
            Err(Error::Windows {
                op: Op::ReadConsoleMode,
                ..
            })
        ));
    }
}
