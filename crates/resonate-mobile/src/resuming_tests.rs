use std::{
    collections::BTreeSet,
    env, fs,
    fs::File,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::{
    Error,
    downloads::{DownloadAccount, Downloads, MusicProvider},
    features::SearchSong,
};

const ECHOES_ISRC: &str = "GBN9Y1100065";
const TRACK: &str = "154140652551016448";
const OTHER_TRACK: &str = "154140652551016449";
const SONG_BYTES: usize = 512 * 1024;
const CUT_AT: usize = 128 * 1024;
const WAITED_FOR_AT_MOST: Duration = Duration::from_secs(20);

#[derive(Clone)]
struct Version {
    body: Vec<u8>,
    etag: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Seen {
    range: Option<String>,
    if_range: Option<String>,
}

struct Gate {
    cut_at: usize,
    released: mpsc::Receiver<()>,
}

struct Serving {
    track: &'static str,
    version: Mutex<Version>,
    honours_range: bool,
    gate: Mutex<Option<Gate>>,
    seen: Mutex<Vec<Seen>>,
}

impl Serving {
    fn of(version: Version) -> Arc<Self> {
        Arc::new(Self {
            track: TRACK,
            version: Mutex::new(version),
            honours_range: true,
            gate: Mutex::new(None),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn cut_after(self: &Arc<Self>, cut_at: usize) -> mpsc::Sender<()> {
        let (release, released) = mpsc::channel();
        *self.gate.lock() = Some(Gate { cut_at, released });
        release
    }

    fn requests_for_the_song(&self) -> Vec<Seen> {
        self.seen.lock().clone()
    }
}

struct Request {
    target: String,
    range: Option<String>,
    if_range: Option<String>,
}

fn pattern(seed: u8) -> Vec<u8> {
    (0..SONG_BYTES)
        .map(|at| (at as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn version(seed: u8, etag: Option<&'static str>) -> Version {
    Version {
        body: pattern(seed),
        etag,
    }
}

fn served(state: Arc<Serving>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let url = format!("http://{}", listener.local_addr().expect("a bound address"));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                return;
            };
            let state = Arc::clone(&state);
            thread::spawn(move || answer(stream, &state));
        }
    });
    url
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let target = line.split(' ').nth(1)?.to_owned();
    let mut range = None;
    let mut if_range = None;
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return None,
            Ok(_) if header == "\r\n" => break,
            Ok(_) => {}
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let value = value.trim().to_owned();
        match name.to_ascii_lowercase().as_str() {
            "range" => range = Some(value),
            "if-range" => if_range = Some(value),
            _ => {}
        }
    }
    Some(Request {
        target,
        range,
        if_range,
    })
}

fn answer(stream: TcpStream, state: &Serving) {
    let mut reader = BufReader::new(stream);
    let Some(request) = read_request(&mut reader) else {
        return;
    };
    let mut stream = reader.into_inner();
    if request.target.starts_with("/search/tracks") {
        let listing = format!(
            r#"{{"tracks":[{{"id":"{0}","trackId":"{0}","title":"Echoes","artistNames":["Pink Floyd"],"duration":1412451,"isrc":"{ECHOES_ISRC}","playable":true}}]}}"#,
            state.track
        );
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            listing.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(listing.as_bytes());
        return;
    }
    if request.target != format!("/track/{}", state.track) {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }

    state.seen.lock().push(Seen {
        range: request.range.clone(),
        if_range: request.if_range.clone(),
    });
    let current = state.version.lock().clone();
    let total = current.body.len();
    let from = request
        .range
        .as_deref()
        .filter(|_| state.honours_range)
        .and_then(|range| range.strip_prefix("bytes="))
        .and_then(|range| range.strip_suffix('-'))
        .and_then(|from| from.parse::<usize>().ok())
        .filter(|&from| from < total)
        .filter(|_| {
            request
                .if_range
                .as_deref()
                .is_none_or(|asked| Some(asked) == current.etag)
        });
    let validator = current
        .etag
        .map(|etag| format!("ETag: {etag}\r\n"))
        .unwrap_or_default();
    let head = match from {
        Some(from) => format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: audio/flac\r\nContent-Length: {}\r\nContent-Range: bytes {from}-{}/{total}\r\n{validator}Connection: close\r\n\r\n",
            total - from,
            total - 1
        ),
        None => format!(
            "HTTP/1.1 200 OK\r\nContent-Type: audio/flac\r\nContent-Length: {total}\r\n{validator}Connection: close\r\n\r\n"
        ),
    };
    let from = from.unwrap_or(0);
    let _ = stream.write_all(head.as_bytes());
    let gate = (from == 0).then(|| state.gate.lock().take()).flatten();
    match gate {
        Some(gate) => {
            let _ = stream.write_all(&current.body[..gate.cut_at]);
            let _ = stream.flush();
            let _ = gate.released.recv_timeout(WAITED_FOR_AT_MOST);
            let _ = stream.write_all(&current.body[gate.cut_at..]);
        }
        None => {
            let _ = stream.write_all(&current.body[from..]);
        }
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("resonate-resuming-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a scratch folder");
        Self(path)
    }

    fn destination(&self, name: &str) -> (PathBuf, File) {
        let path = self.0.join(name);
        let file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("a fresh destination");
        (path, file)
    }

    fn partials(&self) -> PathBuf {
        self.0.join("download-partials")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn echoes() -> SearchSong {
    SearchSong {
        recording: "8f2eb3e7-4ff2-4b14-8c0e-8b7c1e7c2b7d".to_owned(),
        title: "Echoes".to_owned(),
        artist: "Pink Floyd".to_owned(),
        duration_ms: Some(1_412_451),
        isrcs: vec![ECHOES_ISRC.to_owned()],
        ..SearchSong::default()
    }
}

fn account(server: &str) -> DownloadAccount {
    DownloadAccount {
        provider: MusicProvider::Monochrome,
        server: server.to_owned(),
        user: String::new(),
        password: String::new(),
        client_id: String::new(),
        client_secret: String::new(),
        refresh_token: String::new(),
    }
}

fn reaching() -> bool {
    true
}

fn offered(downloads: &Downloads, server: &str) -> u64 {
    let job = downloads.begin(echoes());
    let searched = downloads.find(
        &downloads.job(job).expect("the job"),
        vec![account(server)],
        &BTreeSet::new(),
        &reaching,
    );
    let offer = searched.offer.expect("an offer");
    assert_eq!(offer.total, Some(SONG_BYTES as u64));
    job
}

fn partial_bytes(scratch: &Scratch) -> Option<usize> {
    fs::read_dir(scratch.partials())
        .ok()?
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .ends_with(".resonate-part")
        })
        .and_then(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len() as usize)
}

fn files_in_the_partials(scratch: &Scratch) -> usize {
    fs::read_dir(scratch.partials()).map_or(0, |entries| entries.count())
}

fn cut_download(
    downloads: &Arc<Downloads>,
    scratch: &Scratch,
    server: &str,
    release: &mpsc::Sender<()>,
) {
    let job = offered(downloads, server);
    let (_, output) = scratch.destination("cut");
    let downloading = {
        let downloads = Arc::clone(downloads);
        thread::spawn(move || {
            let job = downloads.job(job).expect("the job");
            downloads.download(&job, output, &reaching)
        })
    };
    let began = Instant::now();
    while downloads
        .progress(job)
        .map_or(0, |progress| progress.received)
        < CUT_AT as u64
    {
        assert!(began.elapsed() < WAITED_FOR_AT_MOST, "the download stalled");
        thread::sleep(Duration::from_millis(10));
    }
    downloads.cancel(job);
    release.send(()).expect("the server waits");

    let cancelled = downloading.join().expect("the download ends");
    assert!(matches!(cancelled, Err(Error::DownloadCancelled)));
}

fn finished_download(
    downloads: &Downloads,
    scratch: &Scratch,
    server: &str,
    name: &str,
) -> (u64, Vec<u8>) {
    let job = offered(downloads, server);
    let (path, output) = scratch.destination(name);
    let bytes = downloads
        .download(&downloads.job(job).expect("the job"), output, &reaching)
        .expect("a finished download");
    (bytes, fs::read(&path).expect("the destination"))
}

#[test]
fn a_download_cut_off_part_way_resumes_with_a_range_and_yields_the_same_bytes() {
    let scratch = Scratch::new("resumes");
    let state = Serving::of(version(1, Some("\"v1\"")));
    let release = state.cut_after(CUT_AT);
    let server = served(Arc::clone(&state));
    let downloads = Arc::new(Downloads::under(&scratch.0));

    cut_download(&downloads, &scratch, &server, &release);
    let kept = partial_bytes(&scratch).expect("a partial kept");
    let (bytes, written) = finished_download(&downloads, &scratch, &server, "resumed");

    let requests = state.requests_for_the_song();
    assert!((CUT_AT..SONG_BYTES).contains(&kept), "kept {kept}");
    assert_eq!(bytes, SONG_BYTES as u64);
    assert!(written == state.version.lock().body, "the bytes differ");
    assert_eq!(
        requests,
        [
            Seen {
                range: None,
                if_range: None
            },
            Seen {
                range: Some(format!("bytes={kept}-")),
                if_range: Some("\"v1\"".to_owned())
            },
        ]
    );
    assert_eq!(files_in_the_partials(&scratch), 0);
}

#[test]
fn a_server_ignoring_range_restarts_cleanly_from_the_start() {
    let scratch = Scratch::new("ignores");
    let state = Arc::new(Serving {
        track: TRACK,
        version: Mutex::new(version(2, Some("\"v1\""))),
        honours_range: false,
        gate: Mutex::new(None),
        seen: Mutex::new(Vec::new()),
    });
    let release = state.cut_after(CUT_AT);
    let server = served(Arc::clone(&state));
    let downloads = Arc::new(Downloads::under(&scratch.0));

    cut_download(&downloads, &scratch, &server, &release);
    let kept = partial_bytes(&scratch).expect("a partial kept");
    let (bytes, written) = finished_download(&downloads, &scratch, &server, "restarted");

    let requests = state.requests_for_the_song();
    assert_eq!(bytes, SONG_BYTES as u64);
    assert!(written == state.version.lock().body, "the bytes differ");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].range, Some(format!("bytes={kept}-")));
    assert_eq!(files_in_the_partials(&scratch), 0);
}

