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

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

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

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    #[test]
    fn every_reason_the_vault_gives_a_listener_is_said_through_its_catalog() {
        let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/vault.rs"))
            .expect("the vault's source");

        let written: Vec<String> = resonate_words_build::prose_of(&source)
            .into_iter()
            .map(|said| format!("vault.rs:{} {:?}", said.line, said.text))
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
