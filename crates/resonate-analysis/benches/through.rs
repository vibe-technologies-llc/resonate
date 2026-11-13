use std::{
    env, fs,
    hint::black_box,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use resonate_analysis::{Applied, Cut, Watch, excerpt, loudest_through, peak_through, print};
use resonate_codec::Sources;
use resonate_core::{
    MediaLocation,
    eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q},
};

const RUNS: usize = 15;

fn profile() -> Arc<Profile> {
    let bands = [
        (80.0, 5.0, 1.0),
        (250.0, -3.0, 1.4),
        (800.0, -4.0, 1.5),
        (2_500.0, 6.0, 2.0),
        (6_000.0, -2.0, 1.0),
        (12_000.0, 4.0, 1.0),
    ]
    .iter()
    .map(|(hertz, decibels, q)| {
        Band::new(
            BandKind::Peaking,
            Frequency::from_hertz(*hertz).expect("in range"),
            BandGain::from_decibels(*decibels).expect("in range"),
            Q::from_units(*q).expect("in range"),
        )
    })
    .collect();
    Arc::new(Profile::new(Preamp::NONE, bands).expect("a profile"))
}

fn fastest<T>(mut run: impl FnMut() -> T) -> (Duration, T) {
    let mut fastest = Duration::MAX;
    let mut last = None;
    for _ in 0..RUNS {
        let started = Instant::now();
        let made = black_box(run());
        fastest = fastest.min(started.elapsed());
        last = Some(made);
    }
    (fastest, last.expect("a run"))
}

fn main() {
    let folder = PathBuf::from(env::var_os("RESONATE_BENCH_SONGS").expect("a folder"));
    let mut songs: Vec<PathBuf> = fs::read_dir(folder)
        .expect("a folder")
        .map(|entry| entry.expect("an entry").path())
        .collect();
    songs.sort();
    let sources = Sources::local();
    let watch = Watch::default();
    let applied = Applied::from(profile());
    let mut kept = String::new();
    for song in &songs {
        let location = MediaLocation::local(song);
        let (peak_time, peak) =
            fastest(|| peak_through(&sources, &location, None, &applied, &watch).expect("peak"));
        let (print_time, printed) =
            fastest(|| print(&sources, &location, None, &watch).expect("print"));
        let (excerpt_time, taken) = fastest(|| {
            excerpt(
                &sources,
                &location,
                None,
                Duration::from_secs(5),
                Duration::from_secs(20),
                &watch,
            )
            .expect("excerpt")
        });
        println!(
            "{}: peak_through {:6.1} ms, print {:6.1} ms, excerpt {:6.1} ms",
            song.file_name().expect("a name").to_string_lossy(),
            peak_time.as_secs_f64() * 1e3,
            print_time.as_secs_f64() * 1e3,
            excerpt_time.as_secs_f64() * 1e3,
        );
        kept.push_str(&format!(
            "{} {:016x} {:?} {:?}\n",
            song.display(),
            peak.to_bits(),
            printed,
            taken.map(|taken| taken.mono.iter().fold(0_u64, |hash, sample| hash
                .wrapping_mul(31)
                .wrapping_add(u64::from(sample.to_bits()))))
        ));
    }
    let cuts: Vec<Cut> = songs
        .iter()
        .cycle()
        .take(16)
        .map(|song| Cut {
            location: MediaLocation::local(song),
            span: None,
        })
        .collect();
    let (time, loudest) =
        fastest(|| loudest_through(&sources, &cuts, &applied, &watch).expect("loudest"));
    println!(
        "loudest_through of {} cuts: {:7.1} ms",
        cuts.len(),
        time.as_secs_f64() * 1e3
    );
    kept.push_str(&format!("loudest {:016x}\n", loudest.peak.to_bits()));
    if let Some(path) = env::var_os("RESONATE_BENCH_KEPT") {
        fs::write(path, kept).expect("kept");
    }
}
