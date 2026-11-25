use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use resonate_core::{Gate, writer::Held};
use resonate_fetch::{Ranged, Trusted, Unfetched, configured, downloading_agent, followed};
use sha2::{Digest, Sha256};
use ureq::Agent;

use crate::{ArchiveOp, Archives, Building, Error, Packet, Phase, Result, packet_read};

pub const DUMP_SERVER: &str = "https://data.metabrainz.org/pub/musicbrainz/data/fullexport";
pub const REPLICATION_SERVER: &str = "https://metabrainz.org/api/musicbrainz";
pub const LARGEST_PACKET: u64 = 64 * 1024 * 1024;
pub const CORE_ARCHIVE: &str = "mbdump.tar.bz2";
pub const DERIVED_ARCHIVE: &str = "mbdump-derived.tar.bz2";
const CHECKSUMS: &str = "SHA256SUMS";
const LATEST: &str = "LATEST";
const DOWNLOADS: &str = "downloads";
const PARTIAL: &str = "part";
const VERIFIED: &str = "verified";
const ANSWERED_WITHIN: Duration = Duration::from_secs(30);
const BROKEN_OFF_AFTER: Duration = Duration::from_secs(60);
const RESUMED_AFTER: Duration = Duration::from_secs(2);
const LONGEST_LISTING: u64 = 64 * 1024;
const COPIED_AT_ONCE: usize = 1024 * 1024;
const RANGE_PAST_THE_END: u16 = 416;
const GATE_LOOKED_AT_EVERY: Duration = Duration::from_secs(1);
const NOT_YET_PUBLISHED: u16 = 404;
const TOKEN_REFUSALS: [u16; 3] = [400, 401, 403];
const TOKEN_LENGTH: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Carried {
    Encrypted,
    #[cfg(test)]
    Plain,
}

pub fn agent() -> Agent {
    agent_carrying(Carried::Encrypted)
}

fn agent_carrying(carried: Carried) -> Agent {
    let config = configured(ANSWERED_WITHIN, Trusted::BuiltInRoots)
        .https_only(carried == Carried::Encrypted);
    downloading_agent(config, BROKEN_OFF_AFTER)
}

fn listed(agent: &Agent, url: &str) -> Result<String> {
    let response = followed(url, |url| agent.get(url).call()).map_err(|error| Error::Fetch {
        url: url.to_owned(),
        source: resonate_fetch::as_io(error),
    })?;
    let status = response.status().as_u16();

    if status != 200 {
        return Err(Error::Refused {
            url: url.to_owned(),
            status,
        });
    }

    let mut text = String::new();

    response
        .into_body()
        .into_reader()
        .take(LONGEST_LISTING)
        .read_to_string(&mut text)
        .map_err(|source| Error::Fetch {
            url: url.to_owned(),
            source,
        })?;
    Ok(text)
}

pub fn is_a_dump_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
}

pub fn latest(agent: &Agent) -> Result<String> {
    let named = listed(agent, &format!("{DUMP_SERVER}/{LATEST}"))?;
    let named = named.trim();
    if is_a_dump_name(named) {
        Ok(named.to_owned())
    } else {
        Err(Error::NoLatest)
    }
}

pub fn checksum_listed(listing: &str, file: &str) -> Option<String> {
    listing.lines().find_map(|line| {
        let (sum, named) = line.split_once(char::is_whitespace)?;
        (named.trim().trim_start_matches('*') == file).then(|| sum.trim().to_ascii_lowercase())
    })
}

