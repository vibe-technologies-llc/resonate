use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use crate::system::Windows;
use crate::{
    error::{Error, HResult, Op, Raw, Result, refused_on},
    wide::{Wide, text_of},
};

const THE_FILES_OWN_DATA: &str = "::$DATA";

pub(crate) trait Streaming {
    type Search;

    fn first(&self, name: &Wide) -> Raw<(Self::Search, Vec<u16>)>;
    fn next(&self, search: &Self::Search) -> Raw<Vec<u16>>;
}

fn name_of(units: &[u16]) -> OsString {
    let length = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    text_of(units.get(..length).unwrap_or_default())
}

fn has_ended(error: HResult) -> bool {
    error == HResult::HANDLE_EOF
}

fn alternate_streams(streaming: &impl Streaming, path: &Path) -> Result<Vec<OsString>> {
    let name = Wide::of_path(path)?;
    let (search, first) = match streaming.first(&name) {
        Ok(found) => found,
        Err(error) if has_ended(error) => return Ok(Vec::new()),
        Err(error) => return Err(refused_on(Op::ListStreams, path)(error)),
    };

    let mut streams = Vec::new();
    let mut found = first;
    loop {
        let stream = name_of(&found);
        if stream != OsStr::new(THE_FILES_OWN_DATA) {
            streams.push(stream);
        }
        found = match streaming.next(&search) {
            Ok(next) => next,
            Err(error) if has_ended(error) => return Ok(streams),
            Err(error) => return Err(refused_on(Op::ListStreams, path)(error)),
        };
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

pub(crate) fn carrying_alternate_streams(
    streaming: &impl Streaming,
    from: &Path,
    to: &Path,
) -> Result<()> {
    for stream in alternate_streams(streaming, from)? {
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

#[cfg(windows)]
pub fn carry_alternate_streams(from: &Path, to: &Path) -> Result<()> {
    carrying_alternate_streams(&Windows, from, to)
}

#[cfg(windows)]
mod real {
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FindClose, FindFirstStreamW, FindNextStreamW, FindStreamInfoStandard,
            WIN32_FIND_STREAM_DATA,
        },
    };

    use super::Streaming;
    use crate::{
        error::Raw,
        system::{Windows, hresult, name_of},
        wide::Wide,
    };

    pub(crate) struct StreamSearch {
        handle: HANDLE,
    }

    impl Drop for StreamSearch {
        fn drop(&mut self) {
            let _ = unsafe { FindClose(self.handle) };
        }
    }

    impl Streaming for Windows {
        type Search = StreamSearch;

        fn first(&self, name: &Wide) -> Raw<(StreamSearch, Vec<u16>)> {
            let mut found = WIN32_FIND_STREAM_DATA::default();
            let handle = unsafe {
                FindFirstStreamW(
                    name_of(name),
                    FindStreamInfoStandard,
                    (&raw mut found).cast(),
                    None,
                )
            }
            .map_err(hresult)?;
            Ok((StreamSearch { handle }, found.cStreamName.to_vec()))
        }

        fn next(&self, search: &StreamSearch) -> Raw<Vec<u16>> {
            let mut found = WIN32_FIND_STREAM_DATA::default();
            unsafe { FindNextStreamW(search.handle, (&raw mut found).cast()) }.map_err(hresult)?;
            Ok(found.cStreamName.to_vec())
        }
    }

    #[cfg(test)]
    mod tests {
        use windows::Win32::Foundation::ERROR_HANDLE_EOF;

        use crate::error::HResult;

        #[test]
        fn the_end_of_a_search_is_the_systems_handle_eof() {
            assert_eq!(
                HResult::HANDLE_EOF,
                HResult(ERROR_HANDLE_EOF.to_hresult().0)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process, sync::Arc};

    use parking_lot::Mutex;

    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        First(Vec<u16>),
        Next,
        Closed,
    }

    struct Search(Arc<Mutex<Vec<Call>>>);

    impl Drop for Search {
        fn drop(&mut self) {
            self.0.lock().push(Call::Closed);
        }
    }

    #[derive(Clone)]
    struct Fake {
        calls: Arc<Mutex<Vec<Call>>>,
        first: Raw<Vec<&'static str>>,
        refusing_next: Option<usize>,
        found: Arc<Mutex<usize>>,
    }

    impl Fake {
        fn listing(names: &[&'static str]) -> Self {
            Self {
                calls: Arc::default(),
                first: Ok(names.to_vec()),
                refusing_next: None,
                found: Arc::default(),
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }

        fn padded(text: &str) -> Vec<u16> {
            let mut units: Vec<u16> = text.encode_utf16().collect();
            units.resize(296, 0);
            units
        }
    }

    impl Streaming for Fake {
        type Search = Search;

        fn first(&self, name: &Wide) -> Raw<(Search, Vec<u16>)> {
            self.calls.lock().push(Call::First(name.units().to_vec()));
            let names = self.first.clone()?;
            Ok((Search(Arc::clone(&self.calls)), Self::padded(names[0])))
        }

        fn next(&self, _: &Search) -> Raw<Vec<u16>> {
            self.calls.lock().push(Call::Next);
            let mut found = self.found.lock();
            *found += 1;
            if self.refusing_next == Some(*found) {
                return Err(HResult(-31));
            }
            match self.first.as_ref().expect("a listing").get(*found) {
                Some(name) => Ok(Self::padded(name)),
                None => Err(HResult::HANDLE_EOF),
            }
        }
    }

    fn folder(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("resonate-streams-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    #[test]
    fn a_file_with_no_streams_at_all_lists_nothing_and_closes_nothing() {
        let fake = Fake {
            first: Err(HResult::HANDLE_EOF),
            ..Fake::listing(&[])
        };

        let streams = alternate_streams(&fake, Path::new("a.mp3")).expect("a listing");

        assert!(streams.is_empty());
        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn the_files_own_data_is_left_out_and_the_search_is_closed_at_the_end() {
        let fake = Fake::listing(&["::$DATA", ":cover:$DATA", ":tags:$DATA"]);

        let streams = alternate_streams(&fake, Path::new("a.mp3")).expect("a listing");

        assert_eq!(
            streams,
            [
                OsString::from(":cover:$DATA"),
                OsString::from(":tags:$DATA")
            ]
        );
        assert_eq!(fake.calls().last(), Some(&Call::Closed));
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| **call == Call::Closed)
                .count(),
            1
        );
    }

    #[test]
    fn a_first_refusal_other_than_the_end_names_the_file() {
        let fake = Fake {
            first: Err(HResult(-30)),
            ..Fake::listing(&[])
        };

        let listed = alternate_streams(&fake, Path::new("a.mp3"));

        assert!(matches!(
            listed,
            Err(Error::WindowsOnFile { op: Op::ListStreams, ref path, source: HResult(-30) })
                if path == Path::new("a.mp3")
        ));
    }

    #[test]
    fn a_next_refusal_other_than_the_end_still_closes_the_search() {
        let fake = Fake {
            refusing_next: Some(1),
            ..Fake::listing(&["::$DATA", ":one:$DATA", ":two:$DATA"])
        };

        let listed = alternate_streams(&fake, Path::new("a.mp3"));

        assert!(matches!(
            listed,
            Err(Error::WindowsOnFile {
                op: Op::ListStreams,
                source: HResult(-31),
                ..
            })
        ));
        assert_eq!(fake.calls().last(), Some(&Call::Closed));
    }

    #[test]
    fn a_name_is_read_up_to_its_nul_in_a_buffer_of_any_length() {
        assert_eq!(
            name_of(&Fake::padded(":a:$DATA")),
            OsString::from(":a:$DATA")
        );
        assert_eq!(name_of(&[104, 105]), OsString::from("hi"));
        assert_eq!(name_of(&[]), OsString::new());
    }

    #[test]
    fn a_name_holding_a_nul_is_refused_before_a_search_starts() {
        let fake = Fake::listing(&["::$DATA"]);

        let carried = carrying_alternate_streams(&fake, Path::new("a\0"), Path::new("b"));

        assert!(matches!(carried, Err(Error::NulInName { .. })));
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn each_stream_is_copied_beside_the_destination_under_the_same_stream_name() {
        let folder = folder("copy");
        let from = folder.join("from.mp3");
        let to = folder.join("to.mp3");
        fs::write(folder.join("from.mp3:cover"), b"cover bytes").expect("a stream");
        fs::write(folder.join("from.mp3:tags"), b"tag bytes").expect("a stream");
        let fake = Fake::listing(&["::$DATA", ":cover", ":tags"]);

        carrying_alternate_streams(&fake, &from, &to).expect("carried");

        assert_eq!(
            fs::read(folder.join("to.mp3:cover")).expect("copied"),
            b"cover bytes"
        );
        assert_eq!(
            fs::read(folder.join("to.mp3:tags")).expect("copied"),
            b"tag bytes"
        );
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_stream_that_cannot_be_copied_is_an_io_error_naming_the_destination_stream() {
        let folder = folder("missing");
        let from = folder.join("from.mp3");
        let to = folder.join("to.mp3");
        let fake = Fake::listing(&["::$DATA", ":gone"]);

        let carried = carrying_alternate_streams(&fake, &from, &to);

        assert!(matches!(
            carried,
            Err(Error::Io { op: Op::CopyStream, ref path, .. }) if *path == folder.join("to.mp3:gone")
        ));
        let _ = fs::remove_dir_all(&folder);
    }
}
