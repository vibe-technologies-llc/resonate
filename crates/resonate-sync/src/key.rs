use std::{fmt, str::FromStr};

use ring::{
    aead::{CHACHA20_POLY1305, LessSafeKey, UnboundKey},
    digest::{Context, SHA256, digest},
    hkdf::{self, HKDF_SHA256, KeyType, Salt},
    rand::{SecureRandom, SystemRandom},
    signature::{Ed25519KeyPair, KeyPair},
};

use crate::{CodeRefused, Error, Result};

pub const SYNC_KEY_BYTES: usize = 20;
pub const TOKEN_BYTES: usize = 32;
pub const SPACE_BYTES: usize = 16;
pub const DEVICE_BYTES: usize = 16;
pub const PEER_PUBLIC_KEY_BYTES: usize = 32;

pub const COMMITMENT_BYTES: usize = 12;
pub const COMMITTED_KEY_BYTES: usize = SYNC_KEY_BYTES + COMMITMENT_BYTES;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BITS_PER_SYMBOL: usize = 5;
const CHECK_SYMBOLS: usize = 3;
const GROUP_SYMBOLS: usize = 5;
const COMMITTED_MARK: [u8; SPACE_BYTES - COMMITMENT_BYTES] = *b"RSc1";
const COMMITMENT_SALT: &[u8] = b"resonate-committed/1";
pub const PAIRING_LINK: &str = "resonate://pair/";
const DERIVATION_SALT: &[u8] = b"resonate-sync/1";
const SPACE_INFO: &[u8] = b"space";
const TOKEN_INFO: &[u8] = b"token";
const SEAL_INFO: &[u8] = b"seal";
const PEER_INFO: &[u8] = b"peer";
const SEAL_KEY_BYTES: usize = 32;
const PEER_SEED_BYTES: usize = 32;
const ED25519_PKCS8_V1_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

#[derive(Clone, PartialEq, Eq)]
pub struct SyncKey([u8; SYNC_KEY_BYTES]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpaceId(pub [u8; SPACE_BYTES]);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub [u8; DEVICE_BYTES]);

#[derive(Clone, PartialEq, Eq)]
pub struct Token(pub [u8; TOKEN_BYTES]);

pub struct Derived {
    pub space: SpaceId,
    pub token: Token,
    pub sealer: LessSafeKey,
    pub peer_seed: [u8; PEER_SEED_BYTES],
    pub peer_public: [u8; PEER_PUBLIC_KEY_BYTES],
}

struct Length(usize);

impl KeyType for Length {
    fn len(&self) -> usize {
        self.0
    }
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::NoRandomness)?;
    Ok(bytes)
}

impl SyncKey {
    pub fn minted() -> Result<Self> {
        random_bytes().map(Self)
    }

    pub(crate) const fn bytes(&self) -> &[u8; SYNC_KEY_BYTES] {
        &self.0
    }

