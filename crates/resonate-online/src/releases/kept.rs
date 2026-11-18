use std::{
    fmt, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{Release, Version};

const KEPT_IN: &str = "latest-release";
const CHECKED_AT: &str = "checked";
const LATEST_VERSION: &str = "latest";
const LATEST_PAGE: &str = "page";

const ASKED_AGAIN_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Heard {
    checked: SystemTime,
    latest: Option<Release>,
}

impl Heard {
    fn written(&self) -> String {
        let seconds = self
            .checked
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let mut text = format!("{CHECKED_AT} {seconds}\n");
        if let Some(latest) = &self.latest {
            text.push_str(&format!(
                "{LATEST_VERSION} {}\n{LATEST_PAGE} {}\n",
                latest.version, latest.page
            ));
        }
        text
    }

    fn read(text: &str) -> Option<Self> {
        let mut checked = None;
        let mut version = None;
        let mut page = None;
        for line in text.lines() {
            match line.split_once(' ') {
                Some((CHECKED_AT, seconds)) => {
                    checked = Some(UNIX_EPOCH + Duration::from_secs(seconds.trim().parse().ok()?));
                }
                Some((LATEST_VERSION, spelt)) => version = Some(Version::read(spelt)?),
                Some((LATEST_PAGE, spelt)) => page = Some(spelt.trim().to_owned()),
                _ => {}
            }
        }
        let latest = match (version, page) {
            (Some(version), Some(page)) => Some(Release { version, page }),
            (None, None) => None,
            _ => return None,
        };
        Some(Self {
            checked: checked?,
            latest,
        })
    }

    fn is_due(&self, now: SystemTime) -> bool {
        now.duration_since(self.checked)
            .is_ok_and(|age| age >= ASKED_AGAIN_AFTER)
            || self.checked > now
    }

    fn newer(self) -> Option<Release> {
        self.latest
            .filter(|released| released.version.outdates_this_build())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptRelease {
    path: PathBuf,
}

impl KeptRelease {
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            path: folder.join(KEPT_IN),
        }
    }

    pub fn heard_now(&self, latest: Option<Release>, now: SystemTime) {
        self.keep(&Heard {
            checked: now,
            latest,
        });
    }

    pub fn newer_lately<E: fmt::Display>(
        &self,
        now: SystemTime,
        reaching: bool,
        ask: impl FnOnce() -> Result<Option<Release>, E>,
    ) -> Option<Release> {
        let held = self.heard();
        let fresh = held.as_ref().is_some_and(|held| !held.is_due(now));
        if fresh || !reaching {
            return held.and_then(Heard::newer);
        }

        match ask() {
            Ok(latest) => {
                let heard = Heard {
                    checked: now,
                    latest,
                };
                self.keep(&heard);
                heard.newer()
            }
            Err(error) => {
                tracing::debug!(%error, "the latest release went unasked; what was heard before stands");
                held.and_then(Heard::newer)
            }
        }
    }

    fn heard(&self) -> Option<Heard> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Heard::read(&text),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => {
                tracing::debug!(%error, path = %self.path.display(), "what was last heard of the releases went unread");
                None
            }
        }
    }

    fn keep(&self, heard: &Heard) {
        let kept = self
            .path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&self.path, heard.written()));
        if let Err(error) = kept {
            tracing::warn!(%error, path = %self.path.display(), "the latest release was heard but not kept, so it is asked again");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::tests::{over_loopback, serving},
        *,
    };
    use crate::{Error, Host};

    const A_DAY: Duration = ASKED_AGAIN_AFTER;

    const NEWER: &str = r#"{"tag_name":"v99.0.0","html_url":"https://github.com/someone/resonate/releases/tag/v99.0.0","draft":false,"prerelease":false}"#;

    fn released(version: &str) -> Release {
        Release {
            version: Version::read(version).expect("a version"),
            page: format!("https://github.com/someone/resonate/releases/tag/v{version}"),
        }
    }

    fn kept_in_a_scratch_folder() -> KeptRelease {
        let folder =
            std::env::temp_dir().join(format!("resonate-latest-release-{}", uuid::Uuid::new_v4()));
        KeptRelease::in_folder(&folder)
    }

    fn morning() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    fn unasked() -> Error {
        Error::Offline {
            host: Host::GitHub,
            op: resonate_library::LookupOp::LatestRelease,
        }
    }

    #[test]
    fn what_was_heard_reads_back_as_it_was_written() {
        for heard in [
            Heard {
                checked: morning(),
                latest: Some(released("99.0.0")),
            },
            Heard {
                checked: morning(),
                latest: Some(released("99.0.0-rc.1")),
            },
            Heard {
                checked: morning(),
                latest: None,
            },
        ] {
            assert_eq!(Heard::read(&heard.written()), Some(heard));
        }
        assert_eq!(Heard::read("latest 1.0.0\n"), None);
        assert_eq!(Heard::read("checked soon\n"), None);
        assert_eq!(Heard::read("checked 5\nlatest 1.0.0\n"), None);
        assert_eq!(Heard::read("checked 5\nlatest soon\npage x\n"), None);
    }

    #[test]
    fn the_releases_are_asked_at_most_once_a_day() {
        let kept = kept_in_a_scratch_folder();

        let (base, served) = serving("200 OK", NEWER);
        let releases = over_loopback(base);
        let first = kept.newer_lately(morning(), true, || releases.latest());
        served.join().expect("the server");

        let later = kept.newer_lately(morning() + A_DAY / 2, true, || -> Result<_, Error> {
            panic!("asked again within the day")
        });

        let (base, served) = serving("200 OK", NEWER);
        let releases = over_loopback(base);
        let tomorrow = kept.newer_lately(morning() + A_DAY, true, || releases.latest());
        served.join().expect("the server asked again the next day");

        assert_eq!(first, Some(released("99.0.0")));
        assert_eq!(later, Some(released("99.0.0")));
        assert_eq!(tomorrow, Some(released("99.0.0")));
    }

    #[test]
    fn nothing_is_asked_while_offline_and_a_failed_ask_keeps_what_was_heard() {
        let kept = kept_in_a_scratch_folder();

        let offline = kept.newer_lately(morning(), false, || -> Result<_, Error> {
            panic!("asked while offline")
        });
        assert_eq!(offline, None);

        kept.newer_lately(morning(), true, || Ok::<_, Error>(Some(released("99.0.0"))));
        let failed = kept.newer_lately(morning() + A_DAY, true, || Err(unasked()));

        assert_eq!(failed, Some(released("99.0.0")));
        assert_eq!(
            kept.heard().map(|heard| heard.checked),
            Some(morning()),
            "a failed ask was kept as though it had answered"
        );
    }

    #[test]
    fn what_resonate_update_heard_is_what_the_window_answers_from() {
        let kept = kept_in_a_scratch_folder();

        kept.heard_now(Some(released("99.0.0")), morning());
        let answered = kept.newer_lately(morning() + A_DAY / 2, true, || -> Result<_, Error> {
            panic!("asked again within the day")
        });

        assert_eq!(answered, Some(released("99.0.0")));
    }

    #[test]
    fn only_a_release_newer_than_this_build_is_told() {
        let kept = kept_in_a_scratch_folder();
        let this_build = env!("CARGO_PKG_VERSION");

        let same = kept.newer_lately(morning(), true, || {
            Ok::<_, Error>(Some(released(this_build)))
        });
        let older = kept.newer_lately(morning() + A_DAY, true, || {
            Ok::<_, Error>(Some(released("0.0.1")))
        });
        let none = kept.newer_lately(morning() + A_DAY * 2, true, || Ok::<_, Error>(None));

        assert_eq!(same, None);
        assert_eq!(older, None);
        assert_eq!(none, None);
    }

    #[test]
    fn a_clock_set_back_past_the_last_ask_asks_again() {
        let heard = Heard {
            checked: morning() + A_DAY,
            latest: None,
        };

        assert!(heard.is_due(morning()));
        assert!(
            !Heard {
                checked: morning(),
                latest: None
            }
            .is_due(morning() + Duration::from_secs(60))
        );
    }
}
