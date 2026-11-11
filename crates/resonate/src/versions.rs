use resonate_engine::{QueueItem, Unclaimed};
use resonate_library::{Library, Played};

pub(crate) fn played_as_chosen(library: Option<&Library>, items: Vec<QueueItem>) -> Vec<QueueItem> {
    let Some(library) = library else {
        return items;
    };
    let queued: Vec<_> = items.iter().map(|item| item.id).collect();
    let chosen = match library.chosen_among(&queued) {
        Ok(chosen) => chosen,
        Err(error) => {
            tracing::warn!(%error, "the versions chosen could not be read");
            return items;
        }
    };
    if chosen.is_empty() {
        return items;
    }

    let mut minting = Unclaimed::beside(&items);
    items
        .into_iter()
        .map(
            |item| match chosen.in_place_of(item.id, &item.location, item.span) {
                Some(Played::Held { track, cut }) => QueueItem {
                    id: *track,
                    location: cut.location.clone(),
                    span: cut.span,
                },
                Some(Played::Served(location)) => {
                    QueueItem::whole(minting.mint(), location.clone())
                }
                None => item,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use resonate_core::{MediaLocation, TrackId};
    use resonate_library::{Cut, Version};

    use super::*;
    use crate::{
        fixtures::{self, ARTIST, TITLE, tagged_wave},
        mpris::claimed_by,
        playlists::queue_items,
    };

    #[test]
    fn a_song_with_a_version_chosen_is_played_as_that_version_from_the_terminal_and_a_playlist() {
        let root = fixtures::scratch("versions-chosen");
        let song = root.join("echoes.wav");
        let copy = root.join("elsewhere.wav");
        let other = root.join("time.wav");
        fs::write(&song, tagged_wave(&[(TITLE, "Echoes"), (ARTIST, "Ada")])).expect("a song");
        fs::write(&copy, tagged_wave(&[(TITLE, "Echoes"), (ARTIST, "Ada")])).expect("a copy");
        fs::write(&other, tagged_wave(&[(TITLE, "Time"), (ARTIST, "Ada")])).expect("a song");
        let library = fixtures::scanned(&root);

        let held = |path| {
            library
                .track_at(path, None)
                .expect("the catalog read")
                .expect("the file is held")
                .id
        };
        let (song_id, copy_id, other_id) = (held(&song), held(&copy), held(&other));
        let served = MediaLocation::from_uri("tidal:track/7").expect("a served song");
        library
            .choose_version(song_id, &Version::Held(copy_id))
            .expect("a version chosen");
        library
            .choose_version(other_id, &Version::Served(served.clone()))
            .expect("a version chosen");
        let playlist = library.create_playlist("Evening").expect("a playlist");
        library
            .add_to_playlist(
                playlist,
                &[
                    Cut::whole(MediaLocation::local(&song)),
                    Cut::whole(MediaLocation::local(&other)),
                ],
            )
            .expect("the rows appended");

        let named = claimed_by(
            Some(&library),
            vec![
                QueueItem::whole(TrackId::MAX, MediaLocation::local(&song)),
                QueueItem::whole(TrackId::MAX, MediaLocation::local(&other)),
            ],
        );
        let played = played_as_chosen(Some(&library), named.clone());
        let listed = played_as_chosen(
            Some(&library),
            queue_items(&library.playlist_entries(playlist, None).expect("the rows")),
        );
        let untouched = played_as_chosen(None, named.clone());
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        for queued in [&played, &listed] {
            assert_eq!(queued[0].id, copy_id);
            assert_eq!(queued[0].location, MediaLocation::local(&copy));
            assert_eq!(queued[1].location, served);
            assert_ne!(queued[1].id, other_id);
        }
        assert_eq!(untouched, named);
    }
}
