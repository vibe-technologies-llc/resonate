use std::fmt;

use ring::{
    aead::{CHACHA20_POLY1305, LessSafeKey, UnboundKey},
    hkdf::{HKDF_SHA256, Prk, Salt},
};

use crate::{
    CodeRefused, Error, Result, SpaceId, SyncKey, Token,
    key::{
        COMMITMENT_BYTES, COMMITTED_KEY_BYTES, SYNC_KEY_BYTES, TOKEN_BYTES, expand, random_bytes,
        read_spelled, spelled,
    },
};

const DERIVATION_SALT: &[u8] = b"resonate-shared/1";
const COMMITTED_SALT: &[u8] = b"resonate-shared/2";
const VIEW_INFO: &[u8] = b"view";
const WRITE_INFO: &[u8] = b"write";
const SPACE_INFO: &[u8] = b"space";
const READ_INFO: &[u8] = b"read";
const SEAL_INFO: &[u8] = b"seal";
const SEAL_KEY_BYTES: usize = 32;

const EDIT_MARK: &str = "E-";
const VIEW_MARK: &str = "V-";

pub const SHARED_LINK: &str = "resonate://shared/";

#[derive(Clone, PartialEq, Eq)]
pub enum Access {
    Edit(EditKey),
    View(ViewKey),
}

#[derive(Clone, PartialEq, Eq)]
pub enum EditKey {
    FirstToClaim(SyncKey),
    Committed([u8; COMMITTED_KEY_BYTES]),
}

#[derive(Clone, PartialEq, Eq)]
pub enum ViewKey {
    FirstToClaim(SyncKey),
    Committed {
        secret: SyncKey,
        commitment: [u8; COMMITMENT_BYTES],
    },
}

pub struct SharedKeys {
    pub space: SpaceId,
    pub read_token: Token,
    pub write_token: Option<Token>,
    pub sealer: LessSafeKey,
}

impl Access {
    pub fn minted() -> Result<Self> {
        random_bytes().map(|bytes| Self::Edit(EditKey::Committed(bytes)))
    }

    pub const fn may_edit(&self) -> bool {
        matches!(self, Self::Edit(_))
    }

    pub fn code(&self) -> String {
        match self {
            Self::Edit(EditKey::FirstToClaim(key)) => format!("{EDIT_MARK}{}", key.code()),
            Self::Edit(EditKey::Committed(key)) => format!("{EDIT_MARK}{}", spelled(key)),
            Self::View(ViewKey::FirstToClaim(key)) => format!("{VIEW_MARK}{}", key.code()),
            Self::View(ViewKey::Committed { secret, commitment }) => {
                format!("{VIEW_MARK}{}", spelled(&joined(secret, commitment)))
            }
        }
    }

    pub fn link(&self) -> String {
        format!("{SHARED_LINK}{}", self.code())
    }

    pub fn from_code(code: &str) -> Result<Self> {
        let code = code.trim();
        if let Some(edit) = strip_mark(code, EDIT_MARK) {
            return match SyncKey::from_code(edit) {
                Err(Error::Code(CodeRefused::Length { .. })) => {
                    read_spelled(edit).map(|key| Self::Edit(EditKey::Committed(key)))
                }
                read => read.map(|key| Self::Edit(EditKey::FirstToClaim(key))),
            };
        }
        if let Some(view) = strip_mark(code, VIEW_MARK) {
            return match SyncKey::from_code(view) {
                Err(Error::Code(CodeRefused::Length { .. })) => {
                    read_spelled(view).map(|key| Self::View(split(&key)))
                }
                read => read.map(|key| Self::View(ViewKey::FirstToClaim(key))),
            };
        }

        Err(Error::Code(CodeRefused::Length { found: code.len() }))
    }

