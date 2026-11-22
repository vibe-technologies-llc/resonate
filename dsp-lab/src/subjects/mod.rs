pub mod played;
pub mod resampler;
#[cfg(feature = "rubato")]
pub mod rubato;
#[cfg(feature = "soxr")]
pub mod soxr;
pub mod stage;

use crate::lab::subject::{Subject, SubjectName};

#[cfg(feature = "soxr")]
fn of_soxr() -> Vec<Box<dyn Subject>> {
    soxr::RECIPES
        .iter()
        .map(|&recipe| Box::new(recipe) as Box<dyn Subject>)
        .collect()
}

#[cfg(not(feature = "soxr"))]
fn of_soxr() -> Vec<Box<dyn Subject>> {
    Vec::new()
}

#[cfg(feature = "rubato")]
fn of_rubato() -> Vec<Box<dyn Subject>> {
    rubato::SETTINGS
        .iter()
        .map(|&setting| Box::new(setting) as Box<dyn Subject>)
        .collect()
}

#[cfg(not(feature = "rubato"))]
fn of_rubato() -> Vec<Box<dyn Subject>> {
    Vec::new()
}

pub fn references() -> Vec<Box<dyn Subject>> {
    of_soxr().into_iter().chain(of_rubato()).collect()
}

#[cfg(feature = "soxr")]
pub fn plotted_references() -> Vec<SubjectName> {
    soxr::PLOTTED.iter().map(Subject::name).collect()
}

#[cfg(not(feature = "soxr"))]
pub fn plotted_references() -> Vec<SubjectName> {
    Vec::new()
}