pub fn summed(path: &Path, building: &Building) -> Result<String> {
    let mut file = fs::File::open(path).map_err(Error::archive(path, ArchiveOp::Open))?;

    building.of(file.metadata().map(|meta| meta.len()).unwrap_or(0));

    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPIED_AT_ONCE];
    let mut read_so_far = 0_u64;

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(Error::archive(path, ArchiveOp::Read))?;

        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        read_so_far += read as u64;
        building.reached(read_so_far);
        building.looked_at()?;
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn fetched(
    agent: &Agent,
    dump: &str,
    folder: &Path,
    gate: &Gate,
    building: &Building,
    _held: &Held,
) -> Result<Archives> {
    let downloads = folder.join(DOWNLOADS);
    let into = downloads.join(dump);

    fs::create_dir_all(&into).map_err(Error::archive(&into, ArchiveOp::Stage))?;
    if let Ok(earlier) = fs::read_dir(&downloads) {
        for entry in earlier.flatten() {
            if entry.file_name() != dump {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    let listing = listed(agent, &format!("{DUMP_SERVER}/{dump}/{CHECKSUMS}"))?;
    let mut held = Vec::new();

    for file in [CORE_ARCHIVE, DERIVED_ARCHIVE] {
        let sum = checksum_listed(&listing, file).ok_or_else(|| Error::Unlisted {
            file: file.to_owned(),
        })?;
        let url = format!("{DUMP_SERVER}/{dump}/{file}");

        held.push(downloaded(agent, &url, &into, file, &sum, gate, building)?);
    }

    let derived = held.pop().unwrap_or_default();
    let core = held.pop().unwrap_or_default();
    Ok(Archives {
        core,
        derived,
        dump: dump.to_owned(),
    })
}

pub fn is_a_replication_token(token: &str) -> bool {
    token.len() == TOKEN_LENGTH && token.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn packet_url(sequence: u64) -> String {
    format!("{REPLICATION_SERVER}/replication-{sequence}-v2.tar.bz2")
}

pub fn packet(agent: &Agent, token: &str, sequence: u64) -> Result<Option<Packet>> {
    packet_from(
        agent,
        &packet_url(sequence),
        token,
        sequence,
        LARGEST_PACKET,
    )
}

fn packet_from(
    agent: &Agent,
    url: &str,
    token: &str,
    sequence: u64,
    largest: u64,
) -> Result<Option<Packet>> {
    let too_large = Error::PacketTooLarge { sequence, largest };
    let response = followed(url, |url| {
        agent
            .get(url)
            .query("token", token)
            .config()
            .http_status_as_error(false)
            .build()
            .call()
    })
    .map_err(|error| Error::Fetch {
        url: url.to_owned(),
        source: resonate_fetch::as_io(error),
    })?;
    let status = response.status().as_u16();

    if status == NOT_YET_PUBLISHED {
        return Ok(None);
    }
    if TOKEN_REFUSALS.contains(&status) {
        return Err(Error::TokenRefused { status });
    }
    if status != 200 {
        return Err(Error::Refused {
            url: url.to_owned(),
            status,
        });
    }
    if response
        .body()
        .content_length()
        .is_some_and(|declared| declared > largest)
    {
        return Err(too_large);
    }

    let mut received = Received {
        inner: response
            .into_body()
            .into_reader()
            .take(largest.saturating_add(1)),
        read: 0,
        failure: None,
    };

    let packet = packet_read(&mut received, sequence);

    if let Some(refusal) = received.refusal(url, largest, too_large) {
        return Err(refusal);
    }
    packet.map(Some)
}

struct Received<R> {
    inner: io::Take<R>,
    read: u64,
    failure: Option<io::Error>,
}

impl<R: Read> Read for Received<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buf) {
            Ok(read) => {
                self.read += read as u64;
                Ok(read)
            }
            Err(failure) => {
                let kind = failure.kind();

                self.failure = Some(failure);
                Err(kind.into())
            }
        }
    }
}

impl<R: Read> Received<R> {
    fn refusal(&mut self, url: &str, largest: u64, too_large: Error) -> Option<Error> {
        if self.failure.is_none() {
            let _ = io::copy(self, &mut io::sink());
        }
        if self.read > largest {
            return Some(too_large);
        }
        self.failure.take().map(|source| Error::Fetch {
            url: url.to_owned(),
            source,
        })
    }
}

pub fn forget_downloads(folder: &Path) {
    let _ = fs::remove_dir_all(folder.join(DOWNLOADS));
}

fn named(path: &Path, extension: &str) -> PathBuf {
    let mut named = path.as_os_str().to_owned();

    named.push(".");
    named.push(extension);
    PathBuf::from(named)
}

fn downloaded(
    agent: &Agent,
    url: &str,
    into: &Path,
    file: &str,
    sum: &str,
    gate: &Gate,
    building: &Building,
) -> Result<PathBuf> {
    let whole = into.join(file);
    let verified = named(&whole, VERIFIED);

    if whole.is_file() && fs::read_to_string(&verified).is_ok_and(|held| held.trim() == sum) {
        return Ok(whole);
    }

    let partial = named(&whole, PARTIAL);

    if whole.is_file() {
        fs::rename(&whole, &partial).map_err(Error::archive(&whole, ArchiveOp::Rename))?;
    }

    let mut started_over = false;

    loop {
        let fetched = fetched_whole(agent, url, &partial, gate, building)?;
        let may_start_over = !started_over && !matches!(fetched, Fetched::FromTheStart);

        if let Fetched::OnAStalePart(source) = fetched {
            let _ = fs::remove_file(&partial);

            if may_start_over {
                tracing::debug!(error = %source, file, "a saved part is not the archive served now and is fetched again from the start");
                started_over = true;
                continue;
            }
            return Err(Error::Fetch {
                url: url.to_owned(),
                source,
            });
        }

        building.enter(Phase::Verifying);

        let found = summed(&partial, building)?;

        if found == sum {
            break;
        }

        let _ = fs::remove_file(&partial);

        if may_start_over {
            tracing::debug!(
                file,
                "an archive grown on a saved part fails its checksum and is fetched again from the start"
            );
            started_over = true;
            continue;
        }
        return Err(Error::Checksum {
            file: file.to_owned(),
        });
    }
    fs::rename(&partial, &whole).map_err(Error::archive(&partial, ArchiveOp::Rename))?;
    fs::write(&verified, sum).map_err(Error::archive(&verified, ArchiveOp::Stage))?;
    Ok(whole)
}

#[derive(Debug)]
enum Fetched {
    FromTheStart,
    OnAPart,
    OnAStalePart(io::Error),
}

fn fetched_whole(
    agent: &Agent,
    url: &str,
    partial: &Path,
    gate: &Gate,
    building: &Building,
) -> Result<Fetched> {
    let mut on_a_part = false;

    loop {
        building.enter(Phase::Downloading);
        while gate.is_shut() {
            building.enter(Phase::Waiting);
            building.looked_at()?;
            gate.wait_while_shut(GATE_LOOKED_AT_EVERY);
        }
        building.enter(Phase::Downloading);

        let from = fs::metadata(partial).map_or(0, |meta| meta.len());

        on_a_part |= from > 0;
        match copied(agent, url, partial, from, gate, building)? {
            Copied::Shut => continue,
            Copied::Stale(source) => return Ok(Fetched::OnAStalePart(source)),
            Copied::Whole | Copied::PastTheEnd if on_a_part => return Ok(Fetched::OnAPart),
            Copied::Whole | Copied::PastTheEnd => return Ok(Fetched::FromTheStart),
        }
    }
}

fn whole_size(total: Option<u64>, length: Option<u64>, held: u64) -> u64 {
    total
        .or_else(|| length.map(|to_come| to_come + held))
        .unwrap_or(0)
}

#[derive(Debug)]
enum Copied {
    Whole,
    PastTheEnd,
    Shut,
    Stale(io::Error),
}

fn copied(
    agent: &Agent,
    url: &str,
    partial: &Path,
    from: u64,
    gate: &Gate,
    building: &Building,
) -> Result<Copied> {
    let mut source = match Ranged::opened_from(agent.clone(), url.to_owned(), from, RESUMED_AFTER) {
        Ok(source) => source,
        Err(Unfetched::Refused(RANGE_PAST_THE_END)) if from > 0 => return Ok(Copied::PastTheEnd),
        Err(stale @ (Unfetched::Changed | Unfetched::Misranged)) if from > 0 => {
            return Ok(Copied::Stale(stale.into_io()));
        }
        Err(Unfetched::Refused(status)) => {
            return Err(Error::Refused {
                url: url.to_owned(),
                status,
            });
        }
        Err(unfetched) => {
            return Err(Error::Fetch {
                url: url.to_owned(),
                source: unfetched.into_io(),
            });
        }
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(partial)
        .map_err(Error::archive(partial, ArchiveOp::Stage))?;

    let _held = Held::over(
        file.try_clone()
            .map_err(Error::archive(partial, ArchiveOp::Stage))?,
    );

    building.of(whole_size(source.total(), source.length(), from));

    let mut buffer = vec![0_u8; COPIED_AT_ONCE];
    let mut written = from;

    loop {
        if gate.is_shut() {
            file.sync_data()
                .map_err(Error::archive(partial, ArchiveOp::Stage))?;
            return Ok(Copied::Shut);
        }
        building.looked_at()?;

        let read = match source.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                return Err(Error::Fetch {
                    url: url.to_owned(),
                    source,
                });
            }
        };

        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(Error::archive(partial, ArchiveOp::Stage))?;
        written += read as u64;
        building.reached(written);
    }
    file.sync_all()
        .map_err(Error::archive(partial, ArchiveOp::Stage))?;
    Ok(Copied::Whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checksum_is_read_off_the_line_naming_the_archive() {
        let listing = "aaa *mbdump-derived.tar.bz2\nBBB *mbdump.tar.bz2\n";

        assert_eq!(
            checksum_listed(listing, CORE_ARCHIVE).as_deref(),
            Some("bbb")
        );
        assert_eq!(
            checksum_listed(listing, DERIVED_ARCHIVE).as_deref(),
            Some("aaa")
        );
        assert_eq!(checksum_listed(listing, "mbdump-edit.tar.bz2"), None);
    }

    #[test]
    fn a_replication_token_is_forty_letters_and_digits() {
        assert!(is_a_replication_token(&"a1".repeat(20)));
        assert!(!is_a_replication_token(&"a1".repeat(19)));
        assert!(!is_a_replication_token(&format!("{}&x", "a".repeat(38))));
    }

    #[test]
    fn a_packet_is_asked_for_by_its_number_in_the_dbmirror2_form() {
        assert_eq!(
            packet_url(171_234),
            "https://metabrainz.org/api/musicbrainz/replication-171234-v2.tar.bz2"
        );
    }

    #[test]
    fn a_dump_is_named_by_its_date_and_nothing_else() {
        assert!(is_a_dump_name("20261104-002222"));
        assert!(!is_a_dump_name("../etc"));
        assert!(!is_a_dump_name(""));
    }

    const ARCHIVE: &[u8] = b"the whole of an export archive";

    fn plain_agent() -> Agent {
        agent_carrying(Carried::Plain)
    }

    fn asked_head(stream: &mut std::net::TcpStream) -> String {
        let mut asked = Vec::new();
        let mut byte = [0_u8; 1];

        while !asked.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).expect("the request") == 0 {
                break;
            }
            asked.push(byte[0]);
        }
        String::from_utf8_lossy(&asked).to_ascii_lowercase()
    }

    #[test]
    fn the_export_is_never_asked_for_over_plain_http() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}/{LATEST}", listener.local_addr().expect("bound"));

        let asked = listed(&agent(), &url);

        listener
            .set_nonblocking(true)
            .expect("a listener that need not wait");

        let reached = listener.accept();

        assert!(matches!(asked, Err(Error::Fetch { .. })), "{asked:?}");
        assert!(
            reached
                .as_ref()
                .is_err_and(|error| error.kind() == io::ErrorKind::WouldBlock),
            "a plain address was asked: {reached:?}"
        );
    }

    fn serving_a_packet(
        declared: bool,
        body: Vec<u8>,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!(
            "http://{}/replication-8-v2.tar.bz2",
            listener.local_addr().expect("bound")
        );
        let serving = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a fetch");
            let asked = asked_head(&mut stream);
            let length = if declared {
                format!("Content-Length: {}\r\n", body.len())
            } else {
                String::new()
            };
            let mut answer =
                format!("HTTP/1.1 200 OK\r\n{length}Connection: close\r\n\r\n").into_bytes();

            answer.extend_from_slice(&body);

            let _ = stream.write_all(&answer);
            asked
        });
        (url, serving)
    }

    #[test]
    fn a_packet_larger_than_a_packet_is_read_up_to_is_refused_as_too_large() {
        const LARGEST: u64 = 64;
        const TOKEN: &str = "a-token";

        for declared in [true, false] {
            let (url, serving) = serving_a_packet(declared, vec![b'x'; LARGEST as usize + 1]);

            let fetched = packet_from(&plain_agent(), &url, TOKEN, 8, LARGEST);
            let asked = serving.join().expect("served");

            assert!(asked.contains("token=a-token"), "{asked}");
            assert!(
                matches!(
                    fetched,
                    Err(Error::PacketTooLarge {
                        sequence: 8,
                        largest: LARGEST
                    })
                ),
                "declared {declared}: {:?}",
                fetched.err()
            );
        }
    }

    fn served_once_more(listener: &std::net::TcpListener) {
        let (mut stream, _) = listener.accept().expect("a fetch");
        let asked = asked_head(&mut stream);
        let from = asked
            .lines()
            .find_map(|line| line.strip_prefix("range: bytes="))
            .and_then(|range| range.trim().trim_end_matches('-').parse::<usize>().ok())
            .unwrap_or(0);
        let total = ARCHIVE.len();

        let answer = if from >= total && from > 0 {
            format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .into_bytes()
        } else {
            let mut answer =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n")
                    .into_bytes();

            answer.extend_from_slice(ARCHIVE);
            answer
        };

        stream.write_all(&answer).expect("the answer");
    }

    fn sum_of(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn a_folder(named: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("resonate-{named}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);

        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    #[test]
    fn a_whole_part_the_server_answers_past_the_end_is_verified_or_fetched_again() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!(
            "http://{}/{CORE_ARCHIVE}",
            listener.local_addr().expect("bound")
        );
        let serving = std::thread::spawn(move || {
            for _ in 0..3 {
                served_once_more(&listener);
            }
        });
        let into = a_folder("whole-part");
        let partial = named(&into.join(CORE_ARCHIVE), PARTIAL);
        let sum = sum_of(ARCHIVE);

        fs::write(&partial, ARCHIVE).expect("a whole part");

        let verified = downloaded(
            &plain_agent(),
            &url,
            &into,
            CORE_ARCHIVE,
            &sum,
            &Gate::open(),
            &Building::default(),
        )
        .expect("the part is verified");
        let held = fs::read(&verified).expect("the archive");

        fs::remove_file(&verified).expect("removed");
        fs::write(&partial, vec![b'x'; ARCHIVE.len()]).expect("a damaged part");

        let fetched_again = downloaded(
            &plain_agent(),
            &url,
            &into,
            CORE_ARCHIVE,
            &sum,
            &Gate::open(),
            &Building::default(),
        )
        .expect("the archive is fetched again");
        let held_again = fs::read(&fetched_again).expect("the archive");

        serving.join().expect("served");

        let _ = fs::remove_dir_all(&into);

        assert_eq!(held, ARCHIVE);
        assert_eq!(held_again, ARCHIVE);
    }

    type Answer = Box<dyn Fn(&str) -> Vec<u8> + Send>;

    const DAMAGED: &[u8] = b"a damaged copy of what was served";
    const SAVED: usize = 10;

    fn whole(body: &'static [u8]) -> Answer {
        Box::new(move |_| {
            let mut answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();

            answer.extend_from_slice(body);
            answer
        })
    }

    fn ranged_as_asked(body: &'static [u8]) -> Answer {
        Box::new(move |asked| {
            let from = asked
                .lines()
                .find_map(|line| line.strip_prefix("range: bytes="))
                .and_then(|range| range.trim().trim_end_matches('-').parse::<usize>().ok())
                .unwrap_or(0);
            let mut answer = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len() - 1,
                body.len(),
                body.len() - from
            )
            .into_bytes();

            answer.extend_from_slice(&body[from..]);
            answer
        })
    }

    fn served_in_turn(answers: Vec<Answer>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!(
            "http://{}/{CORE_ARCHIVE}",
            listener.local_addr().expect("bound")
        );
        let serving = std::thread::spawn(move || {
            answers
                .into_iter()
                .map(|answer| {
                    let (mut stream, _) = listener.accept().expect("a fetch");
                    let asked = asked_head(&mut stream);

                    stream.write_all(&answer(&asked)).expect("the answer");
                    asked
                })
                .collect()
        });
        (url, serving)
    }

    fn downloaded_over(
        saved: Option<Vec<u8>>,
        answers: Vec<Answer>,
    ) -> (Result<Vec<u8>>, Vec<String>, bool) {
        let (url, serving) = served_in_turn(answers);
        let into = a_folder("stale-part");
        let partial = named(&into.join(CORE_ARCHIVE), PARTIAL);

        if let Some(saved) = saved {
            fs::write(&partial, saved).expect("a saved part");
        }

        let fetched = downloaded(
            &plain_agent(),
            &url,
            &into,
            CORE_ARCHIVE,
            &sum_of(ARCHIVE),
            &Gate::open(),
            &Building::default(),
        )
        .map(|whole| fs::read(whole).expect("the archive"));
        let asked = serving.join().expect("served");
        let part_left = partial.exists();

        let _ = fs::remove_dir_all(&into);

        (fetched, asked, part_left)
    }

    fn ranges_of(asked: &[String]) -> Vec<Option<String>> {
        asked
            .iter()
            .map(|head| {
                head.lines()
                    .find_map(|line| line.strip_prefix("range: ").map(str::to_owned))
            })
            .collect()
    }

    fn redirecting_to(location: String) -> Answer {
        Box::new(move |_| {
            format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .into_bytes()
        })
    }

    fn refused_as_redirected<T>(fetched: &Result<T>) -> bool {
        matches!(
            fetched,
            Err(Error::Fetch { source, .. }) if source.kind() == io::ErrorKind::PermissionDenied
        )
    }

    #[test]
    fn a_redirect_off_the_origin_is_never_followed_for_a_listing_a_packet_or_an_archive() {
        let elsewhere = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let location = format!("http://{}/moved", elsewhere.local_addr().expect("bound"));
        let (url, serving) = served_in_turn(vec![
            redirecting_to(location.clone()),
            redirecting_to(location.clone()),
            redirecting_to(location),
        ]);
        let into = a_folder("redirected");
        let partial = named(&into.join(CORE_ARCHIVE), PARTIAL);

        let listing = listed(&plain_agent(), &url);
        let packet = packet_from(&plain_agent(), &url, "a-token", 8, LARGEST_PACKET);
        let archive = copied(
            &plain_agent(),
            &url,
            &partial,
            0,
            &Gate::open(),
            &Building::default(),
        );
        let asked = serving.join().expect("served");

        elsewhere
            .set_nonblocking(true)
            .expect("a listener that need not wait");

        let reached = elsewhere.accept();

        let _ = fs::remove_dir_all(&into);

        assert!(refused_as_redirected(&listing), "{listing:?}");
        assert!(refused_as_redirected(&packet), "{:?}", packet.err());
        assert!(refused_as_redirected(&archive), "{archive:?}");
        assert_eq!(asked.len(), 3);
        assert!(
            reached
                .as_ref()
                .is_err_and(|error| error.kind() == io::ErrorKind::WouldBlock),
            "the other port was asked: {reached:?}"
        );
    }

    #[test]
    fn a_redirect_within_the_origin_is_followed() {
        let (url, serving) = served_in_turn(vec![
            redirecting_to("/moved/LATEST".to_owned()),
            whole(b"20261104-002222\n"),
        ]);

        let listing = listed(&plain_agent(), &url);
        let asked = serving.join().expect("served");

        assert_eq!(listing.expect("the listing"), "20261104-002222\n");
        assert!(asked[1].starts_with("get /moved/latest "), "{}", asked[1]);
    }

    #[test]
    fn a_resumed_answer_failing_its_checksum_is_fetched_again_once_from_the_start() {
        let (fetched, asked, part_left) = downloaded_over(
            Some(vec![b'x'; SAVED]),
            vec![ranged_as_asked(ARCHIVE), whole(ARCHIVE)],
        );

        assert_eq!(fetched.expect("a verified archive"), ARCHIVE);
        assert_eq!(ranges_of(&asked), [Some(format!("bytes={SAVED}-")), None]);
        assert!(!part_left);
    }

    #[test]
    fn a_whole_answer_shorter_than_the_saved_part_is_fetched_again_once_from_the_start() {
        let saved = ARCHIVE.len() + SAVED;

        let (fetched, asked, part_left) = downloaded_over(
            Some(vec![b'x'; saved]),
            vec![whole(ARCHIVE), whole(ARCHIVE)],
        );

        assert_eq!(fetched.expect("a verified archive"), ARCHIVE);
        assert_eq!(ranges_of(&asked), [Some(format!("bytes={saved}-")), None]);
        assert!(!part_left);
    }

    #[test]
    fn a_damaged_whole_answer_still_fails_its_checksum_with_a_saved_part_or_without() {
        let (unsaved, asked_once, unsaved_left) = downloaded_over(None, vec![whole(DAMAGED)]);
        let (saved, asked_twice, saved_left) = downloaded_over(
            Some(vec![b'x'; SAVED]),
            vec![whole(DAMAGED), whole(DAMAGED)],
        );

        assert!(
            matches!(unsaved, Err(Error::Checksum { .. })),
            "{unsaved:?}"
        );
        assert!(matches!(saved, Err(Error::Checksum { .. })), "{saved:?}");
        assert_eq!(ranges_of(&asked_once), [None]);
        assert_eq!(
            ranges_of(&asked_twice),
            [Some(format!("bytes={SAVED}-")), None]
        );
        assert!(!unsaved_left && !saved_left);
    }

    fn served_from_half_way(listener: &std::net::TcpListener) {
        let (mut stream, _) = listener.accept().expect("a fetch");
        let mut asked = Vec::new();
        let mut byte = [0_u8; 1];

        while !asked.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).expect("the request") == 0 {
                break;
            }
            asked.push(byte[0]);
        }

        let from = ARCHIVE.len() / 2;
        let rest = &ARCHIVE[from..];
        let total = ARCHIVE.len();

        let mut answer = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            total - 1,
            rest.len()
        )
        .into_bytes();

        answer.extend_from_slice(rest);
        stream.write_all(&answer).expect("the answer");
    }

    #[test]
    fn the_whole_size_is_the_total_else_the_length_to_come_and_what_was_held() {
        assert_eq!(whole_size(Some(100), Some(40), 60), 100);
        assert_eq!(whole_size(None, Some(40), 60), 100);
        assert_eq!(whole_size(None, None, 60), 0);
    }

    #[test]
    fn a_resumed_download_reads_up_to_the_size_of_the_whole_and_no_further() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!(
            "http://{}/{CORE_ARCHIVE}",
            listener.local_addr().expect("bound")
        );
        let serving = std::thread::spawn(move || served_from_half_way(&listener));
        let into = a_folder("resumed-size");
        let partial = named(&into.join(CORE_ARCHIVE), PARTIAL);
        let building = Building::default();
        let held = ARCHIVE.len() / 2;

        fs::write(&partial, &ARCHIVE[..held]).expect("half a part");

        let reached = copied(
            &plain_agent(),
            &url,
            &partial,
            held as u64,
            &Gate::open(),
            &building,
        )
        .expect("the rest is fetched");
        let (read, of) = building.bytes();
        let kept = fs::read(&partial).expect("the part");

        serving.join().expect("served");

        let _ = fs::remove_dir_all(&into);

        assert!(matches!(reached, Copied::Whole), "{reached:?}");
        assert_eq!(kept, ARCHIVE);
        assert_eq!(of, ARCHIVE.len() as u64);
        assert_eq!(read, of);
    }
}
