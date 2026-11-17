use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the file is open elsewhere, so no write lease is taken on it")]
    OpenElsewhere,

    #[error("{op:?} was refused by the kernel")]
    Fcntl {
        op: LeaseOp,
        #[source]
        source: io::Error,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseOp {
    Signal,
    Take,
    Disown,
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn the_error_stays_small() {
        assert!(size_of::<Error>() <= 128);
    }
}
