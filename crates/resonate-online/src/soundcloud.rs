use resonate_codec::CoverArt;

use crate::{
    Client, Host, Result,
    shared::{on_host, page_of, picture_at, shared_picture},
};

const PICTURES_SERVED_BY: &str = "sndcdn.com";
const AN_AVATAR: &str = "avatars-";

pub(crate) fn portrait(client: &Client, url: &str) -> Result<Option<CoverArt>> {
    let Some(user) = user(url) else {
        tracing::debug!(
            url,
            "a SoundCloud link that names no account is passed over"
        );
        return Ok(None);
    };
    let page = format!("{}/{user}", Host::SoundCloud.base());
    let Some(held) = page_of(client, Host::SoundCloud, &page)? else {
        return Ok(None);
    };
    let Some(picture) = shared_picture(&held).filter(|picture| an_avatar(picture)) else {
        tracing::debug!(page, "SoundCloud shows no picture the account chose");
        return Ok(None);
    };

    picture_at(client, Host::SoundCloudPictures, picture)
}

pub(crate) fn user(url: &str) -> Option<&str> {
    let path = on_host(url, "soundcloud.com")?;
    let path = path.split(['?', '#']).next()?;
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let user = segments.next()?;
    let usable = segments.next().is_none()
        && user
            .chars()
            .all(|glyph| glyph.is_ascii_alphanumeric() || glyph == '-' || glyph == '_');

    usable.then_some(user)
}

fn an_avatar(picture: &str) -> bool {
    on_host(picture, PICTURES_SERVED_BY).is_some_and(|path| path.starts_with(AN_AVATAR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_soundcloud_link_names_the_account_it_points_at_and_nothing_under_it() {
        assert_eq!(
            user("https://soundcloud.com/michal-cielecki"),
            Some("michal-cielecki")
        );
        assert_eq!(user("https://soundcloud.com/wierza/"), Some("wierza"));
        assert_eq!(user("https://soundcloud.com/wierza/a-track"), None);
        assert_eq!(user("https://example.com/wierza"), None);
        assert_eq!(user("https://soundcloud.com/"), None);
    }

    #[test]
    fn only_an_avatar_the_account_chose_is_a_portrait() {
        assert!(an_avatar(
            "https://i1.sndcdn.com/avatars-000271376401-f17sn9-t500x500.jpg"
        ));
        assert!(!an_avatar(
            "https://a-v2.sndcdn.com/assets/images/default_avatar_large.png"
        ));
        assert!(!an_avatar(
            "https://example.com/avatars-000271376401-f17sn9-t500x500.jpg"
        ));
    }
}
