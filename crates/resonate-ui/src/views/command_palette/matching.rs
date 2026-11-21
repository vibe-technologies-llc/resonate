use std::ops::Range;

use resonate_library::folded_letters;

pub(crate) const RECENT_AT_MOST: usize = 4;

const SCORE_EXACT: i32 = 1_000;
const SCORE_PREFIX: i32 = 800;
const SCORE_WORD_PREFIX: i32 = 650;
const SCORE_SUBSTRING: i32 = 500;
const SCORE_SUBSEQUENCE_BASE: i32 = 260;
const SCORE_WORD_START_BONUS: i32 = 32;
const SCORE_CONSECUTIVE_BONUS: i32 = 24;
const SCORE_SUBTITLE_MATCH: i32 = 180;
const SCORE_KEYWORD_MATCH: i32 = 120;
const SCORE_MRU_BOOST: i32 = 90;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Scope {
    #[default]
    All,
    Commands,
    Settings,
    Music,
    Playlists,
}

impl Scope {
    pub(crate) const FILTERS: [Self; 4] =
        [Self::Commands, Self::Settings, Self::Music, Self::Playlists];

    pub(crate) const fn prefix(self) -> Option<char> {
        match self {
            Self::All => None,
            Self::Commands => Some('>'),
            Self::Settings => Some('?'),
            Self::Music => Some('@'),
            Self::Playlists => Some('#'),
        }
    }

    pub(crate) fn parse(raw: &str) -> (Self, &str) {
        let trimmed = raw.trim_start();
        let Some(first) = trimmed.chars().next() else {
            return (Self::All, "");
        };
        let scope = match first {
            '>' => Self::Commands,
            '?' => Self::Settings,
            '@' => Self::Music,
            '#' => Self::Playlists,
            _ => return (Self::All, trimmed),
        };
        (scope, trimmed[first.len_utf8()..].trim_start())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MatchResult {
    pub(crate) score: i32,
    pub(crate) highlights: Vec<Range<usize>>,
}

struct FoldedText {
    folded: String,
    byte_spans: Vec<Range<usize>>,
}

impl FoldedText {
    fn of(original: &str) -> Self {
        let mut folded = String::with_capacity(original.len());
        let mut byte_spans = Vec::with_capacity(original.len());

        for (start, ch) in original.char_indices() {
            let end = start + ch.len_utf8();
            let mut buf = [0_u8; 4];
            let piece = ch.encode_utf8(&mut buf);
            let mapped = folded_letters(piece);
            for mapped_ch in mapped.chars() {
                let len = mapped_ch.len_utf8();
                folded.push(mapped_ch);
                for _ in 0..len {
                    byte_spans.push(start..end);
                }
            }
        }

        Self { folded, byte_spans }
    }

    fn original_span(&self, folded_range: Range<usize>) -> Option<Range<usize>> {
        if folded_range.is_empty() || folded_range.end > self.byte_spans.len() {
            return None;
        }
        let start = self.byte_spans[folded_range.start].start;
        let end = self.byte_spans[folded_range.end - 1].end;
        (start < end).then_some(start..end)
    }
}

fn is_word_boundary(folded: &str, byte_index: usize) -> bool {
    if byte_index == 0 {
        return true;
    }
    folded[..byte_index]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_alphanumeric())
}

fn match_token_in_title(title: &FoldedText, token: &str) -> Option<(i32, Vec<Range<usize>>)> {
    let text = &title.folded;
    if text == token {
        return Some((
            SCORE_EXACT,
            title.original_span(0..text.len()).into_iter().collect(),
        ));
    }

    if text.starts_with(token) {
        let penalty = (text.len().saturating_sub(token.len()) as i32).min(120);
        return Some((
            SCORE_PREFIX - penalty,
            title.original_span(0..token.len()).into_iter().collect(),
        ));
    }

    if let Some(pos) = find_word_boundary_match(text, token) {
        let penalty = (pos as i32).min(80);
        return Some((
            SCORE_WORD_PREFIX - penalty,
            title
                .original_span(pos..pos + token.len())
                .into_iter()
                .collect(),
        ));
    }

    if let Some(pos) = text.find(token) {
        let penalty = (pos as i32).min(100);
        return Some((
            SCORE_SUBSTRING - penalty,
            title
                .original_span(pos..pos + token.len())
                .into_iter()
                .collect(),
        ));
    }

    subsequence_match(title, token)
}

fn find_word_boundary_match(text: &str, token: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find(token) {
        let pos = search_from + rel;
        if is_word_boundary(text, pos) {
            return Some(pos);
        }
        let next_char = text[pos..].chars().next()?.len_utf8();
        search_from = pos + next_char;
    }
    None
}

fn subsequence_match(title: &FoldedText, token: &str) -> Option<(i32, Vec<Range<usize>>)> {
    let mut matched_ranges = Vec::new();
    let mut score = SCORE_SUBSEQUENCE_BASE;
    let mut cursor = 0_usize;
    let mut prev_end: Option<usize> = None;

    for wanted in token.chars() {
        let mut found = None;
        for (rel, ch) in title.folded[cursor..].char_indices() {
            if ch == wanted {
                let start = cursor + rel;
                let end = start + ch.len_utf8();
                found = Some((start, end));
                if is_word_boundary(&title.folded, start) {
                    break;
                }
            }
        }
        let (start, end) = found?;
        if is_word_boundary(&title.folded, start) {
            score += SCORE_WORD_START_BONUS;
        }
        if prev_end == Some(start) {
            score += SCORE_CONSECUTIVE_BONUS;
        } else if let Some(prev) = prev_end {
            let gap = start.saturating_sub(prev) as i32;
            score -= gap.min(20);
        }
        if let Some(span) = title.original_span(start..end) {
            matched_ranges.push(span);
        }
        prev_end = Some(end);
        cursor = end;
    }

    Some((score.max(1), matched_ranges))
}

fn merge_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
            continue;
        }
        merged.push(range);
    }
    merged
}

