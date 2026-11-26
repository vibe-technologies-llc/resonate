use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

#[cfg(windows)]
use crate::system::Windows;
use crate::{
    error::{HResult, Raw},
    wide::Wide,
};

const LONGEST_FINITE_WAIT: u32 = u32::MAX - 1;
const NANOSECONDS_A_MILLISECOND: u128 = 1_000_000;
const ONE_TRANSFER_AT_MOST: usize = u32::MAX as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Received {
    Bytes(usize),
    Nothing,
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transfer {
    Done(u32),
    Expired,
}

pub(crate) trait Connecting {
    type Pipe: Copy;

    fn open_identified(&self, name: &Wide) -> Raw<Self::Pipe>;
    fn wait_for_an_instance(&self, name: &Wide, milliseconds: u32) -> Raw<()>;
    fn write_within(&self, pipe: Self::Pipe, bytes: &[u8], milliseconds: u32) -> Raw<Transfer>;
    fn read_within(&self, pipe: Self::Pipe, into: &mut [u8], milliseconds: u32) -> Raw<Transfer>;
    fn close_handle(&self, pipe: Self::Pipe);
}

pub(crate) struct Connected<T: Connecting> {
    connecting: T,
    pipe: T::Pipe,
}

impl<T: Connecting> Connected<T> {
    pub(crate) fn open(connecting: T, path: &Path, within: Duration) -> io::Result<Self> {
        let name = Wide::of(path.as_os_str()).ok_or(io::ErrorKind::InvalidInput)?;
        let deadline =
            Instant::now() + within.min(Duration::from_millis(LONGEST_FINITE_WAIT.into()));

        loop {
            match connecting.open_identified(&name) {
                Ok(pipe) => return Ok(Self { connecting, pipe }),
                Err(HResult::PIPE_BUSY) => {}
                Err(code) => return Err(code.as_io()),
            }

            let left = milliseconds_of(deadline.saturating_duration_since(Instant::now()));

            if left == 0 {
                return Err(io::ErrorKind::TimedOut.into());
            }
            match connecting.wait_for_an_instance(&name, left) {
                Ok(()) => {}
                Err(HResult::SEM_TIMEOUT) => return Err(io::ErrorKind::TimedOut.into()),
                Err(code) => return Err(code.as_io()),
            }
        }
    }

    pub(crate) fn write_within(&self, bytes: &[u8], within: Duration) -> io::Result<usize> {
        let bytes = &bytes[..bytes.len().min(ONE_TRANSFER_AT_MOST)];

        if bytes.is_empty() {
            return Ok(0);
        }
        match self
            .connecting
            .write_within(self.pipe, bytes, milliseconds_of(within))
        {
            Ok(Transfer::Done(written)) => Ok(written as usize),
            Ok(Transfer::Expired) => Err(io::ErrorKind::TimedOut.into()),
            Err(code) => Err(code.as_io()),
        }
    }

    pub(crate) fn read_within(&self, into: &mut [u8], within: Duration) -> io::Result<Received> {
        let length = into.len().min(ONE_TRANSFER_AT_MOST);

        if length == 0 {
            return Ok(Received::Nothing);
        }
        match self
            .connecting
            .read_within(self.pipe, &mut into[..length], milliseconds_of(within))
        {
            Ok(Transfer::Done(0) | Transfer::Expired) => Ok(Received::Nothing),
            Ok(Transfer::Done(read)) => Ok(Received::Bytes(read as usize)),
            Err(HResult::BROKEN_PIPE | HResult::PIPE_NOT_CONNECTED | HResult::HANDLE_EOF) => {
                Ok(Received::Ended)
            }
            Err(code) => Err(code.as_io()),
        }
    }
}

impl<T: Connecting> Drop for Connected<T> {
    fn drop(&mut self) {
        self.connecting.close_handle(self.pipe);
    }
}

fn milliseconds_of(duration: Duration) -> u32 {
    let rounded_up = duration.as_nanos().div_ceil(NANOSECONDS_A_MILLISECOND);

    u32::try_from(rounded_up)
        .unwrap_or(LONGEST_FINITE_WAIT)
        .min(LONGEST_FINITE_WAIT)
}

#[cfg(windows)]
pub struct ClientPipe(Connected<Windows>);

#[cfg(windows)]
impl ClientPipe {
    pub fn open(path: &Path, within: Duration) -> io::Result<Self> {
        Connected::open(Windows, path, within).map(Self)
    }

    pub fn write_within(&self, bytes: &[u8], within: Duration) -> io::Result<usize> {
        self.0.write_within(bytes, within)
    }

    pub fn read_within(&self, into: &mut [u8], within: Duration) -> io::Result<Received> {
        self.0.read_within(into, within)
    }
}

#[cfg(windows)]
mod real {
    use windows::Win32::{
        Foundation::{
            CloseHandle, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE,
            HANDLE, WAIT_OBJECT_0,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
            ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Pipes::WaitNamedPipeW,
            Threading::{CreateEventW, WaitForSingleObject},
        },
    };
    use windows_core::PCWSTR;

    use super::{Connecting, Transfer};
    use crate::{
        error::Raw,
        system::{Windows, handle_of, hresult, name_of, raw_of},
        wide::Wide,
    };

    struct Event(HANDLE);

    impl Event {
        fn unsignalled() -> Raw<Self> {
            unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
                .map(Self)
                .map_err(hresult)
        }
    }

    impl Drop for Event {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn settled_within(
        pipe: HANDLE,
        milliseconds: u32,
        begin: impl FnOnce(*mut OVERLAPPED) -> windows_core::Result<()>,
    ) -> Raw<Transfer> {
        let event = Event::unsignalled()?;
        let mut overlapped = OVERLAPPED {
            hEvent: event.0,
            ..OVERLAPPED::default()
        };

        if let Err(error) = begin(&raw mut overlapped)
            && error.code() != ERROR_IO_PENDING.to_hresult()
        {
            return Err(hresult(error));
        }

        let in_time = unsafe { WaitForSingleObject(event.0, milliseconds) } == WAIT_OBJECT_0;

        if !in_time {
            let _ = unsafe { CancelIoEx(pipe, Some(&raw const overlapped)) };
        }

        let mut moved = 0;

        match unsafe { GetOverlappedResult(pipe, &raw const overlapped, &raw mut moved, true) } {
            Ok(()) => Ok(Transfer::Done(moved)),
            Err(error) if !in_time && error.code() == ERROR_OPERATION_ABORTED.to_hresult() => {
                Ok(Transfer::Expired)
            }
            Err(error) => Err(hresult(error)),
        }
    }

    impl Connecting for Windows {
        type Pipe = usize;

        fn open_identified(&self, name: &Wide) -> Raw<usize> {
            unsafe {
                CreateFileW(
                    name_of(name),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    None,
                )
            }
            .map(raw_of)
            .map_err(hresult)
        }

        fn wait_for_an_instance(&self, name: &Wide, milliseconds: u32) -> Raw<()> {
            unsafe { WaitNamedPipeW(name_of(name), milliseconds) }
                .ok()
                .map_err(hresult)
        }

        fn write_within(&self, pipe: usize, bytes: &[u8], milliseconds: u32) -> Raw<Transfer> {
            let pipe = handle_of(pipe);

            settled_within(pipe, milliseconds, |overlapped| unsafe {
                WriteFile(pipe, Some(bytes), None, Some(overlapped))
            })
        }

        fn read_within(&self, pipe: usize, into: &mut [u8], milliseconds: u32) -> Raw<Transfer> {
            let pipe = handle_of(pipe);

            settled_within(pipe, milliseconds, |overlapped| unsafe {
                ReadFile(pipe, Some(into), None, Some(overlapped))
            })
        }

        fn close_handle(&self, pipe: usize) {
            let _ = unsafe { CloseHandle(handle_of(pipe)) };
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{
            path::{Path, PathBuf},
            process,
            time::{Duration, Instant},
        };

        use windows::Win32::{
            Foundation::{
                CloseHandle, ERROR_BROKEN_PIPE, ERROR_HANDLE_EOF, ERROR_PIPE_BUSY,
                ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, ERROR_SEM_TIMEOUT, HANDLE,
            },
            Security::{
                GetTokenInformation, RevertToSelf, SECURITY_IMPERSONATION_LEVEL,
                SecurityIdentification, TOKEN_QUERY, TokenImpersonationLevel,
            },
            Storage::FileSystem::{PIPE_ACCESS_DUPLEX, ReadFile},
            System::{
                Pipes::{
                    ConnectNamedPipe, CreateNamedPipeW, ImpersonateNamedPipeClient,
                    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
                },
                Threading::{GetCurrentThread, OpenThreadToken},
            },
        };

        use super::super::{ClientPipe, Received};
        use crate::{error::HResult, system::name_of, wide::Wide};

        const SMALLEST_BUFFER: u32 = 1;
        const ONE_INSTANCE: u32 = 1;
        const THE_DEFAULT_TIMEOUT: u32 = 0;
        const GIVEN_UP_AFTER: Duration = Duration::from_millis(300);
        const MORE_THAN_ANY_BUFFER: usize = 4 * 1024 * 1024;

        struct Served(HANDLE);

        impl Served {
            fn at(path: &Path) -> Self {
                let name = Wide::of_path(path).expect("a name");
                let handle = unsafe {
                    CreateNamedPipeW(
                        name_of(&name),
                        PIPE_ACCESS_DUPLEX,
                        PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                        ONE_INSTANCE,
                        SMALLEST_BUFFER,
                        SMALLEST_BUFFER,
                        THE_DEFAULT_TIMEOUT,
                        None,
                    )
                };

                assert!(!handle.is_invalid(), "a pipe served");
                Self(handle)
            }

            fn accepted(&self) {
                match unsafe { ConnectNamedPipe(self.0, None) } {
                    Ok(()) => {}
                    Err(error) => assert_eq!(error.code(), ERROR_PIPE_CONNECTED.to_hresult()),
                }
            }

            fn read_one(&self) -> u8 {
                let mut byte = [0];
                let mut read = 0;

                unsafe { ReadFile(self.0, Some(&mut byte), Some(&raw mut read), None) }
                    .expect("a byte read");

                assert_eq!(read, 1);
                byte[0]
            }
        }

        impl Drop for Served {
            fn drop(&mut self) {
                let _ = unsafe { CloseHandle(self.0) };
            }
        }

        fn pipe_named(what: &str) -> PathBuf {
            PathBuf::from(format!(r"\\.\pipe\resonate-win32-{what}-{}", process::id()))
        }

        fn level_the_server_holds(served: &Served) -> SECURITY_IMPERSONATION_LEVEL {
            unsafe { ImpersonateNamedPipeClient(served.0) }.expect("the client impersonated");

            let mut token = HANDLE::default();

            unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &raw mut token) }
                .expect("the impersonation token");

            let mut level = SECURITY_IMPERSONATION_LEVEL::default();
            let mut returned = 0;

            let asked = unsafe {
                GetTokenInformation(
                    token,
                    TokenImpersonationLevel,
                    Some((&raw mut level).cast()),
                    size_of::<SECURITY_IMPERSONATION_LEVEL>() as u32,
                    &raw mut returned,
                )
            };
            let _ = unsafe { CloseHandle(token) };

            unsafe { RevertToSelf() }.expect("reverted");
            asked.expect("the impersonation level");
            level
        }

        #[test]
        fn the_codes_the_sequencing_reads_are_the_systems() {
            for (ours, theirs) in [
                (HResult::PIPE_BUSY, ERROR_PIPE_BUSY),
                (HResult::SEM_TIMEOUT, ERROR_SEM_TIMEOUT),
                (HResult::BROKEN_PIPE, ERROR_BROKEN_PIPE),
                (HResult::PIPE_NOT_CONNECTED, ERROR_PIPE_NOT_CONNECTED),
                (HResult::HANDLE_EOF, ERROR_HANDLE_EOF),
            ] {
                assert_eq!(ours, HResult(theirs.to_hresult().0));
            }
        }

        #[test]
        fn a_server_may_identify_the_client_but_never_impersonate_it() {
            let path = pipe_named("identified");
            let served = Served::at(&path);
            let client = ClientPipe::open(&path, Duration::from_secs(2)).expect("opened");

            served.accepted();
            client
                .write_within(b"x", Duration::from_secs(2))
                .expect("written");

            assert_eq!(served.read_one(), b'x');
            assert_eq!(level_the_server_holds(&served), SecurityIdentification);
        }

        #[test]
        fn a_write_nobody_reads_is_given_up_once_its_time_is_out() {
            let path = pipe_named("unread");
            let served = Served::at(&path);
            let client = ClientPipe::open(&path, Duration::from_secs(2)).expect("opened");

            served.accepted();

            let began = Instant::now();
            let refused = client
                .write_within(&vec![0; MORE_THAN_ANY_BUFFER], GIVEN_UP_AFTER)
                .expect_err("never read");
            let took = began.elapsed();

            assert_eq!(refused.kind(), std::io::ErrorKind::TimedOut);
            assert!(took >= GIVEN_UP_AFTER, "{took:?}");
            assert!(took < Duration::from_secs(2), "{took:?}");
        }

        #[test]
        fn nothing_sent_reads_as_nothing_and_a_server_gone_reads_as_ended() {
            let path = pipe_named("ended");
            let served = Served::at(&path);
            let client = ClientPipe::open(&path, Duration::from_secs(2)).expect("opened");
            let mut chunk = [0; 16];

            served.accepted();

            let quiet = client
                .read_within(&mut chunk, Duration::from_millis(50))
                .expect("a quiet pipe");

            drop(served);

            let gone = client
                .read_within(&mut chunk, Duration::from_secs(2))
                .expect("a closed pipe");

            assert_eq!(quiet, Received::Nothing);
            assert_eq!(gone, Received::Ended);
        }

        #[test]
        fn a_pipe_nobody_serves_is_the_systems_missing_file() {
            let missing = ClientPipe::open(&pipe_named("absent"), Duration::from_millis(50))
                .err()
                .expect("nobody serves it");

            assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;

    const PIPE: u32 = 7;
    const MISSING: HResult = HResult(0x8007_0002_u32.cast_signed());

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Open,
        Wait(u32),
        Write(usize, u32),
        Read(usize, u32),
        Close(u32),
    }

    struct Fake {
        calls: Arc<Mutex<Vec<Call>>>,
        opened: Mutex<Vec<Raw<u32>>>,
        waited: Raw<()>,
        transferred: Raw<Transfer>,
    }

    impl Fake {
        fn opening(opened: Vec<Raw<u32>>) -> Self {
            Self {
                calls: Arc::default(),
                opened: Mutex::new(opened),
                waited: Ok(()),
                transferred: Ok(Transfer::Done(0)),
            }
        }

        fn transferring(transferred: Raw<Transfer>) -> Self {
            Self {
                transferred,
                ..Self::opening(vec![Ok(PIPE)])
            }
        }
    }

    impl Connecting for Fake {
        type Pipe = u32;

        fn open_identified(&self, _: &Wide) -> Raw<u32> {
            self.calls.lock().push(Call::Open);
            self.opened.lock().remove(0)
        }

        fn wait_for_an_instance(&self, _: &Wide, milliseconds: u32) -> Raw<()> {
            self.calls.lock().push(Call::Wait(milliseconds));
            self.waited
        }

        fn write_within(&self, _: u32, bytes: &[u8], milliseconds: u32) -> Raw<Transfer> {
            self.calls
                .lock()
                .push(Call::Write(bytes.len(), milliseconds));
            self.transferred
        }

        fn read_within(&self, _: u32, into: &mut [u8], milliseconds: u32) -> Raw<Transfer> {
            self.calls.lock().push(Call::Read(into.len(), milliseconds));
            self.transferred
        }

        fn close_handle(&self, pipe: u32) {
            self.calls.lock().push(Call::Close(pipe));
        }
    }

    fn opened(fake: Fake) -> (Connected<Fake>, Arc<Mutex<Vec<Call>>>) {
        let calls = Arc::clone(&fake.calls);
        let connected =
            Connected::open(fake, Path::new(r"\\.\pipe\a"), Duration::from_secs(2)).expect("open");

        (connected, calls)
    }

    #[test]
    fn a_busy_pipe_is_waited_for_and_opened_again() {
        let fake = Fake::opening(vec![Err(HResult::PIPE_BUSY), Ok(PIPE)]);
        let calls = Arc::clone(&fake.calls);

        let connected =
            Connected::open(fake, Path::new(r"\\.\pipe\a"), Duration::from_secs(2)).expect("open");

        drop(connected);

        let calls = calls.lock();

        assert_eq!(calls[0], Call::Open);
        assert!(matches!(calls[1], Call::Wait(left) if left > 0 && left <= 2000));
        assert_eq!(calls[2..], [Call::Open, Call::Close(PIPE)]);
    }

    #[test]
    fn a_pipe_busy_with_no_time_left_is_timed_out_without_waiting() {
        let fake = Fake::opening(vec![Err(HResult::PIPE_BUSY)]);
        let calls = Arc::clone(&fake.calls);

        let refused = Connected::open(fake, Path::new(r"\\.\pipe\a"), Duration::ZERO)
            .err()
            .expect("timed out");

        assert_eq!(refused.kind(), io::ErrorKind::TimedOut);
        assert_eq!(*calls.lock(), [Call::Open]);
    }

    #[test]
    fn an_instance_not_freed_in_time_is_timed_out() {
        let fake = Fake {
            waited: Err(HResult::SEM_TIMEOUT),
            ..Fake::opening(vec![Err(HResult::PIPE_BUSY)])
        };

        let refused = Connected::open(fake, Path::new(r"\\.\pipe\a"), Duration::from_secs(2))
            .err()
            .expect("timed out");

        assert_eq!(refused.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn a_pipe_nobody_serves_is_the_code_windows_gave() {
        let refused = Connected::open(
            Fake::opening(vec![Err(MISSING)]),
            Path::new(r"\\.\pipe\a"),
            Duration::from_secs(2),
        )
        .err()
        .expect("refused");

        assert_eq!(refused.raw_os_error(), Some(2));
    }

    #[test]
    fn a_name_holding_a_nul_is_refused_before_anything_is_opened() {
        let fake = Fake::opening(Vec::new());
        let calls = Arc::clone(&fake.calls);

        let refused = Connected::open(fake, Path::new("a\0b"), Duration::from_secs(2))
            .err()
            .expect("refused");

        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
        assert!(calls.lock().is_empty());
    }

    #[test]
    fn a_write_waits_its_time_rounded_up_and_one_not_taken_in_time_is_timed_out() {
        let (connected, calls) = opened(Fake::transferring(Ok(Transfer::Expired)));

        let refused = connected
            .write_within(b"frame", Duration::from_micros(1_500))
            .expect_err("timed out");

        assert_eq!(refused.kind(), io::ErrorKind::TimedOut);
        assert_eq!(calls.lock()[1], Call::Write(5, 2));
    }

    #[test]
    fn a_write_taken_answers_what_was_written_and_nothing_to_write_asks_nothing() {
        let (connected, calls) = opened(Fake::transferring(Ok(Transfer::Done(5))));

        let written = connected
            .write_within(b"frame", Duration::from_secs(2))
            .expect("written");
        let none = connected
            .write_within(b"", Duration::from_secs(2))
            .expect("nothing");

        assert_eq!(written, 5);
        assert_eq!(none, 0);
        assert_eq!(calls.lock()[1..], [Call::Write(5, 2000)]);
    }

    #[test]
    fn a_wait_past_what_windows_can_name_is_never_infinite() {
        let (connected, calls) = opened(Fake::transferring(Ok(Transfer::Done(1))));

        connected
            .write_within(b"x", Duration::MAX)
            .expect("written");

        assert_eq!(calls.lock()[1], Call::Write(1, LONGEST_FINITE_WAIT));
    }

    #[test]
    fn a_read_answers_what_arrived_nothing_in_time_or_an_empty_read_as_nothing() {
        for (transferred, received) in [
            (Transfer::Done(3), Received::Bytes(3)),
            (Transfer::Done(0), Received::Nothing),
            (Transfer::Expired, Received::Nothing),
        ] {
            let (connected, calls) = opened(Fake::transferring(Ok(transferred)));
            let mut chunk = [0; 16];

            let read = connected
                .read_within(&mut chunk, Duration::ZERO)
                .expect("read");

            assert_eq!(read, received);
            assert_eq!(calls.lock()[1], Call::Read(16, 0));
        }
    }

    #[test]
    fn a_pipe_the_server_let_go_of_reads_as_ended() {
        for code in [
            HResult::BROKEN_PIPE,
            HResult::PIPE_NOT_CONNECTED,
            HResult::HANDLE_EOF,
        ] {
            let (connected, _) = opened(Fake::transferring(Err(code)));
            let mut chunk = [0; 16];

            let read = connected
                .read_within(&mut chunk, Duration::from_secs(2))
                .expect("ended");

            assert_eq!(read, Received::Ended);
        }
    }

    #[test]
    fn any_other_failure_to_read_is_the_code_windows_gave() {
        let (connected, _) = opened(Fake::transferring(Err(MISSING)));
        let mut chunk = [0; 16];

        let refused = connected
            .read_within(&mut chunk, Duration::from_secs(2))
            .expect_err("refused");

        assert_eq!(refused.raw_os_error(), Some(2));
    }

    #[test]
    fn the_pipe_is_closed_once_when_dropped() {
        let (connected, calls) = opened(Fake::transferring(Ok(Transfer::Done(1))));

        drop(connected);

        assert_eq!(*calls.lock(), [Call::Open, Call::Close(PIPE)]);
    }
}
