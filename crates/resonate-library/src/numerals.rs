use std::{num::NonZeroU32, sync::LazyLock};

use ahash::AHashMap;

use crate::store::folded_letters;

pub(crate) const SEPARATORS: [char; 4] = [' ', '-', '_', '.'];

const FEMININE_ENDING: char = 'a';
const MASCULINE_ENDING: char = 'o';
const HIGHEST: u32 = 99;

pub(crate) static ELSEWHERE: LazyLock<Numerals> = LazyLock::new(Numerals::spelled);

pub(crate) fn plainly(text: &str) -> String {
    folded_letters(text.trim())
        .split(SEPARATORS)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) struct Numerals {
    cardinals: AHashMap<String, u32>,
    ordinals: Vec<(String, u32)>,
}

impl Numerals {
    fn spelled() -> Self {
        let mut cardinals = AHashMap::new();
        let mut ordinals = AHashMap::new();
        for language in LANGUAGES {
            for number in 1..=HIGHEST {
                for spelling in (language.cardinals)(number) {
                    cardinals.insert(plainly(&spelling), number);
                }
                for spelling in (language.ordinals)(number) {
                    ordinals.insert(plainly(&spelling), number);
                }
            }
        }
        let mut ordinals: Vec<(String, u32)> = ordinals.into_iter().collect();
        ordinals.sort_by(|(one, _), (other, _)| other.len().cmp(&one.len()).then(one.cmp(other)));
        Self {
            cardinals,
            ordinals,
        }
    }

    pub(crate) fn cardinal(&self, word: &str) -> Option<NonZeroU32> {
        self.cardinals.get(word).copied().and_then(NonZeroU32::new)
    }

    pub(crate) fn ordinal_at_the_front<'a>(
        &self,
        folded: &'a str,
    ) -> Option<(NonZeroU32, &'a str)> {
        self.ordinals.iter().find_map(|(spelling, number)| {
            Some((
                NonZeroU32::new(*number)?,
                folded.strip_prefix(spelling.as_str())?,
            ))
        })
    }
}

struct Language {
    cardinals: fn(u32) -> Vec<String>,
    ordinals: fn(u32) -> Vec<String>,
}

const LANGUAGES: [Language; 6] = [
    Language {
        cardinals: french_cardinals,
        ordinals: french_ordinals,
    },
    Language {
        cardinals: spanish_cardinals,
        ordinals: spanish_ordinals,
    },
    Language {
        cardinals: italian_cardinals,
        ordinals: italian_ordinals,
    },
    Language {
        cardinals: german_cardinals,
        ordinals: german_ordinals,
    },
    Language {
        cardinals: dutch_cardinals,
        ordinals: dutch_ordinals,
    },
    Language {
        cardinals: portuguese_cardinals,
        ordinals: portuguese_ordinals,
    },
];

fn units_of(number: u32) -> usize {
    (number % 10) as usize
}

fn tens_of(number: u32) -> usize {
    (number / 10) as usize
}

