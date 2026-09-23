use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::LookupOp;

use crate::{Client, Host, Result, client::LARGEST_PICTURE, query::escape_query};

const PORTRAIT_WIDTH: u32 = 600;
const FILE_PAGE: &str = "/wiki/File:";
const INDEX_PAGE: &str = "/w/index.php?title=File:";
const SCALED: &str = "/wiki/Special:FilePath/";
const UPLOAD: &str = "upload.wikimedia.org/wikipedia/commons/";
const THUMBNAIL: &str = "thumb/";
const FANNED_OUT: usize = 2;

pub(crate) fn portrait(client: &Client, url: &str) -> Result<Option<CoverArt>> {
    let Some(scaled) = file_path(url) else {
        tracing::debug!(url, "a portrait is only fetched from Wikimedia Commons");
        return Ok(None);
    };

    fetch(client, &scaled)
}

pub(crate) fn fetch(client: &Client, scaled: &str) -> Result<Option<CoverArt>> {
    let op = LookupOp::Portrait;

    Ok(client
        .bytes(Host::Commons, op, scaled, LARGEST_PICTURE)?
        .and_then(|bytes| drawable(scaled, bytes)))
}

fn drawable(scaled: &str, bytes: Vec<u8>) -> Option<CoverArt> {
    let Some(format) = ImageFormat::sniff(&bytes) else {
        tracing::debug!(
            url = scaled,
            "a portrait came back in a format this build cannot draw; it is passed over"
        );
        return None;
    };
    Some(CoverArt { format, bytes })
}

pub(crate) fn file_path(url: &str) -> Option<String> {
    at_the_width_wanted(named(url)?)
}

pub(crate) fn scaled(name: &str) -> Option<String> {
    at_the_width_wanted(&escape_query(name.trim()))
}

fn at_the_width_wanted(escaped: &str) -> Option<String> {
    if escaped.is_empty() {
        return None;
    }

    Some(format!(
        "{}{SCALED}{escaped}?width={PORTRAIT_WIDTH}",
        Host::Commons.base()
    ))
}

fn named(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;

    if let Some(rest) = rest.strip_prefix(UPLOAD) {
        let rest = rest.split(['?', '#']).next()?;
        return match rest.strip_prefix(THUMBNAIL) {
            Some(thumbnail) => thumbnail.split('/').nth(FANNED_OUT),
            None => rest.rsplit('/').next(),
        }
        .filter(|name| !name.is_empty());
    }

    let rest = rest
        .strip_prefix(Host::Commons.base().trim_start_matches("https://"))
        .or_else(|| rest.strip_prefix("www.commons.wikimedia.org"))?;

    if let Some(name) = rest.strip_prefix(FILE_PAGE) {
        return name.split(['?', '#']).next();
    }
    if let Some(name) = rest.strip_prefix(SCALED) {
        return name.split(['?', '#']).next();
    }

    rest.strip_prefix(INDEX_PAGE)?.split(['&', '#']).next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_portrait_in_a_format_this_build_cannot_draw_is_a_miss_rather_than_a_refusal() {
        let url = "https://commons.wikimedia.org/wiki/Special:FilePath/Logo.svg?width=600";
        assert_eq!(
            drawable(url, b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec()),
            None
        );

        let png = b"\x89PNG\r\n\x1a\n rest".to_vec();
        assert_eq!(
            drawable(url, png.clone()),
            Some(CoverArt {
                format: ImageFormat::Png,
                bytes: png,
            })
        );
    }

    #[test]
    fn a_file_named_by_wikidata_is_escaped_on_the_way_into_a_path() {
        assert_eq!(
            scaled("Bella Poarch - Pink Aura Tour.jpg").as_deref(),
            Some(
                "https://commons.wikimedia.org/wiki/Special:FilePath/\
                 Bella%20Poarch%20-%20Pink%20Aura%20Tour.jpg?width=600"
            ),
            "a name with spaces in it was sent as a URL no server will read"
        );
        assert_eq!(scaled("   "), None);
        assert_eq!(scaled(""), None);
    }

    #[test]
    fn a_commons_file_page_becomes_a_scaled_file_path() {
        let scaled = "https://commons.wikimedia.org/wiki/Special:FilePath/PinkFloyd1973_retouched.jpg?width=600";

        assert_eq!(
            file_path("https://commons.wikimedia.org/wiki/File:PinkFloyd1973_retouched.jpg")
                .as_deref(),
            Some(scaled)
        );
        assert_eq!(
            file_path(
                "https://commons.wikimedia.org/w/index.php?title=File:PinkFloyd1973_retouched.jpg&oldid=1"
            )
            .as_deref(),
            Some(scaled)
        );
        assert_eq!(
            file_path("http://commons.wikimedia.org/wiki/File:PinkFloyd1973_retouched.jpg#top")
                .as_deref(),
            Some(scaled)
        );
        assert_eq!(
            file_path("https://commons.wikimedia.org/wiki/File:Pink%20Floyd.jpg").as_deref(),
            Some("https://commons.wikimedia.org/wiki/Special:FilePath/Pink%20Floyd.jpg?width=600")
        );

        assert_eq!(
            file_path(
                "https://commons.wikimedia.org/wiki/Special:FilePath/PinkFloyd1973_retouched.jpg"
            )
            .as_deref(),
            Some(scaled),
            "a path the service already scales was not asked for at the width this build wants"
        );
        assert_eq!(
            file_path(
                "https://upload.wikimedia.org/wikipedia/commons/a/ab/PinkFloyd1973_retouched.jpg"
            )
            .as_deref(),
            Some(scaled),
            "the original an upload URL names was not read back as its file"
        );
        assert_eq!(
            file_path(
                "https://upload.wikimedia.org/wikipedia/commons/thumb/a/ab/\
                 PinkFloyd1973_retouched.jpg/640px-PinkFloyd1973_retouched.jpg"
            )
            .as_deref(),
            Some(scaled),
            "a thumbnail URL did not name the file it is a thumbnail of"
        );

        assert_eq!(file_path("https://commons.wikimedia.org/wiki/File:"), None);
        assert_eq!(
            file_path("https://commons.wikimedia.org/wiki/Category:Pink_Floyd"),
            None
        );
        assert_eq!(
            file_path("https://en.wikipedia.org/wiki/File:PinkFloyd1973_retouched.jpg"),
            None
        );
        assert_eq!(file_path("https://example.invalid/pink.jpg"), None);
        assert_eq!(file_path("not a url"), None);
    }
}