pub(crate) fn fuzzy_match(
    query: &str,
    title: &str,
    subtitle: Option<&str>,
    keywords: &str,
    mru_rank: Option<usize>,
) -> Option<MatchResult> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(folded_letters)
        .filter(|token| !token.is_empty())
        .collect();

    if tokens.is_empty() {
        let boost = mru_rank
            .map(|rank| SCORE_MRU_BOOST - (rank as i32 * 10))
            .unwrap_or_default();
        return Some(MatchResult {
            score: boost,
            highlights: Vec::new(),
        });
    }

    let folded_title = FoldedText::of(title);
    let folded_subtitle = subtitle.map(folded_letters);
    let folded_keywords = folded_letters(keywords);

    let mut total_score = 0_i32;
    let mut highlights = Vec::new();

    for token in &tokens {
        if let Some((score, spans)) = match_token_in_title(&folded_title, token) {
            total_score += score;
            highlights.extend(spans);
        } else if folded_subtitle
            .as_deref()
            .is_some_and(|sub| sub.contains(token.as_str()))
        {
            total_score += SCORE_SUBTITLE_MATCH;
        } else if folded_keywords.contains(token.as_str()) {
            total_score += SCORE_KEYWORD_MATCH;
        } else {
            return None;
        }
    }

    if let Some(rank) = mru_rank {
        total_score += (SCORE_MRU_BOOST / 2) - (rank as i32 * 5);
    }

    Some(MatchResult {
        score: total_score,
        highlights: merge_ranges(highlights),
    })
}

pub(crate) fn record_recent<T: PartialEq>(recent: &mut Vec<T>, key: T) {
    recent.retain(|existing| *existing != key);
    recent.insert(0, key);
    recent.truncate(RECENT_AT_MOST);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_scopes_parse_cleanly_and_strip_leading_space() {
        assert_eq!(Scope::parse(""), (Scope::All, ""));
        assert_eq!(Scope::parse("  echoes"), (Scope::All, "echoes"));
        assert_eq!(Scope::parse("> shuffle"), (Scope::Commands, "shuffle"));
        assert_eq!(Scope::parse(">shuffle"), (Scope::Commands, "shuffle"));
        assert_eq!(Scope::parse("? dither"), (Scope::Settings, "dither"));
        assert_eq!(Scope::parse("@ pink floyd"), (Scope::Music, "pink floyd"));
        assert_eq!(Scope::parse("# chill"), (Scope::Playlists, "chill"));
    }

    #[test]
    fn exact_and_prefix_matches_outrank_subsequence_and_keyword_matches() {
        let exact = fuzzy_match("queue", "Queue", None, "", None).expect("exact matches");
        let prefix = fuzzy_match("queue", "Queue manager", None, "", None).expect("prefix matches");
        let word = fuzzy_match("queue", "Clear queue", None, "", None).expect("word matches");
        let subseq = fuzzy_match("cq", "Clear queue", None, "", None).expect("subseq matches");
        let keyword =
            fuzzy_match("queue", "Something else", None, "queue list", None).expect("kw matches");

        assert!(exact.score > prefix.score);
        assert!(prefix.score > word.score);
        assert!(word.score > subseq.score);
        assert!(subseq.score > keyword.score);
    }

    #[test]
    fn diacritics_fold_and_highlight_on_valid_utf8_boundaries() {
        let title = "Rosé Pine";
        let matched = fuzzy_match("rose", title, None, "", None).expect("folded diacritic matches");

        assert_eq!(matched.highlights.len(), 1);
        let range = &matched.highlights[0];
        assert!(title.is_char_boundary(range.start));
        assert!(title.is_char_boundary(range.end));
        assert_eq!(&title[range.clone()], "Rosé");
    }

    #[test]
    fn multi_token_queries_require_every_word_to_match() {
        assert!(fuzzy_match("switch dark", "Switch theme", None, "dark light", None).is_some());
        assert!(fuzzy_match("switch nonexistent", "Switch theme", None, "dark", None).is_none());
    }

    #[test]
    fn mru_records_newest_first_without_duplicates_within_the_cap() {
        let mut recent = Vec::new();
        for id in [1, 2, 3, 4, 5, 2] {
            record_recent(&mut recent, id);
        }
        assert_eq!(recent, vec![2, 5, 4, 3]);
    }
}
