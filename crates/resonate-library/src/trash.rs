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

use resonate_core::{Calendar, file_facts, naming, owner::Owner};
use rustix::process::getuid;

use crate::{Error, Result, error::TrashOp, unreplacing};

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

impl Bin {
    fn in_folder(trash: &Path, named_from: Option<PathBuf>) -> Self {
        Self {
            files: trash.join(FILES_FOLDER),
            info: trash.join(INFO_FOLDER),
            named_from,
        }
    }
}

pub(crate) fn send(file: &Path, data_home: Option<&Path>, at: SystemTime) -> Result<()> {
    let held =
        fs::symlink_metadata(file).map_err(|source| trash_fault(TrashOp::Stat, file, source))?;
    let bin = bin_for(file, held.dev(), data_home)?;
    let date = deletion_date(&Calendar::local(), at);

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
        match moved_in(file, &bin, &stem, &info_name, &record)? {
            Moved::Landed => return Ok(()),
            Moved::NameTaken => {}
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Moved {
    Landed,
    NameTaken,
}

fn moved_in(
    file: &Path,
    bin: &Bin,
    stem: &OsStr,
    info_name: &OsStr,
    record: &str,
) -> Result<Moved> {
    let info = bin.info.join(info_name);
    let landing = bin.files.join(stem);

    let mut written = match OpenOptions::new().write(true).create_new(true).open(&info) {
        Ok(written) => written,
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            return Ok(Moved::NameTaken);
        }
        Err(source) => return Err(trash_fault(TrashOp::Record, &info, source)),
    };
    let recorded = written
        .write_all(record.as_bytes())
        .and_then(|()| written.sync_all())
        .map_err(|source| trash_fault(TrashOp::Record, &info, source))
        .and_then(|()| {
            file_facts::sync_directory(&bin.info)
                .map_err(|source| trash_fault(TrashOp::Settle, &bin.info, source))
        });
    if let Err(error) = recorded {
        drop(fs::remove_file(&info));
        return Err(error);
    }

    match unreplacing::renamed_over_nothing(file, &landing) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            drop(fs::remove_file(&info));
            return Ok(Moved::NameTaken);
        }
        Err(source) => {
            drop(fs::remove_file(&info));
            return Err(trash_fault(TrashOp::Rename, file, source));
        }
    }

    for folder in [Some(bin.files.as_path()), file.parent()]
        .into_iter()
        .flatten()
    {
        settled(folder);
    }
    Ok(Moved::Landed)
}

fn settled(folder: &Path) {
    if let Err(source) = file_facts::sync_directory(folder) {
        let error = trash_fault(TrashOp::Settle, folder, source);
        tracing::warn!(%error, "a file moved to the trash is not settled onto the disk");
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
        let bin = made(&trash)?;
        let on_the_same_disc = fs::metadata(&bin.files).is_ok_and(|held| held.dev() == device);
        if on_the_same_disc {
            return Ok(bin);
        }
    }

    let top = mount_point_of(file, device).ok_or_else(no_trash)?;
    let trash = top.join(format!(".Trash-{}", getuid().as_raw()));
    kept_alone(&trash, top)
}

fn made(trash: &Path) -> Result<Bin> {
    let bin = Bin::in_folder(trash, None);
    for folder in [&bin.files, &bin.info] {
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(folder)
            .map_err(|source| trash_fault(TrashOp::MakeFolder, folder, source))?;
    }
    Ok(bin)
}

fn kept_alone(trash: &Path, top: PathBuf) -> Result<Bin> {
    let bin = Bin::in_folder(trash, Some(top));
    let owner = Owner::this_user();
    for folder in [trash, bin.files.as_path(), bin.info.as_path()] {
        owner
            .keeps_alone(folder)
            .map_err(|source| trash_fault(TrashOp::KeepAlone, folder, source))?;
    }
    Ok(bin)
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

fn deletion_date(calendar: &Calendar, at: SystemTime) -> String {
    let date = calendar.date_at(at);
    let into_the_day = u64::from(calendar.second_of_the_day(at));

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        date.year,
        date.month,
        date.day,
        into_the_day / SECONDS_PER_HOUR,
        into_the_day % SECONDS_PER_HOUR / SECONDS_PER_MINUTE,
        into_the_day % SECONDS_PER_MINUTE
    )
}

