use std::sync::LazyLock;

use symphonia::core::{codecs::registry::CodecRegistry, common::Tier, formats::probe::Probe};
use symphonia_codec_wavpack::WavPackReader;

use crate::{
    ape::{Ape, ApeReader},
    framed::{Adts, Framed, Mpeg},
    opus::Opus,
    vorbis::Vorbis,
    wavpack::WavPack,
    wide::WideReader,
};

static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder::<Opus>();
    registry.register_audio_decoder::<Vorbis>();
    registry.register_audio_decoder::<WavPack>();
    registry.register_audio_decoder::<Ape>();
    registry
});

static FORMATS: LazyLock<Probe> = LazyLock::new(|| {
    let mut probe = Probe::new();
    symphonia::default::register_enabled_formats(&mut probe);
    probe.register_format_at_tier::<Framed<'_, Mpeg>>(Tier::Preferred);
    probe.register_format_at_tier::<Framed<'_, Adts>>(Tier::Preferred);
    probe.register_format::<WavPackReader<'_>>();
    probe.register_format::<ApeReader<'_>>();
    probe.register_format::<WideReader<'_>>();
    probe
});

pub(crate) fn codecs() -> &'static CodecRegistry {
    &CODECS
}

pub(crate) fn formats() -> &'static Probe {
    &FORMATS
}
