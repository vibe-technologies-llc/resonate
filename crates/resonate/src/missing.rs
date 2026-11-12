use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, SystemTime},
};

use resonate_core::{AlbumId, ArtistId};
use resonate_library::{
    Library, Missing, MissingTrack, PassKind, PollOptions, TRIES_BEFORE_GIVING_UP, UnheldRelease,
    Want, folded_letters,
};
use resonate_providers::Providers;

use crate::{
    config::Config,
    error::{ArtistName, Error, Result},
    online,
    online::Asking,
    passes::{finished, until_told},
    playlist::{ago, played},
    providers,
    table::Table,
    words,
    words::columns,
};

pub(crate) fn wants(library: &Library) -> Result<()> {
    let wanted = library.wants()?;
    if wanted.is_empty() {
        said!("{}", words::main::nothing_wanted());
        return Ok(());
    }

    let mut table = Table::new(vec![
        columns::title(),
        columns::artist(),
        columns::album(),
        columns::state(),
        columns::wanted(),
        columns::tried(),
        columns::offered(),
        columns::links(),
    ]);
    let now = SystemTime::now();
    for want in &wanted {
        table.push(vec![
            want.title.clone(),
            want.artist.clone().unwrap_or_default(),
            want.album_title.clone(),
            standing_of(want, now),
            ago(Some(want.wanted)),
            ago(want.tried),
            want.offered.clone().unwrap_or_else(|| "-".to_owned()),
            linked_through(want),
        ]);
    }

    said_on!("{}", table.render());
    Ok(())
}

pub(crate) fn standing_of(want: &Want, now: SystemTime) -> String {
    if want.held.is_some() {
        return words::main::held().to_owned();
    }
    if want.gave_up() {
        return words::main::gave_up(TRIES_BEFORE_GIVING_UP);
    }
    let due = want
        .due_at()
        .and_then(|at| at.duration_since(now).ok())
        .filter(|left| !left.is_zero());
    let waiting = |said: String| match due {
        Some(left) => words::main::again_in(lasting_for(left), said),
        None => words::main::due_again(said),
    };

    match (want.offered.is_some(), want.misses) {
        (true, _) => waiting(words::main::offered().to_owned()),
        (false, 0) if want.tried.is_none() => words::main::due().to_owned(),
        (false, 0) => waiting(words::main::unanswered().to_owned()),
        (false, misses) => waiting(words::main::no_match(misses, TRIES_BEFORE_GIVING_UP)),
    }
}

fn lasting_for(left: Duration) -> String {
    let minutes = left.as_secs().div_ceil(60);
    match (minutes / 60, minutes % 60) {
        (0, minutes) => words::main::lasting_minutes(minutes),
        (hours, 0) => words::main::lasting_hours(hours),
        (hours, minutes) => words::main::lasting_hours_and_minutes(hours, minutes),
    }
}

fn linked_through(want: &Want) -> String {
    let mut named: Vec<&str> = Vec::with_capacity(want.links.len());
    for link in &want.links {
        let name = link.service.name();
        if !named.contains(&name) {
            named.push(name);
        }
    }
    named.join(", ")
}

pub(crate) fn held_artist(library: &Library, named: &str) -> Result<ArtistId> {
    library
        .artist_named(named)?
        .ok_or_else(|| Error::NoSuchArtist(ArtistName::new(named)))
}

pub(crate) fn read_the_rest_of(
    library: &Library,
    config: &Config,
    named: &str,
    artist: ArtistId,
) -> Result<()> {
    let reference = online::reference_asked_for(config, Asking::ForTheListener)?;
    let read = library.read_the_rest_of(artist, reference.as_ref())?;
    said!("{}", words::main::read_the_rest(named, read));
    Ok(())
}

fn dismissed_beside(dismissed: Missing) -> String {
    match dismissed.tracks + dismissed.releases {
        0 => String::new(),
        many => format!(" · {}", words::main::dismissed_beside(many)),
    }
}

pub(crate) fn brought_back(brought: Missing) -> String {
    words::main::brought_back(brought.releases, brought.tracks)
}

pub(crate) fn missing(library: &Library, artist: Option<&str>) -> Result<()> {
    let mut tracks = library.missing_tracks(None, None)?;
    let mut releases = library.unheld_releases(None, None)?;
    let (tracks_missing, releases_unheld) = match artist {
        None => {
            let counted = library.missing_counted(None)?;
            (counted.tracks, counted.releases)
        }
        Some(named) => {
            let wanted = folded_name(named);
            tracks.retain(|track| {
                track
                    .owner
                    .as_deref()
                    .is_some_and(|owner| folded_name(owner) == wanted)
            });
            releases.retain(|release| folded_name(&release.artist_name) == wanted);
            (tracks.len() as u64, releases.len() as u64)
        }
    };

    if tracks.is_empty() && releases.is_empty() {
        match artist {
            None => said!("{}", words::main::nothing_missing()),
            Some(named) => said!("{}", words::main::nothing_of_missing(named)),
        }
        return Ok(());
    }

    let discs_of = discs_per_album(&tracks);
    let dismissed = match artist {
        None => library.dismissed()?,
        Some(_) => Missing::default(),
    };
    said!(
        "{}{}",
        words::main::missing_across(discs_of.len(), releases_unheld, tracks_missing),
        dismissed_beside(dismissed)
    );

    said!();
    said!("{}", words::main::tracks_missing());
    if tracks.is_empty() {
        said!("{}", words::main::every_track_held());
    } else {
        said_on!("{}", missing_tracks_table(&tracks, &discs_of).render());
    }

    said!();
    said!("{}", words::main::releases_not_held());
    if releases.is_empty() {
        said!("{}", words::main::every_release_held());
    } else {
        said_on!("{}", unheld_releases_table(&releases).render());
    }
    if let Some(named) = artist
        && let Some(id) = library.artist_named(named)?
        && let Some(detail) = library.artist_detail(id)?
        && detail.releases_unread > 0
    {
        said!(
            "{}",
            words::main::credits_unread(named, detail.releases_unread)
        );
    }
    Ok(())
}

