mod error;
mod heard;
#[cfg(any(target_os = "linux", windows))]
mod hearing;
#[cfg(target_os = "linux")]
mod listener;
#[cfg(any(target_os = "linux", windows))]
mod recording;
#[cfg(any(target_os = "linux", windows))]
mod session;

#[cfg(any(windows, test))]
mod endpoints;
#[cfg(windows)]
mod wasapi_apartment;
#[cfg(windows)]
mod wasapi_capture;
#[cfg(windows)]
mod wasapi_listener;
#[cfg(windows)]
mod wasapi_playback;

#[cfg(windows)]
pub use crate::endpoints::Microphone;
#[cfg(any(target_os = "linux", windows))]
pub use crate::hearing::{CLIP_BY_DEFAULT, Hearing, Listening};
#[cfg(target_os = "linux")]
pub use crate::listener::Listener;
#[cfg(any(target_os = "linux", windows))]
pub use crate::recording::Recording;
#[cfg(windows)]
pub use crate::wasapi_listener::Listener;
pub use crate::{
    error::{CaptureOp, Error, Result},
    heard::{Clip, Heard, Picture, PictureFormat, Recogniser, Recognisers, Recognition},
};
