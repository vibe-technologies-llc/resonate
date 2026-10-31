use gpui::{App, ClipboardItem};

pub(crate) fn copy(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}
