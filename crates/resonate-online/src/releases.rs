use std::{cmp::Ordering, fmt, sync::Arc};

use resonate_library::LookupOp;
use serde::Deserialize;

use crate::{Client, Host, Result};

const RELEASE_PAGES: &str = "https://github.com/";

const LARGEST_RELEASE: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Repository {
    owner: String,
    name: String,
}

impl Repository {
    pub fn named(spelt: &str) -> Option<Self> {
        let (owner, name) = spelt.trim().split_once('/')?;
        let reads = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|letter| letter.is_ascii_alphanumeric() || "-_.".contains(letter))
                && part != "."
                && part != ".."
        };
        (reads(owner) && reads(name)).then(|| Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }

    fn latest_path(&self) -> String {
        format!("/repos/{}/{}/releases/latest", self.owner, self.name)
    }

    fn page_of(&self, tag: &str) -> String {
        format!(
            "{RELEASE_PAGES}{}/{}/releases/tag/{tag}",
            self.owner, self.name
        )
    }
}

impl fmt::Display for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Identifier {
    Numeric(u64),
    Named(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Standing {
    Candidate(Vec<Identifier>),
    Released,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    standing: Standing,
}

impl Version {
    pub fn of_this_build() -> Option<Self> {
        Self::read(env!("CARGO_PKG_VERSION"))
    }

    pub fn read(spelt: &str) -> Option<Self> {
        let spelt = spelt.trim();
        let spelt = spelt
            .strip_prefix('v')
            .or_else(|| spelt.strip_prefix('V'))
            .unwrap_or(spelt);
        let spelt = spelt.split_once('+').map_or(spelt, |(version, _)| version);
        let (numbers, ahead) = match spelt.split_once('-') {
            Some((numbers, ahead)) => (numbers, Some(ahead)),
            None => (spelt, None),
        };

        let mut parts = numbers.split('.');
        let number = |part: Option<&str>| -> Option<u64> {
            let part = part?;
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        };
        let major = number(parts.next())?;
        let minor = number(parts.next())?;
        let patch = number(parts.next())?;
        if parts.next().is_some() {
            return None;
        }

        let standing = match ahead {
            None => Standing::Released,
            Some(ahead) => Standing::Candidate(
                ahead
                    .split('.')
                    .map(identifier)
                    .collect::<Option<Vec<_>>>()?,
            ),
        };

        Some(Self {
            major,
            minor,
            patch,
            standing,
        })
    }
}

fn identifier(part: &str) -> Option<Identifier> {
    if part.is_empty()
        || !part
            .chars()
            .all(|letter| letter.is_ascii_alphanumeric() || letter == '-')
    {
        return None;
    }
    if part.bytes().all(|byte| byte.is_ascii_digit()) {
        return part.parse().ok().map(Identifier::Numeric);
    }
    Some(Identifier::Named(part.to_owned()))
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| self.standing.cmp(&other.standing))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Standing::Candidate(identifiers) = &self.standing {
            for (at, identifier) in identifiers.iter().enumerate() {
                f.write_str(if at == 0 { "-" } else { "." })?;
                match identifier {
                    Identifier::Numeric(number) => write!(f, "{number}")?,
                    Identifier::Named(name) => f.write_str(name)?,
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub page: String,
}

#[derive(Deserialize)]
struct ReleaseDoc {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

impl ReleaseDoc {
    fn into_release(self, repository: &Repository) -> Option<Release> {
        if self.draft || self.prerelease {
            return None;
        }
        let version = Version::read(&self.tag_name)?;
        let page = self
            .html_url
            .filter(|page| page.starts_with(RELEASE_PAGES))
            .unwrap_or_else(|| repository.page_of(&self.tag_name));
        Some(Release { version, page })
    }
}

pub struct Releases {
    client: Arc<Client>,
    repository: Repository,
    base: String,
}

impl Releases {
    pub fn of(client: Arc<Client>, repository: Repository) -> Self {
        Self {
            client,
            repository,
            base: Host::GitHub.base().to_owned(),
        }
    }

    #[cfg(test)]
    fn answering_at(client: Arc<Client>, repository: Repository, base: String) -> Self {
        Self {
            client,
            repository,
            base,
        }
    }

    pub fn latest(&self) -> Result<Option<Release>> {
        let url = format!("{}{}", self.base, self.repository.latest_path());
        let op = LookupOp::LatestRelease;
        let Some(bytes) = self.client.bytes(Host::GitHub, op, &url, LARGEST_RELEASE)? else {
            return Ok(None);
        };
        let read: ReleaseDoc = serde_json::from_slice(&bytes).map_err(|error| {
            tracing::debug!(%error, url, "the latest release was not the document expected");
            crate::Error::Unreadable {
                host: Host::GitHub,
                op,
            }
        })?;
        Ok(read.into_release(&self.repository))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use super::*;
    use crate::{
        Identity,
        client::{Carried, Introduction, WallClock},
    };

    const LATEST: &str = include_str!("../tests/fixtures/github_latest_release.json");

    fn version(spelt: &str) -> Version {
        Version::read(spelt).expect("a version")
    }

    #[test]
    fn versions_order_as_semantic_versioning_orders_them() {
        let rising = [
            "0.1.0",
            "0.7.0",
            "0.8.0-alpha",
            "0.8.0-alpha.1",
            "0.8.0-rc.1",
            "0.8.0-rc1",
            "0.8.0-rc2",
            "0.8.0",
            "0.8.1",
            "0.10.0",
            "1.0.0",
        ];

        for pair in rising.windows(2) {
            assert!(version(pair[0]) < version(pair[1]), "{pair:?}");
        }
        assert_eq!(version("v0.8.0"), version("0.8.0"));
        assert_eq!(version("0.8.0+build.7"), version("0.8.0"));
        assert_eq!(version("v0.8.0-rc1").to_string(), "0.8.0-rc1");
    }

    #[test]
    fn what_is_no_version_reads_as_none() {
        for spelt in [
            "",
            "0.8",
            "0.8.0.1",
            "latest",
            "0.x.0",
            "0.8.0-",
            "0.8.0-rc..1",
            "-1.0.0",
        ] {
            assert_eq!(Version::read(spelt), None, "{spelt:?}");
        }
    }

    #[test]
    fn this_build_reads_as_a_version() {
        assert!(Version::of_this_build().is_some());
    }

    #[test]
    fn a_repository_is_named_owner_and_name_and_nothing_else() {
        assert!(Repository::named("someone/resonate").is_some());
        for spelt in [
            "",
            "someone",
            "someone/",
            "/resonate",
            "a/b/c",
            "some one/resonate",
            "../x",
        ] {
            assert_eq!(Repository::named(spelt), None, "{spelt:?}");
        }
    }

    #[test]
    fn a_captured_release_names_its_version_and_its_page() {
        let repository = Repository::named("someone/resonate").expect("a repository");
        let doc: ReleaseDoc = serde_json::from_str(LATEST).expect("the captured document");

        let release = doc.into_release(&repository).expect("a release");

        assert_eq!(release.version, version("0.8.0"));
        assert_eq!(
            release.page,
            "https://github.com/someone/resonate/releases/tag/v0.8.0"
        );
    }

    #[test]
    fn a_page_off_github_is_not_offered_and_a_draft_or_candidate_is_no_release() {
        let repository = Repository::named("someone/resonate").expect("a repository");
        let elsewhere = ReleaseDoc {
            tag_name: "v0.9.0".to_owned(),
            html_url: Some("https://example.com/v0.9.0".to_owned()),
            draft: false,
            prerelease: false,
        };
        let draft = ReleaseDoc {
            tag_name: "v0.9.0".to_owned(),
            html_url: None,
            draft: true,
            prerelease: false,
        };
        let candidate = ReleaseDoc {
            tag_name: "v0.9.0-rc1".to_owned(),
            html_url: None,
            draft: false,
            prerelease: true,
        };

        assert_eq!(
            elsewhere
                .into_release(&repository)
                .map(|release| release.page),
            Some("https://github.com/someone/resonate/releases/tag/v0.9.0".to_owned())
        );
        assert_eq!(draft.into_release(&repository), None);
        assert_eq!(candidate.into_release(&repository), None);
    }

    fn serving(status: &'static str, body: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let served = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a connection");
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).expect("a request") == 1 {
                head.push(byte[0]);
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            String::from_utf8(head).expect("a head in ASCII")
        });
        (base, served)
    }

    fn over_loopback(base: String) -> Releases {
        let client = Client::on_clock(
            Introduction::as_(&Identity::of_this_build()),
            Arc::new(WallClock),
            Carried::Plain,
        );
        Releases::answering_at(
            Arc::new(client),
            Repository::named("someone/resonate").expect("a repository"),
            base,
        )
    }

    #[test]
    fn the_latest_release_is_asked_of_the_repository_saying_only_what_this_build_is() {
        let (base, served) = serving("200 OK", LATEST);

        let release = over_loopback(base).latest().expect("an answer");
        let head = served.join().expect("the server");

        assert_eq!(
            release.map(|release| release.version),
            Some(version("0.8.0"))
        );
        assert!(
            head.starts_with("GET /repos/someone/resonate/releases/latest "),
            "{head}"
        );
        let user_agent = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("user-agent")
                    .then(|| value.trim().to_owned())
            })
            .expect("a user agent");
        assert_eq!(
            user_agent,
            format!("resonate/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn a_repository_with_no_release_answers_none_and_a_page_that_is_no_release_is_unreadable() {
        let (base, served) = serving("404 Not Found", "");
        assert_eq!(over_loopback(base).latest().expect("an answer"), None);
        served.join().expect("the server");

        let (base, served) = serving("200 OK", "<html>sign in</html>");
        assert!(matches!(
            over_loopback(base).latest(),
            Err(crate::Error::Unreadable {
                host: Host::GitHub,
                op: LookupOp::LatestRelease
            })
        ));
        served.join().expect("the server");
    }
}
