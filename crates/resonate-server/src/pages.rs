use resonate_sync::{
    shared::{ENTRIES_KEPT_AT_MOST, LARGEST_BUNDLE_OPENED, ROWS_AT_MOST},
    wire::PROTOCOL_VERSION,
};

use crate::words;

const PAGE_REQUEST_READ_AT_MOST: usize = 2_048;

const SHARED_PLAYLIST: &str = "shared";

const PARTY: &str = "party";

const ASSET_LINKS_TARGET: &str = "/.well-known/assetlinks.json";

const ANDROID_PACKAGE: &str = "org.resonate.android";

const SHARED_LIST: &str = include_str!("shared_list.html");

const SHARED_READING: &str = include_str!("shared_reading.html");

const SHARED_READER: &str = include_str!("shared_list.js");

const PAGE: &str = r##"<!doctype html>
<html lang="@LANGUAGE@">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>Resonate</title>
<style>
:root{color-scheme:light dark;--ground:#fff;--ink:#17151c;--faint:#5f5a69;--accent:#6d4aa0;--paper:#fff;--line:#e4e1ea}
@media (prefers-color-scheme:dark){:root{--ground:#121016;--ink:#f3f1f6;--faint:#a9a3b4;--accent:#b79ae0;--paper:#17121f;--line:#2c2833}}
body{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:var(--ground);color:var(--ink);font:16px/1.5 system-ui,sans-serif;padding:16px;box-sizing:border-box}
main{width:100%;max-width:34rem}
h1{font-size:1.6rem;margin:0 0 .5rem}
p{margin:0 0 1rem;color:var(--faint)}
a.open{display:inline-block;padding:.7rem 1.2rem;border-radius:.6rem;background:var(--accent);color:var(--paper);font-weight:600;text-decoration:none}
code{display:block;margin-top:1.2rem;word-break:break-all;color:var(--faint)}
h2{font-size:1.25rem;margin:1.6rem 0 .4rem;overflow-wrap:anywhere}
ol{list-style:none;padding:0;margin:0}
li{display:flex;align-items:baseline;gap:.75rem;padding:.55rem 0;border-top:1px solid var(--line)}
li .n,li .l{color:var(--faint);font-variant-numeric:tabular-nums}
li .n{min-width:1.6rem;text-align:right}
li .t{flex:1;min-width:0}
li .t b{display:block;font-weight:600;overflow-wrap:anywhere}
li .t span{display:block;color:var(--faint);font-size:.9rem;overflow-wrap:anywhere}
#status:empty{display:none}
</style>
</head>
<body>
<main>
<h1>@HEADING@</h1>
<p>@SAYING@</p>
<a class="open" id="open" href="#">@OPEN@</a>
<code id="code"></code>
<noscript><p>@NEEDS_SCRIPT@</p></noscript>
@LIST@
</main>
<script>
var code = location.hash.slice(1).split(/[?&#]/)[0].replace(/[^A-Za-z0-9-]/g, "");
var server = "wss://" + location.host + "/";
document.getElementById("code").textContent = code;
document.getElementById("open").href =
  "resonate://@KIND@/" + code + "?server=" + encodeURIComponent(server);
</script>
@READING@
</body>
</html>
"##;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Page(&'static str),
    AssetLinks,
}

pub fn asked_of(request: &[u8]) -> Option<Asked> {
    let line = request.split(|byte| *byte == b'\r').next()?;
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.split(' ');
    if words.next()? != "GET" {
        return None;
    }
    let target = words.next()?;
    let target = target.split(['?', '#']).next()?;

    match target {
        "/s" | "/s/" => Some(Asked::Page(SHARED_PLAYLIST)),
        "/p" | "/p/" => Some(Asked::Page(PARTY)),
        ASSET_LINKS_TARGET => Some(Asked::AssetLinks),
        _ => None,
    }
}

pub const fn reads_at_most() -> usize {
    PAGE_REQUEST_READ_AT_MOST
}

pub fn answer(asked: Asked, android_certificates: &[String]) -> Vec<u8> {
    match asked {
        Asked::Page(kind) => answered("200 OK", "text/html; charset=utf-8", &page_of(kind)),
        Asked::AssetLinks if android_certificates.is_empty() => {
            answered("404 Not Found", "text/plain; charset=utf-8", "")
        }
        Asked::AssetLinks => answered(
            "200 OK",
            "application/json",
            &asset_links(android_certificates),
        ),
    }
}

fn page_of(kind: &str) -> String {
    let (heading, saying, list, reading) = if kind == PARTY {
        (
            words::pages::party_heading(),
            words::pages::party_saying(),
            String::new(),
            String::new(),
        )
    } else {
        (
            words::pages::shared_heading(),
            words::pages::shared_saying(),
            SHARED_LIST.to_owned(),
            shared_reading(),
        )
    };

    PAGE.replace("@LIST@", &list)
        .replace("@READING@", &reading)
        .replace("@LANGUAGE@", resonate_words::spoken().tag())
        .replace("@HEADING@", &escaped(heading))
        .replace("@SAYING@", &escaped(saying))
        .replace("@OPEN@", &escaped(words::pages::open()))
        .replace("@NEEDS_SCRIPT@", &escaped(words::pages::needs_script()))
        .replace("@KIND@", kind)
}

fn shared_reading() -> String {
    let reader = SHARED_READER
        .replace("@VERSION@", &PROTOCOL_VERSION.to_string())
        .replace("@LARGEST@", &LARGEST_BUNDLE_OPENED.to_string())
        .replace("@ROWS@", &ROWS_AT_MOST.to_string())
        .replace("@ENTRIES@", &ENTRIES_KEPT_AT_MOST.to_string());
    let said = [
        ("reading", words::pages::shared_reading()),
        ("unnamed", words::pages::shared_unnamed()),
        ("empty", words::pages::shared_empty()),
        ("unread", words::pages::shared_unread()),
        ("unknown", words::pages::shared_unknown()),
        ("moved", words::pages::shared_moved()),
        ("unreached", words::pages::shared_unreached()),
    ];
    let words = said
        .iter()
        .map(|(key, said)| format!("{}:{}", in_json(key), in_json(said)))
        .collect::<Vec<_>>()
        .join(",");

    SHARED_READING
        .replace("@WORDS@", &format!("{{{words}}}"))
        .replace("@READ@", &reader)
}

fn in_json(text: &str) -> String {
    let mut written = String::with_capacity(text.len() + 2);
    written.push('"');
    for letter in text.chars() {
        match letter {
            '"' => written.push_str("\\\""),
            '\\' => written.push_str("\\\\"),
            '<' | '>' | '&' => written.push_str(&format!("\\u{:04x}", u32::from(letter))),
            letter if letter < ' ' => written.push_str(&format!("\\u{:04x}", u32::from(letter))),
            letter => written.push(letter),
        }
    }
    written.push('"');
    written
}

fn escaped(said: &str) -> String {
    said.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn asset_links(certificates: &[String]) -> String {
    let certificates = certificates
        .iter()
        .map(|certificate| format!("\"{certificate}\""))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "[{{\"relation\":[\"delegate_permission/common.handle_all_urls\"],\"target\":\
         {{\"namespace\":\"android_app\",\"package_name\":\"{ANDROID_PACKAGE}\",\
         \"sha256_cert_fingerprints\":[{certificates}]}}}}]"
    )
}

fn answered(status: &str, content_type: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src \
         'unsafe-inline'; connect-src 'self'\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_shared_playlists_page_reads_the_list_and_its_words_cannot_close_the_script() {
        let shared = page_of(SHARED_PLAYLIST);
        let party = page_of(PARTY);

        assert!(shared.contains("readShared(code, address)"));
        assert!(
            !shared
                .split('@')
                .skip(1)
                .any(|after| after.starts_with(|letter: char| letter.is_ascii_uppercase())),
            "a placeholder was left unfilled"
        );
        assert!(!party.contains("readShared"));
        assert!(!party.contains("id=\"songs\""));
        assert_eq!(
            in_json("</script> \"said\" \\ \n"),
            "\"\\u003c/script\\u003e \\\"said\\\" \\\\ \\u000a\""
        );
    }

    #[test]
    fn a_page_is_told_from_a_socket_by_its_request_line_alone() {
        assert_eq!(
            asked_of(b"GET /s/ HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some(Asked::Page(SHARED_PLAYLIST))
        );
        assert_eq!(
            asked_of(b"GET /p HTTP/1.1\r\n\r\n"),
            Some(Asked::Page(PARTY))
        );
        assert_eq!(
            asked_of(b"GET /.well-known/assetlinks.json HTTP/1.1\r\n\r\n"),
            Some(Asked::AssetLinks)
        );
        assert_eq!(
            asked_of(b"GET / HTTP/1.1\r\nUpgrade: websocket\r\n\r\n"),
            None
        );
        assert_eq!(asked_of(b"POST /s/ HTTP/1.1\r\n\r\n"), None);
        assert_eq!(asked_of(b"\xff\xfe"), None);
    }

    #[test]
    fn a_page_holds_no_invitation_and_hands_its_fragment_to_the_app() {
        let page = page_of(PARTY);

        assert!(page.contains("resonate://party/"));
        assert!(page.contains("location.hash"));
        assert!(!page.contains("@KIND@"));
        assert!(!page.contains("@HEADING@"));
    }

    #[test]
    fn a_page_hands_the_app_the_code_alone_and_never_a_query_of_the_fragment() {
        let page = page_of(PARTY);

        assert!(page.contains(r#"split(/[?&#]/)[0].replace(/[^A-Za-z0-9-]/g, "")"#));
        assert_eq!(
            page.matches("?server=").count(),
            1,
            "the page names the server it was served from, and nothing else does"
        );
    }

    #[test]
    fn no_certificate_named_means_no_app_links_are_claimed() {
        let absent = answer(Asked::AssetLinks, &[]);
        let named = answer(Asked::AssetLinks, &["AA:BB".to_owned()]);

        assert!(absent.starts_with(b"HTTP/1.1 404"));
        assert!(String::from_utf8_lossy(&named).contains("\"AA:BB\""));
        assert!(String::from_utf8_lossy(&named).contains(ANDROID_PACKAGE));
    }

    #[test]
    fn an_answer_says_how_long_it_is_and_that_the_connection_ends() {
        let answer = String::from_utf8(answer(Asked::Page(PARTY), &[])).expect("text");
        let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");

        assert!(head.contains(&format!("Content-Length: {}", body.len())));
        assert!(head.contains("Connection: close"));
        assert!(head.contains("no-referrer"));
    }
}
