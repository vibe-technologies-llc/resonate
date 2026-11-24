use std::{
    cell::{Cell, RefCell},
    io,
};

type Hook = Box<dyn FnOnce()>;

thread_local! {
    static ONCE_STAGED: RefCell<Option<Hook>> = const { RefCell::new(None) };
    static RUNS_LAID_BEFORE_FAILING: Cell<Option<usize>> = const { Cell::new(None) };
}

pub(crate) fn once_staged_do(hook: impl FnOnce() + 'static) {
    ONCE_STAGED.with(|armed| *armed.borrow_mut() = Some(Box::new(hook)));
}

pub(crate) fn staged() {
    if let Some(hook) = ONCE_STAGED.with(|armed| armed.borrow_mut().take()) {
        hook();
    }
}

#[cfg(unix)]
pub(crate) fn laying_fails_after(runs: usize) {
    RUNS_LAID_BEFORE_FAILING.with(|armed| armed.set(Some(runs)));
}

pub(crate) fn before_laying_a_run() -> io::Result<()> {
    RUNS_LAID_BEFORE_FAILING.with(|armed| match armed.get() {
        Some(0) => {
            armed.set(None);
            Err(io::Error::from(io::ErrorKind::StorageFull))
        }
        Some(left) => {
            armed.set(Some(left - 1));
            Ok(())
        }
        None => Ok(()),
    })
}
