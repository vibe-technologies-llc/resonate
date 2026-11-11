use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    thread,
    time::{Duration, SystemTime},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use resonate_ui::AppIcon;

use crate::{Error, Result, config, error::IconOp};

const ICON_NAME: &str = "resonate";
const SCALABLE_APPS: &str = "scalable/apps";
const SETTLES_AFTER: Duration = Duration::from_millis(1_500);
const SERVICE_CACHE_BUILDER: &str = "kbuildsycoca6";
const GTK_ICON_CACHE: &str = "icon-theme.cache";
const GTK_ICON_CACHE_BUILDERS: [&str; 2] = ["gtk-update-icon-cache", "gtk4-update-icon-cache"];

pub(crate) struct Icons {
    wanted: Sender<AppIcon>,
}

impl Icons {
    pub(crate) fn start() -> Self {
        let (wanted, asked) = unbounded();

        match config::icon_theme_dir() {
            Ok(theme) => {
                let started = thread::Builder::new()
                    .name("resonate-icon".to_owned())
                    .spawn(move || follow(&asked, &theme));
                if let Err(error) = started {
                    tracing::warn!(%error, "the application's icon will not follow the accent");
                }
            }
            Err(error) => {
                tracing::debug!(%error, "the application's icon will not follow the accent");
            }
        }

        Self { wanted }
    }
}

impl resonate_ui::Launcher for Icons {
    fn show(&self, icon: AppIcon) {
        if self.wanted.send(icon).is_err() {
            tracing::debug!("nothing is placing the application's icon");
        }
    }
}

fn follow(asked: &Receiver<AppIcon>, theme: &Path) {
    while let Ok(mut icon) = asked.recv() {
        while let Ok(newer) = asked.recv_timeout(SETTLES_AFTER) {
            icon = newer;
        }
        shown(&icon, theme);
    }
}

