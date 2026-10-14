#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SyncedSongs {
    pub favourites: bool,
    pub playlists: bool,
    pub everything: bool,
}

impl SyncedSongs {
    pub const FAVOURITES: &str = "favourites";
    pub const PLAYLISTS: &str = "playlists";
    pub const EVERYTHING: &str = "everything";

    pub fn parse(named: &[&str]) -> Option<Self> {
        let mut songs = Self::default();
        for name in named {
            match name.trim() {
                Self::FAVOURITES => songs.favourites = true,
                Self::PLAYLISTS => songs.playlists = true,
                Self::EVERYTHING => songs.everything = true,
                _ => return None,
            }
        }
        Some(songs)
    }

    pub fn named(self) -> Vec<&'static str> {
        [
            (self.favourites, Self::FAVOURITES),
            (self.playlists, Self::PLAYLISTS),
            (self.everything, Self::EVERYTHING),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_songs_followed_read_back_as_named() {
        let songs = SyncedSongs::parse(&["playlists", " favourites"]).expect("read");

        assert!(songs.favourites && songs.playlists && !songs.everything);
        assert_eq!(SyncedSongs::parse(&songs.named()), Some(songs));
        assert_eq!(SyncedSongs::parse(&[]), Some(SyncedSongs::default()));
        assert_eq!(SyncedSongs::parse(&["all"]), None);
    }
}
