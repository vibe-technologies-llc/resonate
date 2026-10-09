use base64::{Engine as _, engine::general_purpose::STANDARD};
use roxmltree::{Document, Node};
use serde::Deserialize;

const BTS: &str = "application/vnd.tidal.bts";
const DASH: &str = "application/dash+xml";
const FLAC_CODEC: &str = "flac";
const FLAC_TYPE: &str = "audio/flac";
const MP4_TYPE: &str = "audio/mp4";
const UNENCRYPTED: &str = "NONE";
const SEGMENTS_AT_MOST: u64 = 20_000;
const FIRST_NUMBER: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Container {
    Flac,
    Mp4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Timeline {
    pub(crate) ticks: u64,
    pub(crate) timescale: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Media {
    pub(crate) container: Container,
    pub(crate) urls: Vec<String>,
    pub(crate) timeline: Option<Timeline>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Withheld {
    Encrypted,
    Lossy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Manifest {
    Media(Media),
    Withheld(Withheld),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unread {
    NotBase64,
    UnknownType,
    NotTheDocument,
    NoMedia,
    TooManySegments,
    NumberedPastTheEnd,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bts {
    mime_type: String,
    #[serde(default)]
    codecs: String,
    #[serde(default)]
    encryption_type: Option<String>,
    #[serde(default)]
    urls: Vec<String>,
}

pub(crate) fn read(mime: &str, encoded: &str) -> Result<Manifest, Unread> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|_| Unread::NotBase64)?;
    read_document(mime, &bytes)
}

pub(crate) fn read_document(mime: &str, bytes: &[u8]) -> Result<Manifest, Unread> {
    match mime.trim() {
        BTS => bts(bytes),
        DASH => dash(bytes),
        _ => Err(Unread::UnknownType),
    }
}

fn is_flac(codecs: &str) -> bool {
    codecs
        .split(',')
        .map(str::trim)
        .any(|codec| codec.eq_ignore_ascii_case(FLAC_CODEC))
}

fn bts(bytes: &[u8]) -> Result<Manifest, Unread> {
    let bts: Bts = serde_json::from_slice(bytes).map_err(|_| Unread::NotTheDocument)?;
    if bts
        .encryption_type
        .as_deref()
        .is_some_and(|kind| !kind.eq_ignore_ascii_case(UNENCRYPTED))
    {
        return Ok(Manifest::Withheld(Withheld::Encrypted));
    }
    if !is_flac(&bts.codecs) {
        return Ok(Manifest::Withheld(Withheld::Lossy));
    }
    let container = match bts.mime_type.trim().to_ascii_lowercase().as_str() {
        FLAC_TYPE => Container::Flac,
        MP4_TYPE => Container::Mp4,
        _ => return Err(Unread::UnknownType),
    };
    if bts.urls.is_empty() {
        return Err(Unread::NoMedia);
    }
    Ok(Manifest::Media(Media {
        container,
        urls: bts.urls,
        timeline: None,
    }))
}

fn first<'a, 'input>(document: &'a Document<'input>, name: &str) -> Option<Node<'a, 'input>> {
    document
        .descendants()
        .find(|node| node.tag_name().name() == name)
}

fn inherited<'a>(node: Node<'a, '_>, attribute: &str) -> Option<&'a str> {
    node.ancestors().find_map(|held| held.attribute(attribute))
}

fn numbered(attribute: Option<&str>) -> Result<Option<u64>, Unread> {
    attribute
        .map(|text| text.trim().parse().map_err(|_| Unread::NotTheDocument))
        .transpose()
}

struct Named<'a> {
    representation: &'a str,
    bandwidth: &'a str,
    number: u64,
    time: u64,
}

fn padded_width(format: Option<&str>) -> Option<usize> {
    format?.strip_prefix('0')?.strip_suffix('d')?.parse().ok()
}

fn filled(template: &str, named: &Named<'_>) -> String {
    let mut written = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find('$') {
        written.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let Some(end) = after.find('$') else {
            written.push_str(&rest[at..]);
            return written;
        };
        let identifier = &after[..end];
        let (name, format) = identifier
            .split_once('%')
            .map_or((identifier, None), |(name, format)| (name, Some(format)));
        let value = match name {
            "" => Some("$".to_owned()),
            "RepresentationID" => Some(named.representation.to_owned()),
            "Bandwidth" => Some(named.bandwidth.to_owned()),
            "Number" => Some(named.number.to_string()),
            "Time" => Some(named.time.to_string()),
            _ => None,
        };
        match value {
            Some(value) => {
                let width = padded_width(format).unwrap_or(0);
                written.push_str(&format!("{value:0>width$}"));
            }
            None => written.push_str(&rest[at..at + end + 2]),
        }
        rest = &after[end + 1..];
    }
    written.push_str(rest);
    written
}

fn resolved(base: Option<&str>, url: String) -> Result<String, Unread> {
    if url.contains("://") {
        return Ok(url);
    }
    let (scheme, rest) = base
        .map(str::trim)
        .and_then(|base| base.split_once("://"))
        .ok_or(Unread::NoMedia)?;
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if let Some(elsewhere) = url.strip_prefix("//") {
        return Ok(format!("{scheme}://{elsewhere}"));
    }
    if url.starts_with('/') {
        return Ok(format!("{scheme}://{authority}{url}"));
    }
    match path.rsplit_once('/') {
        Some((folder, _)) => Ok(format!("{scheme}://{authority}/{folder}/{url}")),
        None => Ok(format!("{scheme}://{authority}/{url}")),
    }
}

fn base_of(representation: Node<'_, '_>) -> Option<String> {
    let mut named: Vec<&str> = representation
        .ancestors()
        .filter_map(|node| {
            node.children()
                .find(|child| child.tag_name().name() == "BaseURL")
                .and_then(|base| base.text())
        })
        .collect();
    named.reverse();
    named.into_iter().fold(None, |base, url| {
        resolved(base.as_deref(), url.trim().to_owned())
            .ok()
            .or(base)
    })
}

fn dash(bytes: &[u8]) -> Result<Manifest, Unread> {
    let text = std::str::from_utf8(bytes).map_err(|_| Unread::NotTheDocument)?;
    let document = Document::parse(text).map_err(|_| Unread::NotTheDocument)?;
    if first(&document, "ContentProtection").is_some() {
        return Ok(Manifest::Withheld(Withheld::Encrypted));
    }

    let representations: Vec<_> = document
        .descendants()
        .filter(|node| node.tag_name().name() == "Representation")
        .collect();
    let Some(representation) = representations
        .iter()
        .copied()
        .filter(|node| is_flac(inherited(*node, "codecs").unwrap_or_default()))
        .max_by_key(|node| {
            numbered(node.attribute("bandwidth"))
                .ok()
                .flatten()
                .unwrap_or_default()
        })
    else {
        return if representations.is_empty() {
            Err(Unread::NoMedia)
        } else {
            Ok(Manifest::Withheld(Withheld::Lossy))
        };
    };
    let template = representation
        .descendants()
        .chain(representation.ancestors())
        .find(|node| node.tag_name().name() == "SegmentTemplate")
        .ok_or(Unread::NoMedia)?;
    let initialization = template
        .attribute("initialization")
        .ok_or(Unread::NoMedia)?;
    let media = template.attribute("media").ok_or(Unread::NoMedia)?;
    let start = numbered(template.attribute("startNumber"))?.unwrap_or(FIRST_NUMBER);
    let timescale = numbered(template.attribute("timescale"))?;

    let mut segments = 0_u64;
    let mut ticks = 0_u64;
    let mut at = 0_u64;
    let mut starts = Vec::new();
    for step in template
        .descendants()
        .filter(|node| node.tag_name().name() == "S")
    {
        if let Some(stated) = numbered(step.attribute("t"))? {
            at = stated;
        }
        let lasting = numbered(step.attribute("d"))?.ok_or(Unread::NotTheDocument)?;
        let repeated = numbered(step.attribute("r"))?.unwrap_or(0);
        let count = repeated.checked_add(1).ok_or(Unread::TooManySegments)?;
        segments = segments
            .checked_add(count)
            .filter(|segments| *segments <= SEGMENTS_AT_MOST)
            .ok_or(Unread::TooManySegments)?;
        ticks = lasting
            .checked_mul(count)
            .and_then(|lasted| ticks.checked_add(lasted))
            .ok_or(Unread::TooManySegments)?;
        for _ in 0..count {
            starts.push(at);
            at = at.checked_add(lasting).ok_or(Unread::TooManySegments)?;
        }
    }
    if segments == 0 {
        return Err(Unread::NoMedia);
    }

    let representation_id = representation.attribute("id").unwrap_or_default();
    let bandwidth = representation.attribute("bandwidth").unwrap_or_default();
    let base = base_of(representation);
    let named = |number, time| Named {
        representation: representation_id,
        bandwidth,
        number,
        time,
    };
    let mut urls = Vec::with_capacity(starts.len() + 1);
    urls.push(resolved(
        base.as_deref(),
        filled(initialization, &named(start, 0)),
    )?);
    start
        .checked_add(segments)
        .ok_or(Unread::NumberedPastTheEnd)?;
    for (number, time) in (start..).zip(starts) {
        urls.push(resolved(
            base.as_deref(),
            filled(media, &named(number, time)),
        )?);
    }

    Ok(Manifest::Media(Media {
        container: Container::Mp4,
        urls,
        timeline: timescale.map(|timescale| Timeline { ticks, timescale }),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DASHED: &str = include_str!("../tests/fixtures/dash.mpd");
    const PROTECTED: &str = include_str!("../tests/fixtures/protected.mpd");

    fn encoded(text: &str) -> String {
        STANDARD.encode(text)
    }

    #[test]
    fn a_bts_manifest_names_one_flac_file() {
        let manifest = read(
            BTS,
            &encoded(
                r#"{"mimeType":"audio/flac","codecs":"flac","encryptionType":"NONE","urls":["https://lgf.audio.tidal.com/mediatracks/a.flac"]}"#,
            ),
        );

        assert_eq!(
            manifest,
            Ok(Manifest::Media(Media {
                container: Container::Flac,
                urls: vec!["https://lgf.audio.tidal.com/mediatracks/a.flac".to_owned()],
                timeline: None,
            }))
        );
    }

    #[test]
    fn an_encrypted_or_lossy_bts_manifest_is_withheld() {
        let encrypted = read(
            BTS,
            &encoded(
                r#"{"mimeType":"audio/flac","codecs":"flac","encryptionType":"OLD_AES","keyId":"k","urls":["https://a.audio.tidal.com/x"]}"#,
            ),
        );
        let lossy = read(
            BTS,
            &encoded(
                r#"{"mimeType":"audio/mp4","codecs":"mp4a.40.2","encryptionType":"NONE","urls":["https://a.audio.tidal.com/x"]}"#,
            ),
        );

        assert_eq!(encrypted, Ok(Manifest::Withheld(Withheld::Encrypted)));
        assert_eq!(lossy, Ok(Manifest::Withheld(Withheld::Lossy)));
    }

    #[test]
    fn a_dash_manifest_names_its_initialization_and_every_segment_of_its_timeline() {
        let Ok(Manifest::Media(media)) = read(DASH, &encoded(DASHED)) else {
            panic!("the manifest was not read as media");
        };

        assert_eq!(media.container, Container::Mp4);
        assert_eq!(media.urls.len(), 1 + 3 + 1);
        assert_eq!(
            media.urls[0],
            "https://sp-ad-fa.audio.tidal.com/mediatracks/abc/0.mp4?token=t&x=1"
        );
        assert_eq!(
            media.urls[1],
            "https://sp-ad-fa.audio.tidal.com/mediatracks/abc/1.mp4?token=t&x=1"
        );
        assert_eq!(
            media.urls[4],
            "https://sp-ad-fa.audio.tidal.com/mediatracks/abc/4.mp4?token=t&x=1"
        );
        assert_eq!(
            media.timeline,
            Some(Timeline {
                ticks: 3 * 176_128 + 12_345,
                timescale: 44_100,
            })
        );
    }

    #[test]
    fn a_dash_manifest_fetched_as_xml_is_read_without_base64() {
        assert_eq!(
            read_document(DASH, DASHED.as_bytes()),
            dash(DASHED.as_bytes())
        );
    }

    #[test]
    fn a_dash_manifest_numbered_past_the_last_number_is_refused() {
        let numbered = DASHED.replace(
            r#"startNumber="1""#,
            &format!(r#"startNumber="{}""#, u64::MAX - 1),
        );
        assert_ne!(numbered, DASHED);

        assert!(matches!(
            read(DASH, &encoded(&numbered)),
            Err(Unread::NumberedPastTheEnd)
        ));
    }

    #[test]
    fn a_dash_manifest_chooses_flac_after_lossy_representations() {
        let alternatives = DASHED
            .replace(
            r#"<Representation id="FLAC,44100,16" codecs="flac" bandwidth="1004547" audioSamplingRate="44100">"#,
            r#"<Representation id="AACLC" codecs="mp4a.40.2" bandwidth="321750" audioSamplingRate="44100"/><Representation id="FLAC,44100,16" codecs="flac" bandwidth="1004547" audioSamplingRate="44100">"#,
            )
            .replace(
                "</AdaptationSet>",
                r#"<Representation id="FLAC_HIRES,96000,24" codecs="flac" bandwidth="1730302" audioSamplingRate="96000"><SegmentTemplate timescale="96000" initialization="https://sp-ad-fa.audio.tidal.com/mediatracks/high/0.mp4" media="https://sp-ad-fa.audio.tidal.com/mediatracks/high/$Number$.mp4" startNumber="1"><SegmentTimeline><S d="384000" r="2"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet>"#,
            );
        let Ok(Manifest::Media(media)) = read(DASH, &encoded(&alternatives)) else {
            panic!("the lossless representation was not read as media");
        };

        assert_eq!(
            media.urls[0],
            "https://sp-ad-fa.audio.tidal.com/mediatracks/high/0.mp4"
        );
    }

    #[test]
    fn a_dash_manifest_with_only_lossy_representations_is_withheld() {
        let lossy = DASHED.replace("codecs=\"flac\"", "codecs=\"mp4a.40.2\"");

        assert_eq!(
            read(DASH, &encoded(&lossy)),
            Ok(Manifest::Withheld(Withheld::Lossy))
        );
    }

    #[test]
    fn a_protected_dash_manifest_is_withheld() {
        assert_eq!(
            read(DASH, &encoded(PROTECTED)),
            Ok(Manifest::Withheld(Withheld::Encrypted))
        );
    }

    #[test]
    fn a_timeline_past_any_track_is_refused() {
        let endless = DASHED.replace(r#"r="2""#, r#"r="99999999""#);

        assert_eq!(read(DASH, &encoded(&endless)), Err(Unread::TooManySegments));
    }

    #[test]
    fn a_relative_segment_is_read_against_the_base_url() {
        assert_eq!(
            resolved(
                Some("https://sp-ad-cf.audio.tidal.com/mediatracks/abc/manifest.mpd"),
                "3.mp4".to_owned()
            ),
            Ok("https://sp-ad-cf.audio.tidal.com/mediatracks/abc/3.mp4".to_owned())
        );
        assert_eq!(resolved(None, "3.mp4".to_owned()), Err(Unread::NoMedia));
    }

    #[test]
    fn a_base_url_is_resolved_as_a_uri_reference_whatever_its_trailing_slash() {
        assert_eq!(
            resolved(Some("https://cdn.audio.tidal.com"), "3.mp4".to_owned()),
            Ok("https://cdn.audio.tidal.com/3.mp4".to_owned())
        );
        assert_eq!(
            resolved(Some("https://cdn.audio.tidal.com/a/b/"), "3.mp4".to_owned()),
            Ok("https://cdn.audio.tidal.com/a/b/3.mp4".to_owned())
        );
        assert_eq!(
            resolved(
                Some("https://cdn.audio.tidal.com/a/b?k=/x"),
                "3.mp4".to_owned()
            ),
            Ok("https://cdn.audio.tidal.com/a/3.mp4".to_owned())
        );
        assert_eq!(
            resolved(
                Some("https://cdn.audio.tidal.com/a/b/"),
                "/c/3.mp4".to_owned()
            ),
            Ok("https://cdn.audio.tidal.com/c/3.mp4".to_owned())
        );
        assert_eq!(
            resolved(
                Some("https://cdn.audio.tidal.com/a/"),
                "//b.audio.tidal.com/3.mp4".to_owned()
            ),
            Ok("https://b.audio.tidal.com/3.mp4".to_owned())
        );
    }

    #[test]
    fn a_template_fills_time_and_padded_numbers_and_leaves_what_it_does_not_know() {
        let named = Named {
            representation: "FLAC",
            bandwidth: "1004547",
            number: 7,
            time: 352_256,
        };

        assert_eq!(
            filled(
                "$RepresentationID$/$Number%05d$-$Time$-$Bandwidth$$$.mp4",
                &named
            ),
            "FLAC/00007-352256-1004547$.mp4"
        );
        assert_eq!(filled("$Unknown$/$Number", &named), "$Unknown$/$Number");
    }

    #[test]
    fn a_dash_manifest_timed_by_its_segments_names_each_by_the_tick_it_starts_at() {
        let manifest = r#"<MPD><BaseURL>https://cdn.audio.tidal.com/tracks/</BaseURL><Period><AdaptationSet><BaseURL>abc/</BaseURL><Representation id="FLAC" codecs="flac" bandwidth="1"><SegmentTemplate timescale="44100" initialization="init.mp4" media="$Time$.mp4"><SegmentTimeline><S t="100" d="10" r="1"/><S d="5"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;

        let Ok(Manifest::Media(media)) = read_document(DASH, manifest.as_bytes()) else {
            panic!("a manifest");
        };

        assert_eq!(
            media.urls,
            [
                "https://cdn.audio.tidal.com/tracks/abc/init.mp4",
                "https://cdn.audio.tidal.com/tracks/abc/100.mp4",
                "https://cdn.audio.tidal.com/tracks/abc/110.mp4",
                "https://cdn.audio.tidal.com/tracks/abc/120.mp4",
            ]
        );
    }

    #[test]
    fn what_is_not_a_manifest_is_unread() {
        assert_eq!(read(BTS, "%%%"), Err(Unread::NotBase64));
        assert_eq!(
            read("application/vnd.tidal.emu", &encoded("{}")),
            Err(Unread::UnknownType)
        );
        assert_eq!(read(DASH, &encoded("<MPD")), Err(Unread::NotTheDocument));
    }
}
