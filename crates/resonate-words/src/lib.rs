mod area;
mod language;
mod locale;

pub use area::{Area, Arg};
pub use language::{Language, LanguageChoice, negotiated, speak, spoken};
pub use locale::languages_the_listener_reads;
