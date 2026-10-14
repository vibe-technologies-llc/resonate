use std::io::{Read, Write};

use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use ring::aead::{Aad, LessSafeKey, NONCE_LEN, Nonce};

use crate::{DeflateOp, Error, Result, SpaceId, key::random_bytes};

pub const LARGEST_OPENED: usize = 64 * 1024 * 1024;

pub fn sealed(sealer: &LessSafeKey, space: SpaceId, plain: &[u8]) -> Result<Vec<u8>> {
    let mut packed = DeflateEncoder::new(Vec::with_capacity(plain.len() / 2), Compression::best());
    packed.write_all(plain).map_err(|source| Error::Deflate {
        op: DeflateOp::Pack,
        source,
    })?;
    let mut in_out = packed.finish().map_err(|source| Error::Deflate {
        op: DeflateOp::Pack,
        source,
    })?;

    let nonce_bytes = random_bytes::<NONCE_LEN>()?;
    sealer
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(space.0),
            &mut in_out,
        )
        .map_err(|_| Error::Unsealed)?;

    let mut blob = Vec::with_capacity(NONCE_LEN + in_out.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&in_out);
    Ok(blob)
}

pub fn opened(sealer: &LessSafeKey, space: SpaceId, blob: &[u8]) -> Result<Vec<u8>> {
    let Some((nonce, sealed)) = blob.split_at_checked(NONCE_LEN) else {
        return Err(Error::Unsealed);
    };
    let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| Error::Unsealed)?;
    let mut in_out = sealed.to_vec();
    let packed = sealer
        .open_in_place(nonce, Aad::from(space.0), &mut in_out)
        .map_err(|_| Error::Unsealed)?;

    let mut plain = Vec::with_capacity(packed.len() * 3);
    let bound = u64::try_from(LARGEST_OPENED + 1).unwrap_or(u64::MAX);
    DeflateDecoder::new(&packed[..])
        .take(bound)
        .read_to_end(&mut plain)
        .map_err(|source| Error::Deflate {
            op: DeflateOp::Unpack,
            source,
        })?;
    if plain.len() > LARGEST_OPENED {
        return Err(Error::TooLarge {
            length: plain.len(),
        });
    }
    Ok(plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SyncKey;

    #[test]
    fn what_one_device_seals_another_of_the_space_opens_and_no_other_key_does() {
        let key = SyncKey::minted().expect("randomness");
        let here = key.derived().expect("derived");
        let there = key.derived().expect("derived");
        let stranger = SyncKey::minted()
            .expect("randomness")
            .derived()
            .expect("derived");
        let plain = b"a playlist named after a summer".repeat(40);

        let blob = sealed(&here.sealer, here.space, &plain).expect("sealed");

        assert!(blob.len() < plain.len());
        assert_eq!(
            opened(&there.sealer, there.space, &blob).expect("opened"),
            plain
        );
        assert!(matches!(
            opened(&stranger.sealer, here.space, &blob),
            Err(Error::Unsealed)
        ));
        assert!(matches!(
            opened(&here.sealer, stranger.space, &blob),
            Err(Error::Unsealed)
        ));
    }

    #[test]
    fn a_tampered_or_cut_blob_is_refused() {
        let derived = SyncKey::minted()
            .expect("randomness")
            .derived()
            .expect("derived");
        let mut blob = sealed(&derived.sealer, derived.space, b"favourites").expect("sealed");
        let last = blob.len() - 1;
        blob[last] ^= 1;

        assert!(matches!(
            opened(&derived.sealer, derived.space, &blob),
            Err(Error::Unsealed)
        ));
        assert!(matches!(
            opened(&derived.sealer, derived.space, &blob[..4]),
            Err(Error::Unsealed)
        ));
    }
}
