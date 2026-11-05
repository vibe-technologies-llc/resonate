use std::{
    env, fs,
    hint::black_box,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use resonate_analysis::{Examined, Watch, analyse, band_figures, study};
use resonate_codec::{DecodeStatus, Decoder, Sources};
use resonate_core::{AudioBuffer, MediaLocation};

const SONGS_HELD_IN: &str = "RESONATE_BENCH_SONGS";
const ANSWERS_KEPT_IN: &str = "RESONATE_BENCH_STUDIES";
const WORKERS_AT_ONCE: &str = "RESONATE_BENCH_WORKERS";
const RATE: u32 = 44_100;
const SYNTHETIC_SECONDS: u32 = 60;
const CPU_TICKS_A_SECOND: f64 = 100.0;
const STAT_FIELDS_BEFORE_USER_TIME: usize = 11;

struct Noise(u32);

impl Noise {
    fn white(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1_u32 << 24) as f32 - 0.5
    }
}

fn wave_of(seconds: u32) -> Vec<u8> {
    let frames = RATE * seconds;
    let bytes = frames * 4;
    let mut noise = Noise(1);
    let mut wave = Vec::with_capacity(bytes as usize + 44);

    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36 + bytes).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&2_u16.to_le_bytes());
    wave.extend_from_slice(&RATE.to_le_bytes());
    wave.extend_from_slice(&(RATE * 4).to_le_bytes());
    wave.extend_from_slice(&4_u16.to_le_bytes());
    wave.extend_from_slice(&16_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&bytes.to_le_bytes());
    for frame in 0..frames {
        let time = f64::from(frame) / f64::from(RATE);
        let tone = (time * 220.0 * std::f64::consts::TAU).sin() as f32 * 0.25;
        let swell = ((time * 2.0 * std::f64::consts::TAU).sin() as f32 + 1.0) * 0.25;
        for _ in 0..2 {
            let sample = tone + noise.white() * swell;
            let scaled = (sample * f32::from(i16::MAX)) as i16;
            wave.extend_from_slice(&scaled.to_le_bytes());
        }
    }
    wave
}

fn cpu_seconds() -> Option<f64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let (_, after_the_name) = stat.rsplit_once(") ")?;
    let mut fields = after_the_name
        .split_ascii_whitespace()
        .skip(STAT_FIELDS_BEFORE_USER_TIME);
    let user: f64 = fields.next()?.parse().ok()?;
    let system: f64 = fields.next()?.parse().ok()?;
    Some((user + system) / CPU_TICKS_A_SECOND)
}

struct Spent {
    wall: Duration,
    cpu: Option<f64>,
}

impl Spent {
    fn line(&self, name: &str, heard: Duration) {
        let wall = self.wall.as_secs_f64();
        let cpu = self
            .cpu
            .map_or_else(|| "    -".to_owned(), |cpu| format!("{cpu:5.1}"));
        println!(
            "  {name:<13} {wall:7.2} s wall, {cpu} s cpu, {:6.1} x real time",
            heard.as_secs_f64() / wall.max(f64::MIN_POSITIVE),
        );
    }
}

fn timed<T>(work: impl FnOnce() -> T) -> (T, Spent) {
    let cpu_before = cpu_seconds();
    let started = Instant::now();
    let answer = black_box(work());
    let wall = started.elapsed();
    let cpu = cpu_before
        .zip(cpu_seconds())
        .map(|(before, after)| after - before);
    (answer, Spent { wall, cpu })
}

fn decoded_alone(sources: &Sources, location: &MediaLocation) -> u64 {
    let (mut decoder, info) = Decoder::open(sources, location).expect("a song opens");
    let mut block = AudioBuffer::empty(info.spec);
    let mut frames = 0;
    decoder.follow_format_changes();
    loop {
        match decoder.next_block(&mut block).expect("a song decodes") {
            DecodeStatus::EndOfStream => return frames,
            DecodeStatus::Reformatted(_) => {}
            DecodeStatus::Decoded => frames += block.frames() as u64,
        }
    }
}

fn kept_as(folder: &Path, song: &Path, measure: &str, answer: &impl std::fmt::Debug) {
    let name = song
        .file_name()
        .expect("a song has a name")
        .to_string_lossy();
    fs::write(
        folder.join(format!("{name}.{measure}")),
        format!("{answer:#?}"),
    )
    .expect("an answer written");
}

fn songs() -> Vec<PathBuf> {
    match env::var_os(SONGS_HELD_IN) {
        Some(folder) => {
            let mut songs: Vec<PathBuf> = fs::read_dir(folder)
                .expect("the songs' folder reads")
                .map(|entry| entry.expect("an entry reads").path())
                .filter(|path| path.is_file())
                .collect();
            songs.sort();
            songs
        }
        None => {
            let held = env::temp_dir().join(format!("resonate-study-{}.wav", process::id()));
            fs::write(&held, wave_of(SYNTHETIC_SECONDS)).expect("the song written");
            vec![held]
        }
    }
}

fn measured(song: &Path, answers: Option<&Path>) {
    let sources = Sources::local();
    let location = MediaLocation::local(song);
    let watch = Arc::new(Watch::default());

    let (frames, decoding) = timed(|| decoded_alone(&sources, &location));
    let (studied, studying) =
        timed(|| study(&sources, &location, None, &*watch).expect("a song is studied"));
    let (analysed, analysing) =
        timed(|| analyse(&sources, &location, None, &*watch).expect("a song is analysed"));
    let (figures, measuring) =
        timed(|| band_figures(&sources, &location, None, &*watch).expect("a song's bands read"));

    let Examined {
        rate,
        channels,
        length,
        ..
    } = studied.examined;
    println!(
        "{} ({rate:?}, {channels} channels, {:.0} s, {frames} frames)",
        song.display(),
        length.as_secs_f64(),
    );
    decoding.line("decode alone", length);
    studying.line("study", length);
    analysing.line("analyse", length);
    measuring.line("band figures", length);

    if let Some(folder) = answers {
        kept_as(folder, song, "study", &studied);
        kept_as(folder, song, "analysis", &analysed);
        kept_as(folder, song, "figures", &figures);
    }
}

fn studied_together(songs: &[PathBuf], workers: usize) {
    let sources = Sources::local();
    let watch = Watch::default();
    let next = AtomicUsize::new(0);
    let heard_millis = AtomicU64::new(0);
    let ((), spent) = timed(|| {
        thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    while let Some(song) = songs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let location = MediaLocation::local(song);
                        let studied =
                            study(&sources, &location, None, &watch).expect("a song is studied");
                        heard_millis.fetch_add(
                            studied.examined.length.as_millis() as u64,
                            Ordering::Relaxed,
                        );
                    }
                });
            }
        });
    });
    println!("{} songs by {workers} workers at once", songs.len());
    spent.line(
        "all",
        Duration::from_millis(heard_millis.load(Ordering::Relaxed)),
    );
}

fn main() {
    let answers = env::var_os(ANSWERS_KEPT_IN).map(PathBuf::from);
    if let Some(folder) = &answers {
        fs::create_dir_all(folder).expect("the answers' folder made");
    }
    let held_here = env::var_os(SONGS_HELD_IN).is_none();
    if let Some(workers) = env::var(WORKERS_AT_ONCE)
        .ok()
        .and_then(|workers| workers.parse().ok())
    {
        studied_together(&songs(), workers);
        return;
    }
    for song in songs() {
        measured(&song, answers.as_deref());
        if held_here {
            fs::remove_file(&song).expect("the song removed");
        }
    }
}
