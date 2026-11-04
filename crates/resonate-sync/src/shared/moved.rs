use ring::{
    aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey},
    hkdf::{HKDF_SHA256, Salt},
};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    Result,
    error::{Error, Malformed},
    key::{expand, random_bytes},
    shared::{
        Access,
        op::{MEMBER_BYTES, MemberId, PUBLIC_BYTES},
    },
    wire::{Reader, Writer},
};

const SECRET_BYTES: usize = 32;
const SEAL_KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const DERIVATION_SALT: &[u8] = b"resonate-shared-moved/1";
const LARGEST_SEALED_CODE: usize = 512;
const SEAT_AT_LEAST: usize = MEMBER_BYTES + PUBLIC_BYTES + 1;

#[derive(Clone)]
pub struct Personal(StaticSecret);

impl Personal {
    pub fn minted() -> Result<Self> {
        random_bytes::<SECRET_BYTES>().map(|bytes| Self(StaticSecret::from(bytes)))
    }

    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        Self(StaticSecret::from(bytes))
    }

    pub fn to_bytes(&self) -> [u8; SECRET_BYTES] {
        self.0.to_bytes()
    }

    pub fn public(&self) -> [u8; PUBLIC_BYTES] {
        *PublicKey::from(&self.0).as_bytes()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub member: MemberId,
    pub sealed: Vec<u8>,
}

pub fn seat_for(member: MemberId, public: &[u8; PUBLIC_BYTES], access: &Access) -> Result<Seat> {
    let once = Personal::minted()?;
    let once_public = once.public();
    let shared = once.0.diffie_hellman(&PublicKey::from(*public));
    let key = sealing_key(shared.as_bytes(), &once_public, public)?;
    let mut in_out = access.code().into_bytes();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key([0; NONCE_BYTES]),
        Aad::empty(),
        &mut in_out,
    )
    .map_err(|_| Error::Unsealed)?;

    let mut sealed = Vec::with_capacity(PUBLIC_BYTES + in_out.len());
    sealed.extend_from_slice(&once_public);
    sealed.extend_from_slice(&in_out);

    Ok(Seat { member, sealed })
}

pub fn access_from(seat: &Seat, personal: &Personal) -> Result<Access> {
    let Some((once_public, sealed)) = seat.sealed.split_at_checked(PUBLIC_BYTES) else {
        return Err(Error::Unsealed);
    };
    let mut once = [0; PUBLIC_BYTES];
    once.copy_from_slice(once_public);
    let public = personal.public();
    let shared = personal.0.diffie_hellman(&PublicKey::from(once));
    let key = sealing_key(shared.as_bytes(), &once, &public)?;
    let mut in_out = sealed.to_vec();
    let code = key
        .open_in_place(
            Nonce::assume_unique_for_key([0; NONCE_BYTES]),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|_| Error::Unsealed)?;
    let code = std::str::from_utf8(code).map_err(|_| Error::Unsealed)?;

    Access::from_code(code)
}

fn sealing_key(
    shared: &[u8; 32],
    once: &[u8; PUBLIC_BYTES],
    to: &[u8; PUBLIC_BYTES],
) -> Result<LessSafeKey> {
    let prk = Salt::new(HKDF_SHA256, DERIVATION_SALT).extract(shared);
    let mut info = [0; PUBLIC_BYTES * 2];
    info[..PUBLIC_BYTES].copy_from_slice(once);
    info[PUBLIC_BYTES..].copy_from_slice(to);
    let mut key = [0; SEAL_KEY_BYTES];
    expand(&prk, &info, &mut key)?;

    UnboundKey::new(&CHACHA20_POLY1305, &key)
        .map(LessSafeKey::new)
        .map_err(|_| Error::KeyDerivation)
}

pub fn encoded_seats(seats: &[Seat]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.length(seats.len());
    for seat in seats {
        writer.fixed(&seat.member.0);
        writer.bytes(&seat.sealed);
    }

    writer.finish()
}

pub fn decoded_seats(bytes: &[u8]) -> Result<Vec<Seat>> {
    let mut reader = Reader::new(bytes);
    let count = reader.count_of(SEAT_AT_LEAST)?;
    let mut seats = Vec::with_capacity(count);
    for _ in 0..count {
        seats.push(Seat {
            member: MemberId(reader.fixed()?),
            sealed: reader.bytes(LARGEST_SEALED_CODE)?.to_vec(),
        });
    }
    reader
        .done()
        .map_err(|_| Error::Malformed(Malformed::Number))?;

    Ok(seats)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: MemberId = MemberId([1; MEMBER_BYTES]);

    #[test]
    fn the_new_key_opens_for_the_one_it_was_sealed_to_and_nobody_else() {
        let ada = Personal::minted().expect("a secret");
        let removed = Personal::minted().expect("a secret");
        let access = Access::minted().expect("a key");

        let seat = seat_for(ADA, &ada.public(), &access).expect("sealed");

        assert_eq!(access_from(&seat, &ada).ok(), Some(access));
        assert!(access_from(&seat, &removed).is_err());
    }

    #[test]
    fn a_seat_tampered_with_opens_for_nobody() {
        let ada = Personal::minted().expect("a secret");
        let mut seat =
            seat_for(ADA, &ada.public(), &Access::minted().expect("a key")).expect("sealed");
        let last = seat.sealed.len() - 1;
        seat.sealed[last] ^= 1;

        assert!(access_from(&seat, &ada).is_err());
    }

    #[test]
    fn a_personal_secret_comes_back_from_its_bytes_as_the_same_person() {
        let ada = Personal::minted().expect("a secret");
        let again = Personal::from_bytes(ada.to_bytes());

        assert_eq!(ada.public(), again.public());
    }

    #[test]
    fn the_seats_read_back_as_themselves() {
        let ada = Personal::minted().expect("a secret");
        let seats =
            vec![seat_for(ADA, &ada.public(), &Access::minted().expect("a key")).expect("sealed")];

        assert_eq!(decoded_seats(&encoded_seats(&seats)).ok(), Some(seats));
    }
}
