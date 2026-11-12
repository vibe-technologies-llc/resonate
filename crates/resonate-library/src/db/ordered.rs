use super::rows::read_as;
use crate::{AlbumOrder, ArtistOrder, Direction, SortOrder};

pub(super) struct Reading {
    pub(super) up: &'static str,
    pub(super) down: &'static str,
}

macro_rules! track_ids_rising {
    () => {
        ", tracks.id"
    };
}

macro_rules! track_ids_falling {
    () => {
        ", tracks.id DESC"
    };
}

macro_rules! album_ids_rising {
    () => {
        ", a.id"
    };
}

macro_rules! album_ids_falling {
    () => {
        ", a.id DESC"
    };
}

macro_rules! artist_ids_rising {
    () => {
        ", r.id"
    };
}

macro_rules! artist_ids_falling {
    () => {
        ", r.id DESC"
    };
}

macro_rules! lossless_codecs {
    () => {
        "tracks.codec IN (1, 2, 5, 10, 11, 12)"
    };
}

pub(super) const fn order_by(sort: SortOrder, reading: Direction, ranked: bool) -> &'static str {
    let order = match sort {
        SortOrder::Relevance if ranked => Reading {
            up: concat!("rank", track_ids_rising!()),
            down: concat!("rank DESC", track_ids_falling!()),
        },
        SortOrder::Relevance | SortOrder::AlbumThenTrack => Reading {
            up: concat!(
                "tracks.album_id, tracks.disc_number, tracks.track_number, \
                 tracks.title_filed COLLATE NOCASE",
                track_ids_rising!()
            ),
            down: concat!(
                "tracks.album_id DESC, tracks.disc_number DESC, tracks.track_number DESC, \
                   tracks.title_filed COLLATE NOCASE DESC",
                track_ids_falling!()
            ),
        },
        SortOrder::Title => Reading {
            up: concat!("tracks.title_filed COLLATE NOCASE", track_ids_rising!()),
            down: concat!(
                "tracks.title_filed COLLATE NOCASE DESC",
                track_ids_falling!()
            ),
        },
        SortOrder::Artist => Reading {
            up: concat!(
                "tracks.artist_filed COLLATE NOCASE, tracks.album_id, tracks.disc_number, \
                 tracks.track_number",
                track_ids_rising!()
            ),
            down: concat!(
                "tracks.artist_filed COLLATE NOCASE DESC, tracks.album_id DESC, \
                   tracks.disc_number DESC, tracks.track_number DESC",
                track_ids_falling!()
            ),
        },
        SortOrder::DateAdded => Reading {
            up: concat!("tracks.added", track_ids_falling!()),
            down: concat!("tracks.added DESC", track_ids_rising!()),
        },
        SortOrder::Duration => Reading {
            up: concat!("tracks.duration", track_ids_rising!()),
            down: concat!("tracks.duration DESC", track_ids_falling!()),
        },
        SortOrder::Format => Reading {
            up: concat!(
                lossless_codecs!(),
                ", tracks.codec, tracks.sample_format, tracks.sample_rate, tracks.album_id, \
                 tracks.disc_number, tracks.track_number",
                track_ids_rising!()
            ),
            down: concat!(
                lossless_codecs!(),
                " DESC, tracks.codec DESC, tracks.sample_format DESC, tracks.sample_rate DESC, \
                 tracks.album_id DESC, tracks.disc_number DESC, tracks.track_number DESC",
                track_ids_falling!()
            ),
        },
        SortOrder::Plays => Reading {
            up: concat!(
                "tracks.plays, tracks.title_filed COLLATE NOCASE DESC",
                track_ids_falling!()
            ),
            down: concat!(
                "tracks.plays DESC, tracks.title_filed COLLATE NOCASE",
                track_ids_rising!()
            ),
        },
        SortOrder::Played => Reading {
            up: concat!(
                "tracks.played, tracks.title_filed COLLATE NOCASE DESC",
                track_ids_falling!()
            ),
            down: concat!(
                "tracks.played DESC, tracks.title_filed COLLATE NOCASE",
                track_ids_rising!()
            ),
        },
        SortOrder::Favourited => Reading {
            up: concat!(
                "tracks.favourite, tracks.title_filed COLLATE NOCASE DESC",
                track_ids_falling!()
            ),
            down: concat!(
                "tracks.favourite DESC, tracks.title_filed COLLATE NOCASE",
                track_ids_rising!()
            ),
        },
        SortOrder::PlaysThisMonth => Reading {
            up: concat!(
                heard_this_month!(),
                ", tracks.plays, tracks.title_filed COLLATE NOCASE DESC",
                track_ids_falling!()
            ),
            down: concat!(
                heard_this_month!(),
                " DESC, tracks.plays DESC, tracks.title_filed COLLATE NOCASE",
                track_ids_rising!()
            ),
        },
    };

    read_as(reading, order)
}

