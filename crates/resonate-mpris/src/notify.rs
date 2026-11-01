use resonate_core::MediaLocation;
use resonate_engine::{Command, TagSet};

pub(crate) const PREVIOUS: &str = "previous";
pub(crate) const PLAY_PAUSE: &str = "play-pause";
pub(crate) const NEXT: &str = "next";
pub(crate) const ACTIONS: [&str; 6] =
    [PREVIOUS, "Previous", PLAY_PAUSE, "Play/Pause", NEXT, "Next"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) summary: String,
    pub(crate) body: String,
    pub(crate) art: Option<String>,
}

impl Shown {
    pub(crate) const fn told(summary: String, body: String, art: Option<String>) -> Self {
        Self { summary, body, art }
    }
}

pub(crate) fn shown(tags: &TagSet, location: &MediaLocation, art: Option<String>) -> Option<Shown> {
    let summary = tags
        .title
        .clone()
        .or_else(|| location.stem().map(|stem| stem.into_owned()))?;

    Some(Shown {
        summary,
        body: underneath(tags),
        art,
    })
}

fn underneath(tags: &TagSet) -> String {
    match (tags.artist.as_deref(), tags.album.as_deref()) {
        (Some(artist), Some(album)) => format!("{artist}\n{album}"),
        (Some(alone), None) | (None, Some(alone)) => alone.to_owned(),
        (None, None) => String::new(),
    }
}

pub(crate) fn pressed(key: &str) -> Option<Command> {
    match key {
        PREVIOUS => Some(Command::Previous),
        PLAY_PAUSE => Some(Command::TogglePlayPause),
        NEXT => Some(Command::Next),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(title: Option<&str>, artist: Option<&str>, album: Option<&str>) -> TagSet {
        TagSet {
            title: title.map(str::to_owned),
            artist: artist.map(str::to_owned),
            album: album.map(str::to_owned),
            ..TagSet::default()
        }
    }

    fn echoes() -> MediaLocation {
        MediaLocation::local("/music/Pink Floyd/Echoes.flac")
    }

    #[test]
    fn a_track_is_told_as_its_title_over_the_artist_and_the_album() {
        let tags = tagged(Some("Echoes"), Some("Pink Floyd"), Some("Meddle"));
        let shown = shown(&tags, &echoes(), Some("file:///tmp/cover.png".to_owned()))
            .expect("a titled track is worth telling");

        assert_eq!(shown.summary, "Echoes");
        assert_eq!(shown.body, "Pink Floyd\nMeddle");
        assert_eq!(shown.art.as_deref(), Some("file:///tmp/cover.png"));
    }

    #[test]
    fn a_track_carrying_no_title_is_told_as_the_file_it_came_out_of() {
        let shown = shown(&TagSet::default(), &echoes(), None).expect("a file names itself");

        assert_eq!(shown.summary, "Echoes");
        assert_eq!(shown.body, "");
    }

    #[test]
    fn one_line_is_told_where_one_of_the_artist_and_the_album_is_all_there_is() {
        let artist = shown(
            &tagged(Some("Echoes"), Some("Pink Floyd"), None),
            &echoes(),
            None,
        )
        .expect("a titled track");
        let album = shown(
            &tagged(Some("Echoes"), None, Some("Meddle")),
            &echoes(),
            None,
        )
        .expect("a titled track");

        assert_eq!(artist.body, "Pink Floyd");
        assert_eq!(album.body, "Meddle");
    }

    #[test]
    fn each_button_a_notification_offers_is_a_transport_command() {
        let keys: Vec<&str> = ACTIONS.iter().step_by(2).copied().collect();

        assert_eq!(keys, vec![PREVIOUS, PLAY_PAUSE, NEXT]);
        assert_eq!(pressed(PREVIOUS), Some(Command::Previous));
        assert_eq!(pressed(PLAY_PAUSE), Some(Command::TogglePlayPause));
        assert_eq!(pressed(NEXT), Some(Command::Next));
        assert_eq!(pressed("default"), None);
    }

    #[test]
    fn a_label_is_carried_beside_every_key_the_desktop_may_press() {
        assert_eq!(ACTIONS.len() % 2, 0);
        for label in ACTIONS.iter().skip(1).step_by(2) {
            assert!(!label.is_empty());
        }
    }
}
