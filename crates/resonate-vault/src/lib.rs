mod bare;
mod blanks;
mod chunks;
mod cover;
mod drawn;
mod encoding;
mod error;
mod files;
mod flac;
mod form;
#[cfg(fuzzing)]
mod fuzzing;
mod key;
mod ogg;
mod pcm;
mod standing;
mod transcoded;
mod unpacking;
mod vault;
mod wave;

#[cfg(fuzzing)]
pub use crate::fuzzing::{bare_a_file, transcode_a_jpeg, unpack_a_file};
pub use crate::{
    encoding::Encoding,
    error::{Error, FlacOp, PictureOp, Result, VaultOp},
    files::VaultFiles,
    form::Form,
    key::VaultKey,
    vault::{
        Compacting, Halt, HeldFile, Holdings, Keeping, Kept, KeptCover, Offered, Reframing,
        Refusal, Replacing, Taking, Vault,
    },
};
