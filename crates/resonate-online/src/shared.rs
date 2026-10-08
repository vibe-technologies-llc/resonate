use resonate_codec::{CoverArt, ImageFormat};
use resonate_library::LookupOp;

use crate::{
    Client, Host, Result,
    client::{LARGEST_DOCUMENT, LARGEST_PICTURE, passed_over_when_refused},
};

const SHARED_AS: &str = "property=\"og:image\" content=\"";

pub(crate) fn shared_picture(page: &str) -> Option<&str> {
    let from = page.find(SHARED_AS)? + SHARED_AS.len();
    let rest = &page[from..];
    Some(&rest[..rest.find('"')?])
}

const PAGE_DATA_OPENS: &str = r#"<script id="__NEXT_DATA__" type="application/json">"#;
const PAGE_DATA_CLOSES: &str = "</script>";

pub(crate) fn stated(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

pub(crate) fn next_data(page: &str) -> Option<&str> {
    let from = page.find(PAGE_DATA_OPENS)? + PAGE_DATA_OPENS.len();
    let rest = &page[from..];
    Some(&rest[..rest.find(PAGE_DATA_CLOSES)?])
}

pub(crate) fn page_of(client: &Client, host: Host, page: &str) -> Result<Option<String>> {
    Ok(
        passed_over_when_refused(client.bytes(host, LookupOp::Portrait, page, LARGEST_DOCUMENT))?
            .map(|held| String::from_utf8_lossy(&held).into_owned()),
    )
}

pub(crate) fn picture_at(client: &Client, host: Host, url: &str) -> Result<Option<CoverArt>> {
    Ok(
        passed_over_when_refused(client.bytes(host, LookupOp::Portrait, url, LARGEST_PICTURE))?
            .and_then(|bytes| {
                let format = ImageFormat::sniff(&bytes)?;
                Some(CoverArt { format, bytes })
            }),
    )
}

pub(crate) fn on_host<'a>(url: &'a str, served_by: &str) -> Option<&'a str> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_at(rest.find(['/', '?', '#', '\\'])?);
    let path = path.strip_prefix('/')?;
    let plain = host
        .chars()
        .all(|letter| letter.is_ascii_alphanumeric() || letter == '-' || letter == '.');

    (plain && (host == served_by || host.ends_with(&format!(".{served_by}")))).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_a_page_shares_is_cut_out_of_its_open_graph_tag() {
        let page = r#"<head><meta property="og:title" content="x"/><meta property="og:image" content="https://i.scdn.co/image/ab676161"/></head>"#;

        assert_eq!(
            shared_picture(page),
            Some("https://i.scdn.co/image/ab676161")
        );
        assert_eq!(shared_picture("<head></head>"), None);
    }

    #[test]
    fn a_url_is_on_a_host_or_one_of_its_subdomains_and_nowhere_else() {
        assert_eq!(
            on_host("https://i.scdn.co/image/a", "i.scdn.co"),
            Some("image/a")
        );
        assert_eq!(
            on_host("https://i1.sndcdn.com/avatars-1.jpg", "sndcdn.com"),
            Some("avatars-1.jpg")
        );
        assert_eq!(on_host("https://notsndcdn.com/a", "sndcdn.com"), None);
        assert_eq!(on_host("http://i.scdn.co/image/a", "i.scdn.co"), None);
    }

    #[test]
    fn a_host_a_delimiter_or_a_userinfo_disguises_is_not_the_host_it_names() {
        for url in [
            "https://evil.com#.sndcdn.com/a",
            "https://evil.com?.sndcdn.com/a",
            "https://evil.com\\.sndcdn.com/a",
            "https://evil.com\\@i1.sndcdn.com/a",
            "https://i1.sndcdn.com@evil.com/a",
            "https://evil.com@i1.sndcdn.com/a",
            "https://i1.sndcdn.com:8080/a",
            "https://i1.sndcdn.com",
            "https://i1.sndcdn.com?a",
        ] {
            assert_eq!(on_host(url, "sndcdn.com"), None, "{url}");
        }
    }
}