pub(super) const fn album_order_by(
    sort: AlbumOrder,
    reading: Direction,
    ranked: bool,
) -> &'static str {
    let order = match sort {
        AlbumOrder::Relevance if ranked => Reading {
            up: concat!(
                "matched.score, ",
                album_filed_as!(),
                " COLLATE NOCASE",
                album_ids_rising!()
            ),
            down: concat!(
                "matched.score DESC, ",
                album_filed_as!(),
                " COLLATE NOCASE DESC",
                album_ids_falling!()
            ),
        },
        AlbumOrder::Relevance | AlbumOrder::Title => Reading {
            up: concat!(album_filed_as!(), " COLLATE NOCASE", album_ids_rising!()),
            down: concat!(
                album_filed_as!(),
                " COLLATE NOCASE DESC",
                album_ids_falling!()
            ),
        },
        AlbumOrder::Artist => Reading {
            up: concat!(
                album_owner_filed_as!(),
                ", a.year, ",
                album_filed_as!(),
                " COLLATE NOCASE",
                album_ids_rising!()
            ),
            down: concat!(
                album_owner_filed_as!(),
                " DESC, a.year DESC, ",
                album_filed_as!(),
                " COLLATE NOCASE DESC",
                album_ids_falling!()
            ),
        },
        AlbumOrder::Year => Reading {
            up: concat!(
                "a.year IS NULL, a.year, ",
                album_filed_as!(),
                " COLLATE NOCASE",
                album_ids_rising!()
            ),
            down: concat!(
                "a.year IS NULL, a.year DESC, ",
                album_filed_as!(),
                " COLLATE NOCASE DESC",
                album_ids_falling!()
            ),
        },
        AlbumOrder::Tracks => Reading {
            up: concat!(
                shown_tracks!(),
                ", ",
                album_filed_as!(),
                " COLLATE NOCASE",
                album_ids_rising!()
            ),
            down: concat!(
                shown_tracks!(),
                " DESC, ",
                album_filed_as!(),
                " COLLATE NOCASE DESC",
                album_ids_falling!()
            ),
        },
        AlbumOrder::Added => Reading {
            up: concat!(
                album_added!(),
                ", ",
                album_filed_as!(),
                " COLLATE NOCASE",
                album_ids_rising!()
            ),
            down: concat!(
                album_added!(),
                " DESC, ",
                album_filed_as!(),
                " COLLATE NOCASE DESC",
                album_ids_falling!()
            ),
        },
        AlbumOrder::Favourited => Reading {
            up: concat!(
                "a.favourite, ",
                album_filed_as!(),
                " COLLATE NOCASE",
                album_ids_rising!()
            ),
            down: concat!(
                "a.favourite DESC, ",
                album_filed_as!(),
                " COLLATE NOCASE DESC",
                album_ids_falling!()
            ),
        },
    };

    read_as(reading, order)
}

pub(super) const fn artist_order_by(
    sort: ArtistOrder,
    reading: Direction,
    ranked: bool,
) -> &'static str {
    let order = match sort {
        ArtistOrder::Relevance if ranked => Reading {
            up: concat!("matched.score, ", filed_as!("r"), artist_ids_rising!()),
            down: concat!(
                "matched.score DESC, ",
                filed_as!("r"),
                " DESC",
                artist_ids_falling!()
            ),
        },
        ArtistOrder::Relevance | ArtistOrder::Name => Reading {
            up: concat!(filed_as!("r"), artist_ids_rising!()),
            down: concat!(filed_as!("r"), " DESC", artist_ids_falling!()),
        },
        ArtistOrder::Albums => Reading {
            up: concat!(shown_albums!(), ", ", filed_as!("r"), artist_ids_rising!()),
            down: concat!(
                shown_albums!(),
                " DESC, ",
                filed_as!("r"),
                " DESC",
                artist_ids_falling!()
            ),
        },
        ArtistOrder::Tracks => Reading {
            up: concat!(shown_tracks!(), ", ", filed_as!("r"), artist_ids_rising!()),
            down: concat!(
                shown_tracks!(),
                " DESC, ",
                filed_as!("r"),
                " DESC",
                artist_ids_falling!()
            ),
        },
        ArtistOrder::Favourited => Reading {
            up: concat!("r.favourite, ", filed_as!("r"), artist_ids_rising!()),
            down: concat!(
                "r.favourite DESC, ",
                filed_as!("r"),
                " DESC",
                artist_ids_falling!()
            ),
        },
    };

    read_as(reading, order)
}
