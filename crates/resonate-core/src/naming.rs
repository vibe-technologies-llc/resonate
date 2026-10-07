use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::{OsStrExt as _, OsStringExt as _},
};

pub const NAME_BYTES_AT_MOST: usize = 255;

pub fn cut_to_leave(name: &OsStr, room: usize) -> &OsStr {
    let budget = NAME_BYTES_AT_MOST.saturating_sub(room);
    let bytes = name.as_bytes();
    if bytes.len() <= budget {
        return name;
    }

    let end = match std::str::from_utf8(bytes) {
        Ok(text) => (0..=budget)
            .rev()
            .find(|at| text.is_char_boundary(*at))
            .unwrap_or_default(),
        Err(_) => budget,
    };
    OsStr::from_bytes(&bytes[..end])
}

pub fn named_within(before: &str, name: &OsStr, after: impl AsRef<OsStr>) -> OsString {
    let after = after.as_ref().as_bytes();
    let kept = cut_to_leave(name, before.len() + after.len());
    let mut named = Vec::with_capacity(before.len() + kept.len() + after.len());
    named.extend_from_slice(before.as_bytes());
    named.extend_from_slice(kept.as_bytes());
    named.extend_from_slice(after);
    OsString::from_vec(named)
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
