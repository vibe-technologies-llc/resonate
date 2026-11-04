use std::fmt;

use ring::{
    aead::{CHACHA20_POLY1305, LessSafeKey, UnboundKey},
    hkdf::{HKDF_SHA256, Salt},
};

use crate::{
    CodeRefused, Error, Result, SpaceId, SyncKey, Token,
    key::{SYNC_KEY_BYTES, expand},
};

const DERIVATION_SALT: &[u8] = b"resonate-shared/1";
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
    Edit(SyncKey),
    View(SyncKey),
}

pub struct SharedKeys {
    pub space: SpaceId,
    pub read_token: Token,
    pub write_token: Option<Token>,
    pub sealer: LessSafeKey,
}

impl Access {
    pub fn minted() -> Result<Self> {
        SyncKey::minted().map(Self::Edit)
    }

    pub const fn may_edit(&self) -> bool {
        matches!(self, Self::Edit(_))
    }

    pub fn code(&self) -> String {
        match self {
            Self::Edit(key) => format!("{EDIT_MARK}{}", key.code()),
            Self::View(key) => format!("{VIEW_MARK}{}", key.code()),
        }
    }

    pub fn link(&self) -> String {
        format!("{SHARED_LINK}{}", self.code())
    }

    pub fn from_code(code: &str) -> Result<Self> {
        let code = code.trim();
        if let Some(edit) = strip_mark(code, EDIT_MARK) {
            return SyncKey::from_code(edit).map(Self::Edit);
        }
        if let Some(view) = strip_mark(code, VIEW_MARK) {
            return SyncKey::from_code(view).map(Self::View);
        }

        Err(Error::Code(CodeRefused::Length { found: code.len() }))
    }

    pub fn viewing(&self) -> Result<Self> {
        match self {
            Self::Edit(edit) => {
                let prk = Salt::new(HKDF_SHA256, DERIVATION_SALT).extract(edit.bytes());
                let mut view = [0; SYNC_KEY_BYTES];
                expand(&prk, VIEW_INFO, &mut view)?;

                Ok(Self::View(SyncKey::from_bytes(view)))
            }
            Self::View(view) => Ok(Self::View(view.clone())),
        }
    }

    pub fn keys(&self) -> Result<SharedKeys> {
        let write_token = match self {
            Self::Edit(edit) => {
                let prk = Salt::new(HKDF_SHA256, DERIVATION_SALT).extract(edit.bytes());
                let mut token = [0; 32];
                expand(&prk, WRITE_INFO, &mut token)?;

                Some(Token(token))
            }
            Self::View(_) => None,
        };
        let Self::View(view) = self.viewing()? else {
            return Err(Error::KeyDerivation);
        };
        let prk = Salt::new(HKDF_SHA256, DERIVATION_SALT).extract(view.bytes());
        let mut space = [0; 16];
        let mut read_token = [0; 32];
        let mut seal = [0; SEAL_KEY_BYTES];
        expand(&prk, SPACE_INFO, &mut space)?;
        expand(&prk, READ_INFO, &mut read_token)?;
        expand(&prk, SEAL_INFO, &mut seal)?;
        let unbound =
            UnboundKey::new(&CHACHA20_POLY1305, &seal).map_err(|_| Error::KeyDerivation)?;

        Ok(SharedKeys {
            space: SpaceId(space),
            read_token: Token(read_token),
            write_token,
            sealer: LessSafeKey::new(unbound),
        })
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
    use crate::seal::{opened, sealed};

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
            opened(&view_keys.sealer, view_keys.space, &told).expect("opened"),
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
        let shared = Access::Edit(edit).keys().expect("derived");

        assert_ne!(paired.space, shared.space);
        assert_ne!(Some(paired.token), shared.write_token);
    }

    #[test]
    fn a_key_is_never_printed() {
        let edit = Access::minted().expect("a key");

        assert_eq!(format!("{edit:?}"), "Access::Edit(<withheld>)");
    }
}
