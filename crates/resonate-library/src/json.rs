const DEPTH_AT_MOST: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Whole(u64),
    Number(f64),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub(crate) fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(members) => members
                .iter()
                .find(|(named, _)| named == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub(crate) fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) const fn as_whole(&self) -> Option<u64> {
        match self {
            Self::Whole(whole) => Some(*whole),
            _ => None,
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

        if spelled.bytes().all(|byte| byte.is_ascii_digit())
            && let Ok(whole) = spelled.parse()
        {
            return Some(Json::Whole(whole));
        }
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
    fn a_listen_is_read_through_its_nesting_and_surrogates() {
        let read = Json::read(
            r#" {"listened_at": 1700000000, "track_metadata": {"track_name": "Say \ud83c\udfb5 \u00e9", "n": [1, -2.5e1, null, true]}} "#,
        )
        .expect("reads");
        let metadata = read.get("track_metadata").expect("metadata");

        assert_eq!(
            read.get("listened_at").and_then(Json::as_whole),
            Some(1_700_000_000)
        );
        assert_eq!(
            metadata.get("track_name").and_then(Json::as_str),
            Some("Say \u{1f3b5} \u{e9}")
        );
        assert!(metadata.is_object());
    }

    #[test]
    fn what_is_not_json_or_nests_too_deep_is_refused() {
        let deep = "[".repeat(DEPTH_AT_MOST + 2) + &"]".repeat(DEPTH_AT_MOST + 2);

        assert_eq!(Json::read("{\"a\":1,}"), None);
        assert_eq!(Json::read("{\"a\":1} trailing"), None);
        assert_eq!(Json::read("\"\\ud800\""), None);
        assert_eq!(Json::read("\"a\nb\""), None);
        assert_eq!(Json::read(&deep), None);
    }

    #[test]
    fn a_number_that_is_not_a_whole_one_is_not_whole() {
        assert_eq!(Json::read("-1").and_then(|json| json.as_whole()), None);
        assert_eq!(Json::read("1.5").and_then(|json| json.as_whole()), None);
        assert_eq!(
            Json::read("99999999999999999999").and_then(|json| json.as_whole()),
            None
        );
    }
}
