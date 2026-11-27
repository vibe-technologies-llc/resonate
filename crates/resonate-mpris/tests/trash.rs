#![cfg(all(unix, not(target_os = "macos")))]

use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use resonate_mpris::{Trashing, trash_through_the_portal};

const REQUIRED_BY: &str = "RESONATE_MPRIS_PORTAL_TESTS";
const SANDBOX_REQUIRED_BY: &str = "RESONATE_MPRIS_SANDBOX_TESTS";
const ON_A_BUS_OF_ITS_OWN: &str = "RESONATE_PORTAL_ON_A_BUS_OF_ITS_OWN";
const TRASH_THIS_FILE: &str = "RESONATE_TRASH_THIS_FILE_FROM_A_SANDBOX";
const APP_ID: &str = "org.resonate.Resonate";
const INSTANCE_ID: &str = "4242424242";
const FLATPAK_INFO_PATH: &str = "/.flatpak-info";
const PORTALS: [&str; 3] = [
    "/usr/lib/xdg-desktop-portal",
    "/usr/libexec/xdg-desktop-portal",
    "/usr/lib64/xdg-desktop-portal",
];
const PATIENCE: Duration = Duration::from_secs(20);
const POLL_EVERY: Duration = Duration::from_millis(100);
const SONG: &str = "echoes.flac";
const TRASH_MODE_THE_PORTAL_REFUSES: u32 = 0o755;

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

fn hosted(binary: &str, data: &Path, runtime: Option<&Path>) -> Portal {
    let mut portal = Command::new(binary);

    if let Some(runtime) = runtime {
        portal.env("XDG_RUNTIME_DIR", runtime);
    }
    Portal {
        process: portal
            .arg("--replace")
            .env("XDG_DATA_HOME", data)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the portal starts"),
    }
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

    let test = crate::this_test();

    let config = tree.root.join("session.conf");

    fs::write(&config, A_BUS_WITH_NOTHING_TO_ACTIVATE).expect("a writable bus configuration");

    let ran = Command::new("dbus-run-session")
        .arg(format!("--config-file={}", config.display()))
        .arg("--")
        .arg(env::current_exe().expect("the test binary names itself"))
        .args(["--exact", test.as_str(), "--nocapture"])
        .env(ON_A_BUS_OF_ITS_OWN, "1")
        .output();

    match ran {
        Ok(ran) => {
            let said = String::from_utf8_lossy(&ran.stdout);

            print!("{said}");

            assert!(ran.status.success(), "the run on a bus of its own failed");
            assert!(
                crate::ran_one_test(&said),
                "{test} was not found to run on a bus of its own"
            );
        }
        Err(error) => passed_over!("no dbus-run-session to host a bus ({error})"),
    }
    false
}

