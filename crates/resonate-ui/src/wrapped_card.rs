use std::sync::Arc;

use resonate_core::wrapped::{REVIEW_NAME, YearReview, spelled_listening, spelled_share};
use resvg::{tiny_skia, usvg};
use thiserror::Error;

const CARD_WIDTH: u32 = 1_080;

const CARD_HEIGHT: u32 = 1_350;

const CARD_MARGIN: u32 = 90;

const A_NAME_DRAWN_AT_MOST: usize = 34;

const FIRST_ROW_AT: u32 = 590;

const ROWS_APART: u32 = 132;

#[derive(Clone, Debug)]
pub(crate) struct CardColours {
    pub(crate) ground: u32,
    pub(crate) raised: u32,
    pub(crate) accent: u32,
    pub(crate) text: u32,
    pub(crate) muted: u32,
    pub(crate) family: String,
}

#[derive(Debug, Error)]
pub(crate) enum CardRefused {
    #[error("the card's drawing would not read")]
    Unreadable(#[source] usvg::Error),

    #[error("no canvas could be made for the card")]
    NoCanvas,

    #[error("the card would not encode as a picture")]
    Unencoded,
}

pub(crate) fn drawn(review: &YearReview, colours: &CardColours) -> String {
    let family = escaped(&colours.family);
    let eyebrow = escaped(&format!("{REVIEW_NAME} {}", review.year).to_uppercase());
    let listened = escaped(&spelled_listening(review.listened));
    let across = escaped(&format!(
        "{} songs · {} artists · {} albums",
        review.songs, review.artists, review.albums
    ));
    let rows: String = rows_of(review)
        .iter()
        .take(4)
        .enumerate()
        .map(|(at, (label, name))| {
            let y = FIRST_ROW_AT + at as u32 * ROWS_APART;

            format!(
                concat!(
                    r##"<text x="{margin}" y="{y}" font-size="28" font-weight="600" "##,
                    r##"fill="#{muted:06x}" letter-spacing="2">{label}</text>"##,
                    r##"<text x="{margin}" y="{name_y}" font-size="50" font-weight="600" "##,
                    r##"fill="#{text:06x}">{name}</text>"##,
                ),
                margin = CARD_MARGIN,
                y = y,
                name_y = y + 62,
                muted = colours.muted,
                text = colours.text,
                label = label,
                name = escaped(&cut(name)),
            )
        })
        .collect();
    let foot = review
        .formats
        .map(|formats| {
            format!(
                "{} lossless · {} above CD quality",
                spelled_share(formats.lossless_permille),
                spelled_share(formats.high_resolution_permille)
            )
        })
        .unwrap_or_default();

    format!(
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" "##,
            r##"viewBox="0 0 {width} {height}" font-family="{family}, sans-serif">"##,
            r##"<rect width="{width}" height="{height}" fill="#{ground:06x}"/>"##,
            r##"<rect width="{width}" height="18" fill="#{accent:06x}"/>"##,
            r##"<text x="{margin}" y="170" font-size="32" font-weight="700" "##,
            r##"fill="#{accent:06x}" letter-spacing="3">{eyebrow}</text>"##,
            r##"<text x="{margin}" y="320" font-size="118" font-weight="800" "##,
            r##"fill="#{text:06x}">{listened}</text>"##,
            r##"<text x="{margin}" y="392" font-size="52" font-weight="600" "##,
            r##"fill="#{text:06x}">of music</text>"##,
            r##"<text x="{margin}" y="462" font-size="32" fill="#{muted:06x}">{across}</text>"##,
            r##"<rect x="{margin}" y="508" width="{rule}" height="2" fill="#{raised:06x}"/>"##,
            "{rows}",
            r##"<text x="{margin}" y="1210" font-size="30" fill="#{muted:06x}">{foot}</text>"##,
            r##"<text x="{margin}" y="1278" font-size="34" font-weight="700" "##,
            r##"fill="#{accent:06x}">Resonate</text>"##,
            "</svg>"
        ),
        width = CARD_WIDTH,
        height = CARD_HEIGHT,
        margin = CARD_MARGIN,
        rule = CARD_WIDTH - 2 * CARD_MARGIN,
        family = family,
        ground = colours.ground,
        accent = colours.accent,
        raised = colours.raised,
        text = colours.text,
        muted = colours.muted,
        eyebrow = eyebrow,
        listened = listened,
        across = across,
        rows = rows,
        foot = escaped(&foot),
    )
}

