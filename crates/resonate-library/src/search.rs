use std::{fmt, ops::Range, time::Duration};

use resonate_codec::Codec;

use crate::store;

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
const WEEK: u64 = 7 * DAY;
const MONTH: u64 = 30 * DAY;
const YEAR: u64 = 365 * DAY;

const KILOHERTZ: f64 = 1_000.0;

const DENIED: char = '-';

const WINDOW: char = '@';

const JOINER: &str = "or";

const EITHER: &str = " or ";

const AGES: [(&str, u64); 6] = [
    ("y", YEAR),
    ("mo", MONTH),
    ("m", MONTH),
    ("w", WEEK),
    ("d", DAY),
    ("h", HOUR),
];

const WRITTEN_AGES: [(&str, u64); 5] = [
    ("y", YEAR),
    ("mo", MONTH),
    ("w", WEEK),
    ("d", DAY),
    ("h", HOUR),
];

const SPANS: [(&str, u64); 3] = [("h", HOUR), ("m", MINUTE), ("s", 1)];

const RATES: [(&str, f64); 4] = [("", 1.0), ("hz", 1.0), ("k", KILOHERTZ), ("khz", KILOHERTZ)];

const DEPTHS: [&str; 3] = ["", "bit", "bits"];

const AGE_WITH_NO_UNIT: u64 = DAY;
const SPAN_WITH_NO_UNIT: u64 = 1;

const CODECS: [(&str, Codec); 13] = [
    ("flac", Codec::Flac),
    ("alac", Codec::Alac),
    ("dsd", Codec::Dsd),
    ("dsf", Codec::Dsd),
    ("dff", Codec::Dsd),
    ("pcm", Codec::Pcm),
    ("wav", Codec::Pcm),
    ("aac", Codec::Aac),
    ("mp3", Codec::Mp3),
    ("vorbis", Codec::Vorbis),
    ("ogg", Codec::Vorbis),
    ("opus", Codec::Opus),
    ("unknown", Codec::Unknown),
];

