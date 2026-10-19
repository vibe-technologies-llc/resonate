use std::sync::Arc;

use resonate_core::Frames;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Music { from: Frames, lasting: Frames },
    Silence { lasting: Frames },
}

impl Edit {
    pub const fn lasting(self) -> Frames {
        match self {
            Self::Music { lasting, .. } | Self::Silence { lasting } => lasting,
        }
    }

    const fn reach(self) -> Frames {
        match self {
            Self::Music { from, lasting } => from.saturating_add(lasting),
            Self::Silence { .. } => Frames::ZERO,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Splices(Arc<[Edit]>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) edit: usize,
    pub(crate) into: Frames,
}

impl Splices {
    pub(crate) fn of(edits: Vec<Edit>) -> Self {
        Self(edits.into())
    }

    pub fn edits(&self) -> &[Edit] {
        &self.0
    }

    pub fn lasting(&self) -> Frames {
        self.0
            .iter()
            .fold(Frames::ZERO, |sum, edit| sum.saturating_add(edit.lasting()))
    }

    pub(crate) fn reach(&self) -> Frames {
        self.0
            .iter()
            .map(|edit| edit.reach())
            .max()
            .unwrap_or(Frames::ZERO)
    }

    pub(crate) fn place_of(&self, heard: Frames) -> Place {
        let mut before = Frames::ZERO;
        for (edit, held) in self.0.iter().enumerate() {
            let after = before.saturating_add(held.lasting());
            if heard < after {
                return Place {
                    edit,
                    into: heard.saturating_sub(before),
                };
            }
            before = after;
        }

        Place {
            edit: self.0.len(),
            into: Frames::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spliced() -> Splices {
        Splices::of(vec![
            Edit::Music {
                from: Frames(1_000),
                lasting: Frames(500),
            },
            Edit::Silence {
                lasting: Frames(200),
            },
            Edit::Music {
                from: Frames::ZERO,
                lasting: Frames(300),
            },
        ])
    }

    #[test]
    fn the_edits_last_as_long_as_they_add_up_to_and_reach_as_far_as_the_furthest_reads() {
        let splices = spliced();

        assert_eq!(splices.lasting(), Frames(1_000));
        assert_eq!(splices.reach(), Frames(1_500));
    }

    #[test]
    fn a_frame_heard_is_placed_in_the_edit_playing_it() {
        let splices = spliced();

        assert_eq!(
            splices.place_of(Frames::ZERO),
            Place {
                edit: 0,
                into: Frames::ZERO
            }
        );
        assert_eq!(
            splices.place_of(Frames(499)),
            Place {
                edit: 0,
                into: Frames(499)
            }
        );
        assert_eq!(
            splices.place_of(Frames(500)),
            Place {
                edit: 1,
                into: Frames::ZERO
            }
        );
        assert_eq!(
            splices.place_of(Frames(750)),
            Place {
                edit: 2,
                into: Frames(50)
            }
        );
        assert_eq!(
            splices.place_of(Frames(1_000)),
            Place {
                edit: 3,
                into: Frames::ZERO
            }
        );
    }
}
