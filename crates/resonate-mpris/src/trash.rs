use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use rustix::fs::{Mode, OFlags};
use thiserror::Error;
use zbus::Fd;

use crate::blocking::connection;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const TRASH: &str = "org.freedesktop.portal.Trash";
const TRASH_FILE: &str = "TrashFile";
const TRASHED: u32 = 1;
const ANSWERS_WITHIN: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum Trashing {
    #[error("{path} could not be opened for the trash portal", path = path.display())]
    Unopened {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("the trash portal could not be reached")]
    Unreached {
        #[source]
        source: Box<zbus::Error>,
    },

    #[error("the trash portal turned {path} away", path = path.display())]
    Refused { path: PathBuf },
}

impl Trashing {
    pub fn is_for_a_file_that_is_gone(&self) -> bool {
        matches!(self, Self::Unopened { source, .. } if source.kind() == io::ErrorKind::NotFound)
    }
}

pub fn trash_through_the_portal(file: &Path) -> Result<(), Trashing> {
    let opened =
        rustix::fs::open(file, OFlags::PATH | OFlags::CLOEXEC, Mode::empty()).map_err(|errno| {
            Trashing::Unopened {
                path: file.to_path_buf(),
                source: io::Error::from(errno),
            }
        })?;
    let descriptor = Fd::from(opened);

    let connection = connection::Builder::session()
        .and_then(|builder| builder.method_timeout(ANSWERS_WITHIN).build())
        .map_err(|source| Trashing::Unreached {
            source: Box::new(source),
        })?;
    let result = connection
        .call_method(
            Some(PORTAL),
            OBJECT_PATH,
            Some(TRASH),
            TRASH_FILE,
            &(descriptor,),
        )
        .and_then(|reply| reply.body().deserialize::<u32>())
        .map_err(|source| Trashing::Unreached {
            source: Box::new(source),
        })?;

    answered(result, file)
}

fn answered(result: u32, file: &Path) -> Result<(), Trashing> {
    if result == TRASHED {
        Ok(())
    } else {
        Err(Trashing::Refused {
            path: file.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_portals_success_code_is_a_file_trashed() {
        let file = Path::new("/music/echoes.flac");

        assert!(answered(1, file).is_ok());
        assert!(matches!(
            answered(0, file),
            Err(Trashing::Refused { path }) if path == file
        ));
        assert!(answered(2, file).is_err());
    }

    #[test]
    fn a_file_that_is_not_there_is_gone_and_not_refused() {
        let missing = Path::new("/nowhere/at/all/echoes.flac");

        let Err(trashing) = trash_through_the_portal(missing) else {
            panic!("a missing file was trashed");
        };

        assert!(trashing.is_for_a_file_that_is_gone());
    }

    #[test]
    fn an_error_stays_small_enough_for_every_result_in_the_crate() {
        assert!(size_of::<Trashing>() <= 128, "{}", size_of::<Trashing>());
    }
}
