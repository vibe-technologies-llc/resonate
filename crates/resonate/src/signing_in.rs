use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_signin::Outcome;

use crate::{Error, Result, config, error::SignOutOp};

#[cfg(windows)]
pub const PROGRAM: &str = "resonate-signin.exe";

#[cfg(not(windows))]
pub const PROGRAM: &str = "resonate-signin";

#[cfg(test)]
pub static INTERRUPTING_THE_TOOLS: parking_lot::RwLock<()> = parking_lot::RwLock::new(());

const JAR: &str = "youtube-music-cookies.txt";

const PROFILE: &str = "youtube-music-sign-in";

const LOOKED_AT_EVERY: Duration = Duration::from_millis(100);

#[cfg(unix)]
const OWNER_ONLY: u32 = 0o700;

static SIGNING_IN: Mutex<Vec<Child>> = Mutex::new(Vec::new());

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub jar: PathBuf,
    pub profile: PathBuf,
}

impl Kept {
    pub fn in_the_data_folder() -> Result<Self> {
        config::data_dir().map(|folder| Self::under(&folder))
    }

    pub fn under(folder: &Path) -> Self {
        Self {
            jar: folder.join(JAR),
            profile: folder.join(PROFILE),
        }
    }

    pub fn jar_named(&self) -> Result<String> {
        self.jar
            .to_str()
            .map(str::to_owned)
            .ok_or(Error::YoutubeMusicJarNotText)
    }
}

pub fn the_program() -> Option<PathBuf> {
    let resonate = env::current_exe().ok();
    found(
        resonate.as_deref().and_then(Path::parent),
        env::var_os("PATH"),
    )
}

fn found(beside: Option<&Path>, path: Option<OsString>) -> Option<PathBuf> {
    beside
        .map(|folder| folder.join(PROGRAM))
        .into_iter()
        .chain(
            path.iter()
                .flat_map(env::split_paths)
                .map(|folder| folder.join(PROGRAM)),
        )
        .find(|program| program.is_file())
}

pub fn names_the_jar(setting: &str, kept: &Kept) -> bool {
    let setting = setting.trim();
    !setting.is_empty() && Path::new(setting) == kept.jar
}

pub fn sign_in(program: &Path, kept: &Kept) -> Result<()> {
    owner_only(&kept.profile).map_err(|source| Error::YoutubeMusicSignInNotStarted { source })?;
    let child = Command::new(program)
        .arg("--jar")
        .arg(&kept.jar)
        .arg("--profile")
        .arg(&kept.profile)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|source| Error::YoutubeMusicSignInNotStarted { source })?;
    let id = child.id();
    SIGNING_IN.lock().push(child);
    match Outcome::of(waited(id)?) {
        Outcome::SignedIn => {
            resonate_ytmusic::interrupt_the_tools();
            Ok(())
        }
        Outcome::Closed => Err(Error::YoutubeMusicSignInClosed),
        Outcome::Failed { code } => Err(Error::YoutubeMusicSignInFailed { code }),
    }
}

fn waited(id: u32) -> Result<std::process::ExitStatus> {
    loop {
        let looked = {
            let mut held = SIGNING_IN.lock();
            let Some(at) = held.iter().position(|child| child.id() == id) else {
                return Err(Error::YoutubeMusicSignInFailed { code: None });
            };
            let looked = held[at].try_wait();
            if !matches!(looked, Ok(None)) {
                let _ = held.swap_remove(at).wait();
            }
            looked
        };
        match looked {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(LOOKED_AT_EVERY),
            Err(source) => return Err(Error::YoutubeMusicSignInNotStarted { source }),
        }
    }
}

fn owner_only(folder: &Path) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    narrowed(folder)
}

#[cfg(unix)]
fn narrowed(folder: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(folder, fs::Permissions::from_mode(OWNER_ONLY))
}

#[cfg(not(unix))]
const fn narrowed(_folder: &Path) -> io::Result<()> {
    Ok(())
}

