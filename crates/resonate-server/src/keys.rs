use std::{fs, net::SocketAddr, path::Path};

use ring::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use toml_edit::{DocumentMut, Table};

use crate::{ConfigField, Error, Result};

const SECRET_BYTES: usize = 32;
const DIGEST_BYTES: usize = 32;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest([u8; DIGEST_BYTES]);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keys(Vec<Digest>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hifi {
    pub listen: SocketAddr,
    pub upstream: SocketAddr,
    pub keys: Keys,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Minted {
    pub secret: String,
    pub digest: Digest,
}

impl Digest {
    pub fn of(secret: &[u8]) -> Self {
        let mut digested = [0; DIGEST_BYTES];
        digested.copy_from_slice(digest(&SHA256, secret).as_ref());
        Self(digested)
    }

    pub fn read(text: &str) -> Option<Self> {
        let bytes = text.trim().as_bytes();
        if bytes.len() != DIGEST_BYTES * 2 {
            return None;
        }
        let mut digested = [0; DIGEST_BYTES];
        let (pairs, _) = bytes.as_chunks::<2>();
        for (slot, [high, low]) in digested.iter_mut().zip(pairs) {
            *slot = nibble(*high)? << 4 | nibble(*low)?;
        }
        Some(Self(digested))
    }

    pub fn hex(&self) -> String {
        hex(&self.0)
    }
}

impl Keys {
    pub fn new(digests: Vec<Digest>) -> Self {
        Self(digests)
    }

    pub fn admits(&self, secret: &[u8]) -> bool {
        let presented = Digest::of(secret);
        self.0.contains(&presented)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Hifi {
    pub fn read(path: &Path) -> Result<Option<Self>> {
        let text = fs::read_to_string(path).map_err(|source| Error::ConfigRead {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(path, &text)
    }

    pub fn parse(path: &Path, text: &str) -> Result<Option<Self>> {
        let document: DocumentMut = text.parse().map_err(|source| Error::ConfigSyntax {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        let Some(item) = document.get("hifi") else {
            return Ok(None);
        };
        let table = item.as_table().ok_or(Error::ConfigInvalid {
            field: ConfigField::Hifi,
        })?;
        let keys = keys_of(table)?;
        if keys.is_empty() {
            return Err(Error::ConfigMissing {
                field: ConfigField::Keys,
            });
        }
        Ok(Some(Self {
            listen: address_of(table, "listen", ConfigField::Listen)?,
            upstream: address_of(table, "upstream", ConfigField::Upstream)?,
            keys,
        }))
    }
}

impl Minted {
    pub fn new() -> Result<Self> {
        let mut secret = [0; SECRET_BYTES];
        SystemRandom::new()
            .fill(&mut secret)
            .map_err(|_| Error::Random)?;
        let secret = hex(&secret);
        let digest = Digest::of(secret.as_bytes());
        Ok(Self { secret, digest })
    }
}

fn address_of(table: &Table, name: &str, field: ConfigField) -> Result<SocketAddr> {
    table
        .get(name)
        .ok_or(Error::ConfigMissing { field })?
        .as_str()
        .and_then(|text| text.parse().ok())
        .ok_or(Error::ConfigInvalid { field })
}

fn keys_of(table: &Table) -> Result<Keys> {
    let Some(item) = table.get("keys") else {
        return Err(Error::ConfigMissing {
            field: ConfigField::Keys,
        });
    };
    let listed = item
        .as_array()
        .ok_or(Error::ConfigInvalid {
            field: ConfigField::Keys,
        })?
        .iter()
        .map(|entry| entry.as_str().and_then(Digest::read))
        .collect::<Option<Vec<_>>>()
        .ok_or(Error::ConfigInvalid {
            field: ConfigField::Keys,
        })?;
    Ok(Keys::new(listed))
}

fn nibble(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELD: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn configured(keys: &str) -> String {
        format!("[hifi]\nlisten = \"[::]:443\"\nupstream = \"127.0.0.1:8000\"\nkeys = {keys}\n")
    }

    fn parsed(text: &str) -> Result<Option<Hifi>> {
        Hifi::parse(Path::new("hifi.toml"), text)
    }

    #[test]
    fn a_secret_is_admitted_only_when_its_digest_is_listed() {
        let keys = Keys::new(vec![Digest::of(b"test")]);

        assert!(keys.admits(b"test"));
        assert!(!keys.admits(b"tesu"));
        assert!(!keys.admits(b""));
        assert!(!Keys::default().admits(b"test"));
    }

    #[test]
    fn a_digest_reads_from_hex_in_either_case_and_prints_in_lower() {
        let digest = Digest::read(&HELD.to_uppercase()).expect("a digest");

        assert_eq!(digest, Digest::of(b"test"));
        assert_eq!(digest.hex(), HELD);
        assert_eq!(Digest::read("9f86"), None);
        assert_eq!(Digest::read(&"zz".repeat(32)), None);
    }

    #[test]
    fn a_minted_secret_is_listed_by_its_digest_and_two_differ() {
        let first = Minted::new().expect("a key");
        let second = Minted::new().expect("a key");

        assert_eq!(first.secret.len(), 64);
        assert_eq!(Digest::of(first.secret.as_bytes()), first.digest);
        assert_ne!(first.secret, second.secret);
        assert!(Keys::new(vec![first.digest]).admits(first.secret.as_bytes()));
    }

    #[test]
    fn a_config_without_a_hifi_table_leaves_the_gateway_off() {
        assert_eq!(parsed("").expect("read"), None);
    }

    #[test]
    fn a_hifi_table_names_where_it_listens_what_it_asks_and_which_keys_it_admits() {
        let hifi = parsed(&configured(&format!("[\"{HELD}\"]")))
            .expect("read")
            .expect("a gateway");

        assert_eq!(hifi.listen, "[::]:443".parse().expect("an address"));
        assert_eq!(hifi.upstream, "127.0.0.1:8000".parse().expect("an address"));
        assert!(hifi.keys.admits(b"test"));
    }

    #[test]
    fn a_hifi_table_with_no_keys_or_a_key_that_is_not_a_digest_is_refused() {
        assert!(matches!(
            parsed(&configured("[]")),
            Err(Error::ConfigMissing {
                field: ConfigField::Keys
            })
        ));
        assert!(matches!(
            parsed(&configured("[\"secret\"]")),
            Err(Error::ConfigInvalid {
                field: ConfigField::Keys
            })
        ));
        assert!(matches!(
            parsed("[hifi]\nlisten = \"[::]:443\"\nkeys = []\n"),
            Err(Error::ConfigMissing {
                field: ConfigField::Keys
            })
        ));
    }

    #[test]
    fn an_address_that_does_not_read_is_refused_by_its_field() {
        let text = configured(&format!("[\"{HELD}\"]")).replace("127.0.0.1:8000", "localhost");

        assert!(matches!(
            parsed(&text),
            Err(Error::ConfigInvalid {
                field: ConfigField::Upstream
            })
        ));
    }
}
