use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ring::digest::{SHA256, SHA256_OUTPUT_LEN, digest};

type SealedDigest = [u8; SHA256_OUTPUT_LEN];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unfresh {
    #[error("sealed at {sealed_at}, outside the window around {now}")]
    OutsideTheWindow { sealed_at: u64, now: u64 },

    #[error("heard already")]
    HeardAlready,

    #[error("too many were heard lately to hold another")]
    TooManyHeard,
}

pub struct HeardOnce {
    within: Duration,
    at_most: usize,
    heard: BTreeMap<SealedDigest, u64>,
}

impl HeardOnce {
    pub const fn new(within: Duration, at_most: usize) -> Self {
        Self {
            within,
            at_most,
            heard: BTreeMap::new(),
        }
    }

    pub fn admit(&mut self, sealed: &[u8], sealed_at: u64, now: u64) -> Result<(), Unfresh> {
        let within = self.within.as_secs();
        if now.abs_diff(sealed_at) > within {
            return Err(Unfresh::OutsideTheWindow { sealed_at, now });
        }
        self.heard.retain(|_, at| now.abs_diff(*at) <= within);
        let mut sealed_digest = [0; SHA256_OUTPUT_LEN];
        sealed_digest.copy_from_slice(digest(&SHA256, sealed).as_ref());
        if self.heard.contains_key(&sealed_digest) {
            return Err(Unfresh::HeardAlready);
        }
        if self.heard.len() >= self.at_most {
            return Err(Unfresh::TooManyHeard);
        }
        self.heard.insert(sealed_digest, sealed_at);
        Ok(())
    }
}

pub fn wall_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_790_000_000;

    #[test]
    fn a_blob_is_admitted_once_within_its_window_and_never_outside_it() {
        let mut heard = HeardOnce::new(Duration::from_secs(60), 2);

        assert_eq!(heard.admit(b"first", NOW - 30, NOW), Ok(()));
        assert_eq!(
            heard.admit(b"first", NOW - 30, NOW),
            Err(Unfresh::HeardAlready)
        );
        assert_eq!(
            heard.admit(b"old", NOW - 61, NOW),
            Err(Unfresh::OutsideTheWindow {
                sealed_at: NOW - 61,
                now: NOW
            })
        );
        assert!(matches!(
            heard.admit(b"ahead", NOW + 61, NOW),
            Err(Unfresh::OutsideTheWindow { .. })
        ));
        assert_eq!(heard.admit(b"second", NOW, NOW), Ok(()));
        assert_eq!(heard.admit(b"third", NOW, NOW), Err(Unfresh::TooManyHeard));
        assert_eq!(heard.admit(b"third", NOW + 40, NOW + 40), Ok(()));
    }
}
