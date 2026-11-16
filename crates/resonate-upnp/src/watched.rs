use std::hash::{DefaultHasher, Hash, Hasher};

use parking_lot::Mutex;

use crate::{
    catalog::{Catalog, Object, Page, Shelf},
    directory::BROWSED_AT_MOST,
};

pub(crate) const WATCHED_AT_MOST: usize = 64;

const PRINTED_AT_MOST: u32 = BROWSED_AT_MOST;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Print {
    Whole(u64),
    Unprinted,
}

#[derive(Clone, Copy, Debug)]
struct Watch {
    seen: u32,
    print: Option<Print>,
    used: u64,
}

#[derive(Default)]
struct Held {
    watches: Vec<(Object, Watch)>,
    used: u64,
}

#[derive(Default)]
pub(crate) struct Watched {
    held: Mutex<Held>,
}

impl Watched {
    pub(crate) fn watch(&self, object: Object, revision: u32) {
        if matches!(object, Object::Song(_)) {
            return;
        }
        let mut held = self.held.lock();
        held.used += 1;
        let used = held.used;
        if let Some((_, watch)) = held
            .watches
            .iter_mut()
            .find(|(watched, _)| *watched == object)
        {
            watch.used = used;
            return;
        }
        if held.watches.len() >= WATCHED_AT_MOST
            && let Some(least) = held
                .watches
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, watch))| watch.used)
                .map(|(at, _)| at)
        {
            held.watches.swap_remove(least);
        }
        held.watches.push((
            object,
            Watch {
                seen: revision,
                print: None,
                used,
            },
        ));
    }

    pub(crate) fn print_new<C: Catalog>(&self, catalog: &C, revision: u32) {
        let unprinted: Vec<Object> = self
            .held
            .lock()
            .watches
            .iter()
            .filter(|(_, watch)| watch.print.is_none() && watch.seen == revision)
            .map(|(object, _)| *object)
            .collect();
        for object in unprinted {
            let Some(print) = print_of(catalog, object) else {
                continue;
            };
            if catalog.revision() != revision {
                return;
            }
            if let Some((_, watch)) = self
                .held
                .lock()
                .watches
                .iter_mut()
                .find(|(watched, watch)| *watched == object && watch.seen == revision)
            {
                watch.print = Some(print);
            }
        }
    }

    pub(crate) fn changed<C: Catalog>(&self, catalog: &C, revision: u32) -> Vec<(Object, u32)> {
        let stale: Vec<(Object, Watch)> = self
            .held
            .lock()
            .watches
            .iter()
            .filter(|(_, watch)| watch.seen != revision)
            .copied()
            .collect();
        let mut changed = Vec::new();
        for (object, before) in stale {
            let now = print_of(catalog, object);
            let moved = match (before.print, now) {
                (Some(Print::Whole(was)), Some(Print::Whole(is))) => was != is,
                _ => true,
            };
            if moved {
                changed.push((object, revision));
            }
            let mut held = self.held.lock();
            let Some(at) = held
                .watches
                .iter()
                .position(|(watched, watch)| *watched == object && watch.seen == before.seen)
            else {
                continue;
            };
            match now {
                Some(print) => {
                    let watch = &mut held.watches[at].1;
                    watch.seen = revision;
                    watch.print = Some(print);
                }
                None => {
                    held.watches.swap_remove(at);
                }
            }
        }
        changed
    }

    #[cfg(test)]
    fn watching(&self) -> Vec<Object> {
        self.held
            .lock()
            .watches
            .iter()
            .map(|(object, _)| *object)
            .collect()
    }
}

fn print_of<C: Catalog>(catalog: &C, object: Object) -> Option<Print> {
    match printed(catalog, object) {
        Ok(print) => print,
        Err(error) => {
            tracing::debug!(%error, %object, "a container watched for changes could not be read, so it is told as changed");
            Some(Print::Unprinted)
        }
    }
}

