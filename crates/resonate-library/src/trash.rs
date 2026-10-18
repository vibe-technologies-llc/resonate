use std::{
    env,
    ffi::{OsStr, OsString},
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Write as _},
    os::unix::{
        ffi::{OsStrExt as _, OsStringExt as _},
        fs::{DirBuilderExt as _, MetadataExt as _},
    },
    path::{Path, PathBuf},
    time::SystemTime,
};

use resonate_core::{Calendar, naming};
use rustix::process::getuid;

use crate::{Error, Result, error::TrashOp};

const TRASH_FOLDER: &str = "Trash";
const FILES_FOLDER: &str = "files";
const INFO_FOLDER: &str = "info";
const INFO_ENDING: &str = ".trashinfo";
const OWNER_ONLY: u32 = 0o700;
const SECONDS_PER_MINUTE: u64 = 60;
const MINUTES_PER_HOUR: u64 = 60;
const SECONDS_PER_HOUR: u64 = SECONDS_PER_MINUTE * MINUTES_PER_HOUR;

pub(crate) fn environment_data_home() -> Option<PathBuf> {
    let named = |variable: &str| env::var_os(variable).map(PathBuf::from);
    named("XDG_DATA_HOME")
        .filter(|path| path.is_absolute())
        .or_else(|| named("HOME").map(|home| home.join(".local/share")))
}

struct Bin {
    files: PathBuf,
    info: PathBuf,
    named_from: Option<PathBuf>,
}

pub(crate) fn send(file: &Path, data_home: Option<&Path>, at: SystemTime) -> Result<()> {
    let held =
        fs::symlink_metadata(file).map_err(|source| trash_fault(TrashOp::Stat, file, source))?;
    let bin = bin_for(file, held.dev(), data_home)?;
    let date = deletion_date(at);

    let name = file.file_name().ok_or_else(|| Error::NoTrashFor {
        path: file.to_path_buf(),
    })?;
    let recorded = match &bin.named_from {
        Some(top) => file.strip_prefix(top).unwrap_or(file),
        None => file,
    };
    let record = format!(
        "[Trash Info]\nPath={}\nDeletionDate={date}\n",
        percent_encoded(recorded)
    );

    let mut discriminator = 0_u32;
    loop {
        let (stem, info_name) = names_for(name, discriminator);
        discriminator += 1;
        let info = bin.info.join(&info_name);
        let landing = bin.files.join(&stem);
        if fs::symlink_metadata(&landing).is_ok() {
            continue;
        }

        match OpenOptions::new().write(true).create_new(true).open(&info) {
            Ok(mut written) => {
                if let Err(source) = written.write_all(record.as_bytes()) {
                    drop(fs::remove_file(&info));
                    return Err(trash_fault(TrashOp::Record, &info, source));
                }
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(trash_fault(TrashOp::Record, &info, source)),
        }

        if let Err(source) = fs::rename(file, &landing) {
            drop(fs::remove_file(&info));
            return Err(trash_fault(TrashOp::Rename, file, source));
        }
        return Ok(());
    }
}

fn bin_for(file: &Path, device: u64, data_home: Option<&Path>) -> Result<Bin> {
    let no_trash = || Error::NoTrashFor {
        path: file.to_path_buf(),
    };

    if let Some(home) = data_home
        .map(Path::to_path_buf)
        .or_else(environment_data_home)
    {
        let trash = home.join(TRASH_FOLDER);
        let bin = made(&trash, None)?;
        let on_the_same_disc = fs::metadata(&bin.files).is_ok_and(|held| held.dev() == device);
        if on_the_same_disc {
            return Ok(bin);
        }
    }

    let top = mount_point_of(file, device).ok_or_else(no_trash)?;
    let trash = top.join(format!(".Trash-{}", getuid().as_raw()));
    made(&trash, Some(top))
}

fn made(trash: &Path, named_from: Option<PathBuf>) -> Result<Bin> {
    let files = trash.join(FILES_FOLDER);
    let info = trash.join(INFO_FOLDER);
    for folder in [&files, &info] {
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(folder)
            .map_err(|source| trash_fault(TrashOp::MakeFolder, folder, source))?;
    }
    Ok(Bin {
        files,
        info,
        named_from,
    })
}

fn mount_point_of(file: &Path, device: u64) -> Option<PathBuf> {
    let mut top = file.parent()?;
    while let Some(up) = top.parent() {
        if fs::metadata(up).ok()?.dev() != device {
            break;
        }
        top = up;
    }
    Some(top.to_path_buf())
}

fn names_for(name: &OsStr, discriminator: u32) -> (OsString, OsString) {
    let after = if discriminator == 0 {
        String::new()
    } else {
        format!(".{discriminator}")
    };
    let cut = naming::named_within("", name, format!("{after}{INFO_ENDING}"));
    let mut stem = cut.into_vec();
    stem.truncate(stem.len() - INFO_ENDING.len() - after.len());
    stem.extend_from_slice(after.as_bytes());

    let mut info = stem.clone();
    info.extend_from_slice(INFO_ENDING.as_bytes());
    (OsString::from_vec(stem), OsString::from_vec(info))
}

fn percent_encoded(path: &Path) -> String {
    let mut encoded = String::new();
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn deletion_date(at: SystemTime) -> String {
    let calendar = Calendar::local();
    let day = calendar.day_of(at);
    let date = calendar.date_at(at);
    let since_midnight = at
        .duration_since(calendar.midnight_of(day))
        .map_or(0, |elapsed| elapsed.as_secs());

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        date.year,
        date.month,
        date.day,
        since_midnight / SECONDS_PER_HOUR,
        since_midnight % SECONDS_PER_HOUR / SECONDS_PER_MINUTE,
        since_midnight % SECONDS_PER_MINUTE
    )
}

fn trash_fault(op: TrashOp, path: &Path, source: io::Error) -> Error {
    Error::Trash {
        op,
        path: path.to_path_buf(),
        source,
    }
}