pub(crate) fn folded_name(name: &str) -> String {
    folded_letters(name.trim())
}

fn discs_per_album(tracks: &[MissingTrack]) -> BTreeMap<AlbumId, BTreeSet<u32>> {
    let mut discs: BTreeMap<AlbumId, BTreeSet<u32>> = BTreeMap::new();
    for track in tracks {
        discs.entry(track.album).or_default().insert(track.disc);
    }
    discs
}

fn missing_tracks_table(
    tracks: &[MissingTrack],
    discs_of: &BTreeMap<AlbumId, BTreeSet<u32>>,
) -> Table {
    let mut table = Table::new(vec![
        columns::album(),
        columns::artist(),
        columns::disc(),
        "#",
        columns::title(),
        columns::length(),
        columns::wanted(),
    ]);
    for track in tracks {
        let on_several_discs = discs_of
            .get(&track.album)
            .is_some_and(|discs| discs.len() > 1);
        table.push(vec![
            track.album_title.clone(),
            track
                .artist
                .clone()
                .or_else(|| track.owner.clone())
                .unwrap_or_else(|| "-".to_owned()),
            if on_several_discs {
                track.disc.to_string()
            } else {
                String::new()
            },
            track
                .number
                .clone()
                .unwrap_or_else(|| track.position.to_string()),
            track.title.clone(),
            track
                .length
                .map_or_else(|| "-".to_owned(), |length| played(Some(length))),
            if track.want.is_some() {
                words::main::yes()
            } else {
                "-"
            }
            .to_owned(),
        ]);
    }
    table
}

fn unheld_releases_table(releases: &[UnheldRelease]) -> Table {
    let mut table = Table::new(vec![
        columns::artist(),
        columns::release(),
        columns::kind(),
        columns::first_released(),
    ]);
    for release in releases {
        table.push(vec![
            release.artist_name.clone(),
            release.title.clone(),
            release.kind.clone().unwrap_or_else(|| "-".to_owned()),
            release
                .first_released
                .clone()
                .unwrap_or_else(|| "-".to_owned()),
        ]);
    }
    table
}

pub(crate) fn poll(
    library: &Library,
    providers: Providers,
    keeping: &providers::Keeping,
    again: bool,
) -> Result<()> {
    if !providers.has_a_source() {
        said!("{}", words::main::no_provider());
    }
    let options = if again {
        PollOptions::ASKING_EVERY_WANT
    } else {
        PollOptions::default()
    };
    let summary = until_told(library.poll(Arc::new(providers), options)?)?;
    let stats = summary.stats;
    said!(
        "{}",
        words::main::polled(
            stats.asked,
            stats.kept,
            stats.late,
            stats.nothing,
            stats.offered,
            stats.refused,
            stats.unkept,
        )
    );
    if summary.cancelled {
        said!("{}", words::main::cancelled());
    }
    finished(PassKind::Poll, summary.cancelled)?;
    keeping.unkept().map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn a_want_says_how_it_is_getting_on_and_when_it_is_asked_again() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let tried = Some(now - Duration::from_secs(60));

        assert_eq!(standing_of(&wanted_at(None, 0), now), "due");
        assert_eq!(
            standing_of(&wanted_at(tried, 2), now),
            "no match 2 of 6, again in 1m"
        );
        assert_eq!(
            standing_of(&wanted_at(tried, 1), now),
            "no match 1 of 6, due"
        );
        assert_eq!(
            standing_of(&wanted_at(tried, TRIES_BEFORE_GIVING_UP), now),
            "gave up after 6 tries"
        );
        assert_eq!(
            standing_of(
                &Want {
                    offered: Some("file:///music/a.flac".to_owned()),
                    ..wanted_at(tried, 0)
                },
                now
            ),
            "offered, again in 5h 59m"
        );
        assert_eq!(
            standing_of(
                &Want {
                    held: Some(resonate_core::TrackId::new(3).expect("an id")),
                    ..wanted_at(tried, 0)
                },
                now
            ),
            "held"
        );
    }

    fn wanted_at(tried: Option<SystemTime>, misses: u32) -> Want {
        Want {
            id: resonate_core::WantId::new(1).expect("an id"),
            release_track: resonate_core::ReleaseTrackId::new(1).expect("an id"),
            album: resonate_core::AlbumId::new(1).expect("an id"),
            album_title: "Heroes Tonight".to_owned(),
            title: "Heroes Tonight".to_owned(),
            artist: Some("Janji".to_owned()),
            recording: None,
            track: None,
            release: None,
            isrcs: Vec::new(),
            length: None,
            disc: 1,
            position: 1,
            wanted: SystemTime::UNIX_EPOCH,
            tried,
            offered: None,
            misses,
            held: None,
            links: Vec::new(),
            release_links: Vec::new(),
        }
    }
}
