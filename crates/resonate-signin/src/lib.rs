mod error;
mod jar;
mod outcome;

pub use crate::{
    error::{Error, JarOp, Result},
    jar::{Cookie, kept, netscape, signed_in, written},
    outcome::{CLOSED, Outcome, SIGNED_IN},
};
