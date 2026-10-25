use std::cell::RefCell;

use ahash::{AHashMap, AHashSet};
use gpui::{App, ClickEvent, FocusHandle, InteractiveElement, SharedString, Window};

use crate::views::hint::{self, Wording};

pub(crate) fn by_the_pointer(click: &ClickEvent) -> bool {
    matches!(click, ClickEvent::Mouse(_))
}

#[derive(Default)]
pub(crate) struct Controls {
    handles: RefCell<AHashMap<SharedString, FocusHandle>>,
    drawn: RefCell<AHashSet<SharedString>>,
    captions: RefCell<AHashMap<SharedString, Wording>>,
}

impl Controls {
    pub(crate) fn opening(&self) {
        let mut drawn = self.drawn.borrow_mut();
        self.handles
            .borrow_mut()
            .retain(|named, _| drawn.contains(named));
        self.captions
            .borrow_mut()
            .retain(|named, _| drawn.contains(named));
        drawn.clear();
    }

    pub(crate) fn captioned_by_its_name(
        &self,
        named: impl Into<SharedString>,
        control: &mut impl InteractiveElement,
    ) {
        let Some(id) = control.interactivity().element_id.clone() else {
            return;
        };
        if let Some(wording) = hint::wording_of(&id) {
            self.captions.borrow_mut().insert(named.into(), wording);
        }
    }

    pub(crate) fn caption_of_the_focused(&self, window: &Window) -> Option<SharedString> {
        let handles = self.handles.borrow();
        let (named, _) = handles
            .iter()
            .find(|(_, handle)| handle.is_focused(window))?;

        self.captions.borrow().get(named).map(|wording| wording())
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

    pub(crate) fn lets_go(&self, window: &mut Window, cx: &mut App) -> bool {
        let held = self.holds_the_caret(window);
        if held {
            window.blur(cx);
        }
        held
    }
}
