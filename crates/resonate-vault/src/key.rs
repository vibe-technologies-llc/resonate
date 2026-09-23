use std::fmt;

use crate::error::{Error, Result};

pub(crate) const KEY_BYTES: usize = 16;
const KEY_LETTERS: usize = KEY_BYTES * 2;
const FANOUT_LETTERS: usize = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultKey([u8; KEY_BYTES]);

impl VaultKey {
    pub const fn of(bytes: [u8; KEY_BYTES]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    pub fn read(text: &str) -> Result<Self> {
        if text.len() != KEY_LETTERS || !text.bytes().all(|letter| letter.is_ascii_hexdigit()) {
            return Err(Error::NotAKey);
        }

        let mut bytes = [0_u8; KEY_BYTES];
        for (at, byte) in bytes.iter_mut().enumerate() {
            let pair = text.get(at * 2..at * 2 + 2).ok_or(Error::NotAKey)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| Error::NotAKey)?;
        }
        Ok(Self(bytes))
    }

    pub fn fanout(&self) -> String {
        self.to_string()[..FANOUT_LETTERS].to_owned()
    }
}

impl fmt::Display for VaultKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_written_out_reads_back_as_itself() {
        let key = VaultKey::of([
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ]);
        let written = key.to_string();

        assert_eq!(written, "00112233445566778899aabbccddeeff");
        assert_eq!(VaultKey::read(&written).unwrap(), key);
        assert_eq!(key.fanout(), "00");
    }

    #[test]
    fn a_signed_or_short_or_uppercase_run_of_letters_is_weighed_as_what_it_is() {
        assert!(VaultKey::read("+0112233445566778899aabbccddeeff").is_err());
        assert!(VaultKey::read("00112233445566778899aabbccddeef").is_err());
        assert!(VaultKey::read("00112233445566778899aabbccddeefff").is_err());
        assert!(VaultKey::read(" 0112233445566778899aabbccddeeff").is_err());
        assert_eq!(
            VaultKey::read("00112233445566778899AABBCCDDEEFF").unwrap(),
            VaultKey::read("00112233445566778899aabbccddeeff").unwrap()
        );
    }
}