fn shown(icon: &AppIcon, theme: &Path) {
    match placed(icon, theme) {
        Ok(Placing::Moved) => flushed(theme),
        Ok(Placing::Standing) => {}
        Ok(Placing::NotOurs) => {
            tracing::debug!(
                theme = %theme.display(),
                "an icon this build did not draw stands under its name, so it is left alone"
            );
        }
        Err(error) => tracing::warn!(%error, "the application's icon did not follow the accent"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placing {
    Moved,
    Standing,
    NotOurs,
}

fn icon_path(theme: &Path) -> PathBuf {
    theme.join(SCALABLE_APPS).join(format!("{ICON_NAME}.svg"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    Nothing,
    Ours,
    ThePackagedCopy,
    Somebody,
}

impl Standing {
    fn of(held: Option<&[u8]>) -> Self {
        match held {
            None => Self::Nothing,
            Some(held) if AppIcon::is_the_packaged_copy(held) => Self::ThePackagedCopy,
            Some(held) if AppIcon::drawn_here(held) => Self::Ours,
            Some(_) => Self::Somebody,
        }
    }
}

fn placed(icon: &AppIcon, theme: &Path) -> Result<Placing> {
    let path = icon_path(theme);
    let held = match fs::read(&path) {
        Ok(held) => Some(held),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(Error::Icon {
                op: IconOp::Read,
                path,
                source,
            });
        }
    };

    match Standing::of(held.as_deref()) {
        Standing::Somebody => return Ok(Placing::NotOurs),
        Standing::Nothing | Standing::Ours | Standing::ThePackagedCopy => {}
    }
    let wanted = icon.drawing();
    if held.as_deref() == Some(wanted.as_bytes()) {
        return Ok(Placing::Standing);
    }
    written(&path, wanted)?;
    Ok(Placing::Moved)
}

fn written(path: &Path, drawn: &str) -> Result<()> {
    let Some(folder) = path.parent() else {
        return Err(Error::Icon {
            op: IconOp::Place,
            path: path.to_path_buf(),
            source: io::ErrorKind::InvalidInput.into(),
        });
    };
    fs::create_dir_all(folder).map_err(|source| Error::CreateDir {
        path: folder.to_path_buf(),
        source,
    })?;

    let staged = folder.join(format!(".{ICON_NAME}.svg.{}", process::id()));
    let outcome = File::create(&staged)
        .and_then(|mut file| {
            file.write_all(drawn.as_bytes())?;
            file.sync_all()
        })
        .map_err(|source| Error::Icon {
            op: IconOp::Stage,
            path: staged.clone(),
            source,
        })
        .and_then(|()| {
            fs::rename(&staged, path).map_err(|source| Error::Icon {
                op: IconOp::Place,
                path: path.to_path_buf(),
                source,
            })
        });
    if outcome.is_err() {
        let _ = fs::remove_file(&staged);
    }
    outcome
}

fn flushed(theme: &Path) {
    if let Err(error) = File::open(theme).and_then(|folder| folder.set_modified(SystemTime::now()))
    {
        tracing::debug!(%error, theme = %theme.display(), "the icon theme's folder was not touched");
    }

    let rebuilt = Command::new(SERVICE_CACHE_BUILDER)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match rebuilt {
        Ok(status) if !status.success() => {
            tracing::debug!(%status, "the desktop's service cache was not rebuilt");
        }
        Ok(_) => {}
        Err(error) => tracing::debug!(%error, "the desktop's service cache was not rebuilt"),
    }

    if theme.join(GTK_ICON_CACHE).exists() {
        let rebuilt = GTK_ICON_CACHE_BUILDERS.iter().find_map(|builder| {
            Command::new(builder)
                .args(["--force", "--ignore-theme-index", "--quiet"])
                .arg(theme)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .ok()
        });
        match rebuilt {
            Some(status) if status.success() => {}
            Some(status) => tracing::debug!(%status, "the GTK icon cache was not rebuilt"),
            None => tracing::debug!("no GTK icon cache builder is installed"),
        }
    }

    if let Err(error) = resonate_mpris::tell_the_icons_changed() {
        tracing::debug!(%error, "the desktop was not told its icons changed");
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        sync::atomic::{AtomicU64, Ordering},
    };

    use resonate_core::{Accent, Appearance, Theme};

    use super::*;

    struct Scratch {
        theme: PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let theme = env::temp_dir().join(format!(
                "resonate-icons-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            Self { theme }
        }

        fn held(&self) -> Option<Vec<u8>> {
            fs::read(icon_path(&self.theme)).ok()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.theme);
        }
    }

    fn worn(theme: Theme, accent: Option<Accent>) -> AppIcon {
        AppIcon::of(Appearance {
            theme,
            accent,
            ..Appearance::DEFAULT
        })
    }

    #[test]
    fn the_icon_is_placed_under_the_name_the_desktop_entry_asks_for() {
        let entry = include_str!("../../../packaging/resonate.desktop");
        let named = entry
            .lines()
            .find_map(|line| line.strip_prefix("Icon="))
            .expect("the desktop entry names an icon");

        assert_eq!(named, ICON_NAME);
    }

    #[test]
    fn an_accent_is_placed_once_and_then_stands() {
        let scratch = Scratch::new();
        let icon = worn(Theme::Resonate, Some(Accent::Teal));

        assert_eq!(
            placed(&icon, &scratch.theme).expect("placed"),
            Placing::Moved
        );
        assert_eq!(scratch.held().as_deref(), Some(icon.drawing().as_bytes()));
        assert_eq!(
            placed(&icon, &scratch.theme).expect("placed"),
            Placing::Standing
        );
    }

    #[test]
    fn the_default_accent_is_placed_in_its_own_colour_rather_than_left_to_the_packaged_icon() {
        let scratch = Scratch::new();
        let teal = worn(Theme::Resonate, Some(Accent::Teal));
        let its_own = worn(Theme::Resonate, None);
        placed(&teal, &scratch.theme).expect("placed");

        assert_eq!(
            placed(&its_own, &scratch.theme).expect("placed"),
            Placing::Moved
        );
        assert_eq!(
            scratch.held().as_deref(),
            Some(its_own.drawing().as_bytes())
        );
        assert!(!AppIcon::is_the_packaged_copy(
            &scratch.held().expect("an icon in the accent")
        ));
    }

    #[test]
    fn a_new_accent_writes_over_the_icon_the_last_one_drew() {
        let scratch = Scratch::new();
        let teal = worn(Theme::Midnight, Some(Accent::Teal));
        let red = worn(Theme::Midnight, Some(Accent::Red));
        placed(&teal, &scratch.theme).expect("placed");

        assert_eq!(
            placed(&red, &scratch.theme).expect("placed"),
            Placing::Moved
        );
        assert_eq!(scratch.held().as_deref(), Some(red.drawing().as_bytes()));
    }

    #[test]
    fn a_copy_of_the_packaged_icon_or_an_earlier_builds_takes_the_accent() {
        let scratch = Scratch::new();
        let path = icon_path(&scratch.theme);
        fs::create_dir_all(path.parent().expect("a folder")).expect("a scratch folder");
        let icon = worn(Theme::Resonate, Some(Accent::Mauve));
        let earlier = icon.drawing().replacen(
            "<svg id=\"resonate-in-the-accent\" ",
            "<svg id=\"resonate-in-the-accent\" data-written-over=\"the-packaged-icon\" ",
            1,
        );

        for standing in [
            include_str!("../../../packaging/resonate.svg"),
            earlier.as_str(),
        ] {
            fs::write(&path, standing).expect("an icon standing");

            assert_eq!(
                placed(&icon, &scratch.theme).expect("placed"),
                Placing::Moved
            );
            assert_eq!(scratch.held().as_deref(), Some(icon.drawing().as_bytes()));
        }
    }

    #[test]
    fn an_icon_this_build_did_not_draw_is_left_as_it_was() {
        let scratch = Scratch::new();
        let path = icon_path(&scratch.theme);
        fs::create_dir_all(path.parent().expect("a folder")).expect("a scratch folder");
        fs::write(&path, b"<svg aria-label=\"mine\"/>").expect("a scratch icon");
        let icon = worn(Theme::Resonate, Some(Accent::Teal));

        assert_eq!(
            placed(&icon, &scratch.theme).expect("read"),
            Placing::NotOurs
        );
        assert_eq!(
            scratch.held().as_deref(),
            Some(b"<svg aria-label=\"mine\"/>".as_slice())
        );
    }
}
