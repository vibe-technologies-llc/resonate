use std::{
    env,
    ffi::{OsStr, OsString},
    fmt, io,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        Arc, Weak,
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::SourceId;
use resonate_providers::{Error, Opened, ProviderOp, Result};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::{A_POLL_LASTS, YTMUSIC};

const THE_PROGRAM: &str = "yt-dlp";
const A_SONG_IS_AT: &str = "https://music.youtube.com/watch?v=";
const LONGEST_VIDEO_ID: usize = 16;
const BROWSERS: [&str; 9] = [
    "brave", "chrome", "chromium", "edge", "firefox", "opera", "safari", "vivaldi", "whale",
];
const BROWSER_NAME_ENDS_AT: [char; 2] = [':', '+'];
const FOUND_BY_ITSELF: &str = "deno";
const RUNTIMES_NAMED: [&str; 2] = ["node", "bun"];
const ASKED_WITHIN: Duration = Duration::from_secs(60);
const A_WHOLE_LISTING_TAKES_AT_MOST: u32 = 2;
const LEFT_SILENT_FOR: Duration = Duration::from_secs(120);
const LOOKED_AT_EVERY: Duration = Duration::from_millis(10);
const STDERR_ENDS_WITHIN: Duration = Duration::from_secs(1);
const STDERR_KEPT: usize = 16 * 1024;
const WITHHELD: &str = "<withheld>";
const A_LINE_ENDS_AT: [u8; 2] = [b'\n', b'\r'];
const A_BROWSERS_PROFILE_FOLLOWS: char = ':';
const A_BROWSERS_CONTAINER_FOLLOWS: &str = "::";
const THE_HOME_FOLDER: char = '~';
const CHUNK_BYTES: usize = 64 * 1024;
const CHUNKS_AHEAD: usize = 4;
const LARGEST_LISTING: usize = 32 * 1024 * 1024;
const COOKIES_LAPSED: &str = "The provided YouTube account cookies are no longer valid";
const COOKIES_TURNED_DOWN: u16 = 401;
const THE_VIDEO_IS_GONE: [&str; 3] = [
    "Private video",
    "Video unavailable",
    "This video is not available",
];

#[derive(Clone, PartialEq, Eq)]
pub enum Cookies {
    None,
    Browser(String),
    File(PathBuf),
}

impl Cookies {
    pub fn read(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty() {
            return Self::None;
        }
        let named = value
            .split(BROWSER_NAME_ENDS_AT)
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if BROWSERS.contains(&named.as_str()) {
            Self::Browser(value.to_owned())
        } else {
            Self::File(value.into())
        }
    }

    fn withheld(&self) -> Vec<String> {
        let mut said = match self {
            Self::None => Vec::new(),
            Self::Browser(browser) => {
                let profile = browser
                    .split_once(A_BROWSERS_PROFILE_FOLLOWS)
                    .map(|(_, rest)| rest)
                    .and_then(|rest| rest.split(A_BROWSERS_CONTAINER_FOLLOWS).next());
                std::iter::once(browser.as_str())
                    .chain(profile)
                    .map(str::to_owned)
                    .collect()
            }
            Self::File(file) => vec![file.display().to_string()],
        };
        let under_home: Vec<String> = said
            .iter()
            .filter_map(|text| text.strip_prefix(THE_HOME_FOLDER))
            .map(str::to_owned)
            .collect();
        said.extend(under_home);
        said.retain(|text| !text.is_empty());
        said.sort_by_key(|text| std::cmp::Reverse(text.len()));
        said
    }

    fn arguments(&self) -> Vec<OsString> {
        match self {
            Self::None => Vec::new(),
            Self::Browser(browser) => vec!["--cookies-from-browser".into(), browser.into()],
            Self::File(file) => vec!["--cookies".into(), file.into()],
        }
    }
}

impl fmt::Debug for Cookies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Browser(_) => f.write_str("Browser(<withheld>)"),
            Self::File(_) => f.write_str("File(<withheld>)"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoId(Box<str>);

impl VideoId {
    pub fn read(id: &str) -> Option<Self> {
        let fits = !id.is_empty()
            && id.len() <= LONGEST_VIDEO_ID
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        fits.then(|| Self(id.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn address(&self) -> String {
        format!("{A_SONG_IS_AT}{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Opus774,
    Aac141,
    Opus251,
}

impl Tier {
    const BEST_FIRST: [Self; 3] = [Self::Opus774, Self::Aac141, Self::Opus251];

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Opus774 | Self::Opus251 => "mka",
            Self::Aac141 => "m4a",
        }
    }

    pub const fn is_premium(self) -> bool {
        match self {
            Self::Opus774 | Self::Aac141 => true,
            Self::Opus251 => false,
        }
    }

    const fn format_id(self) -> &'static str {
        match self {
            Self::Opus774 => "774",
            Self::Aac141 => "141",
            Self::Opus251 => "251",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Formats {
    pub best: Option<Tier>,
    pub length: Option<u64>,
}

fn offers_readable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<Offer>, D::Error> {
    let offered = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(offered
        .into_iter()
        .filter_map(|offer| {
            serde_json::from_value(offer)
                .inspect_err(|error| {
                    tracing::debug!(%error, "a format that does not read is passed over");
                })
                .ok()
        })
        .collect())
}

#[derive(Deserialize)]
struct Listing {
    #[serde(default, deserialize_with = "offers_readable")]
    formats: Vec<Offer>,
}

#[derive(Deserialize)]
struct Offer {
    format_id: String,
    #[serde(default)]
    filesize: Option<Value>,
}

impl Listing {
    fn formats(&self) -> Formats {
        Tier::BEST_FIRST
            .into_iter()
            .find_map(|tier| {
                self.formats
                    .iter()
                    .find(|offer| offer.format_id == tier.format_id())
                    .map(|offer| Formats {
                        best: Some(tier),
                        length: offer.filesize.as_ref().and_then(Value::as_u64),
                    })
            })
            .unwrap_or_default()
    }
}

fn holds_a_program(folders: &[PathBuf], name: &str) -> bool {
    folders
        .iter()
        .any(|folder| folder.join(name).is_file() || folder.join(format!("{name}.exe")).is_file())
}

fn runtime_named_on(path: Option<&OsStr>) -> Option<&'static str> {
    let folders: Vec<PathBuf> = env::split_paths(path?).collect();
    if holds_a_program(&folders, FOUND_BY_ITSELF) {
        return None;
    }
    RUNTIMES_NAMED
        .into_iter()
        .find(|runtime| holds_a_program(&folders, runtime))
}

fn says_the_video_is_gone(said: &str) -> bool {
    THE_VIDEO_IS_GONE.iter().any(|words| said.contains(words))
}

#[derive(Clone, Debug, Default)]
struct Lapse(Arc<Mutex<Option<Instant>>>);

impl Lapse {
    fn heard(&self) {
        *self.0.lock() = Some(Instant::now());
    }

    fn stands(&self) -> bool {
        self.0
            .lock()
            .is_some_and(|heard| heard.elapsed() < A_POLL_LASTS)
    }

    fn forgotten(&self) {
        *self.0.lock() = None;
    }
}

struct Living {
    stopped: bool,
    children: Vec<Weak<Mutex<Child>>>,
}

static LIVING: Mutex<Living> = Mutex::new(Living {
    stopped: false,
    children: Vec::new(),
});

pub fn stop_the_tools() {
    let mut living = LIVING.lock();
    living.stopped = true;
    for child in living
        .children
        .drain(..)
        .filter_map(|child| child.upgrade())
    {
        let _ = child.lock().kill();
    }
}

struct Reaped(Arc<Mutex<Child>>);

impl Reaped {
    fn spawned(command: &mut Command) -> io::Result<Self> {
        let mut living = LIVING.lock();
        if living.stopped {
            return Err(ErrorKind::Interrupted.into());
        }
        let child = Arc::new(Mutex::new(command.spawn()?));
        living.children.retain(|held| held.strong_count() > 0);
        living.children.push(Arc::downgrade(&child));
        Ok(Self(child))
    }

    fn kill(&self) -> io::Result<()> {
        self.0.lock().kill()
    }

    fn exited_within(&mut self, within: Duration) -> io::Result<Option<ExitStatus>> {
        let began = Instant::now();
        loop {
            if let Some(status) = self.0.lock().try_wait()? {
                return Ok(Some(status));
            }
            if began.elapsed() >= within {
                self.kill()?;
                return Ok(None);
            }
            thread::sleep(LOOKED_AT_EVERY);
        }
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        let mut child = self.0.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
}

struct Telling {
    withheld: Vec<String>,
    line: Vec<u8>,
}

impl Telling {
    fn heard(&mut self, said: &[u8]) {
        self.line.extend_from_slice(said);
        while let Some(end) = self
            .line
            .iter()
            .position(|byte| A_LINE_ENDS_AT.contains(byte))
        {
            let line: Vec<u8> = self.line.drain(..=end).collect();
            self.told(&line);
        }
        if self.line.len() > STDERR_KEPT {
            let line = std::mem::take(&mut self.line);
            self.told(&line);
        }
    }

    fn told(&self, line: &[u8]) {
        let said = self
            .withheld
            .iter()
            .fold(String::from_utf8_lossy(line).into_owned(), |said, value| {
                said.replace(value.as_str(), WITHHELD)
            });
        let said = said.trim_end();
        if !said.is_empty() {
            tracing::debug!(said, "yt-dlp said");
        }
    }
}

impl Drop for Telling {
    fn drop(&mut self) {
        let line = std::mem::take(&mut self.line);
        self.told(&line);
    }
}

struct Complaints {
    kept: Arc<Mutex<Vec<u8>>>,
    ended: Receiver<()>,
}

impl Complaints {
    fn drained_from(mut stderr: ChildStderr, withheld: Vec<String>) -> io::Result<Self> {
        let kept = Arc::new(Mutex::new(Vec::new()));
        let (ending, ended) = mpsc::channel();
        let tail = Arc::clone(&kept);
        thread::Builder::new()
            .name("resonate-ytmusic-stderr".to_owned())
            .spawn(move || {
                let _ending = ending;
                let mut telling = Telling {
                    withheld,
                    line: Vec::new(),
                };
                let mut chunk = vec![0; CHUNK_BYTES];
                loop {
                    match stderr.read(&mut chunk) {
                        Ok(0) => return,
                        Ok(read) => {
                            let said = &chunk[..read];
                            telling.heard(said);
                            kept_at_its_end(&mut tail.lock(), said);
                        }
                        Err(error) if error.kind() == ErrorKind::Interrupted => {}
                        Err(_) => return,
                    }
                }
            })?;
        Ok(Self { kept, ended })
    }

    fn said(&self) -> String {
        let _ = self.ended.recv_timeout(STDERR_ENDS_WITHIN);
        String::from_utf8_lossy(&self.kept.lock()).into_owned()
    }
}

fn kept_at_its_end(kept: &mut Vec<u8>, said: &[u8]) {
    kept.extend_from_slice(said);
    let past = kept.len().saturating_sub(STDERR_KEPT);
    kept.drain(..past);
}

type Chunks = Receiver<io::Result<Vec<u8>>>;

fn pumped_from(mut stdout: ChildStdout) -> io::Result<Chunks> {
    let (sending, chunks) = mpsc::sync_channel(CHUNKS_AHEAD);
    thread::Builder::new()
        .name("resonate-ytmusic-stdout".to_owned())
        .spawn(move || {
            loop {
                let mut chunk = vec![0; CHUNK_BYTES];
                match stdout.read(&mut chunk) {
                    Ok(0) => return,
                    Ok(read) => {
                        chunk.truncate(read);
                        if sending.send(Ok(chunk)).is_err() {
                            return;
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(error) => {
                        let _ = sending.send(Err(error));
                        return;
                    }
                }
            }
        })?;
    Ok(chunks)
}

struct Running {
    child: Reaped,
    chunks: Chunks,
    complaints: Complaints,
}

impl Running {
    fn of(program: &Path, arguments: &[OsString], withheld: Vec<String>) -> io::Result<Self> {
        let child = Reaped::spawned(
            Command::new(program)
                .args(arguments)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )?;
        let (stdout, stderr) = {
            let mut held = child.0.lock();
            (held.stdout.take(), held.stderr.take())
        };
        let stdout = stdout.ok_or(ErrorKind::BrokenPipe)?;
        let stderr = stderr.ok_or(ErrorKind::BrokenPipe)?;
        Ok(Self {
            complaints: Complaints::drained_from(stderr, withheld)?,
            chunks: pumped_from(stdout)?,
            child,
        })
    }
}

#[derive(Clone, Copy)]
enum Ended {
    Whole,
    Failed(Option<i32>),
    Silent,
}

struct Reading {
    running: Running,
    held: Vec<u8>,
    read: usize,
    ended: Option<Ended>,
    silent_for: Duration,
    provider: SourceId,
    lapse: Lapse,
}

impl Reading {
    fn ending(&mut self) -> Ended {
        let ended = match self.running.child.exited_within(self.silent_for) {
            Ok(Some(status)) if status.success() => Ended::Whole,
            Ok(Some(status)) => Ended::Failed(status.code()),
            Ok(None) => Ended::Silent,
            Err(error) => {
                tracing::debug!(%error, "yt-dlp could not be waited on");
                Ended::Failed(None)
            }
        };
        if self.running.complaints.said().contains(COOKIES_LAPSED) {
            self.lapse.heard();
        }
        ended
    }

    fn take_next(&mut self) {
        match self.running.chunks.recv_timeout(self.silent_for) {
            Ok(Ok(chunk)) => {
                self.held = chunk;
                self.read = 0;
            }
            Ok(Err(error)) => {
                tracing::debug!(%error, "yt-dlp's stdout could not be read");
                self.ended = Some(Ended::Failed(None));
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = self.running.child.kill();
                self.ended = Some(Ended::Silent);
            }
            Err(RecvTimeoutError::Disconnected) => self.ended = Some(self.ending()),
        }
    }
}

impl Read for Reading {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        loop {
            let left = &self.held[self.read..];
            if !left.is_empty() || into.is_empty() {
                let taken = left.len().min(into.len());
                into[..taken].copy_from_slice(&left[..taken]);
                self.read += taken;
                return Ok(taken);
            }
            match self.ended {
                Some(Ended::Whole) => return Ok(0),
                Some(Ended::Silent) => {
                    return Err(io::Error::new(
                        ErrorKind::TimedOut,
                        Error::Io {
                            provider: self.provider.clone(),
                            op: ProviderOp::Download,
                            source: ErrorKind::TimedOut.into(),
                        },
                    ));
                }
                Some(Ended::Failed(code)) => {
                    return Err(io::Error::other(Error::ToolFailed {
                        provider: self.provider.clone(),
                        op: ProviderOp::Download,
                        code,
                    }));
                }
                None => self.take_next(),
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tool {
    provider: SourceId,
    program: PathBuf,
    cookies: Cookies,
    runtime: Option<&'static str>,
    asked_within: Duration,
    left_silent_for: Duration,
    lapse: Lapse,
}

impl Tool {
    pub fn new(program: Option<PathBuf>, cookies: Cookies) -> Self {
        Self {
            provider: SourceId::new(YTMUSIC).unwrap_or_else(|_| SourceId::local()),
            program: program.unwrap_or_else(|| THE_PROGRAM.into()),
            cookies,
            runtime: runtime_named_on(env::var_os("PATH").as_deref()),
            asked_within: ASKED_WITHIN,
            left_silent_for: LEFT_SILENT_FOR,
            lapse: Lapse::default(),
        }
    }

    #[must_use]
    pub fn waiting_at_most(self, within: Duration) -> Self {
        Self {
            asked_within: within,
            left_silent_for: within,
            ..self
        }
    }

    pub(crate) fn holds_cookies(&self) -> bool {
        self.cookies != Cookies::None
    }

    fn arguments(&self, asked: &[&str], video: &VideoId) -> Vec<OsString> {
        let mut arguments: Vec<OsString> = vec!["--ignore-config".into(), "--no-playlist".into()];
        if let Some(runtime) = self.runtime {
            arguments.extend(["--js-runtimes".into(), runtime.into()]);
        }
        arguments.extend(self.cookies.arguments());
        arguments.extend(asked.iter().map(OsString::from));
        arguments.push(video.address().into());
        arguments
    }

    fn running(&self, op: ProviderOp, arguments: &[OsString]) -> Result<Running> {
        Running::of(&self.program, arguments, self.cookies.withheld())
            .map_err(|source| self.failed_io(op, source))
    }

    fn failed_io(&self, op: ProviderOp, source: io::Error) -> Error {
        Error::Io {
            provider: self.provider.clone(),
            op,
            source,
        }
    }

    fn hung(&self, op: ProviderOp) -> Error {
        self.failed_io(op, ErrorKind::TimedOut.into())
    }

    fn failed(&self, op: ProviderOp, code: Option<i32>) -> Error {
        Error::ToolFailed {
            provider: self.provider.clone(),
            op,
            code,
        }
    }

    fn cookies_lapsed(&self, op: ProviderOp) -> Error {
        Error::Unwelcome {
            provider: self.provider.clone(),
            op,
            code: COOKIES_TURNED_DOWN,
        }
    }

    pub(crate) fn unreadable(&self, op: ProviderOp) -> Error {
        Error::Unreadable {
            provider: self.provider.clone(),
            op,
        }
    }

    fn listed_within(&self, running: &Running, op: ProviderOp, by: Instant) -> Result<Vec<u8>> {
        let mut listing = Vec::new();
        loop {
            match running
                .chunks
                .recv_timeout(by.saturating_duration_since(Instant::now()))
            {
                Ok(Ok(chunk)) if listing.len() + chunk.len() > LARGEST_LISTING => {
                    return Err(self.unreadable(op));
                }
                Ok(Ok(chunk)) => listing.extend(chunk),
                Ok(Err(source)) => return Err(self.failed_io(op, source)),
                Err(RecvTimeoutError::Timeout) => return Err(self.hung(op)),
                Err(RecvTimeoutError::Disconnected) => return Ok(listing),
            }
        }
    }

    pub fn formats(&self, video: &VideoId) -> Result<Formats> {
        if self.lapse.stands() {
            return Err(self.cookies_lapsed(ProviderOp::Search));
        }
        self.formats_asked(video)
    }

    pub(crate) fn formats_checked(&self, video: &VideoId) -> Result<Formats> {
        let formats = self.formats_asked(video)?;
        self.lapse.forgotten();
        Ok(formats)
    }

    fn formats_asked(&self, video: &VideoId) -> Result<Formats> {
        let op = ProviderOp::Search;
        let by = Instant::now() + self.asked_within;
        let arguments = self.arguments(&["-J", "--skip-download"], video);
        let mut running = self.running(op, &arguments)?;
        let listing = self.listed_within(&running, op, by)?;
        let status = running
            .child
            .exited_within(by.saturating_duration_since(Instant::now()))
            .map_err(|source| self.failed_io(op, source))?
            .ok_or_else(|| self.hung(op))?;
        let said = running.complaints.said();

        if said.contains(COOKIES_LAPSED) {
            self.lapse.heard();
            return Err(self.cookies_lapsed(op));
        }
        if status.success() {
            return serde_json::from_slice::<Listing>(&listing)
                .map(|listing| listing.formats())
                .map_err(|error| {
                    tracing::debug!(%error, "yt-dlp's listing was not the document expected");
                    self.unreadable(op)
                });
        }
        if says_the_video_is_gone(&said) {
            return Ok(Formats::default());
        }
        Err(self.failed(op, status.code()))
    }

    pub(crate) fn listed_at(&self, address: &str) -> Result<Vec<u8>> {
        let op = ProviderOp::Browse;
        let by = Instant::now() + self.asked_within * A_WHOLE_LISTING_TAKES_AT_MOST;
        let mut arguments: Vec<OsString> = vec!["--ignore-config".into()];
        if let Some(runtime) = self.runtime {
            arguments.extend(["--js-runtimes".into(), runtime.into()]);
        }
        arguments.extend(self.cookies.arguments());
        arguments.extend(["-J".into(), "--flat-playlist".into(), address.into()]);
        let mut running = self.running(op, &arguments)?;
        let listing = self.listed_within(&running, op, by)?;
        let status = running
            .child
            .exited_within(by.saturating_duration_since(Instant::now()))
            .map_err(|source| self.failed_io(op, source))?
            .ok_or_else(|| self.hung(op))?;
        if running.complaints.said().contains(COOKIES_LAPSED) {
            self.lapse.heard();
            return Err(self.cookies_lapsed(op));
        }
        if status.success() {
            return Ok(listing);
        }
        Err(self.failed(op, status.code()))
    }

    pub fn download(&self, video: &VideoId, format: Tier) -> Result<Opened> {
        let op = ProviderOp::Download;
        let arguments = self.arguments(&["-f", format.format_id(), "-o", "-"], video);
        let mut running = self.running(op, &arguments)?;
        let first = match running.chunks.recv_timeout(self.left_silent_for) {
            Ok(Ok(chunk)) => chunk,
            Ok(Err(source)) => return Err(self.failed_io(op, source)),
            Err(RecvTimeoutError::Timeout) => return Err(self.hung(op)),
            Err(RecvTimeoutError::Disconnected) => {
                return self.answered_nothing(&mut running);
            }
        };

        Ok(Opened::Reading {
            reader: Box::new(Reading {
                running,
                held: first,
                read: 0,
                ended: None,
                silent_for: self.left_silent_for,
                provider: self.provider.clone(),
                lapse: self.lapse.clone(),
            }),
            length: None,
        })
    }

    fn answered_nothing(&self, running: &mut Running) -> Result<Opened> {
        let op = ProviderOp::Download;
        let status = running
            .child
            .exited_within(self.left_silent_for)
            .map_err(|source| self.failed_io(op, source))?
            .ok_or_else(|| self.hung(op))?;
        let said = running.complaints.said();
        if said.contains(COOKIES_LAPSED) {
            self.lapse.heard();
            return Err(self.cookies_lapsed(op));
        }
        if says_the_video_is_gone(&said) {
            return Ok(Opened::Gone);
        }
        Err(self.failed(op, status.code()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn folder_holding(names: &[&str], called: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!(
            "resonate-ytmusic-runtimes-{}-{called}",
            std::process::id()
        ));
        fs::create_dir_all(&folder).expect("a folder");
        for name in names {
            fs::write(folder.join(name), b"").expect("a program");
        }
        folder
    }

    #[test]
    fn a_runtime_is_named_only_where_deno_is_not_on_the_path() {
        let deno = folder_holding(&["deno", "node"], "deno");
        let node = folder_holding(&["node", "bun"], "node");
        let bun = folder_holding(&["bun"], "bun");
        let none = folder_holding(&[], "none");
        let path = |folders: &[&PathBuf]| env::join_paths(folders).expect("a path");

        assert_eq!(runtime_named_on(Some(&path(&[&none, &deno]))), None);
        assert_eq!(runtime_named_on(Some(&path(&[&node]))), Some("node"));
        assert_eq!(runtime_named_on(Some(&path(&[&bun, &node]))), Some("node"));
        assert_eq!(runtime_named_on(Some(&path(&[&bun]))), Some("bun"));
        assert_eq!(runtime_named_on(Some(&path(&[&none]))), None);
        assert_eq!(runtime_named_on(None), None);

        for folder in [deno, node, bun, none] {
            fs::remove_dir_all(folder).expect("the folder removed");
        }
    }

    #[test]
    fn the_runtime_named_is_handed_to_the_tool() {
        let tool = Tool {
            runtime: Some("node"),
            ..Tool::new(None, Cookies::None)
        };
        let video = VideoId::read("abc").expect("a video id");

        let arguments = tool.arguments(&["-J"], &video);

        let at = arguments
            .iter()
            .position(|argument| argument == "--js-runtimes")
            .expect("the runtime named");
        assert_eq!(arguments[at + 1], "node");
    }

    #[test]
    fn a_lapse_stands_for_a_poll_and_no_longer() {
        let lapse = Lapse::default();
        assert!(!lapse.stands());

        lapse.heard();
        assert!(lapse.stands());

        *lapse.0.lock() = Instant::now().checked_sub(A_POLL_LASTS);
        assert!(!lapse.stands());
    }

    #[test]
    fn a_whole_listing_is_given_two_minutes() {
        assert_eq!(
            ASKED_WITHIN * A_WHOLE_LISTING_TAKES_AT_MOST,
            Duration::from_secs(120)
        );
    }

    #[test]
    fn a_cookie_value_is_withheld_whole_and_by_its_profile_longest_first() {
        assert_eq!(
            Cookies::read("firefox:~/zen::work").withheld(),
            ["firefox:~/zen::work", "~/zen", "/zen"]
        );
        assert_eq!(Cookies::read("firefox").withheld(), ["firefox"]);
        assert_eq!(
            Cookies::read("/home/a/cookies.txt").withheld(),
            ["/home/a/cookies.txt"]
        );
        assert!(Cookies::None.withheld().is_empty());
    }

    #[test]
    fn what_a_tool_says_is_kept_at_its_end_and_no_more_of_it() {
        let mut kept = Vec::new();

        kept_at_its_end(&mut kept, &vec![b'a'; STDERR_KEPT]);
        kept_at_its_end(&mut kept, b"the end");

        assert_eq!(kept.len(), STDERR_KEPT);
        assert!(kept.ends_with(b"the end"));
    }
}