    pub fn viewing(&self) -> Result<Self> {
        match self {
            Self::Edit(EditKey::FirstToClaim(edit)) => {
                let mut view = [0; SYNC_KEY_BYTES];
                expand(
                    &extracted(DERIVATION_SALT, edit.bytes()),
                    VIEW_INFO,
                    &mut view,
                )?;

                Ok(Self::View(ViewKey::FirstToClaim(SyncKey::from_bytes(view))))
            }
            Self::Edit(EditKey::Committed(edit)) => {
                let prk = extracted(COMMITTED_SALT, edit);
                let mut secret = [0; SYNC_KEY_BYTES];
                expand(&prk, VIEW_INFO, &mut secret)?;
                let commitment = SpaceId::committed_to(&written_with(&prk)?.digest())
                    .commitment()
                    .ok_or(Error::KeyDerivation)?;

                Ok(Self::View(ViewKey::Committed {
                    secret: SyncKey::from_bytes(secret),
                    commitment,
                }))
            }
            Self::View(view) => Ok(Self::View(view.clone())),
        }
    }

    pub fn keys(&self) -> Result<SharedKeys> {
        let write_token = match self {
            Self::Edit(EditKey::FirstToClaim(edit)) => {
                Some(written_with(&extracted(DERIVATION_SALT, edit.bytes()))?)
            }
            Self::Edit(EditKey::Committed(edit)) => {
                Some(written_with(&extracted(COMMITTED_SALT, edit))?)
            }
            Self::View(_) => None,
        };
        let Self::View(view) = self.viewing()? else {
            return Err(Error::KeyDerivation);
        };
        let (prk, space) = match &view {
            ViewKey::FirstToClaim(view) => {
                let prk = extracted(DERIVATION_SALT, view.bytes());
                let mut space = [0; 16];
                expand(&prk, SPACE_INFO, &mut space)?;
                (prk, SpaceId(space))
            }
            ViewKey::Committed { secret, commitment } => (
                extracted(COMMITTED_SALT, secret.bytes()),
                SpaceId::of_commitment(*commitment),
            ),
        };
        let mut read_token = [0; TOKEN_BYTES];
        let mut seal = [0; SEAL_KEY_BYTES];
        expand(&prk, READ_INFO, &mut read_token)?;
        expand(&prk, SEAL_INFO, &mut seal)?;
        let unbound =
            UnboundKey::new(&CHACHA20_POLY1305, &seal).map_err(|_| Error::KeyDerivation)?;

        Ok(SharedKeys {
            space,
            read_token: Token(read_token),
            write_token,
            sealer: LessSafeKey::new(unbound),
        })
    }
}

fn extracted(salt: &[u8], key: &[u8]) -> Prk {
    Salt::new(HKDF_SHA256, salt).extract(key)
}

fn written_with(prk: &Prk) -> Result<Token> {
    let mut token = [0; TOKEN_BYTES];
    expand(prk, WRITE_INFO, &mut token)?;

    Ok(Token(token))
}

fn joined(secret: &SyncKey, commitment: &[u8; COMMITMENT_BYTES]) -> [u8; COMMITTED_KEY_BYTES] {
    let mut bytes = [0; COMMITTED_KEY_BYTES];
    bytes[..SYNC_KEY_BYTES].copy_from_slice(secret.bytes());
    bytes[SYNC_KEY_BYTES..].copy_from_slice(commitment);
    bytes
}

fn split(bytes: &[u8; COMMITTED_KEY_BYTES]) -> ViewKey {
    let mut secret = [0; SYNC_KEY_BYTES];
    let mut commitment = [0; COMMITMENT_BYTES];
    secret.copy_from_slice(&bytes[..SYNC_KEY_BYTES]);
    commitment.copy_from_slice(&bytes[SYNC_KEY_BYTES..]);

    ViewKey::Committed {
        secret: SyncKey::from_bytes(secret),
        commitment,
    }
}

fn strip_mark<'a>(code: &'a str, mark: &str) -> Option<&'a str> {
    let (head, rest) = code.split_at_checked(mark.len())?;

    head.eq_ignore_ascii_case(mark).then_some(rest)
}

impl fmt::Debug for Access {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Edit(_) => "Access::Edit(<withheld>)",
            Self::View(_) => "Access::View(<withheld>)",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seal::{LARGEST_OPENED, opened, sealed};

