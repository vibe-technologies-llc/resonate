mod artwork;
mod boxes;
mod container;
mod cue;
mod decoder;
mod dsd;
mod error;
mod flac;
mod matroska;
mod opus;
mod prescan;
mod probe;
mod riff;
mod source;
mod speakers;
mod stream;
mod sylt;
mod tags;
mod text;
mod timeline;
mod writing;

pub use symphonia::core::{codecs::audio::AudioCodecId, formats::FormatId};

pub use crate::{
    artwork::Drawing,
    boxes::{BoxKind, BoxLayout, Faststart, TopLevelBox, read as probe_boxes},
    cue::{
        CueFile, CueSheet, CueStamp, CueStart, CueTrack, CueTrackKind, read as read_cue,
        read_media as read_cue_media, renamed as renamed_cue,
    },
    decoder::{Codec, Container, DecodeStatus, Decoder, Delivery, MediaInfo},
    dsd::{DsdChunk, DsdField, DsdRate, Packing},
    error::{CodecOp, Error, Result, StreamTrackId, TrackProperty},
    probe::{
        CoverArt, ImageFormat, Pictured, Picturing, probe, probe_cover_art, probe_pictured,
        probe_span,
    },
    source::{
        FormatHint, Hinting, LocalFiles, Media, MediaProvider, MediaStream, Reading, Sources,
        StandIn, StoodIn,
    },
    speakers::{Placement, Speakers},
    stream::{
        BitRate, PacketSpan, Percentiles, ProfileBuilder, StreamProfile, StreamReport, Variability,
        WINDOW, probe_stream,
    },
    tags::{Credits, RawTag, ReplayGain, TagName, TagSet, TagSource, TagValue, Tagged},
    text::TextEncoding,
    timeline::Timeline,
    writing::{FileTags, TagEdit, TagField, TagSink, Writing},
};
