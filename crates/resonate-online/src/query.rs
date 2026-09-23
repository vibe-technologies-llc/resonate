use std::fmt::Write;

const HEX: &[u8; 16] = b"0123456789ABCDEF";

pub(crate) fn escape_query(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            escaped.push(char::from(byte));
        } else {
            escaped.push('%');
            escaped.push(char::from(HEX[usize::from(byte >> 4)]));
            escaped.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }

    escaped
}

pub(crate) fn escape_path(path: &str) -> String {
    path.split('/')
        .map(escape_query)
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn lucene_quoted(term: &str) -> String {
    let mut quoted = String::with_capacity(term.len() + 2);
    quoted.push('"');
    for character in term.chars() {
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');

    quoted
}

pub(crate) struct Params(String);

impl Params {
    pub(crate) fn new() -> Self {
        Self(String::new())
    }

    pub(crate) fn with(mut self, name: &str, value: &str) -> Self {
        self.0.push(if self.0.is_empty() { '?' } else { '&' });
        let _ = write!(self.0, "{name}={}", escape_query(value));
        self
    }

    pub(crate) fn maybe(self, name: &str, value: Option<&str>) -> Self {
        match value {
            Some(value) => self.with(name, value),
            None => self,
        }
    }

    pub(crate) fn finish(self) -> String {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lucene_term_is_quoted_and_a_query_value_is_percent_escaped() {
        assert_eq!(lucene_quoted("Meddle"), "\"Meddle\"");
        assert_eq!(
            lucene_quoted("Say \"hi\" \\ wave"),
            "\"Say \\\"hi\\\" \\\\ wave\""
        );

        assert_eq!(escape_query("Pink Floyd"), "Pink%20Floyd");
        assert_eq!(
            escape_query("release:\"Meddle\" AND tracks:6"),
            "release%3A%22Meddle%22%20AND%20tracks%3A6"
        );
        assert_eq!(escape_query("a-b.c_d~e"), "a-b.c_d~e");
        assert_eq!(escape_query("Ø/&+"), "%C3%98%2F%26%2B");
    }

    #[test]
    fn params_are_joined_with_an_ampersand_and_an_absent_one_is_left_out() {
        let query = Params::new()
            .with("track_name", "Echoes")
            .maybe("album_name", None)
            .maybe("artist_name", Some("Pink Floyd"))
            .finish();

        assert_eq!(query, "?track_name=Echoes&artist_name=Pink%20Floyd");
        assert_eq!(Params::new().finish(), "");
    }
}
