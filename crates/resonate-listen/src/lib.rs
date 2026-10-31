mod error;
mod heard;
#[cfg(any(target_os = "linux", windows))]
mod hearing;
#[cfg(target_os = "linux")]
mod listener;
#[cfg(target_os = "linux")]
mod recording;
#[cfg(windows)]
mod unavailable;

#[cfg(any(target_os = "linux", windows))]
pub use crate::hearing::{CLIP_BY_DEFAULT, Hearing, Listening};
#[cfg(windows)]
pub use crate::unavailable::{Listener, Microphone};
pub use crate::{
    error::{CaptureOp, Error, Result},
    heard::{Clip, Heard, Picture, PictureFormat, Recogniser, Recognisers, Recognition},
};
#[cfg(target_os = "linux")]
pub use crate::{listener::Listener, recording::Recording};
