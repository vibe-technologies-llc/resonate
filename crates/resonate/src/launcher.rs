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
    if held
        .as_deref()
        .is_some_and(|held| !AppIcon::drawn_here(held))
    {
        return Ok(Placing::NotOurs);
    }

    match (icon, held) {
        (AppIcon::Packaged, None) => Ok(Placing::Standing),
        (AppIcon::Packaged, Some(_)) => {
            fs::remove_file(&path).map_err(|source| Error::Icon {
                op: IconOp::Remove,
                path,
                source,
            })?;
            Ok(Placing::Moved)
        }
        (AppIcon::Recoloured(drawn), Some(held)) if held == drawn.as_bytes() => {
            Ok(Placing::Standing)
        }
        (AppIcon::Recoloured(drawn), _) => {
            written(&path, drawn)?;
            Ok(Placing::Moved)
        }
    }
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

    use resonate_core::{Accent, Appearance};

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

    fn recoloured() -> (AppIcon, String) {
        let icon = AppIcon::of(Appearance {
            accent: Some(Accent::Teal),
            ..Appearance::DEFAULT
        });
        let AppIcon::Recoloured(drawn) = icon.clone() else {
            panic!("a teal accent is not the packaged icon's");
        };
        (icon, drawn)
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
        let (icon, drawn) = recoloured();

        assert_eq!(
            placed(&icon, &scratch.theme).expect("placed"),
            Placing::Moved
        );
        assert_eq!(scratch.held().as_deref(), Some(drawn.as_bytes()));
        assert_eq!(
            placed(&icon, &scratch.theme).expect("placed"),
            Placing::Standing
        );
    }

    #[test]
    fn the_packaged_icon_takes_the_drawn_one_away_and_then_stands() {
        let scratch = Scratch::new();
        let (icon, _) = recoloured();
        placed(&icon, &scratch.theme).expect("placed");

        assert_eq!(
            placed(&AppIcon::Packaged, &scratch.theme).expect("taken away"),
            Placing::Moved
        );
        assert_eq!(scratch.held(), None);
        assert_eq!(
            placed(&AppIcon::Packaged, &scratch.theme).expect("nothing to do"),
            Placing::Standing
        );
    }

    #[test]
    fn a_new_accent_writes_over_the_icon_the_last_one_drew() {
        let scratch = Scratch::new();
        let (teal, _) = recoloured();
        let red = AppIcon::of(Appearance {
            accent: Some(Accent::Red),
            ..Appearance::DEFAULT
        });
        placed(&teal, &scratch.theme).expect("placed");

        assert_eq!(
            placed(&red, &scratch.theme).expect("placed"),
            Placing::Moved
        );
        let AppIcon::Recoloured(drawn) = red else {
            panic!("a red accent is not the packaged icon's");
        };
        assert_eq!(scratch.held().as_deref(), Some(drawn.as_bytes()));
    }

    #[test]
    fn an_icon_this_build_did_not_draw_is_left_as_it_was() {
        let scratch = Scratch::new();
        let path = icon_path(&scratch.theme);
        fs::create_dir_all(path.parent().expect("a folder")).expect("a scratch folder");
        fs::write(&path, b"<svg aria-label=\"mine\"/>").expect("a scratch icon");
        let (icon, _) = recoloured();

        assert_eq!(
            placed(&icon, &scratch.theme).expect("read"),
            Placing::NotOurs
        );
        assert_eq!(
            placed(&AppIcon::Packaged, &scratch.theme).expect("read"),
            Placing::NotOurs
        );
        assert_eq!(
            scratch.held().as_deref(),
            Some(b"<svg aria-label=\"mine\"/>".as_slice())
        );
    }
}
