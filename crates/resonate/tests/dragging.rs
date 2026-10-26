use std::{
    env,
    fs::{self, File},
    io::BufReader,
    ops::Range,
    path::{Path, PathBuf},
    process::Command,
};

use png::{BitDepth, ColorType, Decoder, Transformations};

const OPTED_IN: &str = "RESONATE_DRAG_TESTS";
const TOOLS: [&str; 5] = [
    "kwin_wayland",
    "spectacle",
    "busctl",
    "dbus-run-session",
    "timeout",
];
const GIVEN_UP_AFTER_SECONDS: &str = "300";
const KILLED_AFTER_SECONDS: &str = "--kill-after=30";
const GTK_IN_PYTHON: &str = "import gi; gi.require_version('Gtk', '4.0')";

const BEYOND_THE_SOURCE_WINDOW: usize = 300;
const DRAWN_AT_LEAST: u8 = 100;
const SCRIM_LEAVES_AT_MOST: u8 = 80;
const WIDEST_BEFORE_THE_DRAG_AT_LEAST: usize = 900;
const CARD_WIDEST: usize = 640;
const CARD_TALLEST: usize = 400;
const DROP_POINT: (usize, usize) = (800, 450);
const CARD_CENTRE_WITHIN: usize = 200;
const COLOURED_BY_AT_LEAST: u8 = 48;
const COLOURED_PIXELS_FOR_A_BORDER: usize = 1500;
const COLOURED_TIMES_THE_QUIET_CARD: usize = 3;

struct Drawn {
    columns: Range<usize>,
    rows: Range<usize>,
}

impl Drawn {
    fn width(&self) -> usize {
        self.columns.len()
    }

    fn height(&self) -> usize {
        self.rows.len()
    }

    fn centre(&self) -> (usize, usize) {
        (
            (self.columns.start + self.columns.end) / 2,
            (self.rows.start + self.rows.end) / 2,
        )
    }
}

struct Shot {
    width: usize,
    channels: usize,
    bytes: Vec<u8>,
}

impl Shot {
    fn read(path: &Path) -> Self {
        let mut decoder = Decoder::new(BufReader::new(File::open(path).expect("the screenshot")));
        decoder.set_transformations(Transformations::EXPAND);
        let mut reader = decoder.read_info().expect("a PNG header");
        let mut bytes = vec![0; reader.output_buffer_size().expect("a bounded PNG")];
        let info = reader.next_frame(&mut bytes).expect("a PNG frame");

        assert_eq!(info.bit_depth, BitDepth::Eight, "{}", path.display());
        let channels = match info.color_type {
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
            other => panic!("{} is {other:?}, not colour", path.display()),
        };
        bytes.truncate(info.buffer_size());

        Self {
            width: info.width as usize,
            channels,
            bytes,
        }
    }

    fn height(&self) -> usize {
        self.bytes.len() / self.channels / self.width
    }

    fn pixel(&self, column: usize, row: usize) -> [u8; 3] {
        let at = (row * self.width + column) * self.channels;
        [self.bytes[at], self.bytes[at + 1], self.bytes[at + 2]]
    }

    fn pixels(
        &self,
        columns: Range<usize>,
        rows: Range<usize>,
    ) -> impl Iterator<Item = [u8; 3]> + '_ {
        rows.flat_map(move |row| columns.clone().map(move |column| self.pixel(column, row)))
    }

    fn drawn(&self, floor: u8) -> Option<Drawn> {
        let lit = (0..self.height())
            .flat_map(|row| {
                (BEYOND_THE_SOURCE_WINDOW..self.width)
                    .filter(move |&column| self.pixel(column, row).iter().max() >= Some(&floor))
                    .map(move |column| (column, row))
            })
            .collect::<Vec<_>>();
        let columns = lit.iter().map(|&(column, _)| column);
        let rows = lit.iter().map(|&(_, row)| row);

        Some(Drawn {
            columns: columns.clone().min()?..columns.max()? + 1,
            rows: rows.clone().min()?..rows.max()? + 1,
        })
    }

    fn coloured(&self, within: &Drawn) -> usize {
        self.pixels(within.columns.clone(), within.rows.clone())
            .filter(|pixel| {
                pixel.iter().max().unwrap() - pixel.iter().min().unwrap() >= COLOURED_BY_AT_LEAST
            })
            .count()
    }
}

