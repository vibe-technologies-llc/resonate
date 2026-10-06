use std::{cell::Cell, time::SystemTime};

use ahash::{AHashMap, AHashSet};

use crate::{Error, Library, Mbid, Reference, Release, Result, songs};

const BROWSED_FROM_GROUPS_DUE: usize = 3;

pub(crate) trait Learner {
    fn take_a_turn(&self) -> bool;

    fn weigh<T>(&self, answered: Result<T>) -> Result<Option<T>>;

    fn landed(&self, group: &Mbid, pressing: Option<&Release>, songs: usize) -> Result<()>;
}

pub(crate) fn learn_the_songs_of(
    library: &Library,
    reference: &dyn Reference,
    due: &songs::SongsDue,
    learner: &impl Learner,
) -> Result<()> {
    let mut browsed = match &due.artist {
        Some(artist) if due.groups.len() >= BROWSED_FROM_GROUPS_DUE => {
            browsed(reference, artist, &due.groups, learner)?
        }
        Some(_) | None => AHashMap::new(),
    };

    for group in &due.groups {
        let now = SystemTime::now();
        if let Some(pressings) = browsed.remove(group) {
            let pressing = songs::pressing_of(pressings);
            let landed = library.land_songs_of(group, pressing.as_ref(), now)?;
            learner.landed(group, pressing.as_ref(), landed)?;
            continue;
        }
        if !library.songs_still_due(group, now)? {
            continue;
        }
        if !learner.take_a_turn() {
            return Ok(());
        }
        match learner.weigh(reference.releases_of_group(group))? {
            Some(pressings) => {
                let pressing = songs::pressing_of(pressings);
                let landed = library.land_songs_of(group, pressing.as_ref(), now)?;
                learner.landed(group, pressing.as_ref(), landed)?;
            }
            None => library.songs_of_refused(group, now)?,
        }
    }
    Ok(())
}

fn browsed(
    reference: &dyn Reference,
    artist: &Mbid,
    due: &[Mbid],
    learner: &impl Learner,
) -> Result<AHashMap<Mbid, Vec<Release>>> {
    let wanted: AHashSet<&Mbid> = due.iter().collect();
    let mut gathered: AHashMap<Mbid, Vec<Release>> = AHashMap::new();
    let mut from = 0;

    loop {
        if !learner.take_a_turn() {
            return Ok(AHashMap::new());
        }
        let Some(page) = learner.weigh(reference.releases_of_artist(artist, from))? else {
            return Ok(AHashMap::new());
        };
        for pressing in page.pressings {
            if let Some(group) = pressing
                .group
                .clone()
                .filter(|group| wanted.contains(group))
            {
                gathered.entry(group).or_default().push(pressing);
            }
        }
        if page.read_to <= from || page.read_to >= page.credited {
            return Ok(gathered);
        }
        let pages_left = (page.credited - page.read_to).div_ceil(page.read_to - from);
        let unseen = due.len().saturating_sub(gathered.len());
        if usize::try_from(pages_left).unwrap_or(usize::MAX) >= unseen {
            return Ok(gathered);
        }
        from = page.read_to;
    }
}

#[derive(Default)]
pub(crate) struct PageLearning {
    pub(crate) landed: Cell<usize>,
}

impl Learner for PageLearning {
    fn take_a_turn(&self) -> bool {
        true
    }

    fn weigh<T>(&self, answered: Result<T>) -> Result<Option<T>> {
        match answered {
            Ok(value) => Ok(Some(value)),
            Err(Error::Refused { op, status }) => {
                tracing::warn!(
                    ?op,
                    status,
                    "the songs of an artist's releases were refused"
                );
                Ok(None)
            }
            Err(Error::Unreadable { op } | Error::TooLarge { op, .. }) => {
                tracing::warn!(?op, "the songs of an artist's releases could not be read");
                Ok(None)
            }
            Err(other) => Err(other),
        }
    }

    fn landed(&self, _group: &Mbid, _pressing: Option<&Release>, songs: usize) -> Result<()> {
        self.landed.set(self.landed.get() + songs);
        Ok(())
    }
}
