mod alternatives;
mod credits;
mod db;
mod deleted;
mod elsewhere;
mod enrich;
mod enriched;
mod error;
mod filed;
mod filters;
mod fingerprint;
mod hinted;
mod history;
mod import;
mod learning;
mod likeness;
mod linked;
mod loose;
mod m3u;
mod meant;
mod model;
mod moves;
mod numerals;
mod organise;
mod paged;
mod pass;
mod playlist;
mod pls;
mod query;
mod reference;
mod renamed;
mod resumed;
mod retag;
mod scan;
mod schema;
mod scrobble;
mod search;
mod share;
mod sheet;
mod songs;
mod spelling;
mod statistics;
mod stem;
mod store;
mod studies;
mod suggest;
mod sung;
mod supply;
mod take_in;
mod undo;
mod vaulted;
mod volumes;
mod watch;
mod xspf;

pub use resonate_analysis::{Cutoff, LossyGuess, Study, Verdict};
pub use resonate_codec::{
    Codec, CoverArt, Drawing, FileTags, Hinting, ImageFormat, Pictured, Picturing, Popularity,
    Raster, Rated, Sources, StandIn, StoodIn, TagEdit, TagField, TagSet, TagSink, TagSource,
    Tagged,
};
pub use resonate_core::{Chromaprint, Isrc, Link, Mbid, Relation, Service, TextEncoding};
pub use resonate_vault::{
    Encoding, Form, HeldFile, Holdings, Keeping, Kept as KeptInVault, KeptCover,
    Refusal as VaultRefusal, Taking, Vault, VaultFiles, VaultKey,
};

#[cfg(fuzzing)]
pub fn read_a_playlist_sheet(text: &str) {
    let _ = crate::sheet::parse(text, std::path::Path::new("/music"));
}

pub use crate::{
    db::{CatalogStamp, Library, WrittenElsewhere},
    deleted::Deleted,
    elsewhere::{
        ALBUMS_FOUND_ELSEWHERE_AT_MOST, ARTISTS_FOUND_ELSEWHERE_AT_MOST, AlbumFound, ArtistFound,
        Covering, FOUND_ELSEWHERE_AT_MOST, Found, Performer, SongsAsked, Sung, Uncovered,
        albums_still_answering, artists_still_answering, asks_elsewhere,
        in_the_order_worth_offering, songs_asked, still_answering, weighed_for,
    },
    enrich::{
        Certainty, EnrichOptions, EnrichProgress, EnrichStats, EnrichSummary, Fruitless,
        REFRESH_AFTER, REFRESH_SPREAD, REFUSED_AGAIN_AFTER, RETRY_AFTER, Sought, WAITS, Waits,
    },
    error::{
        EncodedColumn, Error, FieldName, LayoutFault, MoveOp, OrderedColumn, PlaylistName, Result,
        SchemaFingerprint, StoreOp, TagName,
    },
    filed::DeliveryFolder,
    filters::{MinimumLength, MusicExtensions, MusicFilters},
    fingerprint::{Fingerprinters, Fingerprints, NoFingerprints, Printed, Recognition, Sounded},
    history::{Aged, HistoryKept},
    import::{
        ImportOptions, ImportPlan, ImportProgress, ImportStats, ImportSummary, Passed, Passing,
        Vaulted, Wanted,
    },
    learning::Learning,
    linked::{
        AlbumLink, AlbumNames, ArtistLink, Barcode, FollowedLink, FollowedPlaylist, LinkNames,
        Linked, LinkedPlaylist, ListedSong, PlaylistLink, SongLink, is_a_followed_link,
    },
    meant::{ByArtist, Meant},
    model::{
        Album, AlbumToAsk, Artist, ArtistDetail, ArtistToAsk, ArtistTotals, Counted, CoverSource,
        Cut, Exported, Favoured, HeldMedium, HeldReleaseTrack, Imported, KeptCorrection, KeptIndex,
        KeptLyrics, Listen, Measured, Missing, MissingTrack, NamedPlaylist, Playing, Playlist,
        PlaylistEntry, PlaylistFormat, PortraitWanted, Pruned, ReleaseDetail, Released, Track,
        TrackToAsk, Unfinished, UnheldRelease, VaultObject, Want,
    },
    organise::{
        Companion, DEFAULT_LAYOUT, Field, Layout, Move, OrganiseOptions, OrganiseProgress,
        OrganiseStats, OrganiseSummary, Plan, Refusal, Refused, Sidecar,
    },
    pass::{
        Cancelling, EnrichHandle, ImportHandle, OrganiseHandle, PassHandle, PassKind, PollHandle,
        RetagHandle, ScanHandle, TakeInHandle,
    },
    playlist::Filled,
    query::{
        AlbumOrder, AlbumQuery, ArtistOrder, ArtistQuery, Direction, Kept, PlaylistOrder, RowOrder,
        SavedQuery, SearchResults, SortOrder, TrackQuery,
    },
    reference::{
        AlbumMatch, ArtistMatch, ArtistPressings, ArtistProfile, ArtistRelease, BarcodeMatch,
        Credit, Discography, Genre, GroupAsked, GroupMatch, GroupRelease, Issued, LifeSpan,
        LookupOp, LyricDetail, LyricText, LyricsAsked, Medium, Recording, RecordingAsked,
        RecordingMatch, RecordingRelease, Reference, Release, ReleaseAsked, ReleaseGroup,
        ReleaseMatch, ReleaseTrack, StreamAsked, Wording, apple_music_urls, deezer_urls,
        may_be_pictured, portrait_urls, soundcloud_urls, spotify_urls, wikidata_urls,
        wikipedia_urls,
    },
    renamed::NamesMoved,
    retag::{
        PassedOver, RetagOptions, RetagProgress, RetagStats, RetagSummary, Retagging, Unwritten,
        Written,
    },
    scan::{Failure, Failures, ScanOptions, ScanProgress, ScanStats, ScanSummary},
    scrobble::{
        Billed, LOVES_TOLD_AT_ONCE, LastfmSession, LastfmSignIn, ListeningService, Love, Loved,
        LovedNames, LovesTold, SUBMITTED_AT_ONCE, Scrobble, Scrobbler, Scrobblers, Submitted,
        TokenHeld,
    },
    search::{
        Asked, Clause, ClockUnit, Column, Compare, Condition, Grain, Lit, Reach, Search, Shape,
        Term, Word,
    },
    share::Shared,
    songs::AlbumNotHeld,
    spelling::Spellings,
    statistics::{Day, Listened, MostListened, Statistics, Window},
    store::folded_letters,
    studies::{Agreement, HEARD_AT_LEAST, Heard, HeardAs, Studied, StudiedTrack, StudyFilter},
    suggest::{Kind as SuggestionKind, PICTURED_BY_AT_MOST, Reason, Suggestion},
    sung::{BETTERED_AFTER, LyricsAhead, MISSED_AGAIN_AFTER},
    supply::{
        ANSWERS_WITHIN, POLL_AGAIN_AFTER, PollOptions, PollProgress, PollStats, PollSummary,
        RETRY_WAITS, TRIES_BEFORE_GIVING_UP,
    },
    take_in::{
        Dropped, Landed, Looks, Passed as TakenPassed, Passing as TakenPassing, TakeInOptions,
        TakeInProgress, TakeInStats, TakeInSummary, take_in, weigh,
    },
    undo::{Edit, Undoable},
    watch::RootsWatch,
};
