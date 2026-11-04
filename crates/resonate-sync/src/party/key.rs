use ring::{
    aead::{AES_256_GCM, LessSafeKey, UnboundKey},
    hkdf::{HKDF_SHA256, KeyType, Salt},
};

use crate::{
    Error, Result, SpaceId, SyncKey, Token,
    shared::{link_to, web_link_to},
};

pub const PARTY_LINK: &str = "resonate://party/";

const AES_256_KEY_BYTES: usize = 32;

const DERIVATION_SALT: &[u8] = b"resonate-party/1";

const ROOM_INFO: &[u8] = b"room";

const TOKEN_INFO: &[u8] = b"token";

const SEALER_INFO: &[u8] = b"sealer";

#[derive(Clone)]
pub struct PartyKey(SyncKey);

pub struct PartyKeys {
    pub room: SpaceId,
    pub token: Token,
    pub sealer: LessSafeKey,
}

struct Length(usize);

impl KeyType for Length {
    fn len(&self) -> usize {
        self.0
    }
}

fn expand(prk: &ring::hkdf::Prk, info: &'static [u8], out: &mut [u8]) -> Result<()> {
    let infos = [info];

    prk.expand(&infos, Length(out.len()))
        .and_then(|okm| okm.fill(out))
        .map_err(|_| Error::KeyDerivation)
}

impl PartyKey {
    pub fn minted() -> Result<Self> {
        SyncKey::minted().map(Self)
    }

    pub fn from_code(code: &str) -> Result<Self> {
        SyncKey::from_code(code).map(Self)
    }

    pub fn code(&self) -> String {
        self.0.code()
    }

    pub fn link(&self, server: Option<&str>) -> String {
        link_to(crate::shared::Invited::Party, &self.code(), server)
    }

    pub fn web_link(&self, server: Option<&str>) -> Option<String> {
        web_link_to(
            crate::shared::Invited::Party,
            &self.code(),
            server.unwrap_or(crate::HOSTED_SERVER),
        )
    }

    pub fn keys(&self) -> Result<PartyKeys> {
        let prk = Salt::new(HKDF_SHA256, DERIVATION_SALT).extract(self.0.bytes());
        let mut room = [0; 16];
        let mut token = [0; 32];
        let mut sealer = [0; AES_256_KEY_BYTES];

        expand(&prk, ROOM_INFO, &mut room)?;
        expand(&prk, TOKEN_INFO, &mut token)?;
        expand(&prk, SEALER_INFO, &mut sealer)?;

        let unbound = UnboundKey::new(&AES_256_GCM, &sealer).map_err(|_| Error::KeyDerivation)?;

        Ok(PartyKeys {
            room: SpaceId(room),
            token: Token(token),
            sealer: LessSafeKey::new(unbound),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_party_keeps_its_room_when_its_code_is_written_out_and_read_back() {
        let key = PartyKey::minted().unwrap();
        let again = PartyKey::from_code(&key.code()).unwrap();

        assert_eq!(key.keys().unwrap().room, again.keys().unwrap().room);
        assert_eq!(
            key.keys().unwrap().token.digest(),
            again.keys().unwrap().token.digest()
        );
    }

    #[test]
    fn one_set_of_bytes_is_a_different_room_as_a_party_than_as_anything_else() {
        let pairing = SyncKey::minted().unwrap();
        let party = PartyKey(pairing.clone());
        let derived = pairing.derived().unwrap();

        assert_ne!(party.keys().unwrap().room, derived.space);
    }

    #[test]
    fn the_link_given_out_reads_back_as_an_invitation_to_a_party() {
        let key = PartyKey::minted().unwrap();
        let read = crate::shared::invitation_in(&key.link(None)).unwrap();

        assert_eq!(read.to, crate::shared::Invited::Party);
        assert_eq!(read.server, None);
        assert_eq!(
            PartyKey::from_code(&read.code)
                .unwrap()
                .keys()
                .unwrap()
                .room,
            key.keys().unwrap().room
        );
    }
}