fn trash_fault(op: TrashOp, path: &Path, source: io::Error) -> Error {
    Error::Trash {
        op,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::fs::{PermissionsExt as _, symlink},
        process,
        time::Duration,
    };

    use super::*;

    const OPEN_TO_READERS: u32 = 0o755;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-trash-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a scratch folder");
            Self(path)
        }

        fn volume_trash(&self) -> PathBuf {
            self.0.join(format!(".Trash-{}", getuid().as_raw()))
        }

        fn kept_alone(&self) -> Result<Bin> {
            kept_alone(&self.volume_trash(), self.0.clone())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn refused_as_open(bin: Result<Bin>) -> bool {
        matches!(
            bin,
            Err(Error::Trash { op: TrashOp::KeepAlone, source, .. })
                if source.kind() == io::ErrorKind::PermissionDenied
        )
    }

    #[test]
    fn a_volumes_trash_is_made_closed_to_everyone_else() {
        let scratch = Scratch::new("made");

        let bin = scratch.kept_alone().expect("a trash made on the volume");

        for folder in [scratch.volume_trash(), bin.files, bin.info] {
            let mode = fs::symlink_metadata(&folder)
                .expect("a folder")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, OWNER_ONLY, "{}", folder.display());
        }
    }

    #[test]
    fn a_volumes_trash_that_is_a_link_or_open_to_others_is_refused() {
        let linked = Scratch::new("linked");
        let elsewhere = linked.0.join("elsewhere");
        fs::create_dir(&elsewhere).expect("a folder another user made");
        fs::set_permissions(&elsewhere, fs::Permissions::from_mode(OWNER_ONLY)).expect("closed");
        symlink(&elsewhere, linked.volume_trash()).expect("a link where the trash goes");

        let open = Scratch::new("open");
        fs::create_dir(open.volume_trash()).expect("a trash made before");
        fs::set_permissions(
            open.volume_trash(),
            fs::Permissions::from_mode(OPEN_TO_READERS),
        )
        .expect("opened");

        let open_files = Scratch::new("open-files");
        let files = open_files.volume_trash().join(FILES_FOLDER);
        fs::create_dir_all(&files).expect("a trash made before");
        fs::set_permissions(
            open_files.volume_trash(),
            fs::Permissions::from_mode(OWNER_ONLY),
        )
        .expect("closed");
        fs::set_permissions(&files, fs::Permissions::from_mode(OPEN_TO_READERS)).expect("opened");

        let linked_info = Scratch::new("linked-info");
        let info_elsewhere = linked_info.0.join("elsewhere");
        fs::create_dir(&info_elsewhere).expect("a folder another user made");
        fs::create_dir(linked_info.volume_trash()).expect("a trash made before");
        fs::set_permissions(
            linked_info.volume_trash(),
            fs::Permissions::from_mode(OWNER_ONLY),
        )
        .expect("closed");
        symlink(
            &info_elsewhere,
            linked_info.volume_trash().join(INFO_FOLDER),
        )
        .expect("a link where the records go");

        assert!(refused_as_open(linked.kept_alone()));
        assert!(
            fs::read_dir(&elsewhere)
                .expect("the linked folder")
                .next()
                .is_none(),
            "a folder was made through the link"
        );
        assert!(refused_as_open(open.kept_alone()));
        assert!(refused_as_open(open_files.kept_alone()));
        assert!(refused_as_open(linked_info.kept_alone()));
    }

    #[test]
    fn a_name_taken_in_the_trash_meanwhile_is_never_moved_over_and_its_record_is_taken_back() {
        let scratch = Scratch::new("taken");
        let bin = made(&scratch.0.join(TRASH_FOLDER)).expect("a trash");
        let file = scratch.0.join("echoes.flac");
        fs::write(&file, b"deleted now").expect("a file to trash");
        let (stem, info_name) = names_for(OsStr::new("echoes.flac"), 0);
        fs::write(bin.files.join(&stem), b"trashed before").expect("a file already trashed");

        let moved = moved_in(&file, &bin, &stem, &info_name, "[Trash Info]\n");

        assert_eq!(moved.expect("no fault"), Moved::NameTaken);
        assert_eq!(fs::read(&file).expect("the file stays"), b"deleted now");
        assert_eq!(
            fs::read(bin.files.join(&stem)).expect("the trashed file stays"),
            b"trashed before"
        );
        assert!(
            !bin.info.join(&info_name).exists(),
            "a record names nothing"
        );
    }

    #[test]
    fn a_file_trashed_beside_another_of_its_name_takes_the_next_name_and_its_record() {
        let scratch = Scratch::new("beside");
        let data_home = scratch.0.join("data");
        let trash = data_home.join(TRASH_FOLDER);
        let bin = made(&trash).expect("a trash");
        fs::write(bin.files.join("echoes.flac"), b"trashed before")
            .expect("a file already trashed");
        let file = scratch.0.join("echoes.flac");
        fs::write(&file, b"deleted now").expect("a file to trash");

        send(&file, Some(&data_home), at(1_774_778_400)).expect("trashed");

        assert!(!file.exists());
        assert_eq!(
            fs::read(bin.files.join("echoes.flac.1")).expect("the second name"),
            b"deleted now"
        );
        assert!(bin.info.join("echoes.flac.1.trashinfo").exists());
        assert_eq!(
            fs::read(bin.files.join("echoes.flac")).expect("the first stays"),
            b"trashed before"
        );
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn a_deletion_on_a_day_the_clocks_change_is_dated_by_the_wall_clock() {
        let berlin = Calendar::named("Europe/Berlin").expect("a zone database");
        let noon_after_the_clocks_went_forward = at(1_774_778_400);
        let half_past_eleven_after_they_went_back = at(1_792_967_400);

        assert_eq!(
            deletion_date(&berlin, noon_after_the_clocks_went_forward),
            "2026-03-29T12:00:00"
        );
        assert_eq!(
            deletion_date(&berlin, half_past_eleven_after_they_went_back),
            "2026-10-25T23:30:00"
        );
    }
}
