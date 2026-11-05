use std::time::{SystemTime, UNIX_EPOCH};

use resonate_library::{KEPT_IN_STEP_EVERY, LookupOp, PlaylistLink, ProfileLink, Reference as _};
use rusqlite::{Connection, params};

use crate::{Error, MobilePlayer, Result, StoreOp, collections::ImportedPlaylist};

#[derive(Clone, Debug, uniffi::Record)]
pub struct FollowedProfile {
    pub name: String,
    pub service: String,
    pub playlists: Vec<ImportedPlaylist>,
    pub let_go: u32,
    pub gone: u32,
    pub unread: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ProfileFollowed {
    pub link: String,
    pub name: String,
    pub service: String,
}

#[uniffi::export]
pub fn profile_link_service(text: String) -> Option<String> {
    ProfileLink::read(&text)
        .filter(|link| *link != ProfileLink::SpotifyAccount)
        .map(|link| link.service().to_owned())
}

#[uniffi::export]
impl MobilePlayer {
    pub fn follow_profile(&self, link: String, kept_in_step: bool) -> Result<FollowedProfile> {
        self.require_online()?;
        let link = ProfileLink::read(&link).ok_or(Error::NotAProfileLink)?;
        let profile = self
            .services
            .online
            .profile_linked(&link)
            .map_err(account_error)?
            .ok_or(match link {
                ProfileLink::SpotifyAccount => Error::NotSignedInToSpotify,
                ProfileLink::Deezer(_) => Error::NoProfileAtTheLink,
            })?;
        let known = pages_of(&self.catalog.0.lock(), &link)?;
        let mut followed = FollowedProfile {
            name: profile.name,
            service: link.service().to_owned(),
            playlists: Vec::new(),
            let_go: 0,
            gone: 0,
            unread: 0,
        };

        for listed in &profile.playlists {
            let page = listed.link.page();
            let kept_here = self.kept_from_page(&page)?;
            if kept_in_step && known.contains(&page) && kept_here.is_none() {
                followed.let_go += 1;
                continue;
            }
            let Some((_, songs)) = self.songs_of_a_playlist(&listed.link)? else {
                followed.unread += 1;
                continue;
            };
            match self.import_linked_playlist(page, listed.title.clone(), songs, kept_in_step) {
                Ok(imported) => {
                    if !kept_in_step {
                        self.forget_playlist_link(imported.id)?;
                    }
                    followed.playlists.push(imported);
                }
                Err(Error::NoLocalMatches) => followed.unread += 1,
                Err(error) => return Err(error),
            }
        }

        let listed: Vec<String> = profile
            .playlists
            .iter()
            .map(|listed| listed.link.page())
            .collect();
        for page in known.iter().filter(|page| !listed.contains(page)) {
            if let Some(playlist) = self.kept_from_page(page)? {
                self.forget_playlist_link(playlist)?;
                followed.gone += 1;
            }
        }

        let connection = self.catalog.0.lock();
        match kept_in_step {
            true => keep_following(&connection, &link, &followed.name, &listed)?,
            false => stop_following(&connection, &link)?,
        }
        Ok(followed)
    }

    pub fn profiles_followed(&self) -> Result<Vec<ProfileFollowed>> {
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare("SELECT link, name FROM profile_links ORDER BY name, link")
            .map_err(read_error)?;
        let rows = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;

        Ok(rows
            .into_iter()
            .filter_map(|(link, name)| {
                let service = ProfileLink::read(&link)?.service().to_owned();
                Some(ProfileFollowed {
                    link,
                    name,
                    service,
                })
            })
            .collect())
    }

