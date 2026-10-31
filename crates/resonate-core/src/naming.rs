use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
};

use crate::os_bytes;

pub const NAME_BYTES_AT_MOST: usize = 255;

pub fn cut_to_leave(name: &OsStr, room: usize) -> Cow<'_, OsStr> {
    let budget = NAME_BYTES_AT_MOST.saturating_sub(room);
    let bytes = os_bytes::bytes_of(name);
    if bytes.len() <= budget {
        return Cow::Borrowed(name);
    }

    let end = match std::str::from_utf8(&bytes) {
        Ok(text) => (0..=budget)
            .rev()
            .find(|at| text.is_char_boundary(*at))
            .unwrap_or_default(),
        Err(_) => budget,
    };
    Cow::Owned(os_bytes::os_string_from(
        bytes.get(..end).unwrap_or_default().to_vec(),
    ))
}

pub fn named_within(before: &str, name: &OsStr, after: impl AsRef<OsStr>) -> OsString {
    let after = os_bytes::bytes_of(after.as_ref());
    let kept = cut_to_leave(name, before.len() + after.len());
    let kept = os_bytes::bytes_of(&kept);
    let mut named = Vec::with_capacity(before.len() + kept.len() + after.len());
    named.extend_from_slice(before.as_bytes());
    named.extend_from_slice(&kept);
    named.extend_from_slice(&after);
    os_bytes::os_string_from(named)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_fits_is_kept_whole() {
        let named = named_within(".", OsStr::new("Echoes.flac"), ".123-0.resonate-part");

        assert_eq!(named, OsString::from(".Echoes.flac.123-0.resonate-part"));
    }

    #[test]
    fn a_name_near_the_limit_is_cut_on_a_letter_to_leave_room_for_what_goes_around_it() {
        let long = format!("{}.flac", "é".repeat(125));
        assert_eq!(long.len(), 255);

        let named = named_within(".", OsStr::new(&long), ".4242-17.resonate-delivery");

        assert!(named.len() <= NAME_BYTES_AT_MOST, "{}", named.len());
        let text = named.to_str().expect("a cut that keeps every letter whole");
        assert!(text.starts_with(".éé"));
        assert!(text.ends_with(".4242-17.resonate-delivery"));
    }
}
