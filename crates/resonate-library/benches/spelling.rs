use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use resonate_library::{Column, Spellings};

const TRACKS: usize = 500_000;
const ARTISTS: usize = 50_000;
const ALBUMS: usize = 60_000;
const GENRES: usize = 400;
const WORDS: usize = 40_000;
const RUNS: u32 = 5;
const SEED: u64 = 0x5265_736F_6E61_7465;
const SYLLABLES: [&str; 24] = [
    "ka", "lo", "mi", "ne", "ru", "ta", "vo", "si", "en", "ar", "go", "le", "pa", "de", "ri",
    "sto", "man", "tel", "or", "is", "ba", "chu", "fe", "zin",
];

struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn skewed(&mut self, bound: usize) -> usize {
        let one = self.below(bound);
        let other = self.below(bound);
        one.min(other)
    }
}

fn words(noise: &mut Noise) -> Vec<String> {
    (0..WORDS)
        .map(|_| {
            let syllables = 2 + noise.below(3);
            let mut word: String = (0..syllables)
                .map(|_| SYLLABLES[noise.below(SYLLABLES.len())])
                .collect();
            if let Some(first) = word.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            word
        })
        .collect()
}

fn name(noise: &mut Noise, words: &[String], most: usize) -> String {
    let count = 1 + noise.below(most);
    (0..count)
        .map(|_| words[noise.skewed(words.len())].as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn catalog() -> (Spellings, Vec<String>, Vec<String>) {
    let mut noise = Noise(SEED);
    let words = words(&mut noise);
    let artists: Vec<String> = (0..ARTISTS).map(|_| name(&mut noise, &words, 3)).collect();
    let albums: Vec<String> = (0..ALBUMS).map(|_| name(&mut noise, &words, 4)).collect();
    let genres: Vec<String> = (0..GENRES).map(|_| name(&mut noise, &words, 2)).collect();

    let mut spellings = Spellings::default();
    for _ in 0..TRACKS {
        spellings.taking(Column::Title, &name(&mut noise, &words, 6));
        spellings.taking(Column::Artist, &artists[noise.skewed(ARTISTS)]);
        spellings.taking(Column::Genre, &genres[noise.skewed(GENRES)]);
    }
    for artist in &artists {
        spellings.taking(Column::Artist, artist);
    }
    for album in &albums {
        spellings.taking(Column::Album, album);
    }
    (spellings, words, artists)
}

fn timed(what: &str, mut run: impl FnMut()) {
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let started = Instant::now();
        run();
        best = best.min(started.elapsed());
    }
    println!("{what:<48} {:>10.3} ms", best.as_secs_f64() * 1e3);
}

fn misspelt(word: &str) -> String {
    let mut letters: Vec<char> = word.to_lowercase().chars().collect();
    if letters.len() > 3 {
        letters.swap(1, 2);
    }
    letters.into_iter().collect()
}

fn main() {
    let started = Instant::now();
    let (spellings, words, artists) = catalog();
    println!(
        "{:<48} {:>10.3} ms",
        "building the vocabulary of 500k tracks",
        started.elapsed().as_secs_f64() * 1e3
    );

    let started = Instant::now();
    black_box(spellings.did_you_mean(&misspelt(&words[9])));
    println!(
        "{:<48} {:>10.3} ms",
        "the first correction, which orders the keys",
        started.elapsed().as_secs_f64() * 1e3
    );

    let common = words[0].to_lowercase();
    let rare = words[WORDS - 1].to_lowercase();
    let typo = misspelt(&words[17]);
    let artist = artists[3].to_lowercase();
    let artist_typo = misspelt(&artists[3]);
    let long = format!(
        "{} {} {} {} {} {}",
        words[1], words[2], words[3], words[4], words[5], words[6]
    )
    .to_lowercase();
    let queries = [
        ("did you mean: a word the catalog holds", common.clone()),
        ("did you mean: a rare word it holds", rare),
        ("did you mean: a word one swap away", typo.clone()),
        ("did you mean: an artist it holds", artist.clone()),
        ("did you mean: an artist one swap away", artist_typo),
        ("did you mean: six words", long),
        (
            "did you mean: nothing like anything",
            "qwxzqwxz vbnmvbnm".to_owned(),
        ),
    ];
    for (what, query) in &queries {
        timed(what, || {
            black_box(spellings.did_you_mean(black_box(query)));
        });
    }

    let prefixes = [
        (
            "completing: one letter",
            common.get(..1).unwrap_or_default().to_owned(),
        ),
        (
            "completing: three letters",
            common.get(..3).unwrap_or_default().to_owned(),
        ),
        ("completing: an artist's first word and a half", {
            let mut typed = artist.clone();
            typed.truncate(typed.len().saturating_sub(2));
            typed
        }),
        ("completing: a word one swap away", typo),
    ];
    for (what, typed) in &prefixes {
        timed(what, || {
            black_box(spellings.completing(black_box(typed)));
        });
    }
}
