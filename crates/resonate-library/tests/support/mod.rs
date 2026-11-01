#![allow(dead_code)]

use std::{fs, io, path::Path};

const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;

#[cfg(unix)]
fn symlink_directory(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_directory(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

fn linked(made: io::Result<()>) -> bool {
    match made {
        Ok(()) => true,
        Err(error) if lacks_the_privilege(&error) => {
            eprintln!("skipped: this account may not create symbolic links ({error})");
            false
        }
        Err(error) => panic!("the fixture tree takes a link: {error}"),
    }
}

fn lacks_the_privilege(error: &io::Error) -> bool {
    cfg!(windows)
        && (error.kind() == io::ErrorKind::PermissionDenied
            || error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD))
}

pub fn link_to_directory(target: &Path, link: &Path) -> bool {
    linked(symlink_directory(target, link))
}

pub fn link_to_file(target: &Path, link: &Path) -> bool {
    linked(symlink_file(target, link))
}

#[cfg(unix)]
pub fn forbid_writing(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o444))
        .expect("the fixture file takes a mode");
}

#[cfg(unix)]
pub fn allow_writing(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .expect("the fixture file takes a mode");
}

#[cfg(windows)]
pub fn forbid_writing(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("the fixture file stands")
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).expect("the fixture file takes the read-only mark");
}

#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
pub fn allow_writing(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("the fixture file stands")
        .permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).expect("the fixture file loses the read-only mark");
}

pub fn can_write(path: &Path) -> bool {
    fs::OpenOptions::new().write(true).open(path).is_ok()
}

#[cfg(unix)]
pub fn unlink_directory(link: &Path) -> io::Result<()> {
    fs::remove_file(link)
}

#[cfg(windows)]
pub fn unlink_directory(link: &Path) -> io::Result<()> {
    fs::remove_dir(link)
}
