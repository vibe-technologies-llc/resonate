use std::{fmt, num::NonZeroU64};

use crate::{Error, Result};

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub const MAX: Self = Self(NonZeroU64::MAX);

            pub const fn new(raw: u64) -> Result<Self> {
                match NonZeroU64::new(raw) {
                    Some(raw) => Ok(Self(raw)),
                    None => Err(Error::ZeroId),
                }
            }

            pub const fn of(raw: NonZeroU64) -> Self {
                Self(raw)
            }

            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.get())
            }
        }
    };
}

id!(TrackId);
id!(AlbumId);
id!(ArtistId);
id!(PlaylistId);
id!(ReleaseTrackId);
id!(WantId);
id!(ListenId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_non_zero_so_option_costs_nothing() {
        assert_eq!(size_of::<Option<TrackId>>(), size_of::<TrackId>());
        assert!(TrackId::new(0).is_err());
        assert_eq!(TrackId::new(1).expect("1 is non-zero").get(), 1);
    }

    #[test]
    fn the_highest_id_is_the_one_a_queue_hands_out_first() {
        assert_eq!(TrackId::MAX.get(), u64::MAX);
        assert_eq!(
            TrackId::new(u64::MAX).expect("u64::MAX is non-zero"),
            TrackId::MAX
        );
    }
}
