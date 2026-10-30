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
mod framed;
#[cfg(fuzzing)]
mod fuzzing;
mod insert;
mod journal;
mod matroska;
mod mpa;
mod netcdf;
mod ogg;
mod opus;
mod overlay;
mod padded;
mod prescan;
mod probe;
mod registry;
mod riff;
mod sheet;
mod sofa;
mod source;
mod speakers;
mod splices;
mod spool;
mod stream;
mod sylt;
mod tags;
mod timeline;
mod vorbis;
mod walked;
mod wavpack;
mod wide;
mod writing;

pub use symphonia::core::{codecs::audio::AudioCodecId, formats::FormatId};

#[cfg(fuzzing)]
pub use crate::fuzzing::{edit_through_an_overlay, read_a_padded_head, read_an_undo_journal};
pub use crate::{
    artwork::{Drawing, Likeness, Raster, decode_cover},
    boxes::{BoxKind, BoxLayout, Faststart, TopLevelBox},
    cue::{
        CueFile, CueSheet, CueStamp, CueStart, CueTrack, CueTrackKind,
        DEEPEST_FOLDER_A_SHEET_NAMES, Naming as CueNaming, folder_named as the_folder_a_cue_names,
        read as read_cue, read_media as read_cue_media, renamed as renamed_cue,
        the_best_named as the_best_a_cue_names, the_file_named as the_file_a_cue_names,
        the_one_named as the_one_a_cue_names,
    },
    decoder::{Codec, Container, DecodeStatus, Decoder, Delivery, Holes, MediaInfo},
    dsd::{DopMarker, DsdChunk, DsdField, DsdRate, Packing},
    error::{CodecOp, Error, Result, StreamTrackId, TrackProperty},
    journal::{
        Mended, mend_a_cut_short_write, mend_what_was_written_back_from_elsewhere,
        names_a_cut_short_write,
    },
    netcdf::ClassicFault as NetcdfFault,
    probe::{
        CoverArt, ImageFormat, PacketDigest, Pictured, Picturing, Scanned, probe, probe_cover_art,
        probe_pictured, probe_scanned, probe_span,
    },
    sheet::{Placing, SheetEdit, SheetField, SheetScope, SheetText},
    sofa::{
        Access as SofaAccess, Fault as SofaFault, Hrir, Member as SofaMember, SOFA_BYTES_AT_MOST,
        SOFA_STREAMED_BYTES_AT_MOST, Side as SofaSide, read_hrir, read_hrir_bytes,
    },
    source::{
        FormatHint, Hinting, LocalFiles, Media, MediaProvider, MediaStream, Reading, Sources,
        StandIn, StoodIn,
    },
    speakers::{Placement, Speakers},
    splices::{Edit, Splices},
    stream::{
        BitRate, PacketSpan, Percentiles, ProfileBuilder, StreamProfile, StreamReport, Variability,
        WINDOW, probe_stream,
    },
    tags::{
        Credits, LISTED_APART_BY, RawTag, ReplayGain, TagName, TagSet, TagSource, TagValue, Tagged,
    },
    timeline::Timeline,
    walked::Walked,
    writing::{
        FileTags, Popularity, RATED_BY, Rated, Standing, TagEdit, TagField, TagSink, Writing,
        carries_what_the_file_did,
    },
};
