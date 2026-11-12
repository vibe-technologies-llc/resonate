use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hasher},
    io::{self, Read},
};

const COMPARED_AT_ONCE: usize = 256 * 1024;

pub fn identical_among<T, R: Read>(
    sharing_a_size: Vec<T>,
    opened: impl Fn(&T) -> Option<R>,
) -> Vec<Vec<T>> {
    let mut by_digest: BTreeMap<u64, Vec<T>> = BTreeMap::new();
    for member in sharing_a_size {
        if let Some(digest) = opened(&member).and_then(digest_of) {
            by_digest.entry(digest).or_default().push(member);
        }
    }

    let mut classes = Vec::new();
    for sharing_a_digest in by_digest.into_values().filter(|members| members.len() > 1) {
        let mut confirmed: Vec<Vec<T>> = Vec::new();
        for member in sharing_a_digest {
            let class =
                confirmed
                    .iter_mut()
                    .find(|class| match (opened(&class[0]), opened(&member)) {
                        (Some(one), Some(other)) => same_bytes(one, other),
                        _ => false,
                    });
            match class {
                Some(class) => class.push(member),
                None => confirmed.push(vec![member]),
            }
        }
        classes.extend(confirmed.into_iter().filter(|class| class.len() > 1));
    }
    classes
}

pub(crate) fn digest_of(mut stream: impl Read) -> Option<u64> {
    let mut chunk = vec![0_u8; COMPARED_AT_ONCE];
    let mut hasher = DefaultHasher::new();

    loop {
        match filled(&mut stream, &mut chunk).ok()? {
            0 => return Some(hasher.finish()),
            read => hasher.write(&chunk[..read]),
        }
    }
}

pub(crate) fn same_bytes(mut one: impl Read, mut other: impl Read) -> bool {
    let mut held = vec![0_u8; COMPARED_AT_ONCE];
    let mut copied = vec![0_u8; COMPARED_AT_ONCE];

    loop {
        let (Ok(read), Ok(again)) = (filled(&mut one, &mut held), filled(&mut other, &mut copied))
        else {
            return false;
        };
        if read != again || held[..read] != copied[..again] {
            return false;
        }
        if read == 0 {
            return true;
        }
    }
}

fn filled(stream: &mut impl Read, into: &mut [u8]) -> io::Result<usize> {
    let mut held = 0;
    while held < into.len() {
        match stream.read(&mut into[held..]) {
            Ok(0) => break,
            Ok(read) => held += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(held)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(bytes: &&'static [u8]) -> Option<&'static [u8]> {
        Some(*bytes)
    }

    #[test]
    fn songs_holding_the_same_bytes_are_one_class_and_a_differing_one_of_the_same_size_is_left_out()
    {
        let same: &[u8] = b"the same song";
        let again: &[u8] = b"the same song";
        let other: &[u8] = b"another  song";

        let classes = identical_among(vec![same, other, again], opened);

        assert_eq!(classes, vec![vec![same, again]]);
    }

    #[test]
    fn a_song_that_will_not_open_is_in_no_class() {
        let same: &[u8] = b"the same song";

        let classes = identical_among(vec![same, same], |_: &&[u8]| None::<&[u8]>);

        assert!(classes.is_empty());
    }

    #[test]
    fn bytes_compared_past_a_chunk_still_tell_a_late_difference() {
        let mut one = vec![7_u8; COMPARED_AT_ONCE + 10];
        let other = one.clone();

        assert!(same_bytes(one.as_slice(), other.as_slice()));

        one[COMPARED_AT_ONCE + 5] = 8;
        assert!(!same_bytes(one.as_slice(), other.as_slice()));
    }
}