fn with_feminine(spellings: Vec<String>) -> Vec<String> {
    let feminine: Vec<String> = spellings
        .iter()
        .filter(|spelling| spelling.ends_with(MASCULINE_ENDING))
        .map(|spelling| {
            spelling
                .split(' ')
                .map(|word| match word.strip_suffix(MASCULINE_ENDING) {
                    Some(stem) => format!("{stem}{FEMININE_ENDING}"),
                    None => word.to_owned(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    spellings.into_iter().chain(feminine).collect()
}

const FRENCH_UNITS: [&str; 10] = [
    "", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf",
];
const FRENCH_TEENS: [&str; 10] = [
    "dix", "onze", "douze", "treize", "quatorze", "quinze", "seize", "dix-sept", "dix-huit",
    "dix-neuf",
];
const FRENCH_TENS: [&str; 7] = [
    "",
    "",
    "vingt",
    "trente",
    "quarante",
    "cinquante",
    "soixante",
];

fn french_cardinal(number: u32) -> String {
    let units = units_of(number);
    match number {
        1..=9 => FRENCH_UNITS[units].to_owned(),
        10..=19 => FRENCH_TEENS[units].to_owned(),
        20..=69 => {
            let tens = FRENCH_TENS[tens_of(number)];
            match units {
                0 => tens.to_owned(),
                1 => format!("{tens}-et-un"),
                _ => format!("{tens}-{}", FRENCH_UNITS[units]),
            }
        }
        71 => "soixante-et-onze".to_owned(),
        70..=79 => format!("soixante-{}", FRENCH_TEENS[units]),
        80 => "quatre-vingts".to_owned(),
        81..=89 => format!("quatre-vingt-{}", FRENCH_UNITS[units]),
        _ => format!("quatre-vingt-{}", FRENCH_TEENS[units]),
    }
}

fn french_cardinals(number: u32) -> Vec<String> {
    let cardinal = french_cardinal(number);
    let mut spellings = vec![cardinal.clone()];
    if number == 80 {
        spellings.push("quatre-vingt".to_owned());
    }
    if let Some(stem) = cardinal.strip_suffix("un") {
        spellings.push(format!("{stem}une"));
    }
    spellings
}

fn french_ordinals(number: u32) -> Vec<String> {
    match number {
        1 => vec!["premier".to_owned(), "premiere".to_owned()],
        _ => {
            let cardinal = match number {
                80 => "quatre-vingt".to_owned(),
                _ => french_cardinal(number),
            };
            let stem = cardinal.strip_suffix('e').unwrap_or(&cardinal);
            let stem = match stem.strip_suffix('f') {
                Some(before) => format!("{before}v"),
                None if stem.ends_with('q') => format!("{stem}u"),
                None => stem.to_owned(),
            };
            let mut spellings = vec![format!("{stem}ieme")];
            if number == 2 {
                spellings.extend(["second".to_owned(), "seconde".to_owned()]);
            }
            spellings
        }
    }
}

const SPANISH_UNITS: [&str; 10] = [
    "", "uno", "dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve",
];
const SPANISH_TEENS: [&str; 10] = [
    "diez",
    "once",
    "doce",
    "trece",
    "catorce",
    "quince",
    "dieciseis",
    "diecisiete",
    "dieciocho",
    "diecinueve",
];
const SPANISH_TENS: [&str; 10] = [
    "",
    "",
    "veinte",
    "treinta",
    "cuarenta",
    "cincuenta",
    "sesenta",
    "setenta",
    "ochenta",
    "noventa",
];

fn spanish_cardinals(number: u32) -> Vec<String> {
    let units = units_of(number);
    let masculine = match number {
        1..=9 => SPANISH_UNITS[units].to_owned(),
        10..=19 => SPANISH_TEENS[units].to_owned(),
        20 => SPANISH_TENS[2].to_owned(),
        21..=29 => format!("veinti{}", SPANISH_UNITS[units]),
        _ if units == 0 => SPANISH_TENS[tens_of(number)].to_owned(),
        _ => format!(
            "{} y {}",
            SPANISH_TENS[tens_of(number)],
            SPANISH_UNITS[units]
        ),
    };
    let mut spellings = vec![masculine.clone()];
    if let Some(stem) = masculine.strip_suffix("uno") {
        spellings.extend([format!("{stem}un"), format!("{stem}una")]);
    }
    spellings
}

const SPANISH_ORDINAL_UNITS: [&str; 10] = [
    "", "primero", "segundo", "tercero", "cuarto", "quinto", "sexto", "septimo", "octavo", "noveno",
];
const SPANISH_ORDINAL_TENS: [&str; 10] = [
    "",
    "decimo",
    "vigesimo",
    "trigesimo",
    "cuadragesimo",
    "quincuagesimo",
    "sexagesimo",
    "septuagesimo",
    "octogesimo",
    "nonagesimo",
];

fn spanish_ordinals(number: u32) -> Vec<String> {
    let units = units_of(number);
    let mut spellings = match (tens_of(number), units) {
        (0, _) => vec![SPANISH_ORDINAL_UNITS[units].to_owned()],
        (tens, 0) => vec![SPANISH_ORDINAL_TENS[tens].to_owned()],
        (tens, _) => {
            let tens = SPANISH_ORDINAL_TENS[tens];
            let units = SPANISH_ORDINAL_UNITS[units];
            vec![format!("{tens} {units}"), format!("{tens}{units}")]
        }
    };
    match number {
        1 => spellings.push("primer".to_owned()),
        3 => spellings.push("tercer".to_owned()),
        7 => spellings.push("setimo".to_owned()),
        9 => spellings.push("nono".to_owned()),
        11 => spellings.push("undecimo".to_owned()),
        12 => spellings.push("duodecimo".to_owned()),
        18 => spellings.push("decimoctavo".to_owned()),
        _ => {}
    }
    with_feminine(spellings)
}

const ITALIAN_UNITS: [&str; 10] = [
    "", "uno", "due", "tre", "quattro", "cinque", "sei", "sette", "otto", "nove",
];
const ITALIAN_TEENS: [&str; 10] = [
    "dieci",
    "undici",
    "dodici",
    "tredici",
    "quattordici",
    "quindici",
    "sedici",
    "diciassette",
    "diciotto",
    "diciannove",
];
const ITALIAN_TENS: [&str; 10] = [
    "",
    "",
    "venti",
    "trenta",
    "quaranta",
    "cinquanta",
    "sessanta",
    "settanta",
    "ottanta",
    "novanta",
];

fn italian_cardinal(number: u32) -> String {
    let units = units_of(number);
    match number {
        1..=9 => ITALIAN_UNITS[units].to_owned(),
        10..=19 => ITALIAN_TEENS[units].to_owned(),
        _ => {
            let tens = ITALIAN_TENS[tens_of(number)];
            match units {
                0 => tens.to_owned(),
                1 | 8 => format!("{}{}", &tens[..tens.len() - 1], ITALIAN_UNITS[units]),
                _ => format!("{tens}{}", ITALIAN_UNITS[units]),
            }
        }
    }
}

fn italian_cardinals(number: u32) -> Vec<String> {
    vec![italian_cardinal(number)]
}

const ITALIAN_ORDINAL_UNITS: [&str; 11] = [
    "", "primo", "secondo", "terzo", "quarto", "quinto", "sesto", "settimo", "ottavo", "nono",
    "decimo",
];

fn italian_ordinals(number: u32) -> Vec<String> {
    let spelling = match number {
        1..=10 => ITALIAN_ORDINAL_UNITS[number as usize].to_owned(),
        _ => {
            let cardinal = italian_cardinal(number);
            if cardinal.ends_with("tre") || cardinal.ends_with("sei") {
                format!("{cardinal}esimo")
            } else {
                format!("{}esimo", &cardinal[..cardinal.len() - 1])
            }
        }
    };
    with_feminine(vec![spelling])
}

const GERMAN_UNITS: [&str; 10] = [
    "", "eins", "zwei", "drei", "vier", "funf", "sechs", "sieben", "acht", "neun",
];
const GERMAN_TEENS: [&str; 10] = [
    "zehn", "elf", "zwolf", "dreizehn", "vierzehn", "funfzehn", "sechzehn", "siebzehn", "achtzehn",
    "neunzehn",
];
const GERMAN_TENS: [&str; 10] = [
    "", "", "zwanzig", "dreissig", "vierzig", "funfzig", "sechzig", "siebzig", "achtzig", "neunzig",
];
const GERMAN_ORDINAL_STEMS: [&str; 13] = [
    "", "erst", "zweit", "dritt", "viert", "funft", "sechst", "siebt", "acht", "neunt", "zehnt",
    "elft", "zwolft",
];
const GERMAN_ENDINGS: [&str; 4] = ["e", "er", "es", "en"];

fn german_cardinal(number: u32) -> String {
    let units = units_of(number);
    match number {
        1..=9 => GERMAN_UNITS[units].to_owned(),
        10..=19 => GERMAN_TEENS[units].to_owned(),
        _ if units == 0 => GERMAN_TENS[tens_of(number)].to_owned(),
        _ => {
            let joined = if units == 1 {
                "ein"
            } else {
                GERMAN_UNITS[units]
            };
            format!("{joined}und{}", GERMAN_TENS[tens_of(number)])
        }
    }
}

fn german_cardinals(number: u32) -> Vec<String> {
    vec![german_cardinal(number)]
}

fn german_ordinals(number: u32) -> Vec<String> {
    let stem = match number {
        1..=12 => GERMAN_ORDINAL_STEMS[number as usize].to_owned(),
        13..=19 => format!("{}t", german_cardinal(number)),
        _ => format!("{}st", german_cardinal(number)),
    };
    GERMAN_ENDINGS
        .iter()
        .map(|ending| format!("{stem}{ending}"))
        .collect()
}

const DUTCH_UNITS: [&str; 10] = [
    "", "een", "twee", "drie", "vier", "vijf", "zes", "zeven", "acht", "negen",
];
const DUTCH_TEENS: [&str; 10] = [
    "tien",
    "elf",
    "twaalf",
    "dertien",
    "veertien",
    "vijftien",
    "zestien",
    "zeventien",
    "achttien",
    "negentien",
];
const DUTCH_TENS: [&str; 10] = [
    "", "", "twintig", "dertig", "veertig", "vijftig", "zestig", "zeventig", "tachtig", "negentig",
];
const DUTCH_ORDINALS: [&str; 13] = [
    "", "eerste", "tweede", "derde", "vierde", "vijfde", "zesde", "zevende", "achtste", "negende",
    "tiende", "elfde", "twaalfde",
];

fn dutch_cardinal(number: u32) -> String {
    let units = units_of(number);
    match number {
        1..=9 => DUTCH_UNITS[units].to_owned(),
        10..=19 => DUTCH_TEENS[units].to_owned(),
        _ if units == 0 => DUTCH_TENS[tens_of(number)].to_owned(),
        _ => format!("{}en{}", DUTCH_UNITS[units], DUTCH_TENS[tens_of(number)]),
    }
}

fn dutch_cardinals(number: u32) -> Vec<String> {
    vec![dutch_cardinal(number)]
}

fn dutch_ordinals(number: u32) -> Vec<String> {
    vec![match number {
        1..=12 => DUTCH_ORDINALS[number as usize].to_owned(),
        13..=19 => format!("{}de", dutch_cardinal(number)),
        _ => format!("{}ste", dutch_cardinal(number)),
    }]
}

const PORTUGUESE_UNITS: [&str; 10] = [
    "", "um", "dois", "tres", "quatro", "cinco", "seis", "sete", "oito", "nove",
];
const PORTUGUESE_TEENS: [&str; 10] = [
    "dez",
    "onze",
    "doze",
    "treze",
    "catorze",
    "quinze",
    "dezesseis",
    "dezessete",
    "dezoito",
    "dezenove",
];
const PORTUGUESE_TEENS_IN_PORTUGAL: [(u32, &str); 4] = [
    (14, "quatorze"),
    (16, "dezasseis"),
    (17, "dezassete"),
    (19, "dezanove"),
];
const PORTUGUESE_TENS: [&str; 10] = [
    "",
    "",
    "vinte",
    "trinta",
    "quarenta",
    "cinquenta",
    "sessenta",
    "setenta",
    "oitenta",
    "noventa",
];
const PORTUGUESE_FEMININE_UNITS: [(usize, &str); 2] = [(1, "uma"), (2, "duas")];

fn portuguese_cardinals(number: u32) -> Vec<String> {
    let units = units_of(number);
    let tens = tens_of(number);
    let mut spellings = match number {
        1..=9 => vec![PORTUGUESE_UNITS[units].to_owned()],
        10..=19 => vec![PORTUGUESE_TEENS[units].to_owned()],
        _ if units == 0 => vec![PORTUGUESE_TENS[tens].to_owned()],
        _ => vec![format!(
            "{} e {}",
            PORTUGUESE_TENS[tens], PORTUGUESE_UNITS[units]
        )],
    };
    spellings.extend(
        PORTUGUESE_TEENS_IN_PORTUGAL
            .iter()
            .filter(|(teen, _)| *teen == number)
            .map(|(_, spelling)| (*spelling).to_owned()),
    );
    if tens != 1 {
        for (unit, feminine) in PORTUGUESE_FEMININE_UNITS {
            if unit == units {
                spellings.push(match tens {
                    0 => feminine.to_owned(),
                    _ => format!("{} e {feminine}", PORTUGUESE_TENS[tens]),
                });
            }
        }
    }
    spellings
}

const PORTUGUESE_ORDINAL_UNITS: [&str; 10] = [
    "", "primeiro", "segundo", "terceiro", "quarto", "quinto", "sexto", "setimo", "oitavo", "nono",
];
const PORTUGUESE_ORDINAL_TENS: [&str; 10] = [
    "",
    "decimo",
    "vigesimo",
    "trigesimo",
    "quadragesimo",
    "quinquagesimo",
    "sexagesimo",
    "septuagesimo",
    "octogesimo",
    "nonagesimo",
];

fn portuguese_ordinals(number: u32) -> Vec<String> {
    let units = units_of(number);
    let mut spellings = match (tens_of(number), units) {
        (0, _) => vec![PORTUGUESE_ORDINAL_UNITS[units].to_owned()],
        (tens, 0) => vec![PORTUGUESE_ORDINAL_TENS[tens].to_owned()],
        (tens, _) => vec![format!(
            "{} {}",
            PORTUGUESE_ORDINAL_TENS[tens], PORTUGUESE_ORDINAL_UNITS[units]
        )],
    };
    if tens_of(number) == 7 {
        spellings.extend(
            spellings
                .clone()
                .into_iter()
                .map(|spelling| spelling.replacen("septuagesimo", "setuagesimo", 1)),
        );
    }
    with_feminine(spellings)
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;

    use super::*;

    fn number_of(word: &str) -> Option<u32> {
        ELSEWHERE.cardinal(&plainly(word)).map(NonZeroU32::get)
    }

    #[test]
    fn a_spelling_names_one_number_whichever_language_spells_it() {
        let mut named: AHashMap<String, AHashSet<u32>> = AHashMap::new();
        for language in LANGUAGES {
            for number in 1..=HIGHEST {
                for spelling in (language.cardinals)(number) {
                    named.entry(plainly(&spelling)).or_default().insert(number);
                }
            }
        }
        let torn: Vec<_> = named
            .iter()
            .filter(|(_, numbers)| numbers.len() > 1)
            .collect();
        assert!(torn.is_empty(), "spellings naming two numbers: {torn:?}");

        let mut ordered: AHashMap<String, AHashSet<u32>> = AHashMap::new();
        for language in LANGUAGES {
            for number in 1..=HIGHEST {
                for spelling in (language.ordinals)(number) {
                    ordered
                        .entry(plainly(&spelling))
                        .or_default()
                        .insert(number);
                }
            }
        }
        let torn: Vec<_> = ordered
            .iter()
            .filter(|(_, numbers)| numbers.len() > 1)
            .collect();
        assert!(torn.is_empty(), "ordinals naming two numbers: {torn:?}");
    }

    #[test]
    fn every_language_composes_its_numbers_the_way_it_writes_them() {
        for (word, number) in [
            ("vingt-et-un", 21),
            ("soixante-dix-sept", 77),
            ("quatre-vingts", 80),
            ("quatre-vingt-onze", 91),
            ("treinta y uno", 31),
            ("veintidós", 22),
            ("ventuno", 21),
            ("trentotto", 38),
            ("ventitré", 23),
            ("einundzwanzig", 21),
            ("dreiunddreißig", 33),
            ("tweeëntwintig", 22),
            ("negenennegentig", 99),
            ("vinte e um", 21),
            ("dezasseis", 16),
            ("trinta e duas", 32),
        ] {
            assert_eq!(number_of(word), Some(number), "{word}");
        }
    }

    #[test]
    fn every_language_composes_its_ordinals_the_way_it_writes_them() {
        let ordinal = |text: &str| {
            let plain = plainly(text);
            ELSEWHERE
                .ordinal_at_the_front(&plain)
                .filter(|(_, beyond)| beyond.is_empty())
                .map(|(number, _)| number.get())
        };
        for (word, number) in [
            ("vingt-et-unième", 21),
            ("quatre-vingtième", 80),
            ("dix-neuvième", 19),
            ("vigésimo primero", 21),
            ("decimotercera", 13),
            ("ventunesimo", 21),
            ("ventitreesimo", 23),
            ("einundzwanzigste", 21),
            ("dreizehnter", 13),
            ("eenentwintigste", 21),
            ("dertiende", 13),
            ("vigésima segunda", 22),
            ("setuagésimo quinto", 75),
        ] {
            assert_eq!(ordinal(word), Some(number), "{word}");
        }
    }
}
