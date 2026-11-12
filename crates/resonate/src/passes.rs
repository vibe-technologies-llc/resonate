use std::sync::Arc;

use resonate_library::{Cancelling, PassHandle, PassKind};

use crate::error::{Error, Result};

pub(crate) fn until_told<Progress, Summary>(
    handle: PassHandle<Progress, Summary>,
) -> Result<Summary>
where
    Progress: Cancelling + 'static,
{
    let progress = Arc::clone(handle.progress());
    let _interrupting = crate::signals::cancel_when_told(move || progress.cancel());
    Ok(handle.join()?)
}

pub(crate) const fn none_failed(pass: PassKind, failed: u64) -> Result<()> {
    if failed > 0 {
        Err(Error::FilesFailed { pass, failed })
    } else {
        Ok(())
    }
}

pub(crate) const fn finished(pass: PassKind, cancelled: bool) -> Result<()> {
    if cancelled {
        Err(Error::Cancelled { pass })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn a_pass_whose_files_failed_answers_an_error_so_the_command_exits_1() {
        assert!(none_failed(PassKind::Organise, 0).is_ok());
        assert!(matches!(
            none_failed(PassKind::Retag, 3),
            Err(Error::FilesFailed {
                pass: PassKind::Retag,
                failed: 3
            })
        ));
    }

    #[test]
    fn a_cancelled_pass_answers_an_error_so_the_command_exits_1() {
        assert!(finished(PassKind::Scan, false).is_ok());
        assert!(matches!(
            finished(PassKind::Scan, true),
            Err(Error::Cancelled {
                pass: PassKind::Scan
            })
        ));
        assert!(matches!(
            finished(PassKind::Import, true),
            Err(Error::Cancelled {
                pass: PassKind::Import
            })
        ));
    }
}
