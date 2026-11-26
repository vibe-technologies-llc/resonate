use std::borrow::Cow;

use resonate_providers::Unfollowed;
use ureq::{
    Body,
    http::{Response, Uri, header::LOCATION},
};

pub const REDIRECTS_FOLLOWED_AT_MOST: u32 = 5;
const FOLLOWED: [u16; 5] = [301, 302, 303, 307, 308];
const PLAIN: &str = "http";
const SECURE: &str = "https";
const PLAIN_PORT: u16 = 80;
const SECURE_PORT: u16 = 443;
const SCHEME_ENDS: &str = "://";

#[derive(Debug, thiserror::Error)]
pub enum Unanswered {
    #[error(transparent)]
    Failed(#[from] ureq::Error),
    #[error(transparent)]
    Redirected(#[from] Unfollowed),
}

pub fn followed(
    url: &str,
    mut asked: impl FnMut(&str) -> Result<Response<Body>, ureq::Error>,
) -> Result<Response<Body>, Unanswered> {
    let mut asking = Cow::Borrowed(url);
    let mut hops = 0;
    loop {
        let response = asked(&asking)?;

        if !FOLLOWED.contains(&response.status().as_u16()) {
            return Ok(response);
        }
        if hops == REDIRECTS_FOLLOWED_AT_MOST {
            return Err(Unfollowed::Endless.into());
        }

        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(Unfollowed::Unlocated)?;
        let onward = onward(&asking, location)?;

        hops += 1;
        tracing::debug!(
            hops,
            "a redirect is followed within the origin it was asked of"
        );
        asking = Cow::Owned(onward);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Origin {
    secure: bool,
    host: String,
    port: u16,
}

impl Origin {
    fn of(url: &str) -> Result<Self, Unfollowed> {
        let (scheme, _) = url.split_once(':').ok_or(Unfollowed::Unlocated)?;
        let secure = if scheme.eq_ignore_ascii_case(SECURE) {
            true
        } else if scheme.eq_ignore_ascii_case(PLAIN) {
            false
        } else {
            return Err(Unfollowed::ElsewhereThanAsked);
        };
        let uri: Uri = url.parse().map_err(|_| Unfollowed::Unlocated)?;
        let default_port = if secure { SECURE_PORT } else { PLAIN_PORT };

        Ok(Self {
            secure,
            host: uri
                .host()
                .ok_or(Unfollowed::Unlocated)?
                .to_ascii_lowercase(),
            port: uri.port_u16().unwrap_or(default_port),
        })
    }

    fn admits(&self, onward: &Self) -> Result<(), Unfollowed> {
        let upgraded = !self.secure && onward.secure;

        if self.secure && !onward.secure {
            return Err(Unfollowed::Downgraded);
        }
        if onward.host != self.host || (!upgraded && onward.port != self.port) {
            return Err(Unfollowed::ElsewhereThanAsked);
        }
        Ok(())
    }
}

fn onward(from: &str, location: &str) -> Result<String, Unfollowed> {
    let onward = resolved(from, location).ok_or(Unfollowed::Unlocated)?;

    Origin::of(from)?.admits(&Origin::of(&onward)?)?;
    Ok(onward)
}

fn names_a_scheme(reference: &str) -> bool {
    let Some((scheme, _)) = reference.split_once(':') else {
        return false;
    };
    let mut letters = scheme.chars();

    letters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && letters.all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '+' | '-' | '.'))
}

fn resolved(base: &str, location: &str) -> Option<String> {
    let reference = location.trim().split('#').next().unwrap_or_default();

    if reference.is_empty() {
        return None;
    }
    if names_a_scheme(reference) {
        return Some(reference.to_owned());
    }

    let (scheme, rest) = base.split_once(SCHEME_ENDS)?;
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = rest.find('/').map_or((rest, "/"), |at| rest.split_at(at));

    if let Some(elsewhere) = reference.strip_prefix("//") {
        return Some(format!("{scheme}{SCHEME_ENDS}{elsewhere}"));
    }
    if reference.starts_with('/') {
        return Some(format!("{scheme}{SCHEME_ENDS}{authority}{reference}"));
    }
    if reference.starts_with('?') {
        return Some(format!("{scheme}{SCHEME_ENDS}{authority}{path}{reference}"));
    }

    let folder = path.rfind('/').map_or("/", |at| &path[..=at]);
    Some(format!(
        "{scheme}{SCHEME_ENDS}{authority}{folder}{reference}"
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ureq::{Agent, http::header::RANGE};

    use super::*;
    use crate::{
        Trusted, configured,
        testing::{WHOLE_BODY, answering, lower, serving},
    };

    const ASKED_FROM: &str = "bytes=10-";

    fn agent() -> Agent {
        configured(Duration::from_secs(5), Trusted::BuiltInRoots)
            .build()
            .new_agent()
    }

    fn redirecting_to(location: &str) -> Vec<u8> {
        answering("302 Found", &[&format!("Location: {location}")], b"", 0)
    }

    fn whole() -> Vec<u8> {
        answering("200 OK", &[], WHOLE_BODY, WHOLE_BODY.len())
    }

    fn asked_with_a_range(agent: &Agent, url: &str) -> Result<Response<Body>, Unanswered> {
        followed(url, |url| agent.get(url).header(RANGE, ASKED_FROM).call())
    }

    #[test]
    fn a_redirect_within_the_origin_is_followed_carrying_every_header_asked_with() {
        let (url, heads) = serving(vec![
            Box::new(|_| redirecting_to("/moved?to=here")),
            Box::new(|_| whole()),
        ]);

        let mut response = asked_with_a_range(&agent(), &url).expect("the redirect is followed");

        let body = response.body_mut().read_to_vec().expect("the body reads");
        let heads = heads.lock();

        assert_eq!(body, WHOLE_BODY);
        assert_eq!(heads.len(), 2);
        assert!(heads[1].starts_with("GET /moved?to=here "), "{}", heads[1]);
        assert!(
            heads
                .iter()
                .all(|head| lower(head).contains(&format!("range: {ASKED_FROM}"))),
            "{heads:?}"
        );
    }

    #[test]
    fn a_redirect_to_another_port_is_refused_and_never_asked() {
        let (elsewhere, reached) = serving(vec![Box::new(|_| whole())]);
        let location = elsewhere.clone();
        let (url, heads) = serving(vec![Box::new(move |_| redirecting_to(&location))]);

        let refused = asked_with_a_range(&agent(), &url).expect_err("the redirect is refused");

        assert!(
            matches!(
                refused,
                Unanswered::Redirected(Unfollowed::ElsewhereThanAsked)
            ),
            "{refused:?}"
        );
        assert_eq!(heads.lock().len(), 1);
        assert!(reached.lock().is_empty(), "{elsewhere} was asked");
    }

    #[test]
    fn redirects_past_the_most_followed_are_refused() {
        let hops = REDIRECTS_FOLLOWED_AT_MOST as usize + 1;
        let (url, heads) = serving(
            (0..hops + 1)
                .map(|_| -> crate::testing::Answer { Box::new(|_| redirecting_to("again")) })
                .collect(),
        );

        let refused = asked_with_a_range(&agent(), &url).expect_err("the redirects are refused");

        assert!(
            matches!(refused, Unanswered::Redirected(Unfollowed::Endless)),
            "{refused:?}"
        );
        assert_eq!(heads.lock().len(), hops);
    }

    #[test]
    fn the_most_redirects_followed_are_all_followed_and_the_answer_after_them_is_handed_back() {
        let redirects = REDIRECTS_FOLLOWED_AT_MOST as usize;
        let (url, heads) = serving(
            (0..redirects)
                .map(|_| -> crate::testing::Answer { Box::new(|_| redirecting_to("again")) })
                .chain([Box::new(|_: &str| whole()) as crate::testing::Answer])
                .collect(),
        );

        let mut response = asked_with_a_range(&agent(), &url).expect("every redirect is followed");

        let body = response.body_mut().read_to_vec().expect("the body reads");

        assert_eq!(body, WHOLE_BODY);
        assert_eq!(heads.lock().len(), redirects + 1);
    }

    #[test]
    fn a_redirect_naming_no_place_is_refused() {
        let (url, _) = serving(vec![Box::new(|_| {
            answering("307 Temporary Redirect", &[], b"", 0)
        })]);

        let refused = asked_with_a_range(&agent(), &url).expect_err("the redirect is refused");

        assert!(
            matches!(refused, Unanswered::Redirected(Unfollowed::Unlocated)),
            "{refused:?}"
        );
    }

    #[test]
    fn an_answer_that_is_no_redirect_is_handed_back_as_it_came() {
        let (url, heads) = serving(vec![Box::new(|_| {
            answering(
                "304 Not Modified",
                &["Location: http://elsewhere.test/"],
                b"",
                0,
            )
        })]);

        let response = asked_with_a_range(&agent(), &url).expect("the answer");

        assert_eq!(response.status().as_u16(), 304);
        assert_eq!(heads.lock().len(), 1);
    }

    #[test]
    fn a_redirect_is_refused_off_its_host_or_port_and_down_to_plain_http() {
        let from = "https://music.local/rest/stream?id=1";

        assert_eq!(
            onward(from, "https://music.local.example/rest/stream"),
            Err(Unfollowed::ElsewhereThanAsked)
        );
        assert_eq!(
            onward(from, "https://music.local:8443/rest/stream"),
            Err(Unfollowed::ElsewhereThanAsked)
        );
        assert_eq!(
            onward(from, "//127.0.0.1/rest/stream"),
            Err(Unfollowed::ElsewhereThanAsked)
        );
        assert_eq!(
            onward(from, "http://music.local/rest/stream"),
            Err(Unfollowed::Downgraded)
        );
        assert_eq!(
            onward(from, "file:///etc/passwd"),
            Err(Unfollowed::ElsewhereThanAsked)
        );
        assert_eq!(onward(from, "  "), Err(Unfollowed::Unlocated));
        assert_eq!(
            onward(from, "https://music.local/rest/stream with spaces"),
            Err(Unfollowed::Unlocated)
        );
    }

    #[test]
    fn a_redirect_is_followed_on_its_host_and_port_and_up_to_https_on_its_host() {
        assert_eq!(
            onward(
                "https://Music.Local/rest/stream",
                "https://music.local:443/rest/moved"
            ),
            Ok("https://music.local:443/rest/moved".to_owned())
        );
        assert_eq!(
            onward(
                "http://music.local:4533/rest/stream",
                "https://music.local/rest/stream"
            ),
            Ok("https://music.local/rest/stream".to_owned())
        );
        assert_eq!(
            onward(
                "http://music.local:4533/rest/stream",
                "http://music.local:4534/rest/stream"
            ),
            Err(Unfollowed::ElsewhereThanAsked)
        );
    }

    #[test]
    fn a_relative_location_is_resolved_against_the_address_asked() {
        let base = "http://music.local:4533/rest/stream?id=1#part";

        assert_eq!(
            resolved(base, "/elsewhere?id=2"),
            Some("http://music.local:4533/elsewhere?id=2".to_owned())
        );
        assert_eq!(
            resolved(base, "download?id=2#part"),
            Some("http://music.local:4533/rest/download?id=2".to_owned())
        );
        assert_eq!(
            resolved(base, "?id=3"),
            Some("http://music.local:4533/rest/stream?id=3".to_owned())
        );
        assert_eq!(
            resolved(base, "//other.local/x"),
            Some("http://other.local/x".to_owned())
        );
        assert_eq!(
            resolved("http://music.local", "song"),
            Some("http://music.local/song".to_owned())
        );
        assert_eq!(
            resolved(base, "HTTPS://music.local/x"),
            Some("HTTPS://music.local/x".to_owned())
        );
        assert_eq!(resolved(base, "#only"), None);
    }

    #[test]
    fn unanswered_stays_small_enough_for_result_large_err() {
        assert!(
            size_of::<Unanswered>() <= 128,
            "{}",
            size_of::<Unanswered>()
        );
    }
}
