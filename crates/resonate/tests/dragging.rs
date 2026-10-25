use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

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
        size_of(&output.file("over.png")) > 0,
        "no screenshot of the overlay: {said}"
    );
    assert!(
        size_of(&output.file("after.png")) > 0,
        "no screenshot after the drop: {said}"
    );
    let _ = fs::remove_dir_all(&output.0);
}
