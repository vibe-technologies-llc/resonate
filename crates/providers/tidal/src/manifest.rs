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

fn filled(template: &str, representation: &str, bandwidth: &str, number: u64) -> String {
    template
        .replace("$RepresentationID$", representation)
        .replace("$Bandwidth$", bandwidth)
        .replace("$Number$", &number.to_string())
        .replace("$$", "$")
}

fn resolved(base: Option<&str>, url: String) -> Result<String, Unread> {
    if url.contains("://") {
        return Ok(url);
    }
    let base = base
        .map(str::trim)
        .filter(|base| base.contains("://"))
        .ok_or(Unread::NoMedia)?;
    let folder = base.rsplit_once('/').map_or(base, |(folder, _)| folder);
    Ok(format!("{folder}/{}", url.trim_start_matches('/')))
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
    for step in template
        .descendants()
        .filter(|node| node.tag_name().name() == "S")
    {
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
    }
    if segments == 0 {
        return Err(Unread::NoMedia);
    }

    let id = representation.attribute("id").unwrap_or_default();
    let bandwidth = representation.attribute("bandwidth").unwrap_or_default();
    let base = first(&document, "BaseURL").and_then(|node| node.text());
    let mut urls = Vec::with_capacity(usize::try_from(segments + 1).unwrap_or_default());
    urls.push(resolved(
        base,
        filled(initialization, id, bandwidth, start),
    )?);
    for number in start..start + segments {
        urls.push(resolved(base, filled(media, id, bandwidth, number))?);
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
    fn what_is_not_a_manifest_is_unread() {
        assert_eq!(read(BTS, "%%%"), Err(Unread::NotBase64));
        assert_eq!(
            read("application/vnd.tidal.emu", &encoded("{}")),
            Err(Unread::UnknownType)
        );
        assert_eq!(read(DASH, &encoded("<MPD")), Err(Unread::NotTheDocument));
    }
}
