#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wanted {
    From(u64),
    Between(u64, u64),
    Last(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) first: u64,
    pub(crate) last: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Unsatisfiable;

fn number(text: &str) -> Option<u64> {
    text.bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| text.parse().ok())
        .flatten()
}

impl Wanted {
    pub(crate) fn parse(header: &str) -> Option<Self> {
        let specification = header.trim().strip_prefix("bytes=")?;
        if specification.contains(',') {
            return None;
        }
        let (first, last) = specification.split_once('-')?;
        let (first, last) = (first.trim(), last.trim());
        match (first.is_empty(), last.is_empty()) {
            (true, true) => None,
            (true, false) => number(last).map(Self::Last),
            (false, true) => number(first).map(Self::From),
            (false, false) => {
                let (first, last) = (number(first)?, number(last)?);
                (first <= last).then_some(Self::Between(first, last))
            }
        }
    }

    pub(crate) fn within(self, total: u64) -> Result<Span, Unsatisfiable> {
        let last_byte = total.checked_sub(1).ok_or(Unsatisfiable)?;
        match self {
            Self::From(first) if first <= last_byte => Ok(Span {
                first,
                last: last_byte,
            }),
            Self::Between(first, last) if first <= last_byte => Ok(Span {
                first,
                last: last.min(last_byte),
            }),
            Self::Last(count) if count > 0 => Ok(Span {
                first: total.saturating_sub(count),
                last: last_byte,
            }),
            Self::From(_) | Self::Between(..) | Self::Last(_) => Err(Unsatisfiable),
        }
    }
}

impl Span {
    pub(crate) const fn bytes(self) -> u64 {
        self.last - self.first + 1
    }
}

pub(crate) fn windowed(
    wanted: Option<Wanted>,
    length: Option<u64>,
) -> Result<Option<Span>, Unsatisfiable> {
    match (wanted, length) {
        (Some(wanted), Some(total)) => wanted.within(total).map(Some),
        (Some(_), None) | (None, _) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_byte_range_reads_in_each_of_its_three_forms() {
        assert_eq!(Wanted::parse("bytes=100-"), Some(Wanted::From(100)));
        assert_eq!(Wanted::parse("bytes=0-99"), Some(Wanted::Between(0, 99)));
        assert_eq!(Wanted::parse(" bytes=-500 "), Some(Wanted::Last(500)));
    }

    #[test]
    fn a_range_that_is_not_one_single_byte_range_is_ignored() {
        for header in [
            "bytes=0-1,5-6",
            "bytes=-",
            "bytes=9-3",
            "bytes=a-3",
            "bytes=+1-",
            "items=0-3",
            "0-3",
            "",
        ] {
            assert_eq!(Wanted::parse(header), None, "{header}");
        }
    }

    #[test]
    fn a_range_is_cut_to_the_bytes_there_are() {
        assert_eq!(
            Wanted::From(10).within(100),
            Ok(Span {
                first: 10,
                last: 99
            })
        );
        assert_eq!(
            Wanted::Between(10, 5000).within(100),
            Ok(Span {
                first: 10,
                last: 99
            })
        );
        assert_eq!(
            Wanted::Last(30).within(100),
            Ok(Span {
                first: 70,
                last: 99
            })
        );
        assert_eq!(
            Wanted::Last(500).within(100),
            Ok(Span { first: 0, last: 99 })
        );
        assert_eq!(Wanted::Between(10, 19).within(100).map(Span::bytes), Ok(10));
    }

    #[test]
    fn a_range_starting_past_the_end_or_of_nothing_is_unsatisfiable() {
        assert_eq!(Wanted::From(100).within(100), Err(Unsatisfiable));
        assert_eq!(Wanted::Between(100, 200).within(100), Err(Unsatisfiable));
        assert_eq!(Wanted::Last(0).within(100), Err(Unsatisfiable));
        assert_eq!(Wanted::From(0).within(0), Err(Unsatisfiable));
    }

    #[test]
    fn a_range_needs_a_length_to_be_honoured() {
        assert_eq!(windowed(Some(Wanted::From(5)), None), Ok(None));
        assert_eq!(windowed(None, Some(10)), Ok(None));
        assert_eq!(
            windowed(Some(Wanted::From(5)), Some(10)),
            Ok(Some(Span { first: 5, last: 9 }))
        );
        assert_eq!(
            windowed(Some(Wanted::From(10)), Some(10)),
            Err(Unsatisfiable)
        );
    }
}
