mod delivery;
mod error;
mod identity;
mod in_place;
mod listing;
mod pacing;
mod provider;
mod sign_in;
mod song_catalog;

pub use resonate_core::titles::{Rating, rating_in_the_title};
pub use song_catalog::{SongCatalog, stream_extension};

pub use crate::{
    delivery::{
        Delivered, Delivery, EntityTag, Extension, Held, Modified, Obtained, Opened, Opening,
        Reopened, Resumed, Validators,
    },
    error::{
        Error, ProviderOp, Result, THE_SIGN_IN_LAPSED, THE_SUBSCRIPTION_LAPSED, Unfollowed,
        WhyAway, is_a_page,
    },
    identity::Identity,
    in_place::{
        Album, AlbumSongs, Artist, ArtistAlbums, Found, Id, Mix, MixSongs, Named, PlaysInPlace,
        SeekingBytes, Song, Streamed, found_exactly, not_offered, songs_of_an_artist,
    },
    listing::{
        A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY, Choosing, LENGTHS_AGREE_WITHIN, Listed, Taken, led_by,
        spelt_as,
    },
    pacing::Pacing,
    provider::{
        ANSWERED_ALONE_WITHIN, Answer, Asking, Away, Provider, Providers, TURNED_TO_APART,
        Unprovided, WAITED_ON_AFTER_AN_OFFER,
    },
    sign_in::{Authorizing, Client, RefreshToken, SignsIn},
};
