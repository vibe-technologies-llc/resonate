use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use resonate_core::{Gate, writer::Held};
use resonate_fetch::{Ranged, Trusted, Unfetched, configured, downloading_agent};
use sha2::{Digest, Sha256};
use ureq::Agent;

use crate::{ArchiveOp, Archives, Building, Error, Phase, Result};

pub const DUMP_SERVER: &str = "https://data.metabrainz.org/pub/musicbrainz/data/fullexport";
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
const GATE_LOOKED_AT_EVERY: Duration = Duration::from_secs(1);

pub fn agent() -> Agent {
    let config = configured(ANSWERED_WITHIN, Trusted::BuiltInRoots).build();
    downloading_agent(config, BROKEN_OFF_AFTER)
}

fn listed(agent: &Agent, url: &str) -> Result<String> {
    let response = agent.get(url).call().map_err(|error| Error::Fetch {
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
        held.push(downloaded(agent, dump, &into, file, &sum, gate, building)?);
    }
    let derived = held.pop().unwrap_or_default();
    let core = held.pop().unwrap_or_default();
    Ok(Archives {
        core,
        derived,
        dump: dump.to_owned(),
    })
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
    dump: &str,
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
    let url = format!("{DUMP_SERVER}/{dump}/{file}");
    loop {
        building.enter(Phase::Downloading);
        while gate.is_shut() {
            building.enter(Phase::Waiting);
            building.looked_at()?;
            gate.wait_while_shut(GATE_LOOKED_AT_EVERY);
        }
        building.enter(Phase::Downloading);
        let from = fs::metadata(&partial).map_or(0, |meta| meta.len());
        match copied(agent, &url, &partial, from, gate, building)? {
            Copied::Whole => break,
            Copied::Shut => continue,
        }
    }
    building.enter(Phase::Verifying);
    let found = summed(&partial, building)?;
    if found != sum {
        let _ = fs::remove_file(&partial);
        return Err(Error::Checksum {
            file: file.to_owned(),
        });
    }
    fs::rename(&partial, &whole).map_err(Error::archive(&partial, ArchiveOp::Rename))?;
    fs::write(&verified, sum).map_err(Error::archive(&verified, ArchiveOp::Stage))?;
    Ok(whole)
}

enum Copied {
    Whole,
    Shut,
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
    building.of(source.length().unwrap_or(0));
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
    fn a_dump_is_named_by_its_date_and_nothing_else() {
        assert!(is_a_dump_name("20261104-002222"));
        assert!(!is_a_dump_name("../etc"));
        assert!(!is_a_dump_name(""));
    }
}
