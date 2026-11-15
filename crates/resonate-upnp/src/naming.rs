use crate::{catalog::Shelf, words};

pub(crate) fn server_name() -> &'static str {
    words::offered::server_name()
}

pub(crate) fn title_of(shelf: Shelf) -> &'static str {
    match shelf {
        Shelf::Artists | Shelf::Artist(_) => words::offered::artists(),
        Shelf::Albums | Shelf::Album(_) => words::offered::albums(),
        Shelf::Songs => words::offered::songs(),
        Shelf::Playlists | Shelf::Playlist(_) => words::offered::playlists(),
    }
}
