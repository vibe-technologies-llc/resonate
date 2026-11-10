#[cfg(any(windows, target_os = "macos"))]
use std::env;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use resonate_core::writer::{Held, Writer, writer_of};

use crate::remote::names_this_build;

#[cfg(any(windows, test))]
const PIPE_STEM: &str = "resonate";
#[cfg(any(windows, test))]
const UNNAMED_USER: &str = "user";
const NO_STAMP: u32 = 0;

#[cfg(any(windows, target_os = "macos", test))]
const FOLDER: &str = "resonate-players";
#[cfg(windows)]
const USER_VARIABLE: &str = "USERNAME";
#[cfg(target_os = "macos")]
const HOME_VARIABLE: &str = "HOME";
#[cfg(any(target_os = "macos", test))]
const BESIDE_WHAT_A_MAC_CAN_MAKE_AGAIN: &str = "Library/Caches/resonate/players";

#[cfg(windows)]
pub(crate) fn players_folder() -> PathBuf {
    env::temp_dir().join(FOLDER)
}

#[cfg(target_os = "macos")]
pub(crate) fn players_folder() -> PathBuf {
    let home = env::var_os(HOME_VARIABLE)
        .filter(|home| !home.is_empty())
        .map(PathBuf::from);

    players_folder_of_a_mac(home.as_deref(), &env::temp_dir())
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn players_folder_of_a_mac(home: Option<&Path>, temporary: &Path) -> PathBuf {
    match home {
        Some(home) => home.join(BESIDE_WHAT_A_MAC_CAN_MAKE_AGAIN),
        None => temporary.join(FOLDER),
    }
}

#[cfg(any(windows, test))]
pub(crate) fn pipe_name(user: &str, player: &str) -> String {
    let user: String = user
        .chars()
        .map(
            |letter| match letter.is_alphanumeric() || matches!(letter, '-' | '_') {
                true => letter,
                false => '_',
            },
        )
        .collect();
    let user = match user.is_empty() {
        true => UNNAMED_USER,
        false => &user,
    };

    format!("{PIPE_STEM}-{user}-{player}")
}

#[cfg(windows)]
pub(crate) fn pipe_name_of(player: &str) -> String {
    pipe_name(&env::var(USER_VARIABLE).unwrap_or_default(), player)
}

pub(crate) struct Claim {
    path: PathBuf,
    held: Option<Held>,
}

impl Claim {
    pub(crate) fn make(folder: &Path, player: &str) -> io::Result<Self> {
        fs::create_dir_all(folder)?;
        let path = folder.join(player);
        let held = Held::taken_over(&path)?;

        Ok(Self {
            path,
            held: Some(held),
        })
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        drop(self.held.take());
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn listed(folder: &Path) -> io::Result<Vec<String>> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut living = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !names_this_build(name) {
            continue;
        }
        match writer_of(&path, NO_STAMP) {
            Writer::Living => living.push(name.to_owned()),
            Writer::Gone => {
                let _ = fs::remove_file(&path);
            }
        }
    }
    living.sort_unstable();

    Ok(living)
}

#[cfg(test)]
mod tests {
    use std::{env, process, time::SystemTime};

    use resonate_core::writer::UNHELD_AND_UNTOUCHED_FOR;

    use super::*;
    use crate::service::BUS_NAME;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-registry-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn zone(number: u32) -> String {
        format!("{BUS_NAME}.zone{number}")
    }

    #[test]
    fn a_player_holding_its_name_is_listed_and_a_second_cannot_take_it() {
        let folder = Folder::new("claimed");

        let first = Claim::make(&folder.0, BUS_NAME).expect("the first claim");
        let second = Claim::make(&folder.0, BUS_NAME).map(drop);

        assert_eq!(
            second.map_err(|error| error.kind()),
            Err(io::ErrorKind::WouldBlock)
        );
        assert_eq!(
            listed(&folder.0).expect("the folder lists"),
            vec![BUS_NAME.to_owned()]
        );
        drop(first);
    }

    #[test]
    fn a_player_that_lets_go_leaves_no_entry_and_its_name_can_be_taken_again() {
        let folder = Folder::new("released");

        let first = Claim::make(&folder.0, BUS_NAME).expect("the first claim");
        drop(first);

        assert_eq!(
            listed(&folder.0).expect("the folder lists"),
            Vec::<String>::new()
        );
        assert!(!folder.0.join(BUS_NAME).exists());
        assert!(Claim::make(&folder.0, BUS_NAME).is_ok());
    }

    #[test]
    fn players_are_listed_by_name_and_only_those_of_this_build() {
        let folder = Folder::new("listed");

        let second = Claim::make(&folder.0, &zone(2)).expect("a claim");
        let first = Claim::make(&folder.0, BUS_NAME).expect("a claim");
        fs::write(folder.0.join("org.mpris.MediaPlayer2.vlc"), b"").expect("a stranger");

        assert_eq!(
            listed(&folder.0).expect("the folder lists"),
            vec![BUS_NAME.to_owned(), zone(2)]
        );
        drop((first, second));
    }

    #[test]
    fn a_player_that_died_holding_its_name_is_swept_once_its_entry_has_aged() {
        let folder = Folder::new("dead");
        fs::create_dir_all(&folder.0).expect("a folder");
        let path = folder.0.join(BUS_NAME);
        fs::write(&path, b"").expect("what a dead player left");
        let long_ago = SystemTime::now() - UNHELD_AND_UNTOUCHED_FOR * 2;
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(long_ago))
            .expect("an aged entry");

        let living = listed(&folder.0).expect("the folder lists");

        assert!(living.is_empty());
        assert!(!path.exists());
        assert!(Claim::make(&folder.0, BUS_NAME).is_ok());
    }

    #[test]
    fn a_macs_players_are_kept_under_its_caches_and_under_the_temporary_folder_with_no_home() {
        assert_eq!(
            players_folder_of_a_mac(Some(Path::new("/Users/ada")), Path::new("/tmp")),
            Path::new("/Users/ada/Library/Caches/resonate/players")
        );
        assert_eq!(
            players_folder_of_a_mac(None, Path::new("/tmp")),
            Path::new("/tmp/resonate-players")
        );
    }

    #[test]
    fn a_folder_nobody_made_lists_no_player() {
        let folder = Folder::new("absent");

        assert_eq!(
            listed(&folder.0).expect("no folder is no player"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_pipe_is_named_for_the_user_and_the_player_and_never_holds_a_separator() {
        assert_eq!(
            pipe_name("Ada Lovelace", BUS_NAME),
            format!("resonate-Ada_Lovelace-{BUS_NAME}")
        );
        assert_eq!(
            pipe_name(r"DOMAIN\ada", BUS_NAME),
            format!("resonate-DOMAIN_ada-{BUS_NAME}")
        );
        assert_eq!(pipe_name("", BUS_NAME), format!("resonate-user-{BUS_NAME}"));
        assert!(!pipe_name("a/b", BUS_NAME).contains(['/', '\\']));
    }
}
