use std::sync::Arc;

use zbus::{
    fdo, interface,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, OwnedObjectPath},
};

use crate::{
    Opened, PlaylistInfo, PlaylistOrder,
    host::Playlists,
    track::{no_playlist, playlist_of, playlist_path},
};

pub(crate) type Listed = (OwnedObjectPath, String, String);

pub(crate) struct PlaylistsInterface {
    pub(crate) playlists: Arc<dyn Playlists>,
}

#[interface(name = "org.mpris.MediaPlayer2.Playlists")]
impl PlaylistsInterface {
    fn activate_playlist(&self, playlist_id: ObjectPath<'_>) -> fdo::Result<()> {
        let playlist = playlist_of(playlist_id.as_str()).ok_or_else(|| {
            fdo::Error::InvalidArgs("that path names no playlist this build holds".to_owned())
        })?;

        match self.playlists.activate(playlist) {
            Opened::Accepted => Ok(()),
            Opened::Refused => Err(fdo::Error::Failed(
                "that playlist could not be played".to_owned(),
            )),
        }
    }

    fn get_playlists(
        &self,
        index: u32,
        max_count: u32,
        order: &str,
        reverse_order: bool,
    ) -> fdo::Result<Vec<Listed>> {
        let order = PlaylistOrder::named(order)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("{order} is not in Orderings")))?;

        Ok(self
            .playlists
            .listing(
                order,
                reverse_order,
                index as usize,
                Some(max_count as usize),
            )
            .into_iter()
            .map(listed)
            .collect())
    }

    #[zbus(property)]
    fn playlist_count(&self) -> u32 {
        u32::try_from(self.playlists.count()).unwrap_or(u32::MAX)
    }

    #[zbus(property)]
    fn orderings(&self) -> Vec<String> {
        PlaylistOrder::ALL
            .into_iter()
            .map(|order| order.name().to_owned())
            .collect()
    }

    #[zbus(property)]
    fn active_playlist(&self) -> (bool, Listed) {
        match self.playlists.playing() {
            Some(playing) => (true, listed(playing)),
            None => (false, (no_playlist(), String::new(), String::new())),
        }
    }

    #[zbus(signal)]
    pub(crate) async fn playlist_changed(
        emitter: &SignalEmitter<'_>,
        playlist: Listed,
    ) -> zbus::Result<()>;
}

pub(crate) fn listed(playlist: PlaylistInfo) -> Listed {
    (playlist_path(playlist.id), playlist.name, String::new())
}

pub(crate) fn unheard_of(before: &[PlaylistInfo], row: &PlaylistInfo) -> bool {
    before
        .iter()
        .find(|held| held.id == row.id)
        .is_none_or(|held| held.name != row.name)
}

#[cfg(test)]
mod tests {
    use resonate_core::PlaylistId;

    use super::{PlaylistInfo, unheard_of};

    fn listing(named: &[(u64, &str)]) -> Vec<PlaylistInfo> {
        named
            .iter()
            .map(|(id, name)| PlaylistInfo {
                id: PlaylistId::new(*id).expect("playlist ids start at one"),
                name: (*name).to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_playlist_the_listing_already_held_under_that_name_is_announced_as_nothing() {
        let held = listing(&[(1, "Evening jazz"), (2, "Workout")]);

        assert!(held.iter().all(|row| !unheard_of(&held, row)));
    }

    #[test]
    fn a_playlist_that_arrived_is_announced_though_another_left_beside_it() {
        let before = listing(&[(1, "Evening jazz"), (2, "Workout")]);
        let now = listing(&[(1, "Evening jazz"), (3, "Road trip")]);

        assert_eq!(
            now.iter()
                .filter(|row| unheard_of(&before, row))
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Road trip"],
            "a listing the same length carried a playlist nothing announced"
        );
    }

    #[test]
    fn a_playlist_renamed_is_announced_under_the_name_it_now_holds() {
        let before = listing(&[(1, "Evening jazz")]);
        let now = listing(&[(1, "Late night jazz")]);

        assert!(unheard_of(&before, &now[0]));
    }
}