#[test]
fn a_file_is_taken_to_the_trash_by_a_portal_hosted_on_a_bus_of_its_own() {
    let Some(binary) = the_portal_installed() else {
        passed_over!("no xdg-desktop-portal to host");
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

    let _portal = hosted(binary, &data, None);

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

fn make_a_trash_the_portal_refuses(trash: &Path) {
    for folder in [trash.join("files"), trash.join("info")] {
        fs::create_dir_all(&folder).expect("a trash folder");
        fs::set_permissions(
            &folder,
            fs::Permissions::from_mode(TRASH_MODE_THE_PORTAL_REFUSES),
        )
        .expect("a trash folder the portal will not use");
    }
}

#[test]
fn a_trash_the_portal_refuses_is_a_refusal_naming_the_file_and_leaves_it_where_it_was() {
    let Some(binary) = the_portal_installed() else {
        passed_over!("no xdg-desktop-portal to host");
        return;
    };
    let tree = Tree::new();

    if !on_a_bus_of_its_own(&tree) {
        return;
    }

    let data = tree.data();
    let trash = data.join("Trash");

    make_a_trash_the_portal_refuses(&trash);

    let file = tree.root.join(SONG);

    fs::write(&file, b"not a song").expect("a file to trash");

    let _portal = hosted(binary, &data, None);

    let trashed = trashed_when_it_answers(&file);

    assert!(
        matches!(&trashed, Err(Trashing::Refused { path }) if *path == file),
        "{trashed:?}"
    );
    assert!(file.exists(), "the file left though the portal refused it");
    assert!(trashed_names(&trash.join("files")).is_empty());
}

fn flatpak_info(tree: &Tree) -> String {
    format!(
        "[Application]\nname={APP_ID}\nruntime=runtime/org.freedesktop.Platform/x86_64/24.08\n\n[Instance]\ninstance-id={INSTANCE_ID}\ninstance-path={}\n",
        tree.root.join("instance").display()
    )
}

fn bus_socket_of(address: &str) -> Option<PathBuf> {
    address
        .strip_prefix("unix:")?
        .split(',')
        .find_map(|part| part.strip_prefix("path="))
        .map(PathBuf::from)
}

fn sandbox(info: &Path) -> Command {
    let mut bwrap = Command::new("bwrap");

    bwrap
        .args([
            "--unshare-pid",
            "--die-with-parent",
            "--tmpfs",
            "/",
            "--ro-bind",
            "/usr",
            "/usr",
            "--symlink",
            "usr/lib",
            "/lib",
            "--symlink",
            "usr/lib64",
            "/lib64",
            "--symlink",
            "usr/bin",
            "/bin",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--ro-bind",
        ])
        .arg(info)
        .arg(FLATPAK_INFO_PATH);
    bwrap
}

fn bound(mut command: Command, writable: bool, path: &Path) -> Command {
    command
        .arg(if writable { "--bind" } else { "--ro-bind" })
        .arg(path)
        .arg(path);
    command
}

fn can_sandbox(info: &Path) -> bool {
    sandbox(info)
        .arg("/usr/bin/true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
        && Command::new("xdg-dbus-proxy")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

fn passed_over_without_a_sandbox() {
    let why = "no bwrap and xdg-dbus-proxy to build a sandbox with";

    assert!(
        env::var(SANDBOX_REQUIRED_BY).as_deref() != Ok("required"),
        "{why}, and {SANDBOX_REQUIRED_BY} asks for it"
    );

    eprintln!("skipped: {why}");
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + PATIENCE;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );

        thread::sleep(POLL_EVERY);
    }
}

#[test]
fn a_file_is_taken_to_the_trash_from_a_sandbox_the_portal_knows_by_its_app_id() {
    let Some(binary) = the_portal_installed() else {
        passed_over!("no xdg-desktop-portal to host");
        return;
    };
    let tree = Tree::new();
    let info = tree.root.join("flatpak-info");

    fs::write(&info, flatpak_info(&tree)).expect("a flatpak-info");
    if !can_sandbox(&info) {
        passed_over_without_a_sandbox();
        return;
    }
    if !on_a_bus_of_its_own(&tree) {
        return;
    }

    let data = tree.data();

    fs::create_dir_all(&data).expect("a data home to trash into");

    let music = tree.root.join("music");

    fs::create_dir_all(&music).expect("a music folder");

    let file = music.join(SONG);

    fs::write(&file, b"not a song").expect("a file to trash");

    let runtime = tree.root.join("run");
    let instance = runtime.join(".flatpak").join(INSTANCE_ID);

    fs::create_dir_all(&instance).expect("the instance's folder in the runtime directory");

    let _portal = hosted(binary, &data, Some(&runtime));

    let address = env::var("DBUS_SESSION_BUS_ADDRESS").expect("the bus this run is on");
    let bus = bus_socket_of(&address).expect("a bus on a socket file");
    let proxied = tree.root.join("proxied");

    fs::create_dir_all(&proxied).expect("a folder for the proxy's socket");

    let socket = proxied.join("bus");
    let mut proxy = bound(bound(sandbox(&info), true, &bus), true, &proxied);

    let _proxy = Portal {
        process: proxy
            .args(["xdg-dbus-proxy", &address])
            .arg(&socket)
            .args(["--filter", "--talk=org.freedesktop.portal.*"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("the proxy starts"),
    };

    wait_for(&socket);

    let executable = env::current_exe().expect("the test binary names itself");
    let folder = executable.parent().expect("a folder holding the binary");
    let child_test = crate::named(&the_trash_asked_for_from_inside_a_sandbox);
    let mut child = bound(
        bound(bound(sandbox(&info), true, &proxied), false, folder),
        true,
        &music,
    );

    child.args(["--info-fd", "3"]).arg(&executable).args([
        "--exact",
        child_test.as_str(),
        "--nocapture",
    ]);

    let ran = Command::new("sh")
        .args(["-c", r#"exec "$@" 3>"$BWRAPINFO""#, "sh", "bwrap"])
        .args(child.get_args())
        .env("BWRAPINFO", instance.join("bwrapinfo.json"))
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", socket.display()),
        )
        .env(TRASH_THIS_FILE, &file)
        .output()
        .expect("the sandbox runs");
    let said = String::from_utf8_lossy(&ran.stdout);

    print!("{said}");

    assert!(ran.status.success(), "the sandboxed trash failed");
    assert!(
        crate::ran_one_test(&said),
        "{child_test} was not found to run in the sandbox"
    );
    assert!(!file.exists(), "the file stayed where it was");

    let record = trashed_names(&data.join("Trash/info")).remove(0);
    let recorded = fs::read_to_string(data.join("Trash/info").join(record)).expect("a record");

    assert!(
        recorded.contains(&format!("Path={}", file.display())),
        "{recorded}"
    );
}

#[test]
fn the_trash_asked_for_from_inside_a_sandbox() {
    let Some(file) = env::var_os(TRASH_THIS_FILE) else {
        return;
    };

    let trashed = trashed_when_it_answers(Path::new(&file));

    assert!(trashed.is_ok(), "{trashed:?}");
}
