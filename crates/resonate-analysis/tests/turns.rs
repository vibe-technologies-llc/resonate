use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use resonate_analysis::{Watch, excerpt, print, study};
use resonate_codec::Sources;
use resonate_core::MediaLocation;

const OPENING_RATE: u32 = 44_100;
const TURNED_RATE: u32 = 48_000;
const LINK_SECONDS: u32 = 2;
const TONE_AMPLITUDE: f32 = 0.5;
const SINE_SOURCE_AMPLITUDE: f32 = 0.125;
const LENGTH_AGREES_WITHIN: Duration = Duration::from_millis(100);
const LEVEL_AGREES_WITHIN: f32 = 0.05;
const LOUDNESS_AGREES_WITHIN: f32 = 0.5;

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "resonate-turns-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a scratch tree");
        Self { root }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Link {
    rate: u32,
    channels: u8,
}

const MONO_AT_THE_OPENING_RATE: Link = Link {
    rate: OPENING_RATE,
    channels: 1,
};

const STEREO_AT_THE_TURNED_RATE: Link = Link {
    rate: TURNED_RATE,
    channels: 2,
};

fn chained(tree: &Tree, name: &str, links: &[Link]) -> Option<PathBuf> {
    let mut whole = Vec::new();
    for (index, link) in links.iter().enumerate() {
        let part = tree.root.join(format!("{index}-{name}"));
        let tone = format!(
            "sine=frequency=997:sample_rate={}:duration={LINK_SECONDS},volume={}",
            link.rate,
            TONE_AMPLITUDE / SINE_SOURCE_AMPLITUDE
        );
        let made = Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i", &tone])
            .args(["-af", twinned(link.channels)])
            .args(["-c:a", "libvorbis", "-q:a", "8", "-f", "ogg"])
            .arg(&part)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !made {
            eprintln!("skipped: no ffmpeg with libvorbis to write a link of {name}");
            return None;
        }
        whole.extend_from_slice(&fs::read(&part).ok()?);
    }
    let path = tree.root.join(name);
    fs::write(&path, whole).ok()?;
    Some(path)
}

fn twinned(channels: u8) -> &'static str {
    if channels == 2 {
        "pan=stereo|c0=c0|c1=c0"
    } else {
        "anull"
    }
}

fn whole_length() -> Duration {
    Duration::from_secs(u64::from(LINK_SECONDS) * 2)
}

fn near(heard: Duration, wanted: Duration) -> bool {
    heard.abs_diff(wanted) <= LENGTH_AGREES_WITHIN
}

fn studied(path: &Path) -> resonate_analysis::Study {
    study(
        &Sources::local(),
        &MediaLocation::local(path),
        None,
        &Watch::default(),
    )
    .expect("a stream that turns is studied")
}

#[test]
fn a_study_of_a_stream_that_turns_covers_every_link_in_the_shape_it_opened_in() {
    let tree = Tree::new();
    let Some(path) = chained(
        &tree,
        "turning.ogg",
        &[MONO_AT_THE_OPENING_RATE, STEREO_AT_THE_TURNED_RATE],
    ) else {
        return;
    };

    let studied = studied(&path);

    assert_eq!(studied.examined.rate.hz(), OPENING_RATE);
    assert_eq!(studied.examined.channels, 1);
    assert!(
        near(studied.examined.length, whole_length()),
        "{:?}",
        studied.examined.length
    );
    assert!(
        (studied.levels.rms - TONE_AMPLITUDE / 2.0_f32.sqrt()).abs() < LEVEL_AGREES_WITHIN,
        "{}",
        studied.levels.rms
    );
    assert_eq!(studied.levels.bits_in_use, None);
    assert_eq!(studied.levels.stereo, None);
}

#[test]
fn a_stream_that_turns_is_as_loud_as_the_same_tone_that_never_does() {
    let tree = Tree::new();
    let Some(turning) = chained(
        &tree,
        "turning.ogg",
        &[MONO_AT_THE_OPENING_RATE, STEREO_AT_THE_TURNED_RATE],
    ) else {
        return;
    };
    let Some(steady) = chained(
        &tree,
        "steady.ogg",
        &[MONO_AT_THE_OPENING_RATE, MONO_AT_THE_OPENING_RATE],
    ) else {
        return;
    };

    let loudness = |path: &Path| studied(path).loudness.integrated.expect("a loudness");

    let (turned, held) = (loudness(&turning), loudness(&steady));
    assert!(
        (turned - held).abs() < LOUDNESS_AGREES_WITHIN,
        "{turned} against {held}"
    );
}

#[test]
fn a_print_and_an_excerpt_of_a_stream_that_turns_run_on_past_the_turn() {
    let tree = Tree::new();
    let Some(path) = chained(
        &tree,
        "turning.ogg",
        &[MONO_AT_THE_OPENING_RATE, STEREO_AT_THE_TURNED_RATE],
    ) else {
        return;
    };
    let location = MediaLocation::local(&path);

    let printed = print(&Sources::local(), &location, None, &Watch::default());
    let taken = excerpt(
        &Sources::local(),
        &location,
        None,
        Duration::ZERO,
        whole_length(),
        &Watch::default(),
    )
    .expect("an excerpt of a stream that turns")
    .expect("sound in the excerpt");

    assert!(printed.is_ok());
    assert_eq!(taken.rate.hz(), OPENING_RATE);
    assert!(near(
        Duration::from_secs_f64(taken.mono.len() as f64 / f64::from(OPENING_RATE)),
        whole_length()
    ));
}