struct Output(PathBuf);

impl Output {
    fn new() -> Self {
        let path = env::temp_dir().join(format!("resonate-drag-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a folder for the drag's logs and screenshots");
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

fn on_the_path(tool: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(tool).is_file()))
}

fn python_has_gtk() -> bool {
    Command::new("python3")
        .args(["-I", "-c", GTK_IN_PYTHON])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.claude/skills/drive-a-drag/drag.sh")
}

fn size_of(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |metadata| metadata.len())
}

#[test]
fn a_file_dragged_from_another_client_onto_a_hosted_kwin_is_copied_into_the_music_folder() {
    if env::var_os(OPTED_IN).is_none() {
        eprintln!("skipped: {OPTED_IN} is not set");
        return;
    }
    if let Some(missing) = TOOLS.into_iter().find(|tool| !on_the_path(tool)) {
        eprintln!("skipped: no {missing} to host a compositor with");
        return;
    }
    if !python_has_gtk() {
        eprintln!("skipped: no GTK 4 in python3 to offer the file with");
        return;
    }
    let output = Output::new();

    let ran = Command::new("timeout")
        .args([KILLED_AFTER_SECONDS, GIVEN_UP_AFTER_SECONDS])
        .arg(script())
        .arg(&output.0)
        .output()
        .expect("the drag script runs");

    let said = format!(
        "{}{} (logs in {})",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr),
        output.0.display()
    );
    assert!(ran.status.success(), "{said}");
    assert!(
        fs::read_to_string(output.file("source.log"))
            .expect("the source's log")
            .contains("source: drag-end False"),
        "the target did not accept the drag: {said}"
    );
    assert!(
        output.file("music/one.flac").is_file(),
        "the dropped file was not copied: {said}"
    );
    assert!(
        size_of(&output.file("before.png")) > 0,
        "no screenshot before the drag: {said}"
    );
    assert!(
        size_of(&output.file("over.png")) > 0,
        "no screenshot of the overlay: {said}"
    );
    assert!(
        size_of(&output.file("after.png")) > 0,
        "no screenshot after the drop: {said}"
    );

    let before = Shot::read(&output.file("before.png"));
    let over = Shot::read(&output.file("over.png"));

    let spread = before
        .drawn(DRAWN_AT_LEAST)
        .expect("the window drew something before the drag");
    assert!(
        spread.width() >= WIDEST_BEFORE_THE_DRAG_AT_LEAST,
        "the window was not up before the drag: {said}"
    );

    let card = over
        .drawn(SCRIM_LEAVES_AT_MOST)
        .expect("something drawn over the scrim");
    assert!(
        card.width() <= CARD_WIDEST && card.height() <= CARD_TALLEST,
        "the scrim did not cover the window mid-drag, {}x{} still drawn: {said}",
        card.width(),
        card.height()
    );
    let (column, row) = card.centre();
    assert!(
        column.abs_diff(DROP_POINT.0) <= CARD_CENTRE_WITHIN
            && row.abs_diff(DROP_POINT.1) <= CARD_CENTRE_WITHIN,
        "the card is not where the drag is, centred at {column},{row}: {said}"
    );

    let border = over.coloured(&card);
    let quiet = before.coloured(&card);
    assert!(
        border >= COLOURED_PIXELS_FOR_A_BORDER && border >= quiet * COLOURED_TIMES_THE_QUIET_CARD,
        "no tinted border drawn, {border} coloured pixels against {quiet} before: {said}"
    );
    let _ = fs::remove_dir_all(&output.0);
}