const KEYS: [(&str, Named); 14] = [
    ("added", Named::Added),
    ("plays", Named::Plays),
    ("played", Named::Played),
    ("heard", Named::Played),
    ("year", Named::Year),
    ("length", Named::Length),
    ("duration", Named::Length),
    ("rate", Named::Rate),
    ("samplerate", Named::Rate),
    ("depth", Named::Depth),
    ("bits", Named::Depth),
    ("codec", Named::Codec),
    ("format", Named::Codec),
    ("is", Named::Shape),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Named {
    Added,
    Plays,
    Played,
    Year,
    Length,
    Rate,
    Depth,
    Codec,
    Shape,
}

impl Named {
    fn of(key: &str) -> Option<Self> {
        KEYS.into_iter()
            .find(|(name, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, named)| named)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Compare {
    Below,
    AtMost,
    #[default]
    Exactly,
    AtLeast,
    Above,
}

impl Compare {
    const PREFIXES: [(&'static str, Self); 5] = [
        ("<=", Self::AtMost),
        (">=", Self::AtLeast),
        ("<", Self::Below),
        (">", Self::Above),
        ("=", Self::Exactly),
    ];

    pub const fn operator(self) -> &'static str {
        match self {
            Self::Below => "<",
            Self::AtMost => "<=",
            Self::Exactly => "=",
            Self::AtLeast => ">=",
            Self::Above => ">",
        }
    }

    pub const fn written(self) -> &'static str {
        match self {
            Self::Exactly => "",
            compare => compare.operator(),
        }
    }

    pub const fn flipped(self) -> Self {
        match self {
            Self::Below => Self::Above,
            Self::AtMost => Self::AtLeast,
            Self::Exactly => Self::Exactly,
            Self::AtLeast => Self::AtMost,
            Self::Above => Self::Below,
        }
    }

    fn of(value: &str) -> (Self, &str) {
        Self::PREFIXES
            .into_iter()
            .find_map(|(prefix, compare)| Some((compare, value.strip_prefix(prefix)?)))
            .unwrap_or((Self::Exactly, value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    Title,
    Artist,
    Album,
    Genre,
    Lyrics,
}

const LYRICS_ALIAS: &str = "lyric";

impl Column {
    pub const ALL: [Self; 5] = [
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::Genre,
        Self::Lyrics,
    ];

    pub const NAMES: [Self; 4] = [Self::Title, Self::Artist, Self::Album, Self::Genre];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Genre => "genre",
            Self::Lyrics => "lyrics",
        }
    }

    fn of(key: &str) -> Option<Self> {
        if key.eq_ignore_ascii_case(LYRICS_ALIAS) {
            return Some(Self::Lyrics);
        }

        Self::ALL
            .into_iter()
            .find(|column| key.eq_ignore_ascii_case(column.name()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Lossless,
    Lossy,
    Mono,
    Stereo,
    Multichannel,
    HiRes,
    Favourite,
    Hidden,
    Fake,
    Suspect,
    Misnamed,
}

impl Shape {
    pub const ALL: [Self; 11] = [
        Self::Lossless,
        Self::Lossy,
        Self::Mono,
        Self::Stereo,
        Self::Multichannel,
        Self::HiRes,
        Self::Favourite,
        Self::Hidden,
        Self::Fake,
        Self::Suspect,
        Self::Misnamed,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Lossless => "lossless",
            Self::Lossy => "lossy",
            Self::Mono => "mono",
            Self::Stereo => "stereo",
            Self::Multichannel => "multichannel",
            Self::HiRes => "hires",
            Self::Favourite => "favourite",
            Self::Hidden => "hidden",
            Self::Fake => "fake",
            Self::Suspect => "suspect",
            Self::Misnamed => "misnamed",
        }
    }

    fn of(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|shape| value.eq_ignore_ascii_case(shape.name()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Term {
    Added {
        compare: Compare,
        age: Duration,
    },
    Plays {
        compare: Compare,
        plays: u32,
        within: Option<Duration>,
    },
    Played {
        compare: Compare,
        age: Duration,
    },
    Year {
        compare: Compare,
        year: i32,
    },
    Length {
        compare: Compare,
        length: Duration,
    },
    Rate {
        compare: Compare,
        hertz: u32,
    },
    Depth {
        compare: Compare,
        bits: u8,
    },
    Codec(Codec),
    Shape(Shape),
}

impl Term {
    const fn key(&self) -> &'static str {
        match self {
            Self::Added { .. } => "added",
            Self::Plays { .. } => "plays",
            Self::Played { .. } => "played",
            Self::Year { .. } => "year",
            Self::Length { .. } => "length",
            Self::Rate { .. } => "rate",
            Self::Depth { .. } => "depth",
            Self::Codec(_) => "codec",
            Self::Shape(_) => "is",
        }
    }

    const fn compare(&self) -> Option<Compare> {
        match self {
            Self::Added { compare, .. }
            | Self::Plays { compare, .. }
            | Self::Played { compare, .. }
            | Self::Year { compare, .. }
            | Self::Length { compare, .. }
            | Self::Rate { compare, .. }
            | Self::Depth { compare, .. } => Some(*compare),
            Self::Codec(_) | Self::Shape(_) => None,
        }
    }

    const fn window(&self) -> Option<Duration> {
        match self {
            Self::Plays { within, .. } => *within,
            Self::Added { .. }
            | Self::Played { .. }
            | Self::Year { .. }
            | Self::Length { .. }
            | Self::Rate { .. }
            | Self::Depth { .. }
            | Self::Codec(_)
            | Self::Shape(_) => None,
        }
    }

    fn range(low: &Self, high: &Self) -> Option<String> {
        let bounds = (low.compare(), high.compare());
        if low.key() != high.key()
            || low.window() != high.window()
            || bounds != (Some(Compare::AtLeast), Some(Compare::AtMost))
        {
            return None;
        }
        let from = low.measured();

        (!from.contains(DENIED)).then(|| format!("{}:{from}-{}", low.key(), high.value()))
    }

    fn measured(&self) -> String {
        match self {
            Self::Added { age, .. } | Self::Played { age, .. } => aged(*age),
            Self::Plays { plays, .. } => plays.to_string(),
            Self::Year { year, .. } => year.to_string(),
            Self::Length { length, .. } => spanned(*length),
            Self::Rate { hertz, .. } => hertz.to_string(),
            Self::Depth { bits, .. } => bits.to_string(),
            Self::Codec(codec) => named_codec(*codec).to_owned(),
            Self::Shape(shape) => shape.name().to_owned(),
        }
    }

    fn value(&self) -> String {
        let measured = self.measured();

        match self.window() {
            Some(age) => format!("{measured}{WINDOW}{}", aged(age)),
            None => measured,
        }
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let compared = self.compare().map_or("", Compare::written);

        write!(f, "{}:{compared}{}", self.key(), self.value())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Word {
    pub column: Option<Column>,
    pub text: String,
    pub phrase: bool,
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(column) = self.column {
            write!(f, "{}:", column.name())?;
        }
        if self.phrase {
            write!(f, "\"{}\"", self.text)
        } else {
            f.write_str(&self.text)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Condition {
    Word(Word),
    Term(Term),
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Word(word) => word.fmt(f),
            Self::Term(term) => term.fmt(f),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Asked {
    pub denied: bool,
    pub all: Vec<Condition>,
}

impl fmt::Display for Asked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.denied
            && let [Condition::Term(low), Condition::Term(high)] = self.all.as_slice()
            && let Some(range) = Term::range(low, high)
        {
            return f.write_str(&range);
        }

        let written: Vec<String> = self
            .all
            .iter()
            .map(|condition| {
                if self.denied {
                    format!("{DENIED}{condition}")
                } else {
                    condition.to_string()
                }
            })
            .collect();

        f.write_str(&written.join(if self.denied { EITHER } else { " " }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Clause {
    pub any: Vec<Asked>,
}

impl Clause {
    pub fn lone_word(&self) -> Option<&Word> {
        let [asked] = self.any.as_slice() else {
            return None;
        };
        let [Condition::Word(word)] = asked.all.as_slice() else {
            return None;
        };

        (!asked.denied).then_some(word)
    }

    pub fn insists_on(&self, shape: Shape) -> bool {
        let [asked] = self.any.as_slice() else {
            return false;
        };

        !asked.denied && asked.all.contains(&Condition::Term(Term::Shape(shape)))
    }
}

impl fmt::Display for Clause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let written: Vec<String> = self.any.iter().map(ToString::to_string).collect();

        f.write_str(&written.join(EITHER))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Search {
    pub clauses: Vec<Clause>,
}

impl fmt::Display for Search {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let written: Vec<String> = self.clauses.iter().map(ToString::to_string).collect();

        f.write_str(&written.join(" "))
    }
}

impl Search {
    pub fn read(text: &str) -> Self {
        Self {
            clauses: clauses(&tokens(text)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    pub fn lit(&self, text: &str, column: Column) -> Vec<Range<usize>> {
        let mut reaching = self.words_reaching(column).peekable();
        if reaching.peek().is_none() {
            return Vec::new();
        }

        let tokens = tokens_of(text);
        let mut lit = Vec::new();

        for word in reaching {
            let pieces = pieces_of(word);
            if pieces.is_empty() {
                continue;
            }
            if word.phrase {
                lit.extend(phrased(&tokens, &pieces));
            } else {
                lit.extend(begun_with(&tokens, &pieces));
            }
        }

        merged(lit)
    }

    fn words_reaching(&self, column: Column) -> impl Iterator<Item = &Word> {
        self.clauses
            .iter()
            .flat_map(|clause| clause.any.iter())
            .filter(|asked| !asked.denied)
            .flat_map(|asked| asked.all.iter())
            .filter_map(|condition| match condition {
                Condition::Word(word) => Some(word),
                Condition::Term(_) => None,
            })
            .filter(move |word| word.column.is_none_or(|scoped| scoped == column))
    }

    pub fn as_sung(&self) -> Option<Self> {
        let mut words = Vec::with_capacity(self.clauses.len());
        for clause in &self.clauses {
            let word = clause.lone_word()?;
            if word.column.is_some() || word.text.contains('"') {
                return None;
            }
            words.push(word.text.as_str());
        }
        if words.is_empty() {
            return None;
        }

        Some(Self {
            clauses: vec![Clause {
                any: vec![Asked {
                    denied: false,
                    all: vec![Condition::Word(Word {
                        column: Some(Column::Lyrics),
                        text: words.join(" "),
                        phrase: true,
                    })],
                }],
            }],
        })
    }

    pub fn reads(&self) -> Vec<String> {
        self.clauses
            .iter()
            .filter(|clause| clause.lone_word().is_none_or(|word| word.column.is_some()))
            .map(ToString::to_string)
            .collect()
    }
}

pub(crate) fn lettered_runs(text: &str) -> Vec<(Range<usize>, String)> {
    tokens_of(text)
        .into_iter()
        .filter(|token| !token.folded.is_empty())
        .map(|token| (token.at, token.folded))
        .collect()
}

pub(crate) fn runs_in(text: &str) -> impl Iterator<Item = (&str, String)> {
    lettered_runs(text)
        .into_iter()
        .filter_map(move |(at, folded)| Some((text.get(at)?, folded)))
}

pub(crate) fn pieces_of(word: &Word) -> Vec<String> {
    word.text
        .split(|letter: char| !letter.is_alphanumeric())
        .map(store::folded_letters)
        .filter(|piece| !piece.is_empty())
        .collect()
}

struct Lettered {
    at: Range<usize>,
    folded: String,
}

fn tokens_of(text: &str) -> Vec<Lettered> {
    let mut tokens = Vec::new();
    let mut begun: Option<usize> = None;

    for (at, letter) in text.char_indices() {
        if letter.is_alphanumeric() {
            begun.get_or_insert(at);
            continue;
        }
        if let Some(from) = begun.take() {
            tokens.push(lettered(text, from..at));
        }
    }
    if let Some(from) = begun {
        tokens.push(lettered(text, from..text.len()));
    }

    tokens
}

fn lettered(text: &str, at: Range<usize>) -> Lettered {
    let folded = store::folded_letters(text.get(at.clone()).unwrap_or_default());
    Lettered { at, folded }
}

fn begun_with(tokens: &[Lettered], pieces: &[String]) -> Vec<Range<usize>> {
    tokens
        .iter()
        .filter(|token| {
            pieces
                .iter()
                .any(|piece| token.folded.starts_with(piece.as_str()))
        })
        .map(|token| token.at.clone())
        .collect()
}

fn phrased(tokens: &[Lettered], pieces: &[String]) -> Vec<Range<usize>> {
    tokens
        .windows(pieces.len())
        .filter(|run| {
            run.iter()
                .zip(pieces)
                .all(|(token, piece)| token.folded == *piece)
        })
        .filter_map(|run| Some(run.first()?.at.start..run.last()?.at.end))
        .collect()
}

fn merged(mut lit: Vec<Range<usize>>) -> Vec<Range<usize>> {
    lit.sort_by_key(|run| (run.start, run.end));
    let mut standing: Vec<Range<usize>> = Vec::with_capacity(lit.len());

    for run in lit {
        match standing.last_mut() {
            Some(held) if run.start <= held.end => held.end = held.end.max(run.end),
            _ => standing.push(run),
        }
    }

    standing
}

struct Token {
    key: Option<String>,
    value: String,
    quoted: bool,
    denied: bool,
}

enum Reading {
    Scoped(Column, String),
    Narrowing(Vec<Term>),
    Loose(String),
}

impl Token {
    fn joins(&self) -> bool {
        self.key.is_none()
            && !self.quoted
            && !self.denied
            && self.value.eq_ignore_ascii_case(JOINER)
    }

    fn asked(&self) -> Vec<Asked> {
        let all = self.conditions();
        if all.is_empty() {
            return Vec::new();
        }
        if self.denied {
            return all
                .into_iter()
                .map(|condition| Asked {
                    denied: true,
                    all: vec![condition],
                })
                .collect();
        }

        vec![Asked { denied: false, all }]
    }

    fn conditions(&self) -> Vec<Condition> {
        match self.reading() {
            Reading::Scoped(_, text) if text.trim().is_empty() => Vec::new(),
            Reading::Scoped(column, text) => vec![Condition::Word(Word {
                column: Some(column),
                text,
                phrase: self.quoted,
            })],
            Reading::Narrowing(terms) => terms.into_iter().map(Condition::Term).collect(),
            Reading::Loose(text) => vec![Condition::Word(Word {
                column: None,
                text,
                phrase: self.quoted,
            })],
        }
    }

    fn reading(&self) -> Reading {
        let Some(key) = self.key.as_deref() else {
            return Reading::Loose(self.value.clone());
        };
        if let Some(column) = Column::of(key) {
            return Reading::Scoped(column, self.value.clone());
        }
        if let Some(terms) = Named::of(key).and_then(|named| narrowed(named, &self.value)) {
            return Reading::Narrowing(terms);
        }

        Reading::Loose(format!("{key}:{}", self.value))
    }
}

fn clauses(tokens: &[Token]) -> Vec<Clause> {
    let mut clauses: Vec<Clause> = Vec::new();
    let mut joining = false;

    for (index, token) in tokens.iter().enumerate() {
        if token.joins() && !clauses.is_empty() && index + 1 < tokens.len() {
            joining = true;
            continue;
        }

        let asked = token.asked();
        if asked.is_empty() {
            continue;
        }
        if joining && let Some(clause) = clauses.last_mut() {
            clause.any.extend(asked);
        } else {
            clauses.push(Clause { any: asked });
        }
        joining = false;
    }

    clauses
}

#[derive(Default)]
struct Pending {
    key: Option<String>,
    value: String,
    quoted: bool,
    denied: bool,
}

impl Pending {
    fn starting(&self) -> bool {
        self.key.is_none() && !self.quoted && !self.denied && self.value.is_empty()
    }

    fn keying(&self) -> bool {
        self.key.is_none() && !self.quoted && !self.value.is_empty()
    }

    fn settle(&mut self, found: &mut Vec<Token>) {
        if self.key.is_some() || !self.value.is_empty() {
            found.push(Token {
                key: self.key.take(),
                value: std::mem::take(&mut self.value),
                quoted: self.quoted,
                denied: self.denied,
            });
        }
        self.quoted = false;
        self.denied = false;
    }
}

fn tokens(text: &str) -> Vec<Token> {
    let mut found = Vec::new();
    let mut pending = Pending::default();
    let mut inside = false;

    for character in text.chars() {
        match character {
            '"' if inside => inside = false,
            _ if inside => pending.value.push(character),
            '"' => {
                inside = true;
                pending.quoted = true;
            }
            DENIED if pending.starting() => pending.denied = true,
            ':' if pending.keying() => {
                pending.key = Some(std::mem::take(&mut pending.value));
            }
            _ if character.is_whitespace() => pending.settle(&mut found),
            _ => pending.value.push(character),
        }
    }
    pending.settle(&mut found);

    found
}

fn narrowed(named: Named, value: &str) -> Option<Vec<Term>> {
    match named {
        Named::Added => {
            let (compare, rest) = Compare::of(value);
            Some(vec![Term::Added {
                compare: within(compare),
                age: age(rest)?,
            }])
        }
        Named::Plays => {
            let (counted, within) = windowed(value)?;
            bounded(counted, plays, move |compare, plays| Term::Plays {
                compare,
                plays,
                within,
            })
        }
        Named::Played => {
            let (compare, rest) = Compare::of(value);
            Some(vec![Term::Played {
                compare: within(compare),
                age: age(rest)?,
            }])
        }
        Named::Year => bounded(value, year, |compare, year| Term::Year { compare, year }),
        Named::Length => bounded(value, span, |compare, length| Term::Length {
            compare,
            length,
        }),
        Named::Rate => bounded(value, hertz, |compare, hertz| Term::Rate { compare, hertz }),
        Named::Depth => bounded(value, bits, |compare, bits| Term::Depth { compare, bits }),
        Named::Codec => Some(vec![Term::Codec(codec_of(value)?)]),
        Named::Shape => Some(vec![Term::Shape(Shape::of(value)?)]),
    }
}

fn windowed(value: &str) -> Option<(&str, Option<Duration>)> {
    let Some((counted, window)) = value.split_once(WINDOW) else {
        return Some((value, None));
    };

    Some((counted, Some(age(window)?)))
}

const fn within(compare: Compare) -> Compare {
    match compare {
        Compare::Exactly => Compare::AtMost,
        compare => compare,
    }
}

fn bounded<T: Copy>(
    value: &str,
    read: impl Fn(&str) -> Option<T>,
    term: impl Fn(Compare, T) -> Term,
) -> Option<Vec<Term>> {
    if let Some((low, high)) = value.split_once('-')
        && let (Some(low), Some(high)) = (read(low), read(high))
    {
        return Some(vec![
            term(Compare::AtLeast, low),
            term(Compare::AtMost, high),
        ]);
    }

    let (compare, rest) = Compare::of(value);
    Some(vec![term(compare, read(rest)?)])
}

fn counted(value: &str) -> (&str, &str) {
    let end = value
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(value.len());
    value.split_at(end)
}

fn scaled(unit: &str, table: &[(&str, u64)], bare: u64) -> Option<f64> {
    if unit.is_empty() {
        return Some(bare as f64);
    }
    table
        .iter()
        .find(|(name, _)| unit.eq_ignore_ascii_case(name))
        .map(|(_, seconds)| *seconds as f64)
}

fn age(value: &str) -> Option<Duration> {
    let (count, unit) = counted(value);
    let count: f64 = count.parse().ok()?;
    let seconds = scaled(unit, &AGES, AGE_WITH_NO_UNIT)?;

    Duration::try_from_secs_f64(count * seconds).ok()
}

fn span(value: &str) -> Option<Duration> {
    if let Some((minutes, seconds)) = value.split_once(':') {
        let minutes: u64 = minutes.parse().ok()?;
        let seconds: u64 = seconds.parse().ok()?;
        return (seconds < MINUTE)
            .then(|| minutes.checked_mul(MINUTE)?.checked_add(seconds))
            .flatten()
            .map(Duration::from_secs);
    }

    let mut rest = value;
    let mut seconds = 0.0;
    while !rest.is_empty() {
        let (count, tail) = counted(rest);
        let count: f64 = count.parse().ok()?;
        let end = tail
            .find(|character: char| character.is_ascii_digit())
            .unwrap_or(tail.len());
        let (unit, tail) = tail.split_at(end);

        seconds += count * scaled(unit, &SPANS, SPAN_WITH_NO_UNIT)?;
        rest = tail;
    }

    Duration::try_from_secs_f64(seconds).ok()
}

fn hertz(value: &str) -> Option<u32> {
    let (count, unit) = counted(value);
    let count: f64 = count.parse().ok()?;
    let scale = RATES
        .into_iter()
        .find(|(name, _)| unit.eq_ignore_ascii_case(name))
        .map(|(_, scale)| scale)?;
    let hertz = (count * scale).round();

    (hertz >= 0.0 && hertz <= f64::from(u32::MAX)).then_some(hertz as u32)
}

fn bits(value: &str) -> Option<u8> {
    let (count, unit) = counted(value);
    DEPTHS
        .into_iter()
        .any(|name| unit.eq_ignore_ascii_case(name))
        .then(|| count.parse().ok())
        .flatten()
}

fn plays(value: &str) -> Option<u32> {
    value.parse().ok()
}

fn year(value: &str) -> Option<i32> {
    value.parse().ok()
}

fn codec_of(value: &str) -> Option<Codec> {
    CODECS
        .into_iter()
        .find(|(name, _)| value.eq_ignore_ascii_case(name))
        .map(|(_, codec)| codec)
}

fn named_codec(codec: Codec) -> &'static str {
    CODECS
        .into_iter()
        .find(|(_, named)| *named == codec)
        .map_or("unknown", |(name, _)| name)
}

fn aged(age: Duration) -> String {
    let seconds = age.as_secs();
    WRITTEN_AGES
        .into_iter()
        .find(|(_, size)| seconds.is_multiple_of(*size) && seconds >= *size)
        .map_or_else(
            || format!("{}h", age.as_secs_f64() / HOUR as f64),
            |(name, size)| format!("{}{name}", seconds / size),
        )
}

fn spanned(length: Duration) -> String {
    if length.subsec_nanos() != 0 {
        return format!("{}s", length.as_secs_f64());
    }

    let seconds = length.as_secs();
    let mut written = String::new();
    let mut rest = seconds;
    for (name, size) in SPANS {
        let held = rest / size;
        if held > 0 {
            written.push_str(&format!("{held}{name}"));
            rest %= size;
        }
    }

    if written.is_empty() {
        return "0s".to_owned();
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(text: &str) -> Vec<Condition> {
        Search::read(text)
            .clauses
            .into_iter()
            .flat_map(|clause| clause.any)
            .flat_map(|asked| asked.all)
            .collect()
    }

    fn terms(text: &str) -> Vec<Term> {
        held(text)
            .into_iter()
            .filter_map(|condition| match condition {
                Condition::Term(term) => Some(term),
                Condition::Word(_) => None,
            })
            .collect()
    }

    fn worded(text: &str) -> Vec<Word> {
        held(text)
            .into_iter()
            .filter_map(|condition| match condition {
                Condition::Word(word) => Some(word),
                Condition::Term(_) => None,
            })
            .collect()
    }

    fn words(text: &str) -> Vec<String> {
        worded(text).iter().map(ToString::to_string).collect()
    }

    fn shape(text: &str) -> Vec<String> {
        Search::read(text)
            .clauses
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn plain_text_is_words_and_nothing_else() {
        assert!(terms("dark side of the moon").is_empty());
        assert_eq!(worded("dark side of the moon").len(), 5);
        assert!(
            worded("dark side of the moon")
                .iter()
                .all(|word| word.column.is_none())
        );
        assert_eq!(Search::read("dark side of the moon").clauses.len(), 5);
    }

    #[test]
    fn a_quoted_run_is_one_word() {
        let held = worded("\"pink floyd\" live");

        assert_eq!(words("\"pink floyd\" live"), vec!["\"pink floyd\"", "live"]);
        assert!(held.first().is_some_and(|word| word.phrase));
        assert!(held.last().is_some_and(|word| !word.phrase));
    }

    #[test]
    fn a_column_scopes_the_words_it_names() {
        let held = worded("artist:\"pink floyd\" moon");

        assert_eq!(
            held.first().map(|word| word.column),
            Some(Some(Column::Artist))
        );
        assert_eq!(
            held.last().map(|word| word.column),
            Some(None),
            "the loose word should stay loose"
        );
        assert!(
            Search::read("artist:").is_empty(),
            "a column with nothing beside it should read as nothing"
        );
    }

    #[test]
    fn every_column_the_index_holds_is_scoped_by_its_own_name_and_written_back_as_it_was_typed() {
        for column in Column::ALL {
            let typed = format!("{}:rock", column.name());

            assert_eq!(
                worded(&typed).first().map(|word| word.column),
                Some(Some(column)),
                "{typed} was not scoped to the column it names"
            );
            assert_eq!(words(&typed), vec![typed.clone()]);
            assert_eq!(Search::read(&typed).to_string(), typed);
        }
    }

    #[test]
    fn a_term_that_cannot_be_read_is_the_words_it_was_written_as() {
        assert!(terms("year:banana").is_empty());
        assert_eq!(words("year:banana"), vec!["year:banana"]);
        assert!(terms("nothing:here").is_empty());
        assert_eq!(words("nothing:here"), vec!["nothing:here"]);
    }

    #[test]
    fn an_age_is_read_as_a_span_back_from_now() {
        assert_eq!(
            terms("added:30d"),
            vec![Term::Added {
                compare: Compare::AtMost,
                age: Duration::from_secs(30 * DAY),
            }]
        );
        assert_eq!(
            terms("added:>1y"),
            vec![Term::Added {
                compare: Compare::Above,
                age: Duration::from_secs(YEAR),
            }]
        );
        assert_eq!(
            terms("added:<6mo"),
            vec![Term::Added {
                compare: Compare::Below,
                age: Duration::from_secs(6 * MONTH),
            }]
        );
    }

    #[test]
    fn a_length_reads_compounds_and_clock_time() {
        assert_eq!(
            terms("length:>3m30s"),
            vec![Term::Length {
                compare: Compare::Above,
                length: Duration::from_secs(210),
            }]
        );
        assert_eq!(
            terms("duration:<4:30"),
            vec![Term::Length {
                compare: Compare::Below,
                length: Duration::from_secs(270),
            }]
        );
        assert_eq!(
            terms("length:90"),
            vec![Term::Length {
                compare: Compare::Exactly,
                length: Duration::from_secs(90),
            }]
        );
    }

    #[test]
    fn a_clock_time_past_what_a_span_can_hold_is_the_words_it_was_written_as() {
        let written = "length:400000000000000000:30";

        assert!(terms(written).is_empty());
        assert_eq!(words(written), vec![written]);
    }

    #[test]
    fn a_rate_reads_kilohertz() {
        assert_eq!(
            terms("rate:>=96k"),
            vec![Term::Rate {
                compare: Compare::AtLeast,
                hertz: 96_000,
            }]
        );
        assert_eq!(
            terms("rate:44.1k"),
            vec![Term::Rate {
                compare: Compare::Exactly,
                hertz: 44_100,
            }]
        );
        assert_eq!(
            terms("samplerate:192000hz"),
            vec![Term::Rate {
                compare: Compare::Exactly,
                hertz: 192_000,
            }]
        );
    }

    #[test]
    fn a_range_becomes_the_two_bounds_it_names() {
        assert_eq!(
            terms("year:1970-1979"),
            vec![
                Term::Year {
                    compare: Compare::AtLeast,
                    year: 1970,
                },
                Term::Year {
                    compare: Compare::AtMost,
                    year: 1979,
                },
            ]
        );
    }

    #[test]
    fn a_shape_and_a_codec_are_named_rather_than_numbered() {
        assert_eq!(terms("is:lossless"), vec![Term::Shape(Shape::Lossless)]);
        assert_eq!(terms("is:HiRes"), vec![Term::Shape(Shape::HiRes)]);
        assert_eq!(terms("codec:FLAC"), vec![Term::Codec(Codec::Flac)]);
        assert_eq!(terms("format:ogg"), vec![Term::Codec(Codec::Vorbis)]);
        assert!(terms("is:sideways").is_empty());
    }

    #[test]
    fn terms_and_words_stand_together() {
        let search = Search::read("radiohead is:lossless added:<90d");

        assert_eq!(worded("radiohead is:lossless added:<90d").len(), 1);
        assert_eq!(terms("radiohead is:lossless added:<90d").len(), 2);
        assert_eq!(
            search.reads(),
            vec!["is:lossless".to_owned(), "added:<3mo".to_owned()]
        );
    }

    #[test]
    fn every_term_is_written_back_as_the_text_that_reads_it() {
        for text in [
            "added:<=1mo",
            "added:>1y",
            "added:<=6mo",
            "added:<=10d",
            "year:1975",
            "year:>=1970",
            "length:>3m30s",
            "length:<=1h5m",
            "rate:>=96000",
            "rate:44100",
            "depth:24",
            "depth:>16",
            "plays:0",
            "plays:>5",
            "plays:>20@1mo",
            "plays:0@2w",
            "plays:>=3@1y",
            "played:<=1mo",
            "played:>1y",
            "codec:flac",
            "codec:alac",
            "is:lossless",
            "is:multichannel",
            "is:favourite",
            "is:hidden",
        ] {
            let read = terms(text);
            assert_eq!(read.len(), 1, "{text} read as {read:?}");
            assert_eq!(read[0].to_string(), text, "{text} was written back wrong");
            assert_eq!(terms(&read[0].to_string()), read);
        }
    }

    #[test]
    fn a_play_count_is_a_number_and_a_play_is_an_age() {
        assert_eq!(
            terms("plays:0"),
            vec![Term::Plays {
                compare: Compare::Exactly,
                plays: 0,
                within: None,
            }]
        );
        assert_eq!(
            terms("plays:1-3"),
            vec![
                Term::Plays {
                    compare: Compare::AtLeast,
                    plays: 1,
                    within: None,
                },
                Term::Plays {
                    compare: Compare::AtMost,
                    plays: 3,
                    within: None,
                },
            ]
        );
        assert_eq!(
            terms("heard:30d"),
            vec![Term::Played {
                compare: Compare::AtMost,
                age: Duration::from_secs(30 * DAY),
            }]
        );
        assert!(terms("plays:often").is_empty());
        assert_eq!(words("plays:often"), vec!["plays:often"]);
    }

    #[test]
    fn a_play_count_takes_a_window_to_count_inside() {
        assert_eq!(
            terms("plays:>20@30d"),
            vec![Term::Plays {
                compare: Compare::Above,
                plays: 20,
                within: Some(Duration::from_secs(30 * DAY)),
            }]
        );
        assert_eq!(
            terms("plays:3@1y"),
            vec![Term::Plays {
                compare: Compare::Exactly,
                plays: 3,
                within: Some(Duration::from_secs(YEAR)),
            }]
        );
        assert_eq!(
            terms("plays:<=2@30"),
            vec![Term::Plays {
                compare: Compare::AtMost,
                plays: 2,
                within: Some(Duration::from_secs(30 * DAY)),
            }],
            "a window with no unit should read in days the way an age does"
        );
    }

    #[test]
    fn a_window_the_grammar_cannot_read_is_the_words_it_was_written_as() {
        for text in ["plays:>20@lately", "plays:>20@", "plays:@30d"] {
            assert!(terms(text).is_empty(), "{text} read as a term");
            assert_eq!(words(text), vec![text]);
        }
    }

    #[test]
    fn a_windowed_range_carries_the_window_on_both_of_its_bounds() {
        let window = Some(Duration::from_secs(30 * DAY));

        assert_eq!(
            terms("plays:5-10@30d"),
            vec![
                Term::Plays {
                    compare: Compare::AtLeast,
                    plays: 5,
                    within: window,
                },
                Term::Plays {
                    compare: Compare::AtMost,
                    plays: 10,
                    within: window,
                },
            ]
        );
        assert_eq!(
            Search::read("plays:5-10@30d").reads(),
            vec!["plays:5-10@1mo".to_owned()]
        );
        assert_eq!(
            Search::read("plays:5-10@1mo"),
            Search::read("plays:5-10@30d")
        );
    }

    #[test]
    fn two_play_counts_only_fold_into_a_range_where_their_windows_agree() {
        let mismatched = Asked {
            denied: false,
            all: vec![
                Condition::Term(Term::Plays {
                    compare: Compare::AtLeast,
                    plays: 5,
                    within: Some(Duration::from_secs(WEEK)),
                }),
                Condition::Term(Term::Plays {
                    compare: Compare::AtMost,
                    plays: 10,
                    within: Some(Duration::from_secs(YEAR)),
                }),
            ],
        };

        assert_eq!(mismatched.to_string(), "plays:>=5@1w plays:<=10@1y");
    }

    #[test]
    fn an_age_is_written_back_in_the_largest_unit_that_divides_it() {
        assert_eq!(aged(Duration::from_secs(30 * DAY)), "1mo");
        assert_eq!(aged(Duration::from_secs(90 * DAY)), "3mo");
        assert_eq!(aged(Duration::from_secs(14 * DAY)), "2w");
        assert_eq!(aged(Duration::from_secs(10 * DAY)), "10d");
        assert_eq!(aged(Duration::from_secs(90 * MINUTE)), "1.5h");
    }

    #[test]
    fn a_leading_minus_denies_the_token_it_sits_on() {
        let search = Search::read("-is:lossy");

        assert_eq!(
            search.clauses,
            vec![Clause {
                any: vec![Asked {
                    denied: true,
                    all: vec![Condition::Term(Term::Shape(Shape::Lossy))],
                }],
            }]
        );
        assert_eq!(search.reads(), vec!["-is:lossy".to_owned()]);
        assert_eq!(terms("-codec:mp3"), vec![Term::Codec(Codec::Mp3)]);
        assert_eq!(words("-live"), vec!["live"]);
        assert_eq!(shape("-live"), vec!["-live"]);
    }

    #[test]
    fn a_minus_anywhere_else_is_part_of_what_it_is_written_in() {
        assert_eq!(words("well-known"), vec!["well-known"]);
        assert_eq!(words("\"-live\""), vec!["\"-live\""]);
        assert_eq!(terms("year:1970-1979").len(), 2);
        assert!(Search::read("-").is_empty());
    }

    #[test]
    fn or_joins_the_tokens_either_side_of_it_into_one_clause() {
        let search = Search::read("codec:flac or codec:alac");

        assert_eq!(search.clauses.len(), 1);
        assert_eq!(search.clauses[0].any.len(), 2);
        assert_eq!(search.reads(), vec!["codec:flac or codec:alac".to_owned()]);
        assert_eq!(
            shape("is:hires OR is:mono or ada"),
            vec!["is:hires or is:mono or ada".to_owned()],
            "or should read in any case and join more than two"
        );
    }

    #[test]
    fn a_clause_stands_beside_the_others_rather_than_inside_them() {
        let search = Search::read("is:lossless year:1975 or year:1995");

        assert_eq!(search.clauses.len(), 2);
        assert_eq!(
            search.reads(),
            vec![
                "is:lossless".to_owned(),
                "year:1975 or year:1995".to_owned()
            ]
        );
    }

    #[test]
    fn a_denied_range_is_either_bound_falling_outside_it() {
        let search = Search::read("-year:1970-1979");

        assert_eq!(search.clauses.len(), 1);
        assert_eq!(search.clauses[0].any.len(), 2);
        assert!(search.clauses[0].any.iter().all(|asked| asked.denied));
        assert_eq!(
            search.reads(),
            vec!["-year:>=1970 or -year:<=1979".to_owned()]
        );
    }

    #[test]
    fn a_range_is_written_back_as_the_range_it_was_typed_as() {
        for text in [
            "year:1970-1979",
            "plays:1-3",
            "plays:5-10@1mo",
            "length:3m-5m30s",
            "rate:44100-96000",
            "depth:16-24",
        ] {
            let search = Search::read(text);

            assert_eq!(search.reads(), vec![text.to_owned()], "{text} reads wrong");
            assert_eq!(Search::read(&search.reads().join(" ")), search);
        }
    }

    #[test]
    fn a_range_beside_an_or_binds_the_alternation_to_the_whole_range() {
        let search = Search::read("year:1970-1979 or is:hires");

        assert_eq!(
            search.reads(),
            vec!["year:1970-1979 or is:hires".to_owned()]
        );
        assert_eq!(Search::read(&search.reads().join(" ")), search);
    }

    #[test]
    fn an_or_with_nothing_to_join_is_the_word_it_was_written_as() {
        assert_eq!(words("or moon"), vec!["or", "moon"]);
        assert_eq!(words("moon or"), vec!["moon", "or"]);
        assert_eq!(words("\"or\""), vec!["\"or\""]);
        assert_eq!(Search::read("or moon").clauses.len(), 2);
    }

    fn lit(text: &str, typed: &str, column: Column) -> Vec<String> {
        Search::read(typed)
            .lit(text, column)
            .into_iter()
            .map(|run| text.get(run).unwrap_or_default().to_owned())
            .collect()
    }

    #[test]
    fn a_word_lights_the_whole_of_every_token_it_begins() {
        assert_eq!(lit("One of These Days", "these", Column::Title), ["These"]);
        assert_eq!(lit("One of These Days", "th", Column::Title), ["These"]);
        assert_eq!(
            lit("One of These Days", "one days", Column::Title),
            ["One", "Days"]
        );
        assert!(lit("One of These Days", "echoes", Column::Title).is_empty());
    }

    #[test]
    fn a_word_lights_a_name_however_the_name_is_spelled() {
        assert_eq!(lit("Björk", "bjork", Column::Artist), ["Björk"]);
        assert_eq!(
            lit("Marcin Przybyłowicz", "przybylowicz", Column::Artist),
            ["Przybyłowicz"]
        );
        assert_eq!(lit("Straße", "strasse", Column::Title), ["Straße"]);
        assert_eq!(lit("Kıskanç", "kiskanc", Column::Title), ["Kıskanç"]);
    }

    #[test]
    fn a_word_scoped_to_a_column_lights_nothing_in_another() {
        assert_eq!(lit("Echoes", "title:echoes", Column::Title), ["Echoes"]);
        assert!(lit("Echoes", "title:echoes", Column::Artist).is_empty());
        assert_eq!(lit("Echoes", "echoes", Column::Artist), ["Echoes"]);
    }

    #[test]
    fn a_denied_word_and_a_term_light_nothing() {
        assert!(lit("Echoes", "-echoes", Column::Title).is_empty());
        assert!(lit("1971", "year:1971", Column::Title).is_empty());
    }

    #[test]
    fn a_phrase_lights_the_run_between_its_ends_and_a_word_lights_only_its_own() {
        assert_eq!(
            lit("One of These Days", "\"of these\"", Column::Title),
            ["of These"]
        );
        assert!(lit("These of One Days", "\"of these\"", Column::Title).is_empty());
    }

    #[test]
    fn runs_that_meet_are_one_run() {
        assert_eq!(
            lit("Echoes", "ech echoes echo", Column::Title),
            ["Echoes"],
            "one token matched three ways was lit three times over"
        );
        assert_eq!(
            lit("Set the Controls", "set controls the", Column::Title),
            ["Set", "the", "Controls"]
        );
    }

    #[test]
    fn nothing_readable_is_an_empty_search() {
        assert!(Search::read("").is_empty());
        assert!(Search::read("   ").is_empty());
        assert!(Search::read("\"\"").is_empty());
    }
}
