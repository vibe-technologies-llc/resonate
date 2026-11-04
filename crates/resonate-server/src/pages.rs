const PAGE_REQUEST_READ_AT_MOST: usize = 2_048;

const SHARED_PLAYLIST: &str = "shared";

const PARTY: &str = "party";

const ASSET_LINKS_TARGET: &str = "/.well-known/assetlinks.json";

const ANDROID_PACKAGE: &str = "org.resonate.android";

const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>Resonate</title>
<style>
:root{color-scheme:light dark;--ground:#fff;--ink:#17151c;--faint:#5f5a69;--accent:#6d4aa0;--paper:#fff}
@media (prefers-color-scheme:dark){:root{--ground:#121016;--ink:#f3f1f6;--faint:#a9a3b4;--accent:#b79ae0;--paper:#17121f}}
body{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:var(--ground);color:var(--ink);font:16px/1.5 system-ui,sans-serif;padding:16px;box-sizing:border-box}
main{max-width:30rem}
h1{font-size:1.6rem;margin:0 0 .5rem}
p{margin:0 0 1rem;color:var(--faint)}
a.open{display:inline-block;padding:.7rem 1.2rem;border-radius:.6rem;background:var(--accent);color:var(--paper);font-weight:600;text-decoration:none}
code{display:block;margin-top:1.2rem;word-break:break-all;color:var(--faint)}
</style>
</head>
<body>
<main>
<h1>@HEADING@</h1>
<p>@SAYING@</p>
<a class="open" id="open" href="#">Open in Resonate</a>
<code id="code"></code>
<noscript><p>This page needs JavaScript to hand the invitation to Resonate.</p></noscript>
</main>
<script>
var code = location.hash.slice(1);
var server = "wss://" + location.host + "/";
document.getElementById("code").textContent = code;
document.getElementById("open").href =
  "resonate://@KIND@/" + code + "?server=" + encodeURIComponent(server);
</script>
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
    let (heading, saying) = if kind == PARTY {
        (
            "You're invited to a listening party",
            "Open it in Resonate to listen along. Everyone plays their own copy of each song, so nothing is sent between you.",
        )
    } else {
        (
            "A playlist was shared with you",
            "Open it in Resonate to follow it. The server keeps it sealed and cannot read it, so this page cannot show it.",
        )
    };

    PAGE.replace("@HEADING@", heading)
        .replace("@SAYING@", saying)
        .replace("@KIND@", kind)
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
         'unsafe-inline'\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

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
