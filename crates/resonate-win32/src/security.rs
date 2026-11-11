use std::path::Path;

#[cfg(windows)]
use crate::system::Windows;
use crate::{
    error::{Op, Raw, Result, refused_on},
    wide::Wide,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Inheritance {
    Protected,
    Inheriting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Laying {
    pub(crate) with_owner: bool,
    pub(crate) inheritance: Inheritance,
}

pub(crate) trait Securing {
    type Descriptor;

    fn read(&self, name: &Wide) -> Raw<Self::Descriptor>;
    fn is_protected(&self, descriptor: &Self::Descriptor) -> Raw<bool>;
    fn is_owned_alike(&self, one: &Self::Descriptor, other: &Self::Descriptor) -> bool;
    fn lay(&self, name: &Wide, from: &Self::Descriptor, laying: Laying) -> Raw<()>;
}

fn descriptor_of<T: Securing>(securing: &T, path: &Path) -> Result<(Wide, T::Descriptor)> {
    let name = Wide::of_path(path)?;
    let descriptor = securing
        .read(&name)
        .map_err(refused_on(Op::ReadSecurity, path))?;
    Ok((name, descriptor))
}

pub(crate) fn carrying_security(securing: &impl Securing, from: &Path, to: &Path) -> Result<()> {
    let (_, standing) = descriptor_of(securing, from)?;
    let (name, copy) = descriptor_of(securing, to)?;
    let with_owner = !securing.is_owned_alike(&standing, &copy);
    let protected = securing
        .is_protected(&standing)
        .map_err(refused_on(Op::ReadSecurity, from))?;
    let inheritance = if protected {
        Inheritance::Protected
    } else {
        Inheritance::Inheriting
    };
    securing
        .lay(
            &name,
            &standing,
            Laying {
                with_owner,
                inheritance,
            },
        )
        .map_err(refused_on(Op::WriteSecurity, to))
}

#[cfg(windows)]
pub fn carry_security(from: &Path, to: &Path) -> Result<()> {
    carrying_security(&Windows, from, to)
}

#[cfg(windows)]
mod real {
    use std::ptr;

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

    use super::{Inheritance, Laying, Securing};
    use crate::{
        error::Raw,
        system::{Windows, hresult, name_of},
        wide::Wide,
    };

    pub(crate) struct Descriptor {
        held: PSECURITY_DESCRIPTOR,
        owner: PSID,
        dacl: *mut ACL,
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe { LocalFree(Some(HLOCAL(self.held.0))) };
        }
    }

    impl Securing for Windows {
        type Descriptor = Descriptor;

        fn read(&self, name: &Wide) -> Raw<Descriptor> {
            let mut held = PSECURITY_DESCRIPTOR(ptr::null_mut());
            let mut owner = PSID(ptr::null_mut());
            let mut dacl: *mut ACL = ptr::null_mut();
            unsafe {
                GetNamedSecurityInfoW(
                    name_of(name),
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
            .map_err(hresult)?;
            Ok(Descriptor { held, owner, dacl })
        }

        fn is_protected(&self, descriptor: &Descriptor) -> Raw<bool> {
            let mut control = 0_u16;
            let mut revision = 0_u32;
            unsafe {
                GetSecurityDescriptorControl(descriptor.held, &raw mut control, &raw mut revision)
            }
            .map_err(hresult)?;
            Ok(control & SE_DACL_PROTECTED.0 != 0)
        }

        fn is_owned_alike(&self, one: &Descriptor, other: &Descriptor) -> bool {
            unsafe { EqualSid(one.owner, other.owner) }.is_ok()
        }

        fn lay(&self, name: &Wide, from: &Descriptor, laying: Laying) -> Raw<()> {
            let inheriting = match laying.inheritance {
                Inheritance::Protected => PROTECTED_DACL_SECURITY_INFORMATION,
                Inheritance::Inheriting => UNPROTECTED_DACL_SECURITY_INFORMATION,
            };
            let owning = if laying.with_owner {
                OWNER_SECURITY_INFORMATION
            } else {
                OBJECT_SECURITY_INFORMATION(0)
            };
            unsafe {
                SetNamedSecurityInfoW(
                    name_of(name),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | inheriting | owning,
                    laying.with_owner.then_some(from.owner),
                    None,
                    Some(from.dacl.cast_const()),
                    None,
                )
            }
            .ok()
            .map_err(hresult)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Read(Vec<u16>),
        IsProtected(u8),
        Compare(u8, u8),
        Lay {
            name: Vec<u16>,
            from: u8,
            laying: Laying,
        },
        Freed(u8),
    }

    type Calls = Arc<Mutex<Vec<Call>>>;

    struct Held {
        id: u8,
        calls: Calls,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            self.calls.lock().push(Call::Freed(self.id));
        }
    }

    #[derive(Clone)]
    struct Fake {
        calls: Calls,
        refusing_read_of: Option<usize>,
        refusing_protection: bool,
        refusing_lay: bool,
        same_owner: bool,
        protected: bool,
        reads: Arc<Mutex<u8>>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                calls: Arc::default(),
                refusing_read_of: None,
                refusing_protection: false,
                refusing_lay: false,
                same_owner: true,
                protected: false,
                reads: Arc::default(),
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }
    }

    impl Securing for Fake {
        type Descriptor = Held;

        fn read(&self, name: &Wide) -> Raw<Held> {
            self.calls.lock().push(Call::Read(name.units().to_vec()));
            let mut reads = self.reads.lock();
            *reads += 1;
            if self.refusing_read_of == Some(usize::from(*reads)) {
                return Err(HResult(-21));
            }
            Ok(Held {
                id: *reads,
                calls: Arc::clone(&self.calls),
            })
        }

        fn is_protected(&self, descriptor: &Held) -> Raw<bool> {
            self.calls.lock().push(Call::IsProtected(descriptor.id));
            if self.refusing_protection {
                return Err(HResult(-22));
            }
            Ok(self.protected)
        }

        fn is_owned_alike(&self, one: &Held, other: &Held) -> bool {
            self.calls.lock().push(Call::Compare(one.id, other.id));
            self.same_owner
        }

        fn lay(&self, name: &Wide, from: &Held, laying: Laying) -> Raw<()> {
            self.calls.lock().push(Call::Lay {
                name: name.units().to_vec(),
                from: from.id,
                laying,
            });
            if self.refusing_lay {
                return Err(HResult(-23));
            }
            Ok(())
        }
    }

    fn name(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    #[test]
    fn the_source_descriptor_is_laid_on_the_copy_by_its_name_and_both_are_freed() {
        let fake = Fake::new();

        carrying_security(&fake, Path::new("a.flac"), Path::new("b.flac")).expect("carried");

        assert_eq!(
            fake.calls(),
            [
                Call::Read(name("a.flac")),
                Call::Read(name("b.flac")),
                Call::Compare(1, 2),
                Call::IsProtected(1),
                Call::Lay {
                    name: name("b.flac"),
                    from: 1,
                    laying: Laying {
                        with_owner: false,
                        inheritance: Inheritance::Inheriting
                    }
                },
                Call::Freed(2),
                Call::Freed(1)
            ]
        );
    }

    #[test]
    fn a_differing_owner_is_carried_and_a_protected_list_stays_protected() {
        let fake = Fake {
            same_owner: false,
            protected: true,
            ..Fake::new()
        };

        carrying_security(&fake, Path::new("a"), Path::new("b")).expect("carried");

        assert!(fake.calls().contains(&Call::Lay {
            name: name("b"),
            from: 1,
            laying: Laying {
                with_owner: true,
                inheritance: Inheritance::Protected
            }
        }));
    }

    #[test]
    fn a_name_holding_a_nul_is_refused_before_anything_is_read() {
        let fake = Fake::new();

        let carried = carrying_security(&fake, &PathBuf::from("a\0"), Path::new("b"));

        assert!(matches!(carried, Err(Error::NulInName { .. })));
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_copy_that_cannot_be_read_frees_the_source_and_names_the_copy() {
        let fake = Fake {
            refusing_read_of: Some(2),
            ..Fake::new()
        };

        let carried = carrying_security(&fake, Path::new("a"), Path::new("b"));

        assert!(matches!(
            carried,
            Err(Error::WindowsOnFile { op: Op::ReadSecurity, ref path, source: HResult(-21) })
                if path == Path::new("b")
        ));
        assert_eq!(
            fake.calls(),
            [Call::Read(name("a")), Call::Read(name("b")), Call::Freed(1)]
        );
    }

    #[test]
    fn a_source_that_cannot_be_read_reads_nothing_more() {
        let fake = Fake {
            refusing_read_of: Some(1),
            ..Fake::new()
        };

        let carried = carrying_security(&fake, Path::new("a"), Path::new("b"));

        assert!(matches!(
            carried,
            Err(Error::WindowsOnFile { op: Op::ReadSecurity, ref path, .. }) if path == Path::new("a")
        ));
        assert_eq!(fake.calls(), [Call::Read(name("a"))]);
    }

    #[test]
    fn a_protection_that_cannot_be_read_names_the_source_and_lays_nothing() {
        let fake = Fake {
            refusing_protection: true,
            ..Fake::new()
        };

        let carried = carrying_security(&fake, Path::new("a"), Path::new("b"));

        assert!(matches!(
            carried,
            Err(Error::WindowsOnFile { op: Op::ReadSecurity, ref path, source: HResult(-22) })
                if path == Path::new("a")
        ));
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, Call::Lay { .. }))
        );
    }

    #[test]
    fn a_refused_write_names_the_copy_and_still_frees_both_descriptors() {
        let fake = Fake {
            refusing_lay: true,
            ..Fake::new()
        };

        let carried = carrying_security(&fake, Path::new("a"), Path::new("b"));

        assert!(matches!(
            carried,
            Err(Error::WindowsOnFile { op: Op::WriteSecurity, ref path, source: HResult(-23) })
                if path == Path::new("b")
        ));
        let calls = fake.calls();
        assert_eq!(&calls[calls.len() - 2..], [Call::Freed(2), Call::Freed(1)]);
    }
}
