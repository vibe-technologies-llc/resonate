mod alternatives;
mod credits;
mod db;
mod elsewhere;
mod enrich;
mod enriched;
mod error;
mod fingerprint;
mod hinted;
mod history;
mod import;
mod likeness;
mod loose;
mod m3u;
mod model;
mod moves;
mod organise;
mod paged;
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
mod sung;
mod supply;
mod undo;
mod vaulted;
mod watch;
mod xspf;

pub use resonate_analysis::{Cutoff, LossyGuess, Study, Verdict};
pub use resonate_codec::{
    Codec, CoverArt, Drawing, FileTags, Hinting, ImageFormat, Pictured, Picturing, Popularity,
    Raster, Rated, Sources, StandIn, StoodIn, TagEdit, TagField, TagSet, TagSink, TagSource,
    Tagged,
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
    history::{Aged, HistoryKept},
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
        ArtistMatch, ArtistProfile, ArtistRelease, Credit, Discography, Genre, GroupAsked,
        GroupMatch, GroupRelease, Issued, LifeSpan, LookupOp, LyricDetail, LyricText, LyricsAsked,
        Medium, Recording, RecordingAsked, RecordingMatch, RecordingRelease, Reference, Release,
        ReleaseAsked, ReleaseGroup, ReleaseMatch, ReleaseTrack, StreamAsked, Wording,
        apple_music_urls, deezer_urls, may_be_pictured, portrait_urls, soundcloud_urls,
        spotify_urls, wikidata_urls, wikipedia_urls,
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
    sung::{BETTERED_AFTER, MISSED_AGAIN_AFTER},
    supply::{ANSWERS_WITHIN, POLL_AGAIN_AFTER, PollOptions, PollProgress, PollStats, PollSummary},
    undo::{Edit, Undoable},
    watch::RootsWatch,
};