pub(crate) fn pictured(review: &YearReview, colours: &CardColours) -> Result<Vec<u8>, CardRefused> {
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_system_fonts();

    png_of(&drawn(review, colours), fonts)
}

fn png_of(drawing: &str, fonts: usvg::fontdb::Database) -> Result<Vec<u8>, CardRefused> {
    let options = usvg::Options {
        fontdb: Arc::new(fonts),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(drawing, &options).map_err(CardRefused::Unreadable)?;
    let mut canvas =
        tiny_skia::Pixmap::new(CARD_WIDTH, CARD_HEIGHT).ok_or(CardRefused::NoCanvas)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut canvas.as_mut(),
    );

    canvas.encode_png().map_err(|error| {
        tracing::error!(%error, "the card would not encode");
        CardRefused::Unencoded
    })
}

fn rows_of(review: &YearReview) -> Vec<(&'static str, String)> {
    let named = |ranked: &[resonate_core::wrapped::Ranked]| {
        ranked.first().map(|leader| leader.name.clone())
    };

    [
        ("TOP SONG", named(&review.top_songs)),
        ("TOP ARTIST", named(&review.top_artists)),
        ("TOP ALBUM", named(&review.top_albums)),
        ("TOP GENRE", named(&review.top_genres)),
    ]
    .into_iter()
    .filter_map(|(label, name)| name.map(|name| (label, name)))
    .collect()
}

fn cut(name: &str) -> String {
    if name.chars().count() <= A_NAME_DRAWN_AT_MOST {
        return name.to_owned();
    }

    let kept: String = name.chars().take(A_NAME_DRAWN_AT_MOST - 1).collect();

    format!("{}…", kept.trim_end())
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::wrapped::{FormatShares, Ranked};

    use super::*;

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    fn colours() -> CardColours {
        CardColours {
            ground: 0x101014,
            raised: 0x24242c,
            accent: 0xb48ead,
            text: 0xf4f4f4,
            muted: 0xa0a0a8,
            family: "Inter".to_owned(),
        }
    }

    fn review() -> YearReview {
        YearReview {
            year: 2026,
            listened: Duration::from_secs(312 * 3_600),
            plays: 900,
            songs: 411,
            artists: 90,
            albums: 120,
            top_songs: vec![Ranked {
                name: "Songs & <Stories> \"Live\"".to_owned(),
                by: Some("Ada".to_owned()),
                plays: 9,
                listened: Duration::from_secs(3_600),
            }],
            formats: Some(FormatShares {
                lossless_permille: 874,
                high_resolution_permille: 120,
            }),
            ..YearReview::default()
        }
    }

    #[test]
    fn a_name_written_on_the_card_cannot_break_out_of_the_drawing() {
        let drawing = drawn(&review(), &colours());

        assert!(drawing.contains("Songs &amp; &lt;Stories&gt; &quot;Live&quot;"));
        assert!(!drawing.contains("<Stories>"));
        assert!(usvg::Tree::from_str(&drawing, &usvg::Options::default()).is_ok());
    }

    #[test]
    fn a_long_name_is_cut_on_a_letter_and_a_short_one_stands_whole() {
        let long = "ñ".repeat(A_NAME_DRAWN_AT_MOST + 9);

        assert_eq!(cut("Echoes"), "Echoes");
        assert_eq!(cut(&long).chars().count(), A_NAME_DRAWN_AT_MOST);
        assert!(cut(&long).ends_with('…'));
    }

    #[test]
    fn the_card_is_a_picture_of_the_size_a_message_carries() {
        let picture = png_of(&drawn(&review(), &colours()), usvg::fontdb::Database::new())
            .expect("a card is drawn");

        assert_eq!(picture[..8], PNG_SIGNATURE);
        assert_eq!(picture[16..20], CARD_WIDTH.to_be_bytes());
        assert_eq!(picture[20..24], CARD_HEIGHT.to_be_bytes());
    }

    #[test]
    fn a_year_of_songs_with_no_ranking_draws_only_what_it_holds() {
        let empty = YearReview {
            year: 2026,
            ..YearReview::default()
        };

        assert!(rows_of(&empty).is_empty());
        assert_eq!(rows_of(&review()).len(), 1);
    }
}
