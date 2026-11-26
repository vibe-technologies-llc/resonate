use std::{borrow::Cow, iter, time::Duration};

const ESCAPE: char = '\\';
const BRACKET: char = '[';
const SECONDS_PER_MINUTE: u64 = 60;
const MINUTES_PER_HOUR: u64 = 60;
const COLONS_WHEN_HOURS_ARE_WRITTEN: usize = 2;
const NANOSECOND_DIGITS: usize = 9;
const VOICE_MARKERS: [(&str, VoiceMarker); 2] =
    [("[v1:", VoiceMarker::One), ("[v2:", VoiceMarker::Two)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identified {
    Title,
    Artist,
    Album,
    Author,
    Transcriber,
    Editor,
    Version,
    Length,
    Offset,
}

impl Identified {
    const NAMED: [(&'static str, Self); 10] = [
        ("ti", Self::Title),
        ("ar", Self::Artist),
        ("al", Self::Album),
        ("au", Self::Author),
        ("by", Self::Transcriber),
        ("re", Self::Editor),
        ("tool", Self::Editor),
        ("ve", Self::Version),
        ("length", Self::Length),
        ("offset", Self::Offset),
    ];

    fn read(name: &str) -> Option<Self> {
        Self::NAMED
            .iter()
            .find(|(tag, _)| name.eq_ignore_ascii_case(tag))
            .map(|(_, identified)| *identified)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceMarker {
    One,
    Two,
}

pub enum Bracket<'a> {
    Moment(Duration),
    Identifying { tag: Identified, value: &'a str },
    Text,
}

impl<'a> Bracket<'a> {
    pub fn read(inside: &'a str) -> Self {
        if let Some(at) = moment(inside).or_else(|| span(inside)) {
            return Self::Moment(at);
        }

        let Some((name, value)) = inside.split_once(':') else {
            return Self::Text;
        };
        let Some(tag) = Identified::read(name.trim()) else {
            return Self::Text;
        };

        Self::Identifying { tag, value }
    }
}

pub fn escaped_for_lrc(text: &str) -> Cow<'_, str> {
    if is_taken_by_the_reader(text) || leads_with_escaped_bracket(text) {
        Cow::Owned(format!("{ESCAPE}{text}"))
    } else {
        Cow::Borrowed(text)
    }
}

pub fn unescaped(text: &str) -> &str {
    let Some(rest) = text.strip_prefix(ESCAPE) else {
        return text;
    };

    if leads_with_escaped_bracket(rest)
        || (rest.starts_with(BRACKET) && is_taken_by_the_reader(rest))
    {
        rest
    } else {
        text
    }
}

fn is_taken_by_the_reader(text: &str) -> bool {
    let opens_something =
        brackets(text).is_some_and(|(inside, tail)| match Bracket::read(inside) {
            Bracket::Text => heads_a_timed_line(tail),
            Bracket::Moment(_) | Bracket::Identifying { .. } => true,
        });

    opens_something || voice_marked(text).is_some()
}

fn leads_with_escaped_bracket(text: &str) -> bool {
    text.starts_with(ESCAPE) && text.trim_start_matches(ESCAPE).starts_with(BRACKET)
}

pub fn voice_marked(text: &str) -> Option<(VoiceMarker, &str)> {
    VOICE_MARKERS.into_iter().find_map(|(marker, voice)| {
        let words = text.strip_prefix(marker)?.strip_suffix(']')?;
        Some((voice, words.trim()))
    })
}

pub fn heads_a_timed_line(mut rest: &str) -> bool {
    while let Some((inside, tail)) = brackets(rest.trim_start()) {
        match Bracket::read(inside) {
            Bracket::Moment(_) => return true,
            Bracket::Identifying { .. } => return false,
            Bracket::Text => rest = tail,
        }
    }

    false
}

pub fn brackets(line: &str) -> Option<(&str, &str)> {
    line.strip_prefix(BRACKET)?.split_once(']')
}

pub fn span(text: &str) -> Option<Duration> {
    let text = text.trim();
    let colons = text.bytes().filter(|byte| *byte == b':').count();

    if colons < COLONS_WHEN_HOURS_ARE_WRITTEN {
        return moment(text);
    }

    let (hours, rest) = text.split_once(':')?;
    let hours = whole(hours)?
        .checked_mul(MINUTES_PER_HOUR)?
        .checked_mul(SECONDS_PER_MINUTE)?;

    moment(rest)?.checked_add(Duration::from_secs(hours))
}

pub fn moment(inside: &str) -> Option<Duration> {
    let (minutes, rest) = inside.split_once(':')?;
    let (seconds, fraction) = match rest.split_once(['.', ':']) {
        Some((seconds, fraction)) => (seconds, Some(fraction)),
        None => (rest, None),
    };
    let nanoseconds = match fraction {
        Some(fraction) => sub_second(fraction)?,
        None => 0,
    };

    let seconds = whole(minutes)?
        .checked_mul(SECONDS_PER_MINUTE)?
        .checked_add(whole(seconds)?)?;

    Some(Duration::new(seconds, nanoseconds))
}

fn whole(text: &str) -> Option<u64> {
    decimal(text)?.parse().ok()
}

fn sub_second(text: &str) -> Option<u32> {
    let scaled: String = decimal(text)?
        .chars()
        .chain(iter::repeat('0'))
        .take(NANOSECOND_DIGITS)
        .collect();

    scaled.parse().ok()
}

fn decimal(text: &str) -> Option<&str> {
    let text = text.trim();
    let all_digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());

    all_digits.then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_the_reader_would_take_is_escaped_once_and_unescaped_back_to_itself() {
        let taken = [
            "[offset:5] x",
            "[ti:Another Song] x",
            "[00:09.00] x",
            "[v1: x]",
            "[Chorus] [00:12.00]x",
            "[length:3:00] x",
            "\\[already] x",
            "\\\\[twice] x",
        ];
        let left = [
            "all that you touch",
            "a [bracket] inside",
            "[Chorus] x",
            "[v2: x] y",
            "\\x",
            "[",
        ];

        for text in taken {
            let escaped = escaped_for_lrc(text);

            assert_eq!(escaped, format!("\\{text}"));
            assert_eq!(unescaped(&escaped), text);
        }
        for text in left {
            assert_eq!(escaped_for_lrc(text), text);
            assert_eq!(unescaped(text), text);
        }
    }
}