#[test]
fn an_offer_that_changed_discards_the_partial_and_starts_over() {
    let scratch = Scratch::new("changed");
    let state = Serving::of(version(3, Some("\"v1\"")));
    let release = state.cut_after(CUT_AT);
    let server = served(Arc::clone(&state));
    let downloads = Arc::new(Downloads::under(&scratch.0));

    cut_download(&downloads, &scratch, &server, &release);
    *state.version.lock() = version(4, Some("\"v2\""));
    let (bytes, written) = finished_download(&downloads, &scratch, &server, "changed");

    let requests = state.requests_for_the_song();
    assert_eq!(bytes, SONG_BYTES as u64);
    assert!(
        written == pattern(4),
        "the new bytes were spliced onto the old"
    );
    assert_eq!(requests[1].if_range, Some("\"v1\"".to_owned()));
    assert_eq!(files_in_the_partials(&scratch), 0);
}

#[test]
fn a_whole_that_changed_length_without_a_validator_is_asked_for_again_from_the_start() {
    let scratch = Scratch::new("regrown");
    let state = Serving::of(version(5, None));
    let release = state.cut_after(CUT_AT);
    let server = served(Arc::clone(&state));
    let downloads = Arc::new(Downloads::under(&scratch.0));

    cut_download(&downloads, &scratch, &server, &release);
    let mut regrown = pattern(5);
    regrown.extend_from_slice(&[7; 1024]);
    *state.version.lock() = Version {
        body: regrown.clone(),
        etag: None,
    };
    let job = downloads.begin(echoes());
    let searched = downloads.find(
        &downloads.job(job).expect("the job"),
        vec![account(&server)],
        &BTreeSet::new(),
        &reaching,
    );
    let offer = searched.offer.expect("an offer");
    let (_, output) = scratch.destination("regrown");
    let bytes = downloads
        .download(&downloads.job(job).expect("the job"), output, &reaching)
        .expect("a finished download");

    let requests = state.requests_for_the_song();
    assert_eq!(offer.total, Some(regrown.len() as u64));
    assert_eq!(bytes, regrown.len() as u64);
    assert_eq!(requests.len(), 3);
    assert!(requests[1].range.is_some());
    assert_eq!(requests[2].range, None);
    assert_eq!(files_in_the_partials(&scratch), 0);
}

