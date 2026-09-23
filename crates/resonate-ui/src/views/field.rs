use std::{ops::Range, time::Duration};

use gpui::{
    App, Bounds, ContentMask, Context, CursorStyle, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render, ShapedLine,
    SharedString, Style, Task, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill,
    point, prelude::*, px, relative, rgb, rgba, size,
};

use crate::{
    app::SEARCH_CONTEXT,
    clipboard,
    edit::{Anchor, Edit, Motion, Removal},
    theme,
};

actions!(
    resonate_search,
    [
        CaretLeft,
        CaretRight,
        CaretWordLeft,
        CaretWordRight,
        CaretToStart,
        CaretToEnd,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectToStart,
        SelectToEnd,
        SelectEverything,
        DeleteBackward,
        DeleteForward,
        DeleteWordBackward,
        DeleteWordForward,
        CopySelection,
        CutSelection,
        PasteClipboard,
        Undo,
        Redo,
        Submit,
    ]
);

pub(crate) struct Submitted;

pub(crate) fn bindings() -> Vec<KeyBinding> {
    let inside = Some(SEARCH_CONTEXT);

    vec![
        KeyBinding::new("left", CaretLeft, inside),
        KeyBinding::new("right", CaretRight, inside),
        KeyBinding::new("ctrl-left", CaretWordLeft, inside),
        KeyBinding::new("ctrl-right", CaretWordRight, inside),
        KeyBinding::new("home", CaretToStart, inside),
        KeyBinding::new("end", CaretToEnd, inside),
        KeyBinding::new("shift-left", SelectLeft, inside),
        KeyBinding::new("shift-right", SelectRight, inside),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, inside),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, inside),
        KeyBinding::new("shift-home", SelectToStart, inside),
        KeyBinding::new("shift-end", SelectToEnd, inside),
        KeyBinding::new("ctrl-a", SelectEverything, inside),
        KeyBinding::new("backspace", DeleteBackward, inside),
        KeyBinding::new("delete", DeleteForward, inside),
        KeyBinding::new("ctrl-backspace", DeleteWordBackward, inside),
        KeyBinding::new("ctrl-delete", DeleteWordForward, inside),
        KeyBinding::new("ctrl-c", CopySelection, inside),
        KeyBinding::new("ctrl-x", CutSelection, inside),
        KeyBinding::new("ctrl-v", PasteClipboard, inside),
        KeyBinding::new("ctrl-z", Undo, inside),
        KeyBinding::new("ctrl-shift-z", Redo, inside),
        KeyBinding::new("ctrl-y", Redo, inside),
        KeyBinding::new("enter", Submit, inside),
    ]
}

const BLINK: Duration = Duration::from_millis(500);

struct Painted {
    line: ShapedLine,
    bounds: Bounds<Pixels>,
    scrolled_by: Pixels,
}

pub(crate) struct Field {
    edit: Edit,
    marked: Option<Range<usize>>,
    placeholder: SharedString,
    focus: FocusHandle,
    painted: Option<Painted>,
    scrolled_by: Pixels,
    dragging: bool,
    holds_focus: bool,
    lit: bool,
    blink: Task<()>,
}

impl Field {
    pub(crate) fn new(
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        cx.on_focus(&focus, window, |this, _, cx| this.take_the_caret(cx))
            .detach();
        cx.on_blur(&focus, window, |this, _, cx| this.drop_the_caret(cx))
            .detach();

        Self {
            edit: Edit::default(),
            marked: None,
            placeholder: SharedString::new_static(placeholder),
            focus,
            painted: None,
            scrolled_by: px(0.0),
            dragging: false,
            holds_focus: false,
            lit: false,
            blink: Task::ready(()),
        }
    }

    const fn caret_is_lit(&self) -> bool {
        self.lit
    }

    fn take_the_caret(&mut self, cx: &mut Context<Self>) {
        self.holds_focus = true;
        self.touched(cx);
    }

    fn drop_the_caret(&mut self, cx: &mut Context<Self>) {
        self.holds_focus = false;
        self.lit = false;
        self.blink = Task::ready(());
        cx.notify();
    }

