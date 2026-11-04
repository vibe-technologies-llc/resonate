use gpui::{Context, FocusHandle, WeakFocusHandle, Window};

use crate::views::root::RootView;

const TAB_STOPS_AT_MOST: usize = 128;

const WELCOMING_TRIES: u8 = 3;

pub(crate) const DELETE_PANEL: &str = "delete-sheet-panel";

pub(crate) const LISTEN_PANEL: &str = "listen-sheet-panel";

pub(crate) const PICKER_PANEL: &str = "playlist-picker-panel-focus";

pub(crate) const MAGNIFIER_PANEL: &str = "magnifier-panel";

pub(crate) const RECORD_PANEL: &str = "record-panel";

pub(crate) const WRAPPED_PANEL: &str = "wrapped-panel";

pub(crate) const SHARING_PANEL: &str = "sharing-panel";

pub(crate) const KEEP_THE_TRACK: &str = "keep-the-track";

pub(crate) const LISTEN_CLOSE: &str = "listen-close";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sheet {
    Deleting,
    Listening,
    Magnifying,
    Naming,
    Recording,
    Wrapped,
    Sharing,
    Menu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Welcome {
    Nowhere,
    Control(&'static str),
    Panel(&'static str),
}

impl Sheet {
    pub(crate) const fn panel(self) -> Option<&'static str> {
        match self {
            Self::Deleting => Some(DELETE_PANEL),
            Self::Listening => Some(LISTEN_PANEL),
            Self::Magnifying => Some(MAGNIFIER_PANEL),
            Self::Naming => Some(PICKER_PANEL),
            Self::Recording => Some(RECORD_PANEL),
            Self::Wrapped => Some(WRAPPED_PANEL),
            Self::Sharing => Some(SHARING_PANEL),
            Self::Menu => None,
        }
    }

    pub(crate) const fn welcome(self) -> Welcome {
        match self {
            Self::Deleting => Welcome::Control(KEEP_THE_TRACK),
            Self::Listening => Welcome::Control(LISTEN_CLOSE),
            Self::Magnifying => Welcome::Panel(MAGNIFIER_PANEL),
            Self::Recording => Welcome::Panel(RECORD_PANEL),
            Self::Wrapped => Welcome::Panel(WRAPPED_PANEL),
            Self::Sharing => Welcome::Panel(SHARING_PANEL),
            Self::Naming | Self::Menu => Welcome::Nowhere,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Unchanged,
    Opened,
    Replaced,
    Closed,
}

impl Change {
    pub(crate) const fn of(before: Option<Sheet>, now: Option<Sheet>) -> Self {
        match (before, now) {
            (None, None) => Self::Unchanged,
            (None, Some(_)) => Self::Opened,
            (Some(_), None) => Self::Closed,
            (Some(before), Some(now)) if before as u8 == now as u8 => Self::Unchanged,
            (Some(_), Some(_)) => Self::Replaced,
        }
    }
}

#[derive(Default)]
pub(crate) struct Standing {
    sheet: Option<Sheet>,
    opener: Option<WeakFocusHandle>,
    offered: Option<WeakFocusHandle>,
    welcomed: bool,
    tries: u8,
}

impl Standing {
    pub(crate) fn offer(&mut self, opener: Option<FocusHandle>) {
        self.offered = opener.map(|opener| opener.downgrade());
    }

    fn opened(&mut self, now: Option<FocusHandle>) {
        self.opener = self
            .offered
            .take()
            .or_else(|| now.map(|now| now.downgrade()));
        self.welcomed = false;
        self.tries = 0;
    }

    fn replaced(&mut self) {
        self.welcomed = false;
        self.tries = 0;
    }

    fn closed(&mut self) -> Option<FocusHandle> {
        self.welcomed = true;
        self.offered = None;
        self.opener.take().and_then(|opener| opener.upgrade())
    }

    fn tried_to_welcome(&mut self) -> bool {
        self.tries += 1;
        self.tries <= WELCOMING_TRIES
    }
}

impl RootView {
    pub(crate) fn sheet_in_front(&self) -> Option<Sheet> {
        if self.deleting.is_some() {
            Some(Sheet::Deleting)
        } else if self.listening_open {
            Some(Sheet::Listening)
        } else if self.magnified.is_some() {
            Some(Sheet::Magnifying)
        } else if self.adding.is_some() {
            Some(Sheet::Naming)
        } else if self.record.is_some() {
            Some(Sheet::Recording)
        } else if self.wrapped.is_some() {
            Some(Sheet::Wrapped)
        } else if self.sharing.is_some() {
            Some(Sheet::Sharing)
        } else if self.menu.is_some() {
            Some(Sheet::Menu)
        } else {
            None
        }
    }

    pub(crate) fn remember_the_opener(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.sheets.offer(window.focused(cx));
    }

    pub(crate) fn keep_the_focus_with_the_sheets(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sheet_panels.opening();

        let now = self.sheet_in_front();
        match Change::of(self.sheets.sheet, now) {
            Change::Unchanged => {}
            Change::Opened => self.sheets.opened(window.focused(cx)),
            Change::Replaced => self.sheets.replaced(),
            Change::Closed => {
                if let Some(opener) = self.sheets.closed() {
                    self.return_the_focus_to(&opener, window, cx);
                } else if self.focus_is_adrift(window, cx) {
                    window.focus(&self.focus, cx);
                }
            }
        }
        self.sheets.sheet = now;

        let Some(sheet) = now else {
            return;
        };
        if self.sheets.welcomed {
            return;
        }
        self.welcome_into(sheet, window, cx);
    }

    fn welcome_into(&mut self, sheet: Sheet, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = match sheet.welcome() {
            Welcome::Nowhere => {
                self.sheets.welcomed = true;
                return;
            }
            Welcome::Control(named) => self.controls.handle(named),
            Welcome::Panel(named) => self.sheet_panels.handle(named),
        };
        match wanted {
            Some(handle) => {
                window.focus(&handle, cx);
                self.sheets.welcomed = true;
            }
            None if self.sheets.tried_to_welcome() => cx.notify(),
            None => self.sheets.welcomed = true,
        }
    }

    fn return_the_focus_to(
        &self,
        opener: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focus_is_adrift(window, cx) {
            window.focus(opener, cx);
        }
    }

    fn focus_is_adrift(&self, window: &Window, cx: &Context<Self>) -> bool {
        match window.focused(cx) {
            None => true,
            Some(here) => {
                here == self.focus
                    || here == self.menu_focus
                    || self.controls.owns(&here)
                    || self.standing_controls.owns(&here)
                    || self.sheet_panels.owns(&here)
            }
        }
    }

    pub(crate) fn front_panel(&self) -> Option<FocusHandle> {
        self.sheets
            .sheet
            .and_then(Sheet::panel)
            .and_then(|named| self.sheet_panels.handle(named))
    }

    pub(crate) fn step_the_focus(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.front_panel() else {
            match forward {
                true => window.focus_next(cx),
                false => window.focus_prev(cx),
            }
            return;
        };

        for _ in 0..TAB_STOPS_AT_MOST {
            match forward {
                true => window.focus_next(cx),
                false => window.focus_prev(cx),
            }
            if panel.contains_focused(window, cx) {
                return;
            }
        }
        window.focus(&panel, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_opening_replacing_or_closing_is_told_apart() {
        assert_eq!(Change::of(None, None), Change::Unchanged);
        assert_eq!(Change::of(None, Some(Sheet::Deleting)), Change::Opened);
        assert_eq!(
            Change::of(Some(Sheet::Deleting), Some(Sheet::Deleting)),
            Change::Unchanged
        );
        assert_eq!(
            Change::of(Some(Sheet::Menu), Some(Sheet::Deleting)),
            Change::Replaced
        );
        assert_eq!(Change::of(Some(Sheet::Naming), None), Change::Closed);
    }

    #[test]
    fn a_sheet_with_a_control_to_land_on_lands_on_the_safe_one() {
        assert_eq!(
            Sheet::Deleting.welcome(),
            Welcome::Control(KEEP_THE_TRACK),
            "no key may land on the control that deletes"
        );
        assert_eq!(Sheet::Listening.welcome(), Welcome::Control(LISTEN_CLOSE));
        assert_eq!(Sheet::Magnifying.welcome(), Welcome::Panel(MAGNIFIER_PANEL));
        assert_eq!(Sheet::Naming.welcome(), Welcome::Nowhere);
        assert_eq!(Sheet::Menu.panel(), None);
    }
}
