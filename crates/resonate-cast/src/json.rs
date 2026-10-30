use std::fmt::Write;

const DEPTH_AT_MOST: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub(crate) fn object<const N: usize>(members: [(&str, Self); N]) -> Self {
        Self::Object(
            members
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub(crate) fn with(mut self, key: &str, value: Self) -> Self {
        if let Self::Object(members) = &mut self {
            members.push((key.to_owned(), value));
        }
        self
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(members) => members
                .iter()
                .find(|(named, _)| named == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub(crate) fn as_list(&self) -> Option<&[Self]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn kind(&self) -> Option<&str> {
        self.get("type").and_then(Self::as_str)
    }

    pub(crate) fn written(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out);
        out
    }

    fn write_into(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Number(number) if number.is_finite() => {
                let _ = write!(out, "{number}");
            }
            Self::Number(_) => out.push_str("null"),
            Self::Text(text) => quoted(text, out),
            Self::List(items) => {
                out.push('[');
                for (at, item) in items.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    item.write_into(out);
                }
                out.push(']');
            }
            Self::Object(members) => {
                out.push('{');
                for (at, (key, value)) in members.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    quoted(key, out);
                    out.push(':');
                    value.write_into(out);
                }
                out.push('}');
            }
        }
    }

    pub(crate) fn read(text: &str) -> Option<Self> {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            at: 0,
        };
        let value = reader.value(0)?;
        reader.blank();
        (reader.at == reader.bytes.len()).then_some(value)
    }
}

impl From<f64> for Json {
    fn from(number: f64) -> Self {
        Self::Number(number)
    }
}

impl From<u32> for Json {
    fn from(number: u32) -> Self {
        Self::Number(f64::from(number))
    }
}

impl From<bool> for Json {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

fn quoted(text: &str, out: &mut String) {
    out.push('"');
    for letter in text.chars() {
        match letter {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            letter if u32::from(letter) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(letter));
            }
            letter => out.push(letter),
        }
    }
    out.push('"');
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn blank(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, expected: &[u8]) -> Option<()> {
        let end = self.at.checked_add(expected.len())?;
        (self.bytes.get(self.at..end)? == expected).then(|| self.at = end)
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > DEPTH_AT_MOST {
            return None;
        }
        self.blank();
        match self.peek()? {
            b'n' => self.eat(b"null").map(|()| Json::Null),
            b't' => self.eat(b"true").map(|()| Json::Bool(true)),
            b'f' => self.eat(b"false").map(|()| Json::Bool(false)),
            b'"' => self.string().map(Json::Text),
            b'[' => self.list(depth),
            b'{' => self.object(depth),
            _ => self.number(),
        }
    }

    fn list(&mut self, depth: usize) -> Option<Json> {
        self.eat(b"[")?;
        let mut items = Vec::new();
        self.blank();
        if self.eat(b"]").is_some() {
            return Some(Json::List(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.blank();
            if self.eat(b",").is_some() {
                continue;
            }
            self.eat(b"]")?;
            return Some(Json::List(items));
        }
    }

    fn object(&mut self, depth: usize) -> Option<Json> {
        self.eat(b"{")?;
        let mut members = Vec::new();
        self.blank();
        if self.eat(b"}").is_some() {
            return Some(Json::Object(members));
        }
        loop {
            self.blank();
            let key = self.string()?;
            self.blank();
            self.eat(b":")?;
            members.push((key, self.value(depth + 1)?));
            self.blank();
            if self.eat(b",").is_some() {
                continue;
            }
            self.eat(b"}")?;
            return Some(Json::Object(members));
        }
    }

    fn number(&mut self) -> Option<Json> {
        let from = self.at;
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
        ) {
            self.at += 1;
        }
        let spelled = std::str::from_utf8(self.bytes.get(from..self.at)?).ok()?;
        spelled.parse().ok().map(Json::Number)
    }

    fn string(&mut self) -> Option<String> {
        self.eat(b"\"")?;
        let mut text = Vec::new();
        loop {
            let byte = self.peek()?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(text).ok(),
                b'\\' => {
                    let escaped = self.peek()?;
                    self.at += 1;
                    let letter = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode()?,
                        _ => return None,
                    };
                    let mut encoded = [0; 4];
                    text.extend_from_slice(letter.encode_utf8(&mut encoded).as_bytes());
                }
                byte if byte < 0x20 => return None,
                byte => text.push(byte),
            }
        }
    }

    fn unicode(&mut self) -> Option<char> {
        let high = self.four_hex()?;
        if !(0xD800..0xDC00).contains(&high) {
            return char::from_u32(high);
        }
        self.eat(b"\\u")?;
        let low = self.four_hex()?;
        if !(0xDC00..0xE000).contains(&low) {
            return None;
        }
        char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))
    }

    fn four_hex(&mut self) -> Option<u32> {
        let end = self.at.checked_add(4)?;
        let spelled = std::str::from_utf8(self.bytes.get(self.at..end)?).ok()?;
        self.at = end;
        u32::from_str_radix(spelled, 16).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back_escapes_and_all() {
        let written = Json::object([
            ("type", Json::text("LOAD")),
            ("requestId", 7_u32.into()),
            ("level", 0.25.into()),
            ("autoplay", true.into()),
            ("title", Json::text("Songs \"&\" \\ <Sounds>\n\u{1}")),
            (
                "list",
                Json::List(vec![Json::Null, Json::Object(Vec::new())]),
            ),
        ]);
        let spelled = written.written();

        assert!(spelled.starts_with(r#"{"type":"LOAD","requestId":7,"level":0.25,"#));
        assert_eq!(Json::read(&spelled), Some(written));
    }

    #[test]
    fn a_receiver_status_is_read_through_its_nesting_and_surrogates() {
        let read = Json::read(
            r#" {"type": "RECEIVER_STATUS", "requestId": 3,
                "status": {"volume": {"level": 4.0E-1, "muted": false},
                "applications": [{"appId": "CC1AD845", "displayName": "Default \ud83c\udfb5 \u00e9"}]}} "#,
        )
        .expect("reads");
        let status = read.get("status").expect("a status");
        let application = &status
            .get("applications")
            .and_then(Json::as_list)
            .expect("a list")[0];

        assert_eq!(read.kind(), Some("RECEIVER_STATUS"));
        assert_eq!(
            status
                .get("volume")
                .and_then(|volume| volume.get("level"))
                .and_then(Json::as_number),
            Some(0.4)
        );
        assert_eq!(
            application.get("displayName").and_then(Json::as_str),
            Some("Default \u{1f3b5} \u{e9}")
        );
    }

    #[test]
    fn what_is_not_json_or_nests_too_deep_is_refused() {
        let deep = "[".repeat(DEPTH_AT_MOST + 2) + &"]".repeat(DEPTH_AT_MOST + 2);

        assert_eq!(Json::read("{\"a\":1,}"), None);
        assert_eq!(Json::read("{\"a\":1} trailing"), None);
        assert_eq!(Json::read("\"\\ud800\""), None);
        assert_eq!(Json::read(&deep), None);
    }
}