    pub(crate) const fn from_bytes(bytes: [u8; SYNC_KEY_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn from_code(code: &str) -> Result<Self> {
        read_spelled(code).map(Self)
    }

    pub fn from_link(link: &str) -> Result<Self> {
        let link = link.trim();
        let code = link
            .get(..PAIRING_LINK.len())
            .filter(|scheme| scheme.eq_ignore_ascii_case(PAIRING_LINK))
            .map_or(link, |_| &link[PAIRING_LINK.len()..]);
        Self::from_code(code.trim_end_matches('/'))
    }

    pub fn link(&self) -> String {
        format!("{PAIRING_LINK}{}", self.code())
    }

    pub fn code(&self) -> String {
        spelled(&self.0)
    }

    pub fn derived(&self) -> Result<Derived> {
        let prk = Salt::new(HKDF_SHA256, DERIVATION_SALT).extract(&self.0);

        let mut space = [0; SPACE_BYTES];
        let mut token = [0; TOKEN_BYTES];
        let mut seal = [0; SEAL_KEY_BYTES];
        let mut peer_seed = [0; PEER_SEED_BYTES];
        expand(&prk, SPACE_INFO, &mut space)?;
        expand(&prk, TOKEN_INFO, &mut token)?;
        expand(&prk, SEAL_INFO, &mut seal)?;
        expand(&prk, PEER_INFO, &mut peer_seed)?;

        let unbound =
            UnboundKey::new(&CHACHA20_POLY1305, &seal).map_err(|_| Error::KeyDerivation)?;
        let peer =
            Ed25519KeyPair::from_seed_unchecked(&peer_seed).map_err(|_| Error::KeyDerivation)?;
        let mut peer_public = [0; PEER_PUBLIC_KEY_BYTES];
        peer_public.copy_from_slice(peer.public_key().as_ref());

        Ok(Derived {
            space: SpaceId(space),
            token: Token(token),
            sealer: LessSafeKey::new(unbound),
            peer_seed,
            peer_public,
        })
    }
}

impl Derived {
    pub fn peer_pkcs8(&self) -> Vec<u8> {
        let mut pkcs8 = Vec::with_capacity(ED25519_PKCS8_V1_PREFIX.len() + PEER_SEED_BYTES);
        pkcs8.extend_from_slice(&ED25519_PKCS8_V1_PREFIX);
        pkcs8.extend_from_slice(&self.peer_seed);
        pkcs8
    }
}

pub(crate) fn expand(prk: &hkdf::Prk, info: &[u8], out: &mut [u8]) -> Result<()> {
    let infos = [info];
    prk.expand(&infos, Length(out.len()))
        .and_then(|okm| okm.fill(out))
        .map_err(|_| Error::KeyDerivation)
}

fn symbol_value(letter: char) -> Option<u8> {
    let shouted = match letter.to_ascii_uppercase() {
        'I' | 'L' => '1',
        'O' => '0',
        other => other,
    };
    ALPHABET
        .iter()
        .position(|symbol| char::from(*symbol) == shouted)
        .and_then(|at| u8::try_from(at).ok())
}

const fn symbols_spelling(bytes: usize) -> usize {
    (bytes * 8).div_ceil(BITS_PER_SYMBOL)
}

pub(crate) fn spelled(bytes: &[u8]) -> String {
    let mut values = packed(bytes);
    values.extend_from_slice(&check_values(bytes));

    let mut code = String::with_capacity(values.len() + values.len() / GROUP_SYMBOLS);
    for (at, value) in values.iter().enumerate() {
        if at > 0 && at % GROUP_SYMBOLS == 0 {
            code.push('-');
        }
        code.push(char::from(ALPHABET[usize::from(*value)]));
    }
    code
}

pub(crate) fn read_spelled<const N: usize>(code: &str) -> Result<[u8; N]> {
    let symbols = code
        .chars()
        .filter(|letter| !matches!(letter, '-' | ' ' | '\t'))
        .collect::<Vec<_>>();
    let spelling = symbols_spelling(N);
    if symbols.len() != spelling + CHECK_SYMBOLS {
        return Err(Error::Code(CodeRefused::Length {
            found: symbols.len(),
        }));
    }
    let mut values = Vec::with_capacity(symbols.len());
    for (at, letter) in symbols.iter().enumerate() {
        let value = symbol_value(*letter).ok_or(Error::Code(CodeRefused::Symbol { at }))?;
        values.push(value);
    }

    let mut bytes = [0; N];
    if !unpacked(&values[..spelling], &mut bytes) || values[spelling..] != check_values(&bytes) {
        return Err(Error::Code(CodeRefused::Check));
    }
    Ok(bytes)
}

fn check_values(bytes: &[u8]) -> [u8; CHECK_SYMBOLS] {
    let digested = digest(&SHA256, bytes);
    let digested = digested.as_ref();
    let mut values = [0; CHECK_SYMBOLS];
    for (at, value) in values.iter_mut().enumerate() {
        *value = digested[at] & 0x1f;
    }
    values
}

fn packed(bytes: &[u8]) -> Vec<u8> {
    let mut values = Vec::with_capacity(symbols_spelling(bytes.len()));
    let mut held = 0_u32;
    let mut bits = 0;
    for byte in bytes {
        held = (held << 8) | u32::from(*byte);
        bits += 8;
        while bits >= BITS_PER_SYMBOL {
            bits -= BITS_PER_SYMBOL;
            values.push(((held >> bits) & 0x1f) as u8);
        }
    }
    if bits > 0 {
        values.push(((held << (BITS_PER_SYMBOL - bits)) & 0x1f) as u8);
    }
    values
}

fn unpacked(values: &[u8], out: &mut [u8]) -> bool {
    let mut held = 0_u32;
    let mut bits = 0;
    let mut at = 0;
    for value in values {
        held = (held << BITS_PER_SYMBOL) | u32::from(*value);
        bits += BITS_PER_SYMBOL;
        if bits >= 8 {
            bits -= 8;
            let Some(byte) = out.get_mut(at) else {
                return false;
            };
            *byte = ((held >> bits) & 0xff) as u8;
            at += 1;
        }
    }
    at == out.len() && held & ((1 << bits) - 1) == 0
}

impl fmt::Debug for SyncKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SyncKey(<withheld>)")
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<withheld>)")
    }
}

