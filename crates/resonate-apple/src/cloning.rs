use std::{
    ffi::{CString, c_char, c_int},
    io,
    os::unix::ffi::OsStrExt as _,
    path::Path,
};

const CLONE_NOFOLLOW: u32 = 0x0001;

unsafe extern "C" {
    fn clonefile(from: *const c_char, to: *const c_char, flags: u32) -> c_int;
}

pub fn clone_file(from: &Path, to: &Path) -> io::Result<()> {
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    if unsafe { clonefile(from.as_ptr(), to.as_ptr(), CLONE_NOFOLLOW) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
