use std::sync::atomic::{AtomicBool, Ordering};

static WATCHED_CLOSELY: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watched {
    Closely,
    Loosely,
}

impl Watched {
    pub fn set(self) {
        WATCHED_CLOSELY.store(self == Self::Closely, Ordering::Relaxed);
    }

    #[must_use]
    pub fn now() -> Self {
        if WATCHED_CLOSELY.load(Ordering::Relaxed) {
            Self::Closely
        } else {
            Self::Loosely
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Watched;

    #[test]
    fn a_player_is_watched_closely_until_told_otherwise_and_follows_each_telling() {
        assert_eq!(Watched::now(), Watched::Closely);

        Watched::Loosely.set();

        assert_eq!(Watched::now(), Watched::Loosely);

        Watched::Closely.set();

        assert_eq!(Watched::now(), Watched::Closely);
    }
}