impl FromStr for SyncKey {
    type Err = Error;

    fn from_str(code: &str) -> Result<Self> {
        Self::from_code(code)
    }
}

impl DeviceId {
    pub fn minted() -> Result<Self> {
        random_bytes().map(Self)
    }

    pub fn short(&self) -> String {
        hex(&self.0[..4])
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({})", self.short())
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(&self.0))
    }
}

impl fmt::Display for SpaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(&self.0))
    }
}

impl SpaceId {
    pub fn committed_to(digest: &[u8; 32]) -> Self {
        let mut hashing = Context::new(&SHA256);
        hashing.update(COMMITMENT_SALT);
        hashing.update(digest);
        let hashed = hashing.finish();
        let mut commitment = [0; COMMITMENT_BYTES];
        commitment.copy_from_slice(&hashed.as_ref()[..COMMITMENT_BYTES]);

        Self::of_commitment(commitment)
    }

    pub fn of_commitment(commitment: [u8; COMMITMENT_BYTES]) -> Self {
        let mut space = [0; SPACE_BYTES];
        space[..COMMITTED_MARK.len()].copy_from_slice(&COMMITTED_MARK);
        space[COMMITTED_MARK.len()..].copy_from_slice(&commitment);

        Self(space)
    }

    pub fn commitment(&self) -> Option<[u8; COMMITMENT_BYTES]> {
        let (mark, commitment) = self.0.split_at(COMMITTED_MARK.len());
        if mark != COMMITTED_MARK {
            return None;
        }
        let mut held = [0; COMMITMENT_BYTES];
        held.copy_from_slice(commitment);

        Some(held)
    }

    pub fn may_be_made_by(&self, digest: &[u8; 32]) -> bool {
        self.commitment().is_none() || *self == Self::committed_to(digest)
    }
}

