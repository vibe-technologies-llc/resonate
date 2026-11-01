use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows_core::HSTRING;

use crate::error::{Op, Result, refused};

pub fn name_the_process(app_user_model_id: &str) -> Result<()> {
    let id = HSTRING::from(app_user_model_id);
    unsafe { SetCurrentProcessExplicitAppUserModelID(&id) }.map_err(refused(Op::NameTheProcess))
}
