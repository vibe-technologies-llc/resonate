use std::{
    fmt,
    fs::{self, File},
    io::Write as _,
    path::{Path, PathBuf},
};

use resonate_core::{file_facts, writer::Held};

use crate::{Error, JarOp, Result};

const HEADER: &str = "# Netscape HTTP Cookie File\n";

const HTTP_ONLY_PREFIX: &str = "#HttpOnly_";

const KEPT_UNDER: [&str; 2] = ["youtube.com", "google.com"];

const SIGNED_IN_UNDER: &str = "youtube.com";

const SIGNED_IN_WITH: [&str; 2] = ["LOGIN_INFO", "SAPISID"];

const BREAKS_A_LINE: [char; 3] = ['\t', '\r', '\n'];

const STAGED_SUFFIX: &str = ".staged";

const SESSION_EXPIRES: u64 = 0;

#[cfg(unix)]
const OWNER_ONLY: u32 = 0o600;

#[derive(Clone, PartialEq, Eq)]
pub struct Cookie {
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires: Option<u64>,
    pub name: String,
    pub value: String,
}

impl Cookie {
    fn host(&self) -> &str {
        self.domain.strip_prefix('.').unwrap_or(&self.domain)
    }

    fn is_under(&self, domain: &str) -> bool {
        let host = self.host();
        host.eq_ignore_ascii_case(domain)
            || host.len().checked_sub(domain.len() + 1).is_some_and(|dot| {
                host.as_bytes()[dot] == b'.' && host[dot + 1..].eq_ignore_ascii_case(domain)
            })
    }

    fn holds_its_line(&self) -> bool {
        [&self.domain, &self.path, &self.name, &self.value]
            .iter()
            .all(|field| !field.contains(BREAKS_A_LINE))
    }

    fn line(&self) -> String {
        let prefix = if self.http_only { HTTP_ONLY_PREFIX } else { "" };
        format!(
            "{prefix}{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            self.domain,
            said(self.domain.starts_with('.')),
            self.path,
            said(self.secure),
            self.expires.unwrap_or(SESSION_EXPIRES),
            self.name,
            self.value,
        )
    }
}

impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cookie")
            .field("domain", &self.domain)
            .field("path", &self.path)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("expires", &self.expires)
            .field("name", &self.name)
            .field("value", &"<withheld>")
            .finish()
    }
}

const fn said(yes: bool) -> &'static str {
    if yes { "TRUE" } else { "FALSE" }
}

pub fn kept(cookies: Vec<Cookie>) -> Vec<Cookie> {
    cookies
        .into_iter()
        .filter(|cookie| KEPT_UNDER.iter().any(|domain| cookie.is_under(domain)))
        .filter(Cookie::holds_its_line)
        .collect()
}

pub fn signed_in(cookies: &[Cookie]) -> bool {
    SIGNED_IN_WITH.iter().all(|name| {
        cookies.iter().any(|cookie| {
            cookie.name == *name && cookie.host().eq_ignore_ascii_case(SIGNED_IN_UNDER)
        })
    })
}

pub fn netscape(cookies: &[Cookie]) -> String {
    let mut text = HEADER.to_owned();
    for cookie in cookies {
        text.push_str(&cookie.line());
    }
    text
}

pub fn written(jar: &Path, cookies: &[Cookie]) -> Result<()> {
    let folder = jar.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(folder).map_err(|source| Error::Jar {
        op: JarOp::MakeFolder,
        source,
    })?;
    let staged = staged_beside(jar);
    let held = Held::taken_over(&staged).map_err(|source| Error::Jar {
        op: JarOp::Stage,
        source,
    })?;
    let filled = fill(held.file(), cookies).and_then(|()| {
        fs::rename(&staged, jar).map_err(|source| Error::Jar {
            op: JarOp::PutInPlace,
            source,
        })
    });
    if filled.is_err() {
        let _ = fs::remove_file(&staged);
        return filled;
    }
    drop(held);
    file_facts::sync_directory(folder).map_err(|source| Error::Jar {
        op: JarOp::Sync,
        source,
    })
}

fn staged_beside(jar: &Path) -> PathBuf {
    let mut name = jar.as_os_str().to_owned();
    name.push(STAGED_SUFFIX);
    PathBuf::from(name)
}

fn fill(mut file: &File, cookies: &[Cookie]) -> Result<()> {
    narrow(file)?;
    file.write_all(netscape(cookies).as_bytes())
        .map_err(|source| Error::Jar {
            op: JarOp::Write,
            source,
        })?;
    file.sync_all().map_err(|source| Error::Jar {
        op: JarOp::Sync,
        source,
    })
}

