use std::{sync::Arc, time::SystemTime};

use resonate_online::{Release, Releases, Repository};

use crate::MobilePlayer;

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NewerRelease {
    pub version: String,
    pub page: String,
}

impl From<Release> for NewerRelease {
    fn from(release: Release) -> Self {
        Self {
            version: release.version.to_string(),
            page: release.page,
        }
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn names_where_it_was_released(&self) -> bool {
        Repository::of_this_build().is_some()
    }

    pub fn newer_release(&self) -> Option<NewerRelease> {
        self.newer_release_of(Repository::of_this_build()?, SystemTime::now())
    }
}

impl MobilePlayer {
    fn newer_release_of(&self, repository: Repository, now: SystemTime) -> Option<NewerRelease> {
        let client = self.services.online.client();
        let releases = Releases::of(Arc::clone(client), repository);
        self.releases
            .newer_lately(now, client.is_reaching(), || releases.latest())
            .map(NewerRelease::from)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use resonate_online::{KeptRelease, Version};

    use super::*;
    use crate::playlists::tests::player;

    const A_RELEASE_PAGE: &str = "https://github.com/someone/resonate/releases/tag/v99.0.0";

    fn released(version: &str) -> Release {
        Release {
            version: Version::read(version).expect("a version"),
            page: A_RELEASE_PAGE.to_owned(),
        }
    }

    fn repository() -> Repository {
        Repository::named("someone/resonate").expect("a repository")
    }

    fn kept_in_a_scratch_folder(test: &str) -> KeptRelease {
        let folder =
            std::env::temp_dir().join(format!("resonate-mobile-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        KeptRelease::in_folder(&folder)
    }

    #[test]
    fn a_phone_offline_is_told_what_was_heard_and_asks_nothing() {
        let mut player = player();
        player.releases = kept_in_a_scratch_folder("offline");
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);

        let unheard = player.newer_release_of(repository(), now);
        player.releases.heard_now(
            Some(released("99.0.0")),
            now - Duration::from_secs(3 * 24 * 60 * 60),
        );
        let heard = player.newer_release_of(repository(), now);

        assert!(!player.services.online.client().is_reaching());
        assert_eq!(unheard, None);
        assert_eq!(
            heard,
            Some(NewerRelease {
                version: "99.0.0".to_owned(),
                page: A_RELEASE_PAGE.to_owned(),
            })
        );
    }

    #[test]
    fn a_release_no_newer_than_this_build_is_not_told() {
        let mut player = player();
        player.releases = kept_in_a_scratch_folder("older");
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);

        player.releases.heard_now(Some(released("0.0.1")), now);

        assert_eq!(player.newer_release_of(repository(), now), None);
    }
}