    pub fn profiles_due_a_refresh(&self) -> Result<Vec<String>> {
        let due_before = seconds(SystemTime::now()) - KEPT_IN_STEP_EVERY.as_secs() as i64;
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare(
                "SELECT link FROM profile_links WHERE refreshed <= ?1 ORDER BY refreshed, link",
            )
            .map_err(read_error)?;
        query
            .query_map([due_before], |row| row.get::<_, String>(0))
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    pub fn unfollow_profile(&self, link: String) -> Result<()> {
        let link = ProfileLink::read(&link).ok_or(Error::NotAProfileLink)?;
        stop_following(&self.catalog.0.lock(), &link)
    }
}

impl MobilePlayer {
    fn kept_from_page(&self, page: &str) -> Result<Option<u64>> {
        let Some(link) = PlaylistLink::read(page) else {
            return Ok(None);
        };
        self.playlist_kept_from(&link)
    }
}

fn pages_of(connection: &Connection, link: &ProfileLink) -> Result<Vec<String>> {
    let mut query = connection
        .prepare("SELECT page FROM profile_pages WHERE profile = ?1 ORDER BY page")
        .map_err(read_error)?;
    query
        .query_map([link.page()], |row| row.get::<_, String>(0))
        .map_err(read_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(read_error)
}

fn keep_following(
    connection: &Connection,
    link: &ProfileLink,
    name: &str,
    pages: &[String],
) -> Result<()> {
    let profile = link.page();
    connection
        .execute(
            "INSERT INTO profile_links(link, name, refreshed) VALUES (?1, ?2, ?3)
             ON CONFLICT(link) DO UPDATE SET name = excluded.name, refreshed = excluded.refreshed",
            params![profile, name, seconds(SystemTime::now())],
        )
        .map_err(write_error)?;
    connection
        .execute("DELETE FROM profile_pages WHERE profile = ?1", [&profile])
        .map_err(write_error)?;
    for page in pages {
        connection
            .execute(
                "INSERT OR IGNORE INTO profile_pages(profile, page) VALUES (?1, ?2)",
                params![profile, page],
            )
            .map_err(write_error)?;
    }
    Ok(())
}

fn stop_following(connection: &Connection, link: &ProfileLink) -> Result<()> {
    connection
        .execute("DELETE FROM profile_links WHERE link = ?1", [link.page()])
        .map_err(write_error)?;
    Ok(())
}

fn seconds(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    })
}

fn account_error(source: resonate_library::Error) -> Error {
    match source {
        resonate_library::Error::Refused {
            op: LookupOp::SpotifyAccount,
            ..
        } => Error::SpotifySignInLapsed,
        source => crate::collections::follow_error(source),
    }
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playlists::tests::player;

    const PROFILE: &str = "https://www.deezer.com/fr/profile/2529";

    #[test]
    fn a_profile_followed_is_due_once_its_hours_pass_and_unfollowed_on_asking() {
        let player = player();
        let link = ProfileLink::read(PROFILE).expect("a profile");
        keep_following(
            &player.catalog.0.lock(),
            &link,
            "dadbond",
            &["https://www.deezer.com/playlist/1".to_owned()],
        )
        .expect("followed");

        let followed = player.profiles_followed().expect("the profiles");
        let due_now = player.profiles_due_a_refresh().expect("the due list");
        player
            .catalog
            .0
            .lock()
            .execute("UPDATE profile_links SET refreshed = 0", [])
            .expect("an old follow");
        let due_later = player.profiles_due_a_refresh().expect("the due list");
        let pages = pages_of(&player.catalog.0.lock(), &link).expect("its pages");
        player.unfollow_profile(PROFILE.into()).expect("unfollowed");

        assert_eq!(
            followed,
            [ProfileFollowed {
                link: link.page(),
                name: "dadbond".to_owned(),
                service: "Deezer".to_owned(),
            }]
        );
        assert!(due_now.is_empty());
        assert_eq!(due_later, [link.page()]);
        assert_eq!(pages, ["https://www.deezer.com/playlist/1"]);
        assert!(player.profiles_followed().expect("the profiles").is_empty());
        assert!(
            pages_of(&player.catalog.0.lock(), &link)
                .expect("its pages")
                .is_empty(),
            "the pages go with the profile"
        );
    }

    #[test]
    fn a_profile_link_names_its_service_and_anything_else_names_none() {
        assert_eq!(
            profile_link_service(PROFILE.into()).as_deref(),
            Some("Deezer")
        );
        assert_eq!(
            profile_link_service("https://www.deezer.com/playlist/1".into()),
            None
        );
        assert!(matches!(
            player().unfollow_profile("Night drive".into()),
            Err(Error::NotAProfileLink)
        ));
    }
}
