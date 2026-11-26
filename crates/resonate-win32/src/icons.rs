use std::process;

use crate::error::{Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Size {
    Big,
    Small,
}

pub(crate) trait Iconing {
    type Icon: Copy;
    type Window: Copy;

    fn icon_of(&self, png: &[u8]) -> Raw<Self::Icon>;
    fn top_level_windows(&self) -> Raw<Vec<(Self::Window, u32)>>;
    fn set(&self, window: Self::Window, size: Size, icon: Self::Icon) -> bool;
    fn destroy(&self, icon: Self::Icon);
}

pub(crate) struct Worn<T: Iconing> {
    iconing: T,
    worn: Option<[T::Icon; 2]>,
    unanswered: Vec<[T::Icon; 2]>,
}

impl<T: Iconing> Worn<T> {
    pub(crate) const fn new(iconing: T) -> Self {
        Self {
            iconing,
            worn: None,
            unanswered: Vec::new(),
        }
    }

    pub(crate) fn wear(&mut self, big: &[u8], small: &[u8]) -> Result<usize> {
        let big = self.iconing.icon_of(big).map_err(refused(Op::MakeIcon))?;
        let small = match self.iconing.icon_of(small) {
            Ok(small) => small,
            Err(source) => {
                self.iconing.destroy(big);
                return Err(refused(Op::MakeIcon)(source));
            }
        };

        let windows = match self.iconing.top_level_windows() {
            Ok(windows) => windows,
            Err(source) => {
                self.iconing.destroy(big);
                self.iconing.destroy(small);
                return Err(refused(Op::ListWindows)(source));
            }
        };

        let ours: Vec<T::Window> = windows
            .into_iter()
            .filter(|(_, owner)| *owner == process::id())
            .map(|(window, _)| window)
            .collect();

        let mut answered = true;

        for window in &ours {
            answered &= self.iconing.set(*window, Size::Big, big);
            answered &= self.iconing.set(*window, Size::Small, small);
        }
        self.unanswered.extend(self.worn.replace([big, small]));
        if answered {
            for icon in self.unanswered.drain(..).flatten() {
                self.iconing.destroy(icon);
            }
        }
        Ok(ours.len())
    }
}

#[cfg(windows)]
pub struct WindowIcons(Worn<Windows>);

#[cfg(windows)]
impl WindowIcons {
    pub const fn new() -> Self {
        Self(Worn::new(Windows))
    }

    pub fn wear(&mut self, big: &[u8], small: &[u8]) -> Result<usize> {
        self.0.wear(big, small)
    }
}

#[cfg(windows)]
impl Default for WindowIcons {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
mod real {
    use std::ptr;

    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            CreateIconFromResourceEx, DestroyIcon, EnumWindows, GetWindowThreadProcessId, HICON,
            ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
            WM_SETICON,
        },
    };
    use windows_core::BOOL;

    use super::{Iconing, Size};
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    const ICON_FORMAT: u32 = 0x0003_0000;
    const SET_WITHIN_MS: u32 = 1_000;

    unsafe extern "system" fn listed(window: HWND, into: LPARAM) -> BOOL {
        let mut owner = 0;

        unsafe { GetWindowThreadProcessId(window, Some(&raw mut owner)) };

        let listing = unsafe {
            &mut *ptr::with_exposed_provenance_mut::<Vec<(isize, u32)>>(into.0.cast_unsigned())
        };

        listing.push((window.0.expose_provenance().cast_signed(), owner));
        BOOL::from(true)
    }

    impl Iconing for Windows {
        type Icon = isize;
        type Window = isize;

        fn icon_of(&self, png: &[u8]) -> Raw<isize> {
            let icon =
                unsafe { CreateIconFromResourceEx(png, true, ICON_FORMAT, 0, 0, LR_DEFAULTCOLOR) }
                    .map_err(hresult)?;
            Ok(icon.0.expose_provenance().cast_signed())
        }

        fn top_level_windows(&self) -> Raw<Vec<(isize, u32)>> {
            let mut listing: Vec<(isize, u32)> = Vec::new();

            let into = LPARAM((&raw mut listing).expose_provenance().cast_signed());

            unsafe { EnumWindows(Some(listed), into) }.map_err(hresult)?;
            Ok(listing)
        }

        fn set(&self, window: isize, size: Size, icon: isize) -> bool {
            let which = match size {
                Size::Big => ICON_BIG,
                Size::Small => ICON_SMALL,
            };

            unsafe {
                SendMessageTimeoutW(
                    HWND(ptr::with_exposed_provenance_mut(window.cast_unsigned())),
                    WM_SETICON,
                    WPARAM(which as usize),
                    LPARAM(icon),
                    SMTO_ABORTIFHUNG,
                    SET_WITHIN_MS,
                    None,
                )
            }
            .0 != 0
        }

        fn destroy(&self, icon: isize) {
            let _ = unsafe {
                DestroyIcon(HICON(ptr::with_exposed_provenance_mut(
                    icon.cast_unsigned(),
                )))
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    const STRANGER: u32 = u32::MAX;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Made(u8),
        Listed,
        Set(u8, Size, u8),
        Destroyed(u8),
    }

    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<Call>>>,
        refusing_the_icon: Option<u8>,
        refusing_the_list: bool,
        hung_window: Option<u8>,
    }

    impl Fake {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }
    }

    impl Iconing for Fake {
        type Icon = u8;
        type Window = u8;

        fn icon_of(&self, png: &[u8]) -> Raw<u8> {
            let icon = png[0];

            self.calls.lock().push(Call::Made(icon));
            if self.refusing_the_icon == Some(icon) {
                return Err(HResult(-3));
            }
            Ok(icon)
        }

        fn top_level_windows(&self) -> Raw<Vec<(u8, u32)>> {
            self.calls.lock().push(Call::Listed);
            if self.refusing_the_list {
                return Err(HResult(-4));
            }
            Ok(vec![(1, process::id()), (2, STRANGER), (3, process::id())])
        }

        fn set(&self, window: u8, size: Size, icon: u8) -> bool {
            self.calls.lock().push(Call::Set(window, size, icon));
            self.hung_window != Some(window)
        }

        fn destroy(&self, icon: u8) {
            self.calls.lock().push(Call::Destroyed(icon));
        }
    }

    #[test]
    fn every_window_of_this_process_wears_both_sizes_and_another_process_is_left_alone() {
        let fake = Fake::default();
        let mut worn = Worn::new(fake.clone());

        let windows = worn.wear(&[10], &[11]).expect("worn");

        assert_eq!(windows, 2);
        assert_eq!(
            fake.calls(),
            [
                Call::Made(10),
                Call::Made(11),
                Call::Listed,
                Call::Set(1, Size::Big, 10),
                Call::Set(1, Size::Small, 11),
                Call::Set(3, Size::Big, 10),
                Call::Set(3, Size::Small, 11),
            ]
        );
    }

    #[test]
    fn the_icons_a_change_replaces_are_destroyed_once_the_windows_wear_the_new_ones() {
        let fake = Fake::default();
        let mut worn = Worn::new(fake.clone());

        worn.wear(&[10], &[11]).expect("the first worn");
        fake.calls.lock().clear();
        worn.wear(&[20], &[21]).expect("the second worn");

        let calls = fake.calls();

        assert_eq!(
            calls[calls.len() - 2..],
            [Call::Destroyed(10), Call::Destroyed(11)]
        );
        assert!(!calls.contains(&Call::Destroyed(20)));
    }

    #[test]
    fn an_icon_refused_leaves_nothing_made_and_names_the_call() {
        let fake = Fake {
            refusing_the_icon: Some(11),
            ..Fake::default()
        };
        let mut worn = Worn::new(fake.clone());

        let refused = worn.wear(&[10], &[11]);

        assert!(matches!(
            refused,
            Err(Error::Windows {
                op: Op::MakeIcon,
                source: HResult(-3)
            })
        ));
        assert_eq!(
            fake.calls(),
            [Call::Made(10), Call::Made(11), Call::Destroyed(10)]
        );
    }

    #[test]
    fn a_listing_refused_destroys_both_icons_and_names_the_call() {
        let fake = Fake {
            refusing_the_list: true,
            ..Fake::default()
        };
        let mut worn = Worn::new(fake.clone());

        let refused = worn.wear(&[10], &[11]);

        assert!(matches!(
            refused,
            Err(Error::Windows {
                op: Op::ListWindows,
                ..
            })
        ));
        assert_eq!(
            fake.calls()[3..],
            [Call::Destroyed(10), Call::Destroyed(11)]
        );
    }

    #[test]
    fn a_send_that_timed_out_keeps_the_old_pair_alive() {
        let mut fake = Fake::default();

        let mut worn = Worn::new(fake.clone());

        worn.wear(&[10], &[11]).expect("the first worn");
        fake.hung_window = Some(3);
        worn.iconing = fake.clone();
        fake.calls.lock().clear();
        worn.wear(&[20], &[21]).expect("the second worn");

        let calls = fake.calls();

        assert!(!calls.iter().any(|call| matches!(call, Call::Destroyed(_))));
    }

    #[test]
    fn the_pairs_kept_for_a_hung_window_are_destroyed_once_every_window_answers() {
        let mut fake = Fake::default();

        let mut worn = Worn::new(fake.clone());

        worn.wear(&[10], &[11]).expect("the first worn");
        fake.hung_window = Some(3);
        worn.iconing = fake.clone();
        worn.wear(&[20], &[21]).expect("the second worn");
        fake.hung_window = None;
        worn.iconing = fake.clone();
        fake.calls.lock().clear();
        worn.wear(&[30], &[31]).expect("the third worn");

        let calls = fake.calls();
        let destroyed: Vec<Call> = calls
            .iter()
            .copied()
            .filter(|call| matches!(call, Call::Destroyed(_)))
            .collect();

        assert_eq!(
            destroyed,
            [
                Call::Destroyed(10),
                Call::Destroyed(11),
                Call::Destroyed(20),
                Call::Destroyed(21)
            ]
        );
    }
}
