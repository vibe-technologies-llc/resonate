use ring::{
    aead::{AES_256_GCM, LessSafeKey, UnboundKey},
    hkdf::{HKDF_SHA256, KeyType, Salt},
};

use crate::{
    CodeRefused, Error, Result, SpaceId, SyncKey, Token,
    key::{COMMITMENT_BYTES, COMMITTED_KEY_BYTES, SYNC_KEY_BYTES, read_spelled, spelled},
    shared::{link_to, web_link_to},
};

pub const PARTY_LINK: &str = "resonate://party/";

const AES_256_KEY_BYTES: usize = 32;

const DERIVATION_SALT: &[u8] = b"resonate-party/1";

const COMMITTED_SALT: &[u8] = b"resonate-party/2";

const ROOM_INFO: &[u8] = b"room";

const TOKEN_INFO: &[u8] = b"token";

const SEALER_INFO: &[u8] = b"sealer";

#[derive(Clone)]
pub enum PartyKey {
    FirstToClaim(SyncKey),
    Committed {
        secret: SyncKey,
        commitment: [u8; COMMITMENT_BYTES],
    },
}

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
    pub fn hosted_by(host: &Token) -> Result<Self> {
        let commitment = SpaceId::committed_to(&host.digest())
            .commitment()
            .ok_or(Error::KeyDerivation)?;

        Ok(Self::Committed {
            secret: SyncKey::minted()?,
            commitment,
        })
    }

    pub fn from_code(code: &str) -> Result<Self> {
        match SyncKey::from_code(code) {
            Err(Error::Code(CodeRefused::Length { .. })) => {
                let bytes = read_spelled::<COMMITTED_KEY_BYTES>(code)?;
                let mut secret = [0; SYNC_KEY_BYTES];
                let mut commitment = [0; COMMITMENT_BYTES];
                secret.copy_from_slice(&bytes[..SYNC_KEY_BYTES]);
                commitment.copy_from_slice(&bytes[SYNC_KEY_BYTES..]);

                Ok(Self::Committed {
                    secret: SyncKey::from_bytes(secret),
                    commitment,
                })
            }
            read => read.map(Self::FirstToClaim),
        }
    }

    pub fn code(&self) -> String {
        match self {
            Self::FirstToClaim(key) => key.code(),
            Self::Committed { secret, commitment } => {
                let mut bytes = [0; COMMITTED_KEY_BYTES];
                bytes[..SYNC_KEY_BYTES].copy_from_slice(secret.bytes());
                bytes[SYNC_KEY_BYTES..].copy_from_slice(commitment);
                spelled(&bytes)
            }
        }
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
        let (salt, secret) = match self {
            Self::FirstToClaim(key) => (DERIVATION_SALT, key),
            Self::Committed { secret, .. } => (COMMITTED_SALT, secret),
        };
        let prk = Salt::new(HKDF_SHA256, salt).extract(secret.bytes());
        let mut token = [0; 32];
        let mut sealer = [0; AES_256_KEY_BYTES];
        expand(&prk, TOKEN_INFO, &mut token)?;
        expand(&prk, SEALER_INFO, &mut sealer)?;
        let room = match self {
            Self::FirstToClaim(_) => {
                let mut room = [0; 16];
                expand(&prk, ROOM_INFO, &mut room)?;
                SpaceId(room)
            }
            Self::Committed { commitment, .. } => SpaceId::of_commitment(*commitment),
        };

        let unbound = UnboundKey::new(&AES_256_GCM, &sealer).map_err(|_| Error::KeyDerivation)?;

        Ok(PartyKeys {
            room,
            token: Token(token),
            sealer: LessSafeKey::new(unbound),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: Token = Token([6; 32]);

    #[test]
    fn a_party_keeps_its_room_when_its_code_is_written_out_and_read_back() {
        let key = PartyKey::hosted_by(&HOST).unwrap();
        let again = PartyKey::from_code(&key.code()).unwrap();

        assert_eq!(key.keys().unwrap().room, again.keys().unwrap().room);
        assert_eq!(
            key.keys().unwrap().token.digest(),
            again.keys().unwrap().token.digest()
        );
    }

    #[test]
    fn a_new_party_names_a_room_only_its_host_may_make() {
        let key = PartyKey::hosted_by(&HOST).unwrap();
        let keys = key.keys().unwrap();

        assert!(keys.room.may_be_made_by(&HOST.digest()));
        assert!(!keys.room.may_be_made_by(&keys.token.digest()));
        assert!(!keys.room.may_be_made_by(&Token([7; 32]).digest()));
    }

    #[test]
    fn a_party_code_handed_out_before_rooms_committed_reads_the_room_it_always_did() {
        let key = PartyKey::from_code("MPJTB-9D5MP-JTB9D-5MPJT-B9D5M-PJTB9-D53QJ").unwrap();
        let room = key.keys().unwrap().room;

        assert_eq!(room.to_string(), "ff3216a1042cf388edcf3a122041c6e3");
        assert!(room.may_be_made_by(&HOST.digest()));
    }

    #[test]
    fn one_set_of_bytes_is_a_different_room_as_a_party_than_as_anything_else() {
        let pairing = SyncKey::minted().unwrap();
        let party = PartyKey::FirstToClaim(pairing.clone());
        let derived = pairing.derived().unwrap();

        assert_ne!(party.keys().unwrap().room, derived.space);
    }

    #[test]
    fn the_link_given_out_reads_back_as_an_invitation_to_a_party() {
        let key = PartyKey::hosted_by(&HOST).unwrap();
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