#[cfg(unix)]
fn narrow(file: &File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    file.set_permissions(fs::Permissions::from_mode(OWNER_ONLY))
        .map_err(|source| Error::Jar {
            op: JarOp::Narrow,
            source,
        })
}

#[cfg(not(unix))]
const fn narrow(_file: &File) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs, io, path::PathBuf, process};

    use resonate_core::writer::Held;

    use super::*;
    use crate::{Error, JarOp};

    fn scratch(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("resonate-signin-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    fn cookie(domain: &str, name: &str, value: &str) -> Cookie {
        Cookie {
            domain: domain.to_owned(),
            path: "/".to_owned(),
            secure: true,
            http_only: false,
            expires: Some(1_900_000_000),
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn a_jar_reads_as_netscape_with_http_only_cookies_marked() {
        let held = Cookie {
            http_only: true,
            ..cookie(".youtube.com", "LOGIN_INFO", "abc")
        };
        let session = Cookie {
            secure: false,
            expires: None,
            path: "/watch".to_owned(),
            ..cookie("music.youtube.com", "PREF", "f6=8")
        };

        assert_eq!(
            netscape(&[held, session]),
            "# Netscape HTTP Cookie File\n\
             #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1900000000\tLOGIN_INFO\tabc\n\
             music.youtube.com\tFALSE\t/watch\tFALSE\t0\tPREF\tf6=8\n"
        );
    }

    #[test]
    fn only_youtube_and_google_cookies_are_kept_and_a_lookalike_domain_is_not() {
        let named: Vec<String> = kept(vec![
            cookie(".youtube.com", "SAPISID", "a"),
            cookie("accounts.google.com", "SID", "b"),
            cookie("youtube.com", "YSC", "c"),
            cookie(".doubleclick.net", "IDE", "d"),
            cookie("evil-youtube.com", "SAPISID", "e"),
            cookie("youtube.com.evil.net", "SAPISID", "f"),
        ])
        .into_iter()
        .map(|cookie| cookie.name)
        .collect();

        assert_eq!(named, ["SAPISID", "SID", "YSC"]);
    }

    #[test]
    fn a_cookie_that_would_break_its_line_is_left_out() {
        let left = kept(vec![
            cookie(".youtube.com", "SAPISID", "a\tb"),
            cookie(".youtube.com", "SAPISID", "a\nb"),
            cookie(".youtube.com", "SAP\rISID", "a"),
            Cookie {
                path: "/\n.evil.com\tTRUE".to_owned(),
                ..cookie(".youtube.com", "SID", "a")
            },
        ]);

        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn google_cookies_alone_are_not_a_youtube_sign_in() {
        assert!(!signed_in(&[
            cookie(".google.com", "SAPISID", "a"),
            cookie(".google.com", "LOGIN_INFO", "b"),
        ]));
        assert!(!signed_in(&[cookie(".youtube.com", "SAPISID", "a")]));
        assert!(signed_in(&[
            cookie(".youtube.com", "LOGIN_INFO", "b"),
            cookie("youtube.com", "SAPISID", "a"),
        ]));
    }

    #[test]
    fn a_jar_is_written_whole_owner_only_and_into_a_folder_made_for_it() {
        let folder = scratch("written");
        let jar = folder.join("not yet").join("youtube-music-cookies.txt");
        let cookies = [cookie(".youtube.com", "SAPISID", "a")];

        written(&jar, &cookies).expect("a jar written");

        assert_eq!(
            fs::read_to_string(&jar).expect("the jar"),
            netscape(&cookies)
        );
        let left: Vec<_> = fs::read_dir(jar.parent().expect("a folder"))
            .expect("a listing")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(left, ["youtube-music-cookies.txt"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&jar).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_jar_another_sign_in_is_writing_is_not_written_over() {
        let folder = scratch("writing");
        let jar = folder.join("youtube-music-cookies.txt");
        fs::write(&jar, "kept").expect("a jar");
        let _writing = Held::taken_over(&folder.join("youtube-music-cookies.txt.staged"))
            .expect("another writer");

        let refused = written(&jar, &[cookie(".youtube.com", "SAPISID", "a")]);

        assert!(
            matches!(
                &refused,
                Err(Error::Jar { op: JarOp::Stage, source }) if source.kind() == io::ErrorKind::WouldBlock
            ),
            "{refused:?}"
        );
        assert_eq!(fs::read_to_string(&jar).expect("the jar"), "kept");
    }

    #[test]
    fn a_cookies_value_never_reaches_debug() {
        let shown = format!("{:?}", cookie(".youtube.com", "SAPISID", "very-secret"));

        assert!(!shown.contains("very-secret"), "{shown}");
        assert!(shown.contains("SAPISID"), "{shown}");
    }
}
