use std::path::{Path, PathBuf};

use resonate_codec::Sources;
use resonate_library::Library;

use crate::{arguments::location_of_argument, error::Result, words};

pub(crate) fn roots(library: &Library) -> Result<()> {
    for root in library.roots()? {
        said!("{}", root.display());
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Forgotten {
    Root,
    Delivered(PathBuf),
    Gone(u64),
    Nothing,
}

pub(crate) fn forget(library: &Library, named: &[PathBuf]) -> Result<()> {
    for argument in named {
        match forgotten(library, argument)? {
            Forgotten::Root => said!("{}", words::main::forgot(argument.display().to_string())),
            Forgotten::Delivered(delivered) => {
                said!(
                    "{}",
                    words::main::forgot_delivered(delivered.display().to_string())
                );
            }
            Forgotten::Gone(tracks) => {
                said!(
                    "{}",
                    words::main::forgot_gone(argument.display().to_string(), tracks)
                );
            }
            Forgotten::Nothing => said!(
                "{}",
                words::main::forgot_nothing(argument.display().to_string())
            ),
        }
    }
    Ok(())
}

pub(crate) fn forgotten(library: &Library, argument: &Path) -> Result<Forgotten> {
    if library.remove_root(argument)? {
        return Ok(Forgotten::Root);
    }
    let named = location_of_argument(argument.as_os_str(), &Sources::local());
    let path = named.as_path().unwrap_or(argument);
    if library.forget_delivered(path)? {
        return Ok(Forgotten::Delivered(path.to_path_buf()));
    }

    Ok(match library.retire(path)? {
        0 => Forgotten::Nothing,
        tracks => Forgotten::Gone(tracks),
    })
}

#[cfg(test)]
mod tests {
    use std::{env, fs, num::NonZeroUsize, process};

    use resonate_library::{PassHandle, ScanOptions};

    use super::*;
    use crate::fixtures::silent_wave;

    #[test]
    fn forgetting_a_folder_inside_a_root_drops_the_tracks_gone_from_it_and_keeps_the_rest() {
        let root = env::temp_dir().join(format!("resonate-forget-{}", process::id()));
        let retired = root.join("retired");
        let kept = root.join("kept");
        fs::create_dir_all(&retired).expect("a scratch folder");
        fs::create_dir_all(&kept).expect("a scratch folder");
        fs::write(retired.join("a.wav"), silent_wave(1)).expect("a track");
        fs::write(retired.join("b.wav"), silent_wave(1)).expect("a track");
        fs::write(kept.join("c.wav"), silent_wave(1)).expect("a track");
        let library = Library::open_in_memory().expect("a catalog");
        library.add_root(&root).expect("a root");

        let scanned = library
            .scan(ScanOptions {
                roots: vec![root.clone()],
                incremental: true,
                follow_symlinks: false,
                extract_cover_art: false,
                workers: NonZeroUsize::MIN,
            })
            .and_then(PassHandle::join)
            .expect("a scan");
        fs::remove_file(retired.join("a.wav")).expect("a track taken away");
        fs::remove_file(kept.join("c.wav")).expect("a track taken away");
        let from_the_folder = forgotten(&library, &retired).expect("a forget");
        let from_nowhere = forgotten(&library, &root.join("never")).expect("a forget");
        let left = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the tracks");
        let the_root = forgotten(&library, &root).expect("a forget");
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert_eq!(scanned.stats.added, 3);
        assert_eq!(from_the_folder, Forgotten::Gone(1));
        assert_eq!(from_nowhere, Forgotten::Nothing);
        assert_eq!(
            left.len(),
            2,
            "a present file or one outside the folder lost its row"
        );
        assert_eq!(the_root, Forgotten::Root);
    }
}