pub fn stop_signing_in() {
    for mut child in SIGNING_IN.lock().drain(..) {
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub fn sign_out(kept: &Kept) -> Result<()> {
    resonate_ytmusic::interrupt_the_tools();
    gone(fs::remove_file(&kept.jar)).map_err(|source| Error::YoutubeMusicSignOut {
        op: SignOutOp::Jar,
        source,
    })?;
    gone(fs::remove_dir_all(&kept.profile)).map_err(|source| Error::YoutubeMusicSignOut {
        op: SignOutOp::Profile,
        source,
    })
}

fn gone(removed: io::Result<()>) -> io::Result<()> {
    match removed {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

#[cfg(feature = "ui")]
pub struct FromTheWindow {
    finds: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    kept: Option<Kept>,
}

#[cfg(feature = "ui")]
impl FromTheWindow {
    pub fn here() -> Self {
        Self {
            finds: Box::new(the_program),
            kept: Kept::in_the_data_folder().ok(),
        }
    }
}

#[cfg(feature = "ui")]
impl resonate_ui::SignsInToYoutubeMusic for FromTheWindow {
    fn is_offered(&self) -> bool {
        self.kept.is_some() && (self.finds)().is_some()
    }

    fn sign_in(&self) -> resonate_ui::YoutubeMusicSignIn {
        use resonate_ui::YoutubeMusicSignIn;

        let (Some(program), Some(kept)) = ((self.finds)(), self.kept.as_ref()) else {
            return YoutubeMusicSignIn::ProgramMissing;
        };
        match sign_in(&program, kept).and_then(|()| kept.jar_named()) {
            Ok(cookies) => YoutubeMusicSignIn::SignedIn { cookies },
            Err(Error::YoutubeMusicSignInClosed) => YoutubeMusicSignIn::Closed,
            Err(error) => {
                tracing::warn!(%error, "the YouTube Music sign-in did not end signed in");
                YoutubeMusicSignIn::Failed
            }
        }
    }

    fn sign_out(&self) -> bool {
        let Some(kept) = self.kept.as_ref() else {
            return false;
        };
        sign_out(kept)
            .inspect_err(
                |error| tracing::warn!(%error, "the YouTube Music sign-in was not taken away"),
            )
            .is_ok()
    }

    fn names_the_window_jar(&self, cookies: &str) -> bool {
        self.kept
            .as_ref()
            .is_some_and(|kept| names_the_jar(cookies, kept))
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("resonate-signing-in-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    #[cfg(unix)]
    fn a_program(folder: &Path, says: &str) -> PathBuf {
        use std::{
            os::unix::fs::PermissionsExt as _,
            process::Command,
            sync::atomic::{AtomicUsize, Ordering},
        };

        static MADE: AtomicUsize = AtomicUsize::new(0);
        let program = folder.join(format!("program-{}", MADE.fetch_add(1, Ordering::Relaxed)));
        fs::write(
            &program,
            format!("#!/bin/sh\n[ \"$1\" = --runs ] && exit 0\n{says}\n"),
        )
        .expect("a program");
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).expect("runnable");
        loop {
            match Command::new(&program).arg("--runs").status() {
                Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy => {
                    std::thread::yield_now();
                }
                ran => {
                    ran.expect("the program runs");
                    return program;
                }
            }
        }
    }

    #[test]
    fn the_program_beside_resonate_is_found_before_one_on_the_path() {
        let beside = scratch("beside");
        let on_the_path = scratch("on-the-path");
        let elsewhere = scratch("elsewhere");
        fs::write(on_the_path.join(PROGRAM), "").expect("one on the path");
        let path = env::join_paths([elsewhere.clone(), on_the_path.clone()]).expect("a path");

        assert_eq!(
            found(Some(&beside), Some(path.clone())),
            Some(on_the_path.join(PROGRAM))
        );
        fs::write(beside.join(PROGRAM), "").expect("one beside");
        assert_eq!(found(Some(&beside), Some(path)), Some(beside.join(PROGRAM)));
        assert_eq!(found(Some(&elsewhere), None), None);
    }

    #[cfg(unix)]
    #[test]
    fn each_ending_of_the_sign_in_program_is_its_own_answer() {
        let _interrupting = INTERRUPTING_THE_TOOLS.write();

        let folder = scratch("endings");
        let kept = Kept::under(&folder);

        let signing = a_program(&folder, "printf 'jar' > \"$2\"; exit 0");
        sign_in(&signing, &kept).expect("signed in");
        assert_eq!(fs::read_to_string(&kept.jar).expect("the jar"), "jar");

        let closing = a_program(&folder, "exit 3");
        assert!(matches!(
            sign_in(&closing, &kept),
            Err(Error::YoutubeMusicSignInClosed)
        ));

        let failing = a_program(&folder, "exit 1");
        assert!(matches!(
            sign_in(&failing, &kept),
            Err(Error::YoutubeMusicSignInFailed { code: Some(1) })
        ));

        assert!(matches!(
            sign_in(&folder.join("gone"), &kept),
            Err(Error::YoutubeMusicSignInNotStarted { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn the_windows_profile_is_made_owner_only_before_the_window_opens() {
        use std::os::unix::fs::PermissionsExt as _;

        let _interrupting = INTERRUPTING_THE_TOOLS.write();

        let folder = scratch("profile");
        let kept = Kept::under(&folder.join("data"));
        fs::create_dir_all(&kept.profile).expect("a profile left open");
        fs::set_permissions(&kept.profile, fs::Permissions::from_mode(0o755)).expect("opened");
        let looking = a_program(
            &folder,
            "{ stat -c %a \"$4\" 2>/dev/null || stat -f %Lp \"$4\"; } > \"$(dirname \
             \"$4\")/mode-seen\"; exit 0",
        );

        sign_in(&looking, &kept).expect("signed in");

        let seen = fs::read_to_string(folder.join("data").join("mode-seen")).expect("a mode seen");
        assert_eq!(seen.trim(), "700");
    }

    #[test]
    fn signing_out_takes_away_the_jar_and_the_profile_and_a_second_time_is_no_failure() {
        let _interrupting = INTERRUPTING_THE_TOOLS.write();

        let folder = scratch("out");
        let kept = Kept::under(&folder);
        fs::write(&kept.jar, "jar").expect("a jar");
        fs::create_dir_all(kept.profile.join("WebKit")).expect("a profile");

        sign_out(&kept).expect("signed out");
        sign_out(&kept).expect("signed out again");

        assert!(!kept.jar.exists());
        assert!(!kept.profile.exists());
    }

    #[cfg(all(unix, feature = "ui"))]
    #[test]
    fn the_window_is_told_what_the_sign_in_program_did() {
        use resonate_ui::{SignsInToYoutubeMusic as _, YoutubeMusicSignIn};

        let _interrupting = INTERRUPTING_THE_TOOLS.write();

        let folder = scratch("window");
        let kept = Kept::under(&folder.join("data"));
        let signing = a_program(&folder, "mkdir -p \"$(dirname \"$2\")\"; : > \"$2\"");
        let closing = a_program(&folder, "exit 3");
        let failing = a_program(&folder, "exit 7");
        let window = |program: Option<PathBuf>| FromTheWindow {
            finds: Box::new(move || program.clone()),
            kept: Some(kept.clone()),
        };

        assert!(!window(None).is_offered());
        assert_eq!(window(None).sign_in(), YoutubeMusicSignIn::ProgramMissing);
        assert_eq!(window(Some(closing)).sign_in(), YoutubeMusicSignIn::Closed);
        assert_eq!(window(Some(failing)).sign_in(), YoutubeMusicSignIn::Failed);
        let signed = window(Some(signing));
        assert!(signed.is_offered());
        assert_eq!(
            signed.sign_in(),
            YoutubeMusicSignIn::SignedIn {
                cookies: kept.jar.display().to_string()
            }
        );
        assert!(signed.names_the_window_jar(&kept.jar.display().to_string()));
        assert!(signed.sign_out());
        assert!(!kept.jar.exists());
    }

    #[test]
    fn only_the_jar_the_window_kept_is_named_as_its_own() {
        let kept = Kept::under(Path::new("/data/resonate"));
        let jar = kept.jar.display().to_string();

        assert!(names_the_jar(&jar, &kept));
        assert!(names_the_jar(&format!("  {jar} "), &kept));
        assert!(!names_the_jar("firefox", &kept));
        assert!(!names_the_jar("/home/a/cookies.txt", &kept));
        assert!(!names_the_jar("", &kept));
    }
}