    fn touched(&mut self, cx: &mut Context<Self>) {
        self.lit = true;
        if self.holds_focus {
            self.blink = cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(BLINK).await;
                    let shown = this.update(cx, |this, cx| {
                        this.lit = !this.lit;
                        cx.notify();
                    });
                    if shown.is_err() {
                        return;
                    }
                }
            });
        }
        cx.notify();
    }

    pub(crate) fn text(&self) -> &str {
        self.edit.content()
    }

    pub(crate) fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    pub(crate) fn take_focus(&self, window: &mut Window) {
        window.focus(&self.focus);
    }

    pub(crate) fn take_focus_and_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.edit.select_all();
        self.touched(cx);
    }

    pub(crate) fn copies(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.put_on_clipboard(cx);
    }

    pub(crate) fn cuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.cut_selection(&CutSelection, window, cx);
    }

    pub(crate) fn pastes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.paste_clipboard(&PasteClipboard, window, cx);
    }

    pub(crate) fn selects_everything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_focus_and_select(window, cx);
    }

    pub(crate) fn holds_a_selection(&self) -> bool {
        !self.edit.selection().is_empty()
    }

    pub(crate) fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.edit.set_content(text);
        self.edit.select_all();
        self.marked = None;
        self.touched(cx);
    }

    pub(crate) fn hold(&mut self, text: String, cx: &mut Context<Self>) {
        self.edit.set_content(text);
        self.marked = None;
        self.touched(cx);
    }

    pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
        self.edit.set_content(String::new());
        self.marked = None;
        self.touched(cx);
    }

    pub(crate) fn append(&mut self, text: &str, cx: &mut Context<Self>) {
        self.edit.go(Motion::End, Anchor::Collapse);
        self.edit.insert(text);
        self.touched(cx);
    }

    pub(crate) fn drop_last(&mut self, cx: &mut Context<Self>) {
        self.edit.go(Motion::End, Anchor::Collapse);
        self.edit.remove(Removal::Backward);
        self.touched(cx);
    }

    fn go(&mut self, motion: Motion, anchor: Anchor, cx: &mut Context<Self>) {
        self.edit.go(motion, anchor);
        self.touched(cx);
    }

    fn remove(&mut self, removal: Removal, cx: &mut Context<Self>) {
        self.edit.remove(removal);
        self.marked = None;
        self.touched(cx);
    }

    fn select_everything(&mut self, _: &SelectEverything, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.select_all();
        self.touched(cx);
    }

    fn copy_selection(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        self.put_on_clipboard(cx);
    }

    fn cut_selection(&mut self, _: &CutSelection, _: &mut Window, cx: &mut Context<Self>) {
        if !self.put_on_clipboard(cx) {
            return;
        }
        self.edit.delete_selection();
        self.touched(cx);
    }

    fn paste_clipboard(&mut self, _: &PasteClipboard, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.edit.paste(&one_line(&text));
        self.marked = None;
        self.touched(cx);
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Submitted);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.edit.undo() {
            self.marked = None;
            self.touched(cx);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.edit.redo() {
            self.marked = None;
            self.touched(cx);
        }
    }

    fn put_on_clipboard(&self, cx: &mut Context<Self>) -> bool {
        let selected = self.edit.selected();
        if selected.is_empty() {
            return false;
        }
        clipboard::copy(selected.to_owned(), cx);
        true
    }

    fn press(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        let at = self.index_at(event.position);

        match event.click_count {
            0 | 1 if event.modifiers.shift => self.edit.select_to(at),
            0 | 1 => self.edit.move_to(at),
            2 => self.edit.select(self.edit.word_at(at)),
            _ => self.edit.select_all(),
        }

        self.dragging = true;
        self.touched(cx);
    }

    fn drag(&mut self, at: Point<Pixels>, held: Option<MouseButton>, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if held != Some(MouseButton::Left) {
            self.dragging = false;
            return;
        }

        let to = self.index_at(at);
        if to == self.edit.cursor() {
            return;
        }
        self.edit.select_to(to);
        self.touched(cx);
    }

    fn release(&mut self) {
        self.dragging = false;
    }

    fn index_at(&self, position: Point<Pixels>) -> usize {
        if self.edit.content().is_empty() {
            return 0;
        }
        let Some(painted) = self.painted.as_ref() else {
            return 0;
        };
        let x = position.x - painted.bounds.left() + painted.scrolled_by;
        painted.line.closest_index_for_x(x)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        offset_of(self.edit.content(), range.start)..offset_of(self.edit.content(), range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        utf16_of(self.edit.content(), range.start)..utf16_of(self.edit.content(), range.end)
    }

    fn target_of(&self, range: Option<Range<usize>>) -> Range<usize> {
        range
            .map(|range| self.range_from_utf16(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.edit.selection())
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn offset_of(text: &str, utf16: usize) -> usize {
    let mut counted = 0;
    let mut at = 0;
    for character in text.chars() {
        if counted >= utf16 {
            break;
        }
        counted += character.len_utf16();
        at += character.len_utf8();
    }
    at
}

fn utf16_of(text: &str, offset: usize) -> usize {
    let mut counted = 0;
    let mut at = 0;
    for character in text.chars() {
        if counted >= offset {
            break;
        }
        counted += character.len_utf8();
        at += character.len_utf16();
    }
    at
}

impl EventEmitter<Submitted> for Field {}

impl Focusable for Field {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Field {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .tab_stop(true)
            .key_context(SEARCH_CONTEXT)
            .flex()
            .flex_1()
            .min_w(px(0.0))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(|this, _: &CaretLeft, _, cx| {
                this.go(Motion::Left, Anchor::Collapse, cx);
            }))
            .on_action(cx.listener(|this, _: &CaretRight, _, cx| {
                this.go(Motion::Right, Anchor::Collapse, cx);
            }))
            .on_action(cx.listener(|this, _: &CaretWordLeft, _, cx| {
                this.go(Motion::WordLeft, Anchor::Collapse, cx);
            }))
            .on_action(cx.listener(|this, _: &CaretWordRight, _, cx| {
                this.go(Motion::WordRight, Anchor::Collapse, cx);
            }))
            .on_action(cx.listener(|this, _: &CaretToStart, _, cx| {
                this.go(Motion::Start, Anchor::Collapse, cx);
            }))
            .on_action(cx.listener(|this, _: &CaretToEnd, _, cx| {
                this.go(Motion::End, Anchor::Collapse, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| {
                this.go(Motion::Left, Anchor::Extend, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| {
                this.go(Motion::Right, Anchor::Extend, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| {
                this.go(Motion::WordLeft, Anchor::Extend, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| {
                this.go(Motion::WordRight, Anchor::Extend, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectToStart, _, cx| {
                this.go(Motion::Start, Anchor::Extend, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectToEnd, _, cx| {
                this.go(Motion::End, Anchor::Extend, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteBackward, _, cx| {
                this.remove(Removal::Backward, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteForward, _, cx| {
                this.remove(Removal::Forward, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteWordBackward, _, cx| {
                this.remove(Removal::WordBackward, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteWordForward, _, cx| {
                this.remove(Removal::WordForward, cx);
            }))
            .on_action(cx.listener(Self::select_everything))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::cut_selection))
            .on_action(cx.listener(Self::paste_clipboard))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::submit))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::press))
            .child(Line { field: cx.entity() })
    }
}

impl EntityInputHandler for Field {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range);
        adjusted.replace(self.range_to_utf16(&range));
        self.edit.content().get(range).map(str::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.edit.selection()),
            reversed: self.edit.is_reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|marked| self.range_to_utf16(marked))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.target_of(range);
        self.edit.replace(target, &one_line(text));
        self.marked = None;
        self.touched(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.target_of(range);
        let start = target.start;
        self.edit.replace(target, text);

        self.marked = (!text.is_empty()).then(|| start..start + text.len());
        if let Some(selected) = selected {
            let from = start + offset_of(text, selected.start);
            let to = start + offset_of(text, selected.end);
            self.edit.select(from..to);
        }
        self.touched(cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let painted = self.painted.as_ref()?;
        let range = self.range_from_utf16(&range);
        let left = element_bounds.left() - painted.scrolled_by;

        Some(Bounds::from_corners(
            point(
                left + painted.line.x_for_index(range.start),
                element_bounds.top(),
            ),
            point(
                left + painted.line.x_for_index(range.end),
                element_bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let painted = self.painted.as_ref()?;
        painted.bounds.contains(&point).then_some(())?;
        Some(utf16_of(self.edit.content(), self.index_at(point)))
    }
}

struct Line {
    field: Entity<Field>,
}

struct Drawing {
    line: ShapedLine,
    origin: Point<Pixels>,
    selection: Option<PaintQuad>,
    caret: Option<PaintQuad>,
    scrolled_by: Pixels,
}

impl IntoElement for Line {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Line {
    type RequestLayoutState = ();
    type PrepaintState = Option<Drawing>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());

        let field = self.field.read(cx);
        let focused = field.focus.is_focused(window);
        let lit = field.caret_is_lit();
        let selection = field.edit.selection();
        let cursor = field.edit.cursor();
        let empty = field.edit.content().is_empty();
        let held = field.scrolled_by;

        let (text, colour) = if empty && !focused {
            (field.placeholder.clone(), rgb(theme::faint()).into())
        } else {
            (
                SharedString::from(field.edit.content().to_owned()),
                style.color,
            )
        };
        let runs = runs_for(&text, colour, field.marked.as_ref(), &style.font());

        let line = window
            .text_system()
            .shape_line(text, font_size, &runs, None);

        let caret_at = line.x_for_index(cursor);
        let scrolled_by = scrolled_to_show(caret_at, line.width, bounds.size.width, held);
        let origin = point(bounds.left() - scrolled_by, bounds.top());

        let caret = fill(
            Bounds::new(
                point(
                    origin.x + caret_at,
                    bounds.center().y - theme::width(theme::caret_height() / 2.0),
                ),
                size(
                    theme::width(theme::CARET_WIDTH),
                    theme::width(theme::caret_height()),
                ),
            ),
            rgb(theme::accent()),
        );
        let highlight = (!selection.is_empty()).then(|| {
            fill(
                Bounds::from_corners(
                    point(origin.x + line.x_for_index(selection.start), bounds.top()),
                    point(origin.x + line.x_for_index(selection.end), bounds.bottom()),
                ),
                rgba(theme::selection()),
            )
        });

        Some(Drawing {
            line,
            origin,
            selection: highlight,
            caret: lit.then_some(caret),
            scrolled_by,
        })
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        drawing: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.field.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.field.clone()),
            cx,
        );
        self.follow_the_pointer(window);

        let Some(drawing) = drawing.take() else {
            return;
        };
        let focused = focus.is_focused(window);
        let line_height = window.line_height();

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = drawing.selection {
                window.paint_quad(selection);
            }
            if let Err(error) = drawing.line.paint(drawing.origin, line_height, window, cx) {
                tracing::warn!(%error, "the search field's text could not be painted");
            }
            if let Some(caret) = drawing.caret.filter(|_| focused) {
                window.paint_quad(caret);
            }
        });

        self.field.update(cx, |field, _| {
            field.scrolled_by = drawing.scrolled_by;
            field.painted = Some(Painted {
                line: drawing.line,
                bounds,
                scrolled_by: drawing.scrolled_by,
            });
        });
    }
}

impl Line {
    fn follow_the_pointer(&self, window: &mut Window) {
        let moved = self.field.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                moved.update(cx, |field, cx| {
                    field.drag(event.position, event.pressed_button, cx);
                });
            }
        });

        let released = self.field.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                released.update(cx, |field, _| field.release());
            }
        });
    }
}

fn runs_for(
    text: &str,
    colour: gpui::Hsla,
    marked: Option<&Range<usize>>,
    font: &gpui::Font,
) -> Vec<TextRun> {
    if text.is_empty() {
        return Vec::new();
    }

    let whole = TextRun {
        len: text.len(),
        font: font.clone(),
        color: colour,
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    let Some(marked) = marked.filter(|marked| marked.end <= text.len()) else {
        return vec![whole];
    };

    [
        TextRun {
            len: marked.start,
            ..whole.clone()
        },
        TextRun {
            len: marked.end - marked.start,
            underline: Some(UnderlineStyle {
                color: Some(colour),
                thickness: px(1.0),
                wavy: false,
            }),
            ..whole.clone()
        },
        TextRun {
            len: text.len() - marked.end,
            ..whole
        },
    ]
    .into_iter()
    .filter(|run| run.len > 0)
    .collect()
}

fn scrolled_to_show(caret: Pixels, line: Pixels, visible: Pixels, held: Pixels) -> Pixels {
    let overflow = (line - visible).max(px(0.0));
    let caret_width = theme::width(theme::CARET_WIDTH);
    let trailing = (caret + caret_width - visible).max(px(0.0));

    held.min(caret).max(trailing).clamp(px(0.0), overflow)
}
