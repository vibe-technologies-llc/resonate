use std::cell::RefCell;

use ahash::{AHashMap, AHashSet};
use gpui::{App, FocusHandle, SharedString, Window};

#[derive(Default)]
pub(crate) struct Controls {
    handles: RefCell<AHashMap<SharedString, FocusHandle>>,
    drawn: RefCell<AHashSet<SharedString>>,
}

impl Controls {
    pub(crate) fn opening(&self) {
        let mut drawn = self.drawn.borrow_mut();
        self.handles
            .borrow_mut()
            .retain(|named, _| drawn.contains(named));
        drawn.clear();
    }

    pub(crate) fn at(&self, named: impl Into<SharedString>, cx: &App) -> FocusHandle {
        let named = named.into();
        self.drawn.borrow_mut().insert(named.clone());

        self.handles
            .borrow_mut()
            .entry(named)
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone()
    }

    pub(crate) fn holding(&self, named: impl Into<SharedString>, cx: &App) -> FocusHandle {
        let named = named.into();
        self.drawn.borrow_mut().insert(named.clone());

        self.handles
            .borrow_mut()
            .entry(named)
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    pub(crate) fn holds_the_caret(&self, window: &Window) -> bool {
        self.handles
            .borrow()
            .values()
            .any(|handle| handle.is_focused(window))
    }

    pub(crate) fn lets_go(&self, window: &mut Window) -> bool {
        let held = self.holds_the_caret(window);
        if held {
            window.blur();
        }
        held
    }
}
