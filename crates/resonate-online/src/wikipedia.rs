use resonate_library::LookupOp;

use crate::{
    Client, Host, Result,
    query::Params,
    shared::on_host,
    wikidata::{self, EntityDoc},
};

const ENTITIES: &str = "/w/api.php";

const WIKIPEDIA: &str = "wikipedia.org";

const MOBILE: &str = "m";

const A_SITE_OF_WIKIPEDIA: &str = "wiki";

pub(crate) fn pictured(client: &Client, url: &str) -> Result<Option<String>> {
    let Some(page) = page(url) else {
        tracing::debug!(url, "a Wikipedia link that names no article is passed over");
        return Ok(None);
    };
    let asked = Params::new()
        .with("action", "wbgetentities")
        .with("sites", &page.site)
        .with("titles", &page.title)
        .with("props", "claims")
        .with("format", "json")
        .finish();
    let Some(held) = client.json::<EntityDoc>(
        Host::Wikidata,
        LookupOp::Portrait,
        &format!("{ENTITIES}{asked}"),
    )?
    else {
        return Ok(None);
    };

    Ok(wikidata::pictured_among(&held))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Page {
    site: String,
    title: String,
}

pub(crate) fn page(url: &str) -> Option<Page> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, _) = rest.split_once('/')?;
    let path = on_host(&format!("https://{rest}"), WIKIPEDIA)?.to_owned();
    let language = host
        .strip_suffix(WIKIPEDIA)?
        .trim_end_matches('.')
        .split('.')
        .find(|label| *label != MOBILE)?;
    let usable = !language.is_empty()
        && language
            .chars()
            .all(|glyph| glyph.is_ascii_lowercase() || glyph == '-');
    if !usable {
        return None;
    }
    let title = path
        .strip_prefix("wiki/")?
        .split(['?', '#'])
        .next()
        .filter(|title| !title.is_empty())?;

    Some(Page {
        site: format!("{}{A_SITE_OF_WIKIPEDIA}", language.replace('-', "_")),
        title: unescaped(title)?,
    })
}

fn unescaped(title: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(title.len());
    let mut rest = title.as_bytes();
    while let [first, tail @ ..] = rest {
        if *first == b'%' {
            let [high, low, after @ ..] = tail else {
                return None;
            };
            let hex = std::str::from_utf8(&[*high, *low]).ok()?.to_owned();
            bytes.push(u8::from_str_radix(&hex, 16).ok()?);
            rest = after;
        } else {
            bytes.push(*first);
            rest = tail;
        }
    }
    String::from_utf8(bytes)
        .ok()
        .map(|title| title.replace('_', " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_of(site: &str, title: &str) -> Option<Page> {
        Some(Page {
            site: site.to_owned(),
            title: title.to_owned(),
        })
    }

    #[test]
    fn a_wikipedia_link_names_the_article_and_the_wiki_it_is_on() {
        assert_eq!(
            page("https://en.wikipedia.org/wiki/Pink_Floyd"),
            page_of("enwiki", "Pink Floyd")
        );
        assert_eq!(
            page("https://is.wikipedia.org/wiki/Bj%C3%B6rk#Early_life"),
            page_of("iswiki", "Björk")
        );
        assert_eq!(
            page("https://en.m.wikipedia.org/wiki/Echoes_(song)"),
            page_of("enwiki", "Echoes (song)")
        );
        assert_eq!(
            page("https://zh-yue.wikipedia.org/wiki/Beyond"),
            page_of("zh_yuewiki", "Beyond")
        );

        assert_eq!(page("https://en.wikipedia.org/w/index.php?title=X"), None);
        assert_eq!(page("https://en.wikipedia.org/wiki/"), None);
        assert_eq!(page("https://www.wikidata.org/wiki/Q2306"), None);
        assert_eq!(page("https://en.wikipedia.org/wiki/Bad%ZZ"), None);
        assert_eq!(page("not a url"), None);
    }

    #[test]
    fn the_entity_an_article_is_about_names_the_picture_it_carries() {
        let held: EntityDoc =
            serde_json::from_str(include_str!("../tests/fixtures/wikipedia.json"))
                .expect("the captured entity reads back");

        assert_eq!(
            wikidata::pictured_among(&held),
            crate::commons::scaled("Pink Floyd, 1971 (HQ).jpg")
        );
    }
}
