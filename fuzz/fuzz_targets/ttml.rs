#![no_main]

mod bounded;

use libfuzzer_sys::fuzz_target;

const NESTED_PARAGRAPHS_AT_MOST: usize = 64;
const NESTABLE: [&str; 3] = ["p", "span", "div"];
const HEADER_BYTES: usize = 3;

fn nested(element: &str, levels: usize) -> String {
    format!(
        "<tt xmlns=\"http://www.w3.org/ns/ttml\"><body><div><p begin=\"1.0\">{}x{}</p></div></body></tt>",
        format!("<{element}>").repeat(levels),
        format!("</{element}>").repeat(levels),
    )
}

fn document_of(data: &[u8]) -> String {
    let element = data
        .first()
        .and_then(|&kind| NESTABLE.get(usize::from(kind)));

    match (element, data.get(1..HEADER_BYTES)) {
        (Some(element), Some(&[low, high])) => {
            nested(element, usize::from(u16::from_le_bytes([low, high])))
        }
        _ => String::from_utf8_lossy(data).into_owned(),
    }
}

fuzz_target!(|data: &[u8]| {
    let document = document_of(data);

    if let Some(lyrics) = resonate_lyrics::read_a_ttml(&document) {
        bounded::within(
            &lyrics,
            document.len(),
            document.len() * NESTED_PARAGRAPHS_AT_MOST,
        );
    }
});
