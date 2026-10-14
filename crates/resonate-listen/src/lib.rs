mod error;
mod heard;
#[cfg(target_os = "linux")]
mod listener;
#[cfg(target_os = "linux")]
mod recording;

pub use crate::{
    error::{CaptureOp, Error, Result},
    heard::{Clip, Heard, Picture, PictureFormat, Recogniser, Recognisers, Recognition},
};
#[cfg(target_os = "linux")]
pub use crate::{
    listener::{CLIP_BY_DEFAULT, Hearing, Listener, Listening},
    recording::Recording,
};
