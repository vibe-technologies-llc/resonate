#[cfg(feature = "ui")]
use std::sync::Arc;
#[cfg(any(feature = "ui", test))]
use std::time::Duration;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    Error, Result,
    config::{self, Config},
    online::{self, Asking, Released},
};

const KEPT_IN: &str = "latest-release";

#[cfg(any(feature = "ui", test))]
const ASKED_AGAIN_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

const THIS_BUILD: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Eq)]
struct Heard {
    checked: SystemTime,
    latest: Option<Released>,
}

impl Heard {
    fn written(&self) -> String {
        let seconds = self
            .checked
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let mut text = format!("checked {seconds}\n");
        if let Some(latest) = &self.latest {
            text.push_str(&format!(
                "latest {}\npage {}\n",
                latest.version, latest.page
            ));
        }
        text
    }

    #[cfg(any(feature = "ui", test))]
    fn read(text: &str) -> Option<Self> {
        let mut checked = None;
        let mut version = None;
        let mut page = None;
        for line in text.lines() {
            match line.split_once(' ') {
                Some(("checked", seconds)) => {
                    checked = Some(UNIX_EPOCH + Duration::from_secs(seconds.trim().parse().ok()?));
                }
                Some(("latest", spelt)) => version = Some(spelt.trim().to_owned()),
                Some(("page", spelt)) => page = Some(spelt.trim().to_owned()),
                _ => {}
            }
        }
        let latest = match (version, page) {
            (Some(version), Some(page)) => Some(Released { version, page }),
            (None, None) => None,
            _ => return None,
        };
        Some(Self {
            checked: checked?,
            latest,
        })
    }

    #[cfg(any(feature = "ui", test))]
    fn is_due(&self, now: SystemTime) -> bool {
        now.duration_since(self.checked)
            .is_ok_and(|age| age >= ASKED_AGAIN_AFTER)
            || self.checked > now
    }
}

fn kept_at() -> Result<PathBuf> {
    Ok(config::data_dir()?.join(KEPT_IN))
}

#[cfg(any(feature = "ui", test))]
fn kept(path: &Path) -> Option<Heard> {
    match fs::read_to_string(path) {
        Ok(text) => Heard::read(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            tracing::debug!(%error, path = %path.display(), "what was last heard of the releases went unread");
            None
        }
    }
}

fn keep(path: &Path, heard: &Heard) -> Result<()> {
    let kept = |source| Error::KeepLatestRelease {
        path: path.to_path_buf(),
        source,
    };
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(kept)?;
    }
    fs::write(path, heard.written()).map_err(kept)
}

#[cfg(any(feature = "ui", test))]
fn newer(latest: Option<Released>) -> Option<Released> {
    latest.filter(|released| online::outdates_this_build(&released.version))
}

#[cfg(any(feature = "ui", test))]
fn heard_lately(
    path: &Path,
    now: SystemTime,
    reaching: bool,
    ask: impl FnOnce() -> Result<Option<Released>>,
) -> Option<Released> {
    let held = kept(path);
    let fresh = held.as_ref().is_some_and(|held| !held.is_due(now));
    if fresh || !reaching {
        return newer(held.and_then(|held| held.latest));
    }

    match ask() {
        Ok(latest) => {
            let heard = Heard {
                checked: now,
                latest,
            };
            if let Err(error) = keep(path, &heard) {
                tracing::warn!(%error, "the latest release was heard but not kept, so it is asked again");
            }
            newer(heard.latest)
        }
        Err(error) => {
            tracing::debug!(%error, "the latest release went unasked; what was heard before stands");
            newer(held.and_then(|held| held.latest))
        }
    }
}

pub fn run(config: &Config) -> Result<()> {
    let latest = online::latest_release(config, Asking::ForTheListener)?;
    if let Ok(path) = kept_at() {
        let heard = Heard {
            checked: SystemTime::now(),
            latest: latest.clone(),
        };
        if let Err(error) = keep(&path, &heard) {
            tracing::warn!(%error, "the latest release was heard but not kept");
        }
    }

    said!("this build: {THIS_BUILD}");
    match latest {
        None => said!("no release is published yet"),
        Some(latest) if online::outdates_this_build(&latest.version) => {
            said!("newer: {}", latest.version);
            said!("{}", latest.page);
        }
        Some(latest) => said!("latest: {}, which this build is not behind", latest.version),
    }
    Ok(())
}