impl Token {
    pub fn digest(&self) -> [u8; 32] {
        let mut digested = [0; 32];
        digested.copy_from_slice(digest(&SHA256, &self.0).as_ref());
        digested
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_reads_back_as_the_key_it_was_written_from() {
        let key = SyncKey::minted().expect("randomness");
        let code = key.code();

        assert_eq!(code.len(), 35 + 35 / GROUP_SYMBOLS - 1);
        assert_eq!(SyncKey::from_code(&code).expect("its own code"), key);
        assert_eq!(
            SyncKey::from_code(&code.to_ascii_lowercase().replace('-', " ")).expect("lowered"),
            key
        );
    }

    #[test]
    fn a_pairing_link_reads_back_as_its_key_and_a_bare_code_reads_too() {
        let key = SyncKey::minted().expect("randomness");

        let link = key.link();
        let shouting = link.to_uppercase();

        assert!(link.starts_with(PAIRING_LINK));
        assert_eq!(SyncKey::from_link(&link).expect("read").code(), key.code());
        assert_eq!(
            SyncKey::from_link(&shouting).expect("read").code(),
            key.code()
        );
        assert_eq!(
            SyncKey::from_link(&key.code()).expect("read").code(),
            key.code()
        );
        assert!(SyncKey::from_link("resonate://pair/NOT-A-CODE").is_err());
    }

    #[test]
    fn letters_read_alike_are_taken_as_the_digit() {
        let key = SyncKey([0; SYNC_KEY_BYTES]);
        let code = key.code().replacen('0', "O", 3);

        assert_eq!(SyncKey::from_code(&code).expect("O is zero"), key);
    }

    #[test]
    fn a_mistyped_code_is_refused_by_its_check() {
        let key = SyncKey([7; SYNC_KEY_BYTES]);
        let code = key.code();
        let first = code.chars().next().expect("a symbol");
        let other = if first == 'A' { 'B' } else { 'A' };
        let mistyped = format!("{other}{}", &code[1..]);

        assert!(matches!(
            SyncKey::from_code(&mistyped),
            Err(Error::Code(CodeRefused::Check))
        ));
        assert!(matches!(
            SyncKey::from_code("ABC"),
            Err(Error::Code(CodeRefused::Length { found: 3 }))
        ));
        assert!(matches!(
            SyncKey::from_code(&code.replacen(first, "U", 1)),
            Err(Error::Code(CodeRefused::Symbol { at: 0 }))
        ));
    }

    #[test]
    fn one_key_derives_the_same_space_on_every_device_and_another_key_another() {
        let key = SyncKey([3; SYNC_KEY_BYTES]);
        let here = key.derived().expect("derived");
        let there = key.clone().derived().expect("derived");
        let elsewhere = SyncKey([4; SYNC_KEY_BYTES]).derived().expect("derived");

        assert_eq!(here.space, there.space);
        assert_eq!(here.token, there.token);
        assert_eq!(here.peer_public, there.peer_public);
        assert_ne!(here.space, elsewhere.space);
        assert_ne!(here.space.0[..], here.token.0[..SPACE_BYTES]);
    }

    #[test]
    fn a_pairing_code_is_spelled_as_it_always_was() {
        let key = SyncKey([0xA5; SYNC_KEY_BYTES]);

        assert_eq!(key.code(), "MPJTB-9D5MP-JTB9D-5MPJT-B9D5M-PJTB9-D53QJ");
    }

    #[test]
    fn a_longer_spelling_reads_back_and_refuses_bits_past_its_bytes() {
        let bytes = [0xC3; COMMITTED_KEY_BYTES];
        let code = spelled(&bytes);

        assert_eq!(read_spelled::<COMMITTED_KEY_BYTES>(&code).ok(), Some(bytes));
        assert!(matches!(
            read_spelled::<SYNC_KEY_BYTES>(&code),
            Err(Error::Code(CodeRefused::Length { found: 55 }))
        ));

        let mut values = packed(&bytes);
        let last = values.len() - 1;
        values[last] |= 1;
        let padded: String = values
            .iter()
            .chain(check_values(&bytes).iter())
            .map(|value| char::from(ALPHABET[usize::from(*value)]))
            .collect();
        assert!(matches!(
            read_spelled::<COMMITTED_KEY_BYTES>(&padded),
            Err(Error::Code(CodeRefused::Check))
        ));
    }

    #[test]
    fn a_space_committed_to_a_token_may_be_made_by_that_token_alone() {
        let token = Token([8; TOKEN_BYTES]);
        let stranger = Token([9; TOKEN_BYTES]);
        let committed = SpaceId::committed_to(&token.digest());
        let unmarked = SyncKey([3; SYNC_KEY_BYTES])
            .derived()
            .expect("derived")
            .space;

        assert!(committed.may_be_made_by(&token.digest()));
        assert!(!committed.may_be_made_by(&stranger.digest()));
        assert!(unmarked.commitment().is_none());
        assert!(unmarked.may_be_made_by(&stranger.digest()));
        assert_eq!(
            committed.commitment().map(SpaceId::of_commitment),
            Some(committed)
        );
    }

    #[test]
    fn the_key_never_prints() {
        let key = SyncKey([9; SYNC_KEY_BYTES]);

        assert_eq!(format!("{key:?}"), "SyncKey(<withheld>)");
    }
}
