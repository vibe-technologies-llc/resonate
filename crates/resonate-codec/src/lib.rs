mod ape;
mod artwork;
mod boxes;
mod caf;
mod chapters;
mod container;
mod counted;
mod cue;
mod decoder;
mod dsd;
mod error;
mod flac;
mod journal;
mod matroska;
mod opus;
mod overlay;
mod padded;
mod prescan;
mod probe;
mod registry;
mod riff;
mod source;
mod speakers;
mod spool;
mod stream;
mod sylt;
mod tags;
mod timeline;
mod vorbis;
mod wavpack;
mod wide;
mod writing;

pub use symphonia::core::{codecs::audio::AudioCodecId, formats::FormatId};

pub use crate::{
    artwork::{Drawing, Likeness, Raster},
    boxes::{BoxKind, BoxLayout, Faststart, TopLevelBox, read as probe_boxes},
    cue::{
        CueFile, CueSheet, CueStamp, CueStart, CueTrack, CueTrackKind,
        DEEPEST_FOLDER_A_SHEET_NAMES, Naming as CueNaming, folder_named as the_folder_a_cue_names,
        read as read_cue, read_media as read_cue_media, renamed as renamed_cue,
        the_best_named as the_best_a_cue_names, the_file_named as the_file_a_cue_names,
        the_one_named as the_one_a_cue_names,
    },
    decoder::{Codec, Container, DecodeStatus, Decoder, Delivery, Holes, MediaInfo},
    dsd::{DsdChunk, DsdField, DsdRate, Packing},
    error::{CodecOp, Error, Result, StreamTrackId, TrackProperty},
    journal::{Mended, mend_a_cut_short_write, names_a_cut_short_write},
    probe::{
        CoverArt, ImageFormat, PacketDigest, Pictured, Picturing, Scanned, probe, probe_cover_art,
        probe_pictured, probe_scanned, probe_span,
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
    timeline::Timeline,
    writing::{FileTags, Popularity, RATED_BY, Rated, TagEdit, TagField, TagSink, Writing},
};
