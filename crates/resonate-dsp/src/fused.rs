const FUSES: bool = cfg!(target_feature = "fma");

#[inline(always)]
pub(crate) fn multiply_add(multiplied: f64, by: f64, added: f64) -> f64 {
    if FUSES {
        multiplied.mul_add(by, added)
    } else {
        multiplied * by + added
    }
}

#[inline(always)]
pub(crate) fn larger(one: f64, other: f64) -> f64 {
    if one > other { one } else { other }
}
