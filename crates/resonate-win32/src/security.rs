use std::{path::Path, ptr};

use windows::Win32::{
    Foundation::{HLOCAL, LocalFree},
    Security::{
        ACL,
        Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW},
        DACL_SECURITY_INFORMATION, EqualSid, GetSecurityDescriptorControl,
        OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
        UNPROTECTED_DACL_SECURITY_INFORMATION,
    },
};

use crate::{
    error::{Op, Result, refused_on},
    wide::Wide,
};

struct Descriptor {
    held: PSECURITY_DESCRIPTOR,
    owner: PSID,
    dacl: *mut ACL,
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(Some(HLOCAL(self.held.0))) };
    }
}

impl Descriptor {
    fn of(path: &Path) -> Result<Self> {
        let name = Wide::of_path(path)?;
        let mut held = PSECURITY_DESCRIPTOR(ptr::null_mut());
        let mut owner = PSID(ptr::null_mut());
        let mut dacl: *mut ACL = ptr::null_mut();
        unsafe {
            GetNamedSecurityInfoW(
                name.name(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&raw mut owner),
                None,
                Some(&raw mut dacl),
                None,
                &raw mut held,
            )
        }
        .ok()
        .map_err(refused_on(Op::ReadSecurity, path))?;
        Ok(Self { held, owner, dacl })
    }

    fn is_protected(&self, path: &Path) -> Result<bool> {
        let mut control = 0_u16;
        let mut revision = 0_u32;
        unsafe { GetSecurityDescriptorControl(self.held, &raw mut control, &raw mut revision) }
            .map_err(refused_on(Op::ReadSecurity, path))?;
        Ok(control & SE_DACL_PROTECTED.0 != 0)
    }

    fn is_owned_as(&self, other: &Self) -> bool {
        unsafe { EqualSid(self.owner, other.owner) }.is_ok()
    }

    fn lay_on(&self, path: &Path, with_owner: bool, protected: bool) -> Result<()> {
        let name = Wide::of_path(path)?;
        let inheriting = if protected {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        let owning = if with_owner {
            OWNER_SECURITY_INFORMATION
        } else {
            OBJECT_SECURITY_INFORMATION(0)
        };
        unsafe {
            SetNamedSecurityInfoW(
                name.name(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | inheriting | owning,
                with_owner.then_some(self.owner),
                None,
                Some(self.dacl.cast_const()),
                None,
            )
        }
        .ok()
        .map_err(refused_on(Op::WriteSecurity, path))
    }
}

pub fn carry_security(from: &Path, to: &Path) -> Result<()> {
    let standing = Descriptor::of(from)?;
    let copy = Descriptor::of(to)?;
    let with_owner = !standing.is_owned_as(&copy);
    standing.lay_on(to, with_owner, standing.is_protected(from)?)
}
