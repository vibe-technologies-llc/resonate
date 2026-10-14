mod error;
mod heard;
#[cfg(not(target_os = "android"))]
mod listener;
#[cfg(not(target_os = "android"))]
mod recording;

pub use crate::{
    error::{CaptureOp, Error, Result},
    heard::{Clip, Heard, Picture, PictureFormat, Recogniser, Recognisers, Recognition},
};
#[cfg(not(target_os = "android"))]
pub use crate::{
    listener::{CLIP_BY_DEFAULT, Hearing, Listener, Listening},
    recording::Recording,
};
