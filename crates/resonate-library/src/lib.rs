mod alternatives;
mod credits;
mod db;
mod elsewhere;
mod enrich;
mod enriched;
mod error;
mod fingerprint;
mod hinted;
mod import;
mod likeness;
mod m3u;
mod model;
mod moves;
mod organise;
mod pass;
mod playlist;
mod pls;
mod query;
mod reference;
mod retag;
mod scan;
mod schema;
mod scrobble;
mod search;
mod share;
mod sheet;
mod spelling;
mod statistics;
mod stem;
mod store;
mod studies;
mod suggest;
mod supply;
mod undo;
mod vaulted;
mod watch;
mod xspf;

pub use resonate_analysis::{Cutoff, LossyGuess, Study, Verdict};
pub use resonate_codec::{
    Codec, CoverArt, Drawing, FileTags, Hinting, ImageFormat, Pictured, Picturing, Sources,
    StandIn, StoodIn, TagEdit, TagField, TagSet, TagSink, TagSource, Tagged,
};
pub use resonate_core::{Chromaprint, Isrc, Link, Mbid, Relation, Service};
pub use resonate_vault::{
    Encoding, Form, HeldCover, Holdings, Keeping, Kept as KeptInVault, KeptCover,
    Refusal as VaultRefusal, Taking, Vault, VaultFiles, VaultKey,
};

#[cfg(fuzzing)]
pub fn read_a_playlist_sheet(text: &str) {
    let _ = crate::sheet::parse(text, std::path::Path::new("/music"));
}

pub use crate::{
    db::{CatalogStamp, Library, WrittenElsewhere},
    elsewhere::{FOUND_ELSEWHERE_AT_MOST, Found, Sung, asks_elsewhere},
    enrich::{
        Certainty, EnrichOptions, EnrichProgress, EnrichStats, EnrichSummary, Fruitless,
        REFRESH_AFTER, REFUSED_AGAIN_AFTER, RETRY_AFTER, Sought, WAITS, Waits,
    },
    error::{
        EncodedColumn, Error, FieldName, LayoutFault, MoveOp, OrderedColumn, PlaylistName, Result,
        SchemaFingerprint, StoreOp, TagName,
    },
    fingerprint::{Fingerprinters, Fingerprints, NoFingerprints, Printed, Recognition, Sounded},
    import::{
        ImportOptions, ImportPlan, ImportProgress, ImportStats, ImportSummary, Passed, Passing,
        Vaulted, Wanted,
    },
    model::{
        Album, AlbumToAsk, Artist, ArtistDetail, ArtistToAsk, ArtistTotals, Counted, CoverSource,
        Cut, Exported, Favoured, HeldMedium, HeldReleaseTrack, Imported, KeptCorrection, KeptIndex,
        KeptLyrics, Measured, Missing, MissingTrack, NamedPlaylist, Playing, Playlist,
        PlaylistEntry, PlaylistFormat, PortraitWanted, Pruned, ReleaseDetail, Released,
        SheetEncoding, Track, TrackToAsk, Unfinished, UnheldRelease, VaultObject, Want,
    },
    organise::{
        Companion, DEFAULT_LAYOUT, Field, Layout, Move, OrganiseOptions, OrganiseProgress,
        OrganiseStats, OrganiseSummary, Plan, Refusal, Refused, Sidecar,
    },
    pass::{
        Cancelling, EnrichHandle, ImportHandle, OrganiseHandle, PassHandle, PassKind, PollHandle,
        RetagHandle, ScanHandle,
    },
    query::{
        AlbumOrder, AlbumQuery, ArtistOrder, ArtistQuery, Direction, Kept, PlaylistOrder, RowOrder,
        SavedQuery, SearchResults, SortOrder, TrackQuery,
    },
    reference::{
        ArtistMatch, ArtistProfile, ArtistRelease, Credit, Genre, GroupAsked, GroupMatch,
        GroupRelease, LifeSpan, LookupOp, Medium, Recording, RecordingAsked, RecordingMatch,
        RecordingRelease, Reference, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch,
        ReleaseTrack, Wording, apple_music_urls, deezer_urls, may_be_pictured, portrait_urls,
        wikidata_urls,
    },
    retag::{
        PassedOver, RetagOptions, RetagProgress, RetagStats, RetagSummary, Retagging, Unwritten,
        Written,
    },
    scan::{Failure, Failures, ScanOptions, ScanProgress, ScanStats, ScanSummary},
    scrobble::{ListeningService, SUBMITTED_AT_ONCE, Scrobble, Scrobbler, Submitted},
    search::{Asked, Clause, Column, Compare, Condition, Lit, Search, Shape, Term, Word},
    share::Shared,
    spelling::Spellings,
    statistics::{Day, Listened, MostListened, Statistics, Window},
    store::folded_letters,
    studies::{Agreement, HEARD_AT_LEAST, Heard, HeardAs, Studied, StudiedTrack, StudyFilter},
    suggest::{Kind as SuggestionKind, PICTURED_BY_AT_MOST, Reason, Suggestion},
    supply::{ANSWERS_WITHIN, POLL_AGAIN_AFTER, PollOptions, PollProgress, PollStats, PollSummary},
    undo::{Edit, Undoable},
    watch::RootsWatch,
};
