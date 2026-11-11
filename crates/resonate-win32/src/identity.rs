use crate::error::{Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

pub(crate) trait Naming {
    fn name_the_process(&self, app_user_model_id: &str) -> Raw<()>;
}

pub(crate) fn named(naming: &impl Naming, app_user_model_id: &str) -> Result<()> {
    naming
        .name_the_process(app_user_model_id)
        .map_err(refused(Op::NameTheProcess))
}

#[cfg(windows)]
pub fn name_the_process(app_user_model_id: &str) -> Result<()> {
    named(&Windows, app_user_model_id)
}

#[cfg(windows)]
mod real {
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows_core::HSTRING;

    use super::Naming;
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    impl Naming for Windows {
        fn name_the_process(&self, app_user_model_id: &str) -> Raw<()> {
            let id = HSTRING::from(app_user_model_id);
            unsafe { SetCurrentProcessExplicitAppUserModelID(&id) }.map_err(hresult)
        }
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    struct Fake {
        named: Mutex<Vec<String>>,
        refusing: bool,
    }

    impl Naming for Fake {
        fn name_the_process(&self, app_user_model_id: &str) -> Raw<()> {
            self.named.lock().push(app_user_model_id.to_owned());
            if self.refusing {
                return Err(HResult(-9));
            }
            Ok(())
        }
    }

    #[test]
    fn the_id_is_handed_over_as_it_was_given() {
        let fake = Fake {
            named: Mutex::default(),
            refusing: false,
        };

        named(&fake, "Resonate.Player").expect("named");

        assert_eq!(*fake.named.lock(), ["Resonate.Player"]);
    }

    #[test]
    fn a_refusal_names_the_call() {
        let fake = Fake {
            named: Mutex::default(),
            refusing: true,
        };

        assert!(matches!(
            named(&fake, "x"),
            Err(Error::Windows {
                op: Op::NameTheProcess,
                source: HResult(-9)
            })
        ));
    }
}
