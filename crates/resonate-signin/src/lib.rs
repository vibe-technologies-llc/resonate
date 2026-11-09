mod error;
mod jar;
mod outcome;
mod place;
#[cfg(feature = "window")]
mod window;

#[cfg(feature = "window")]
pub use crate::{error::EngineOp, window::sign_in};
pub use crate::{
    error::{Error, JarOp, Result},
    jar::{Cookie, kept, netscape, signed_in, written},
    outcome::{CLOSED, FAILED, Outcome, SIGNED_IN},
    place::{SIGN_IN_PAGE, is_youtube_music},
};
