use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io,
    os::windows::ffi::OsStringExt as _,
    path::{Path, PathBuf},
};

use windows::Win32::{
    Foundation::{ERROR_HANDLE_EOF, HANDLE},
    Storage::FileSystem::{
        FindClose, FindFirstStreamW, FindNextStreamW, FindStreamInfoStandard,
        WIN32_FIND_STREAM_DATA,
    },
};

use crate::{
    error::{Error, Op, Result, refused_on},
    wide::Wide,
};

const THE_FILES_OWN_DATA: &str = "::$DATA";

struct StreamSearch {
    handle: HANDLE,
}

impl Drop for StreamSearch {
    fn drop(&mut self) {
        let _ = unsafe { FindClose(self.handle) };
    }
}

impl StreamSearch {
    fn next(&self, found: &mut WIN32_FIND_STREAM_DATA) -> windows_core::Result<()> {
        unsafe { FindNextStreamW(self.handle, (&raw mut *found).cast()) }
    }
}

fn name_of(found: &WIN32_FIND_STREAM_DATA) -> OsString {
    let units = &found.cStreamName;
    let length = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    OsString::from_wide(units.get(..length).unwrap_or_default())
}

fn has_ended(error: &windows_core::Error) -> bool {
    error.code() == ERROR_HANDLE_EOF.to_hresult()
}

fn alternate_streams(path: &Path) -> Result<Vec<OsString>> {
    let name = Wide::of_path(path)?;
    let mut found = WIN32_FIND_STREAM_DATA::default();
    let first = unsafe {
        FindFirstStreamW(
            name.name(),
            FindStreamInfoStandard,
            (&raw mut found).cast(),
            None,
        )
    };
    let search = match first {
        Ok(handle) => StreamSearch { handle },
        Err(error) if has_ended(&error) => return Ok(Vec::new()),
        Err(error) => return Err(refused_on(Op::ListStreams, path)(error)),
    };

    let mut streams = Vec::new();
    loop {
        let stream = name_of(&found);
        if stream != OsStr::new(THE_FILES_OWN_DATA) {
            streams.push(stream);
        }
        match search.next(&mut found) {
            Ok(()) => {}
            Err(error) if has_ended(&error) => return Ok(streams),
            Err(error) => return Err(refused_on(Op::ListStreams, path)(error)),
        }
    }
}

fn stream_of(path: &Path, stream: &OsStr) -> PathBuf {
    let mut named = path.as_os_str().to_os_string();
    named.push(stream);
    PathBuf::from(named)
}

fn copied(from: &Path, to: &Path) -> io::Result<()> {
    let mut reading = File::open(from)?;
    let mut writing = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(to)?;
    io::copy(&mut reading, &mut writing)?;
    writing.sync_all()
}

pub fn carry_alternate_streams(from: &Path, to: &Path) -> Result<()> {
    for stream in alternate_streams(from)? {
        let source = stream_of(from, &stream);
        let destination = stream_of(to, &stream);
        copied(&source, &destination).map_err(|source| Error::Io {
            op: Op::CopyStream,
            path: destination,
            source,
        })?;
    }
    Ok(())
}
