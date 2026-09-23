use std::{fmt, sync::Arc, time::Duration};

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chromaprint {
    encoded: Arc<str>,
    length: Duration,
}

impl Chromaprint {
    pub fn new(encoded: &str, length: Duration) -> Result<Self> {
        if encoded.is_empty() || !encoded.bytes().all(url_safe) {
            return Err(Error::NotAChromaprint);
        }

        Ok(Self {
            encoded: Arc::from(encoded),
            length,
        })
    }

    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    pub const fn length(&self) -> Duration {
        self.length
    }
}

const fn url_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

impl fmt::Display for Chromaprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_print_is_url_safe_base64_and_nothing_else() {
        let length = Duration::from_secs(215);
        let print = Chromaprint::new("AQADtEmUaEkS-_x", length).expect("a print");
        assert_eq!(print.encoded(), "AQADtEmUaEkS-_x");
        assert_eq!(print.length(), length);

        assert_eq!(Chromaprint::new("", length), Err(Error::NotAChromaprint));
        assert_eq!(
            Chromaprint::new("AQAD+/", length),
            Err(Error::NotAChromaprint)
        );
        assert_eq!(
            Chromaprint::new("AQAD==", length),
            Err(Error::NotAChromaprint)
        );
    }
}