    #[test]
    fn a_view_key_comes_from_an_edit_key_and_never_the_reverse_and_both_read_one_space() {
        let edit = Access::minted().expect("a key");
        let view = edit.viewing().expect("a view key");

        let edit_keys = edit.keys().expect("derived");
        let view_keys = view.keys().expect("derived");

        assert_eq!(edit_keys.space, view_keys.space);
        assert_eq!(edit_keys.read_token, view_keys.read_token);
        assert!(edit_keys.write_token.is_some());
        assert!(view_keys.write_token.is_none());
        assert!(!view.may_edit());

        let told = sealed(&edit_keys.sealer, edit_keys.space, b"a record").expect("sealed");
        assert_eq!(
            opened(&view_keys.sealer, view_keys.space, &told, LARGEST_OPENED).expect("opened"),
            b"a record"
        );
    }

    #[test]
    fn a_code_reads_back_as_the_key_and_the_access_it_gives() {
        let edit = Access::minted().expect("a key");
        let view = edit.viewing().expect("a view key");

        assert_eq!(Access::from_code(&edit.code()).ok(), Some(edit.clone()));
        assert_eq!(Access::from_code(&view.code()).ok(), Some(view.clone()));
        assert_eq!(
            Access::from_code(&view.code().to_lowercase()).ok(),
            Some(view.clone())
        );
        assert!(edit.link().starts_with("resonate://shared/E-"));
        assert!(view.link().starts_with("resonate://shared/V-"));
        assert!(Access::from_code("nonsense").is_err());
    }

    #[test]
    fn a_shared_space_is_not_the_space_a_pairing_code_of_the_same_bytes_would_name() {
        let edit = SyncKey::minted().expect("a key");
        let paired = edit.derived().expect("derived");
        let shared = Access::Edit(EditKey::FirstToClaim(edit))
            .keys()
            .expect("derived");

        assert_ne!(paired.space, shared.space);
        assert_ne!(Some(paired.token), shared.write_token);
    }

    #[test]
    fn a_new_list_names_a_space_only_its_write_token_may_make_and_its_links_say_so() {
        let edit = Access::minted().expect("a key");
        let view = edit.viewing().expect("a view key");

        let edit_keys = edit.keys().expect("derived");
        let view_keys = view.keys().expect("derived");
        let write = edit_keys.write_token.expect("an editor writes");

        assert!(edit_keys.space.commitment().is_some());
        assert_eq!(edit_keys.space, view_keys.space);
        assert!(view_keys.space.may_be_made_by(&write.digest()));
        assert!(
            !view_keys
                .space
                .may_be_made_by(&view_keys.read_token.digest())
        );
        assert!(
            !view_keys
                .space
                .may_be_made_by(&Token([0; TOKEN_BYTES]).digest())
        );
        assert_eq!(Access::from_code(&view.code()).ok(), Some(view.clone()));
        assert_eq!(Access::from_code(&edit.code()).ok(), Some(edit.clone()));
    }

    #[test]
    fn a_link_handed_out_before_spaces_committed_reads_the_same_space_and_keys_as_it_did() {
        let edit = Access::from_code("E-MPJTB-9D5MP-JTB9D-5MPJT-B9D5M-PJTB9-D53QJ").expect("read");
        let view = edit.viewing().expect("a view key");

        let edit_keys = edit.keys().expect("derived");
        let view_keys = Access::from_code(&view.code())
            .expect("read")
            .keys()
            .expect("derived");

        assert_eq!(view.code().len(), 2 + 35 + 6);
        assert_eq!(edit_keys.space, view_keys.space);
        assert_eq!(
            edit_keys.space.to_string(),
            "c6406a36d5968b54d44cf2a2aa50d819"
        );
        assert!(edit_keys.space.commitment().is_none());
        assert!(
            edit_keys
                .space
                .may_be_made_by(&Token([0; TOKEN_BYTES]).digest())
        );
    }

    #[test]
    fn a_key_is_never_printed() {
        let edit = Access::minted().expect("a key");

        assert_eq!(format!("{edit:?}"), "Access::Edit(<withheld>)");
    }
}
