#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Encoding(u16);

impl Encoding {
    pub const UNRECORDED: Self = Self(0);
    pub const OF_THIS_BUILD: Self = Self(2);

    pub const fn of(held: u16) -> Self {
        Self(held)
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn is_behind(self) -> bool {
        self.0 < Self::OF_THIS_BUILD.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_nobody_recorded_is_behind_and_one_of_this_build_is_not() {
        assert!(Encoding::UNRECORDED.is_behind());
        assert!(!Encoding::OF_THIS_BUILD.is_behind());
        assert!(!Encoding::of(Encoding::OF_THIS_BUILD.get() + 1).is_behind());
    }
}