#[cfg(feature = "ui")]
struct Checking {
    config: Config,
}

#[cfg(feature = "ui")]
impl resonate_ui::Updates for Checking {
    fn newer(&self, reaching: bool) -> Option<resonate_ui::Newer> {
        let path = kept_at()
            .map_err(
                |error| tracing::debug!(%error, "there is no folder to keep the latest release in"),
            )
            .ok()?;
        heard_lately(&path, SystemTime::now(), reaching, || {
            online::latest_release(&self.config, Asking::InTheBackground)
        })
        .map(|released| resonate_ui::Newer {
            version: released.version,
            page: released.page,
        })
    }
}

#[cfg(feature = "ui")]
pub fn checking(config: &Config) -> Option<Arc<dyn resonate_ui::Updates>> {
    online::names_where_it_was_released().then(|| {
        Arc::new(Checking {
            config: Config {
                online: Some(true),
                ..config.clone()
            },
        }) as Arc<dyn resonate_ui::Updates>
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const A_DAY: Duration = ASKED_AGAIN_AFTER;

    fn released(version: &str) -> Released {
        Released {
            version: version.to_owned(),
            page: format!("https://github.com/someone/resonate/releases/tag/v{version}"),
        }
    }

    fn kept_in_a_scratch_folder() -> PathBuf {
        crate::fixtures::scratch("latest-release").join(KEPT_IN)
    }

    #[test]
    fn what_was_heard_reads_back_as_it_was_written() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        for heard in [
            Heard {
                checked: now,
                latest: Some(released("99.0.0")),
            },
            Heard {
                checked: now,
                latest: None,
            },
        ] {
            assert_eq!(Heard::read(&heard.written()), Some(heard));
        }
        assert_eq!(Heard::read("latest 1.0.0\n"), None);
        assert_eq!(Heard::read("checked soon\n"), None);
        assert_eq!(Heard::read("checked 5\nlatest 1.0.0\n"), None);
    }

    #[test]
    fn the_releases_are_asked_at_most_once_a_day() {
        let path = kept_in_a_scratch_folder();
        let asked = Cell::new(0);
        let ask = || {
            asked.set(asked.get() + 1);
            Ok(Some(released("99.0.0")))
        };
        let morning = UNIX_EPOCH + Duration::from_secs(1_800_000_000);

        let first = heard_lately(&path, morning, true, ask);
        let later = heard_lately(&path, morning + A_DAY / 2, true, ask);
        let tomorrow = heard_lately(&path, morning + A_DAY, true, ask);

        assert_eq!(first, Some(released("99.0.0")));
        assert_eq!(later, Some(released("99.0.0")));
        assert_eq!(tomorrow, Some(released("99.0.0")));
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn nothing_is_asked_while_offline_and_a_failed_ask_keeps_what_was_heard() {
        let path = kept_in_a_scratch_folder();
        let morning = UNIX_EPOCH + Duration::from_secs(1_800_000_000);

        let offline = heard_lately(&path, morning, false, || panic!("asked while offline"));
        assert_eq!(offline, None);

        heard_lately(&path, morning, true, || Ok(Some(released("99.0.0"))));
        let failed = heard_lately(&path, morning + A_DAY, true, || Err(Error::OnlineOff));

        assert_eq!(failed, Some(released("99.0.0")));
        assert_eq!(
            kept(&path).map(|heard| heard.checked),
            Some(morning),
            "a failed ask was kept as though it had answered"
        );
    }

    #[test]
    fn only_a_release_newer_than_this_build_is_told() {
        let path = kept_in_a_scratch_folder();
        let morning = UNIX_EPOCH + Duration::from_secs(1_800_000_000);

        let same = heard_lately(&path, morning, true, || Ok(Some(released(THIS_BUILD))));
        let older = heard_lately(&path, morning + A_DAY, true, || Ok(Some(released("0.0.1"))));
        let none = heard_lately(&path, morning + A_DAY * 2, true, || Ok(None));

        assert_eq!(same, None);
        assert_eq!(older, None);
        assert_eq!(none, None);
    }

    #[test]
    fn a_clock_set_back_past_the_last_ask_asks_again() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let heard = Heard {
            checked: now + A_DAY,
            latest: None,
        };

        assert!(heard.is_due(now));
        assert!(
            !Heard {
                checked: now,
                latest: None
            }
            .is_due(now + Duration::from_secs(60))
        );
    }
}
