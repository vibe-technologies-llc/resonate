use std::fmt::Write as _;

const DOCUMENT_TYPES: [&str; 3] = ["text/", "json", "xml"];

pub fn escaped(text: &str) -> String {
    text.bytes()
        .fold(String::with_capacity(text.len()), |mut written, byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                written.push(char::from(byte));
            } else {
                let _ = write!(written, "%{byte:02X}");
            }
            written
        })
}

pub fn is_a_document(mime: Option<&str>) -> bool {
    mime.is_some_and(|mime| {
        let mime = mime.to_ascii_lowercase();
        DOCUMENT_TYPES.iter().any(|kind| mime.contains(kind))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_escaped_byte_by_byte_but_for_the_unreserved() {
        assert_eq!(escaped("Echoes & more"), "Echoes%20%26%20more");
        assert_eq!(escaped("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(escaped("Sigur Rós"), "Sigur%20R%C3%B3s");
    }

    #[test]
    fn text_json_and_xml_are_documents_and_audio_is_not() {
        assert!(is_a_document(Some("Text/XML; charset=utf-8")));
        assert!(is_a_document(Some("application/json")));
        assert!(!is_a_document(Some("audio/flac")));
        assert!(!is_a_document(None));
    }
}
