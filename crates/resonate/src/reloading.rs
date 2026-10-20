use std::{fs, path::Path};

use resonate_mpris::{Reloadable, Running};

use crate::Result;

pub(crate) fn delivering<Written>(
    settings: &Path,
    group: Option<Reloadable>,
    write: impl FnOnce() -> Result<Written>,
) -> Result<Written> {
    let before = fs::read(settings).ok();
    let written = write()?;

    if fs::read(settings).ok() != before {
        deliver(settings, group);
    }
    Ok(written)
}

fn deliver(settings: &Path, group: Option<Reloadable>) {
    let players = match Running::listed() {
        Ok(players) => players,
        Err(error) => {
            tracing::debug!(%error, "the session bus was not asked which players are running");
            return;
        }
    };

    let mut taking = Vec::new();
    let mut keeping = Vec::new();
    for player in &players {
        let name = player.name().to_string();
        let reached = group.map(|group| player.reload(group, settings));
        match reached {
            Some(Ok(())) => taking.push(name),
            Some(Err(error)) => {
                tracing::debug!(%error, player = %name, "a running player did not take the change");
                keeping.push(name);
            }
            None => keeping.push(name),
        }
    }

    if let Some(told) = taken_by_running(&taking) {
        said!("{told}");
    }
    if let Some(told) = kept_by_running(&keeping) {
        said!("{told}");
    }
}

fn taken_by_running(players: &[String]) -> Option<String> {
    let (first, rest) = players.split_first()?;

    Some(if rest.is_empty() {
        format!("{first} is running and takes this up")
    } else {
        format!("{} are running and take this up", players.join(", "))
    })
}

fn kept_by_running(players: &[String]) -> Option<String> {
    let (first, rest) = players.split_first()?;

    Some(if rest.is_empty() {
        format!(
            "{first} is running and keeps the settings it started with; it hears this only if it \
             reads this settings file, once it is started again"
        )
    } else {
        format!(
            "{} are running and keep the settings they started with; they hear this only if \
             they read this settings file, once they are started again",
            players.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = "org.mpris.MediaPlayer2.resonate";
    const ANOTHER: &str = "org.mpris.MediaPlayer2.resonate.instance7";

    #[test]
    fn a_running_player_is_told_to_keep_the_settings_it_started_with_and_none_is_no_remark() {
        let one = kept_by_running(&[PLAYER.to_owned()]);
        let two = kept_by_running(&[PLAYER.to_owned(), ANOTHER.to_owned()]);

        assert_eq!(
            one.as_deref(),
            Some(
                "org.mpris.MediaPlayer2.resonate is running and keeps the settings it started \
                 with; it hears this only if it reads this settings file, once it is started \
                 again"
            )
        );
        assert!(two.is_some_and(|told| told.contains("are running and keep the settings they")));
        assert_eq!(kept_by_running(&[]), None);
    }

    #[test]
    fn a_running_player_that_took_the_change_is_said_to_take_it_up() {
        let one = taken_by_running(&[PLAYER.to_owned()]);
        let two = taken_by_running(&[PLAYER.to_owned(), ANOTHER.to_owned()]);

        assert_eq!(
            one.as_deref(),
            Some("org.mpris.MediaPlayer2.resonate is running and takes this up")
        );
        assert!(two.is_some_and(|told| told.contains("are running and take this up")));
        assert_eq!(taken_by_running(&[]), None);
    }

    #[test]
    fn a_write_is_returned_whether_or_not_it_moved_the_settings_file() {
        let folder = crate::fixtures::scratch("reloading");
        let settings = folder.join("config.toml");
        fs::write(&settings, "online = true\n").expect("a settings file");

        let untouched = delivering(&settings, None, || Ok(1));
        let rewritten = delivering(&settings, None, || {
            fs::write(&settings, "online = false\n").expect("a rewrite");
            Ok(2)
        });
        let refused = delivering(&settings, None, || -> Result<()> {
            Err(crate::Error::SyncOff)
        });

        let _ = fs::remove_dir_all(&folder);
        assert_eq!(untouched.expect("no write fails"), 1);
        assert_eq!(rewritten.expect("no write fails"), 2);
        assert!(matches!(refused, Err(crate::Error::SyncOff)));
    }
}