#[test]
fn a_partial_of_another_offer_is_never_ranged_into() {
    let scratch = Scratch::new("other-offer");
    let first = Serving::of(version(6, Some("\"v1\"")));
    let release = first.cut_after(CUT_AT);
    let first_server = served(Arc::clone(&first));
    let other = Arc::new(Serving {
        track: OTHER_TRACK,
        version: Mutex::new(version(7, Some("\"w1\""))),
        honours_range: true,
        gate: Mutex::new(None),
        seen: Mutex::new(Vec::new()),
    });
    let other_server = served(Arc::clone(&other));
    let downloads = Arc::new(Downloads::under(&scratch.0));

    cut_download(&downloads, &scratch, &first_server, &release);
    let (bytes, written) = finished_download(&downloads, &scratch, &other_server, "other");

    assert_eq!(bytes, SONG_BYTES as u64);
    assert!(written == pattern(7), "the other offer's bytes differ");
    assert_eq!(
        other.requests_for_the_song(),
        [Seen {
            range: None,
            if_range: None
        }]
    );
    assert_eq!(files_in_the_partials(&scratch), 0);
}

#[test]
fn a_download_that_finishes_without_a_cut_leaves_no_partial() {
    let scratch = Scratch::new("tidy");
    let state = Serving::of(version(8, Some("\"v1\"")));
    let server = served(Arc::clone(&state));
    let downloads = Downloads::under(&scratch.0);

    let (bytes, written) = finished_download(&downloads, &scratch, &server, "whole");

    assert_eq!(bytes, SONG_BYTES as u64);
    assert!(written == pattern(8), "the bytes differ");
    assert_eq!(files_in_the_partials(&scratch), 0);
}