fn printed<C: Catalog>(catalog: &C, object: Object) -> Result<Option<Print>, C::Failure> {
    let mut hasher = DefaultHasher::new();
    match object {
        Object::Root => {
            for shelf in Shelf::TOP {
                catalog
                    .shelved(shelf, Page::counting())?
                    .map(|shelved| shelved.total)
                    .hash(&mut hasher);
            }
        }
        Object::Shelf(shelf) => {
            if !shelf.is_top() {
                let Some(held) = catalog.shelf(shelf)? else {
                    return Ok(None);
                };
                held.hash(&mut hasher);
            }
            let Some(shelved) = catalog.shelved(shelf, Page::first(PRINTED_AT_MOST))? else {
                return Ok(None);
            };
            if shelved.held.len() < shelved.total as usize {
                return Ok(Some(Print::Unprinted));
            }
            shelved.hash(&mut hasher);
        }
        Object::Song(_) => return Ok(Some(Print::Unprinted)),
    }
    Ok(Some(Print::Whole(hasher.finish())))
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::atomic::{AtomicU32, Ordering},
    };

    use resonate_core::{AlbumId, PlaylistId, TrackId};

    use super::*;
    use crate::catalog::{
        Entry, Held as Listed, Kind, Opened, Picture, Query, Searched, Shelved, Song, Within,
    };

    struct Changing {
        listed: Mutex<Vec<u32>>,
        revision: AtomicU32,
    }

    impl Changing {
        fn new() -> Self {
            Self {
                listed: Mutex::new(vec![1, 2]),
                revision: AtomicU32::new(1),
            }
        }

        fn change(&self, listed: Vec<u32>) {
            *self.listed.lock() = listed;
            self.revision.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn playlist() -> PlaylistId {
        PlaylistId::new(1).expect("an id")
    }

    fn album() -> AlbumId {
        AlbumId::new(1).expect("an id")
    }

    impl Catalog for Changing {
        type Failure = io::Error;

        fn searched(&self, _: Kind, _: Within, _: &Query, _: Page) -> io::Result<Searched> {
            Ok(Searched::Unanswered)
        }

        fn shelved(&self, shelf: Shelf, page: Page) -> io::Result<Option<Shelved>> {
            let listed = self.listed.lock().clone();
            let held = |count: usize| Shelved {
                held: (0..count.min(page.most as usize))
                    .map(|at| {
                        Entry::Shelf(Listed {
                            shelf: Shelf::Album(album()),
                            title: at.to_string(),
                            artist: None,
                            children: 0,
                            cover: None,
                        })
                    })
                    .collect(),
                total: count as u32,
            };
            Ok(match shelf {
                Shelf::Playlist(id) if id == playlist() => Some(Shelved {
                    held: listed
                        .iter()
                        .map(|title| {
                            Entry::Shelf(Listed {
                                shelf: Shelf::Album(album()),
                                title: title.to_string(),
                                artist: None,
                                children: 0,
                                cover: None,
                            })
                        })
                        .collect(),
                    total: listed.len() as u32,
                }),
                Shelf::Songs => Some(held(PRINTED_AT_MOST as usize + listed.len())),
                Shelf::Artists | Shelf::Albums | Shelf::Playlists => Some(held(1)),
                Shelf::Album(_) | Shelf::Artist(_) | Shelf::Playlist(_) => None,
            })
        }

        fn shelf(&self, shelf: Shelf) -> io::Result<Option<Listed>> {
            Ok(match shelf {
                Shelf::Playlist(id) if id == playlist() => Some(Listed {
                    shelf,
                    title: "Mixed".to_owned(),
                    artist: None,
                    children: self.listed.lock().len() as u32,
                    cover: None,
                }),
                _ => None,
            })
        }

        fn song(&self, _: TrackId) -> io::Result<Option<Song>> {
            Ok(None)
        }

        fn bytes_of(&self, _: TrackId) -> io::Result<Option<Opened>> {
            Ok(None)
        }

        fn cover(&self, _: AlbumId) -> io::Result<Option<Picture>> {
            Ok(None)
        }

        fn revision(&self) -> u32 {
            self.revision.load(Ordering::Acquire)
        }
    }

    #[test]
    fn a_container_watched_is_named_only_once_what_it_holds_has_changed() {
        let catalog = Changing::new();
        let watched = Watched::default();
        let listed = Object::Shelf(Shelf::Playlist(playlist()));
        let albums = Object::Shelf(Shelf::Albums);

        watched.watch(listed, 1);
        watched.watch(albums, 1);
        watched.print_new(&catalog, 1);
        catalog.change(vec![2, 1]);
        let reordered = watched.changed(&catalog, 2);
        let unchanged = watched.changed(&catalog, 2);
        catalog.change(vec![2, 1]);
        let moved_alone = watched.changed(&catalog, 3);

        assert_eq!(reordered, [(listed, 2)]);
        assert!(unchanged.is_empty());
        assert!(moved_alone.is_empty());
    }

    #[test]
    fn a_container_too_large_to_print_or_never_printed_is_named_at_every_change() {
        let catalog = Changing::new();
        let watched = Watched::default();
        let songs = Object::Shelf(Shelf::Songs);
        let listed = Object::Shelf(Shelf::Playlist(playlist()));

        watched.watch(songs, 1);
        watched.print_new(&catalog, 1);
        watched.watch(listed, 1);
        catalog.change(vec![1, 2]);
        let first = watched.changed(&catalog, 2);
        catalog.change(vec![1, 2]);
        let second = watched.changed(&catalog, 3);

        assert_eq!(first, [(songs, 2), (listed, 2)]);
        assert_eq!(second, [(songs, 3)]);
    }

    #[test]
    fn a_container_gone_is_named_once_and_no_longer_watched() {
        let catalog = Changing::new();
        let watched = Watched::default();
        let gone = Object::Shelf(Shelf::Album(album()));

        watched.watch(gone, 1);
        watched.watch(Object::Song(TrackId::new(1).expect("an id")), 1);
        let told = watched.changed(&catalog, 2);

        assert_eq!(told, [(gone, 2)]);
        assert!(watched.watching().is_empty());
    }

    #[test]
    fn a_print_taken_after_the_catalog_moved_on_is_not_kept() {
        let catalog = Changing::new();
        let watched = Watched::default();
        let listed = Object::Shelf(Shelf::Playlist(playlist()));

        watched.watch(listed, 1);
        catalog.change(vec![1, 2]);
        watched.print_new(&catalog, 1);
        let told = watched.changed(&catalog, 2);

        assert_eq!(told, [(listed, 2)]);
    }

    #[test]
    fn the_containers_watched_are_held_to_a_number_the_least_used_leaving_first() {
        let watched = Watched::default();
        let first = Object::Shelf(Shelf::Playlist(playlist()));

        watched.watch(first, 1);
        for raw in 1..=WATCHED_AT_MOST as u64 {
            watched.watch(
                Object::Shelf(Shelf::Album(AlbumId::new(raw).expect("an id"))),
                1,
            );
            watched.watch(first, 1);
        }
        let watching = watched.watching();

        assert_eq!(watching.len(), WATCHED_AT_MOST);
        assert!(watching.contains(&first));
        assert!(!watching.contains(&Object::Shelf(Shelf::Album(album()))));
    }
}
