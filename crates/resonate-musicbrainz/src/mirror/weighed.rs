use crate::{Entity, Error, Result};

const ORDINAL_BITS: u32 = 32;
const ORDINALS_PLACED: i64 = 1 << ORDINAL_BITS;
const STEP_BITS: u32 = 2;
const STEPS_AN_OCTAVE: u64 = 1 << STEP_BITS;
const LIGHTEST: u64 = 127;

fn heaviness(weight: u64) -> u64 {
    if weight < STEPS_AN_OCTAVE {
        return weight;
    }

    let octave = u64::from(weight.ilog2());
    let step = (weight >> (octave - u64::from(STEP_BITS))) & (STEPS_AN_OCTAVE - 1);
    (STEPS_AN_OCTAVE * (octave - 1) + step).min(LIGHTEST)
}

pub(crate) fn placed_by_weight(kind: Entity, ordinal: i64, weight: u64) -> Result<i64> {
    if !(0..ORDINALS_PLACED).contains(&ordinal) {
        return Err(Error::OrdinalPastWeighing { kind, ordinal });
    }

    let lighter_by = i64::try_from(LIGHTEST - heaviness(weight)).unwrap_or(0);
    Ok((lighter_by << ORDINAL_BITS) | ordinal)
}

pub(crate) const fn ordinal_of(placed: i64) -> i64 {
    placed & (ORDINALS_PLACED - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heavier_document_is_placed_before_a_lighter_and_alike_ones_keep_their_order() {
        let heavy = placed_by_weight(Entity::Recording, 900, 40).expect("placed");
        let light = placed_by_weight(Entity::Recording, 3, 1).expect("placed");
        let alone = placed_by_weight(Entity::Recording, 2, 0).expect("placed");
        let alike = placed_by_weight(Entity::Recording, 4, 1).expect("placed");

        assert!(heavy < light);
        assert!(light < alone);
        assert!(light < alike);
        assert_eq!(ordinal_of(heavy), 900);
        assert_eq!(ordinal_of(light), 3);
        assert_eq!(ordinal_of(alone), 2);
    }

    #[test]
    fn a_rowid_placed_by_no_weight_is_its_own_ordinal() {
        for ordinal in [1, 4_096, 35_000_000, ORDINALS_PLACED - 1] {
            assert_eq!(ordinal_of(ordinal), ordinal);
        }
    }

    #[test]
    fn heaviness_never_falls_as_the_weight_grows_and_counts_the_first_weights_apart() {
        let steps: Vec<u64> = (0..100_000).map(heaviness).collect();

        assert!(steps.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(&steps[..8], &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(heaviness(u64::MAX), LIGHTEST);
    }

    #[test]
    fn an_ordinal_past_what_a_rowid_holds_beside_its_weight_is_refused() {
        assert!(matches!(
            placed_by_weight(Entity::Artist, ORDINALS_PLACED, 1),
            Err(Error::OrdinalPastWeighing {
                kind: Entity::Artist,
                ordinal: ORDINALS_PLACED
            })
        ));
    }
}
