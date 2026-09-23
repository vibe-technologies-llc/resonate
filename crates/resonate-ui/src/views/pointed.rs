use std::cell::RefCell;

use gpui::{ElementId, StatefulInteractiveElement, Window, prelude::FluentBuilder};

thread_local! {
    static POINTED_AT: RefCell<Vec<ElementId>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn is_pointed_at(id: &ElementId) -> bool {
    POINTED_AT.with_borrow(|pointed| pointed.contains(id))
}

pub(crate) fn forget() {
    POINTED_AT.with_borrow_mut(Vec::clear);
}

pub(crate) fn let_go(window: &mut Window) {
    let held = POINTED_AT.with_borrow_mut(|pointed| !std::mem::take(pointed).is_empty());
    if held {
        window.refresh();
    }
}

fn point_at(id: &ElementId, hovered: bool, window: &mut Window) {
    if POINTED_AT.with_borrow_mut(|pointed| moves(pointed, id, hovered)) {
        window.refresh();
    }
}

fn moves(pointed: &mut Vec<ElementId>, id: &ElementId, hovered: bool) -> bool {
    let held = pointed.contains(id);
    match (hovered, held) {
        (true, false) => pointed.push(id.clone()),
        (false, true) => pointed.retain(|other| other != id),
        (true, true) | (false, false) => return false,
    }

    true
}

pub(crate) trait LitUnderThePointer: StatefulInteractiveElement + FluentBuilder {
    fn follows_the_pointer(self, id: impl Into<ElementId>) -> Self {
        let id = id.into();

        self.on_hover(move |hovered, window, _| point_at(&id, *hovered, window))
    }

    fn lit_under_the_pointer(
        self,
        id: impl Into<ElementId>,
        lit: impl FnOnce(Self) -> Self,
    ) -> Self {
        let id = id.into();
        let pointed = is_pointed_at(&id);

        self.follows_the_pointer(id).when(pointed, lit)
    }
}

impl<E: StatefulInteractiveElement + FluentBuilder> LitUnderThePointer for E {}

#[cfg(test)]
mod tests {
    use std::slice;

    use super::*;

    #[test]
    fn the_pointer_lights_what_it_enters_and_only_leaving_that_puts_it_out() {
        let cell = ElementId::from(("album-cell", 3usize));
        let artist = ElementId::from(("album-artist", 3usize));
        let mut pointed = Vec::new();

        assert!(moves(&mut pointed, &cell, true));
        assert!(!moves(&mut pointed, &cell, true));
        assert!(moves(&mut pointed, &artist, true));
        assert_eq!(pointed, [cell.clone(), artist.clone()]);
        assert!(moves(&mut pointed, &artist, false));
        assert!(!moves(&mut pointed, &artist, false));
        assert_eq!(pointed, slice::from_ref(&cell));
        assert!(moves(&mut pointed, &cell, false));
        assert!(pointed.is_empty());
    }
}
