use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use resonate_mpris::{Trashing, trash_through_the_portal};

const ON_A_BUS_OF_ITS_OWN: &str = "RESONATE_PORTAL_ON_A_BUS_OF_ITS_OWN";
const THIS_TEST: &str = "a_file_is_taken_to_the_trash_by_a_portal_hosted_on_a_bus_of_its_own";
const PORTALS: [&str; 3] = [
    "/usr/lib/xdg-desktop-portal",
    "/usr/libexec/xdg-desktop-portal",
    "/usr/lib64/xdg-desktop-portal",
];
const PATIENCE: Duration = Duration::from_secs(20);
const POLL_EVERY: Duration = Duration::from_millis(100);
const SONG: &str = "echoes.flac";

const A_BUS_WITH_NOTHING_TO_ACTIVATE: &str = r#"<!DOCTYPE busconfig PUBLIC
 "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let root = env::temp_dir().join(format!("resonate-portal-{}", std::process::id()));
        fs::create_dir_all(&root).expect("a folder for the portal's data");
        Self { root }
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Portal {
    process: Child,
}

impl Drop for Portal {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn the_portal_installed() -> Option<&'static str> {
    PORTALS
        .into_iter()
        .find(|binary| Path::new(binary).exists())
}

fn trashed_when_it_answers(file: &Path) -> Result<(), Trashing> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        match trash_through_the_portal(file) {
            Err(Trashing::Unreached { .. }) if Instant::now() < deadline => {
                thread::sleep(POLL_EVERY);
            }
            answered => return answered,
        }
    }
}

fn trashed_names(folder: &Path) -> Vec<String> {
    fs::read_dir(folder)
        .expect("a trash folder")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect()
}

fn on_a_bus_of_its_own(tree: &Tree) -> bool {
    if env::var_os(ON_A_BUS_OF_ITS_OWN).is_some() {
        return true;
    }

    let config = tree.root.join("session.conf");
    fs::write(&config, A_BUS_WITH_NOTHING_TO_ACTIVATE).expect("a writable bus configuration");
    let ran = Command::new("dbus-run-session")
        .arg(format!("--config-file={}", config.display()))
        .arg("--")
        .arg(env::current_exe().expect("the test binary names itself"))
        .args(["--exact", THIS_TEST, "--nocapture"])
        .env(ON_A_BUS_OF_ITS_OWN, "1")
        .status();
    match ran {
        Ok(status) => assert!(status.success(), "the run on a bus of its own failed"),
        Err(error) => eprintln!("skipped: no dbus-run-session to host a bus ({error})"),
    }
    false
}

#[test]
fn a_file_is_taken_to_the_trash_by_a_portal_hosted_on_a_bus_of_its_own() {
    let Some(binary) = the_portal_installed() else {
        eprintln!("skipped: no xdg-desktop-portal to host");
        return;
    };
    let tree = Tree::new();
    if !on_a_bus_of_its_own(&tree) {
        return;
    }

    let data = tree.data();
    fs::create_dir_all(&data).expect("a data home to trash into");
    let file = tree.root.join(SONG);
    fs::write(&file, b"not a song").expect("a file to trash");
    let _portal = Portal {
        process: Command::new(binary)
            .arg("--replace")
            .env("XDG_DATA_HOME", &data)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the portal starts"),
    };

    let trashed = trashed_when_it_answers(&file);

    assert!(trashed.is_ok(), "{trashed:?}");
    assert!(!file.exists(), "the file stayed where it was");
    assert_eq!(trashed_names(&data.join("Trash/files")).len(), 1);
    let record = trashed_names(&data.join("Trash/info")).remove(0);
    let recorded = fs::read_to_string(data.join("Trash/info").join(record)).expect("a record");
    assert!(
        recorded.contains(&format!("Path={}", file.display())),
        "{recorded}"
    );
}
