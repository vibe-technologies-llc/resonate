use resonate_core::ReleaseTrackId;
use resonate_library::{Library, MissingTrack, UnheldRelease, Want};

use crate::{
    Error, Result,
    cli::WantsArgs,
    counted,
    error::{Given, Named},
    folded_name,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Narrowing<'a> {
    pub artist: Option<&'a str>,
    pub album: Option<&'a str>,
}

pub fn run(library: &Library, args: &WantsArgs) -> Result<()> {
    let narrowing = Narrowing {
        artist: args.artist.as_deref(),
        album: args.album.as_deref(),
    };

    match (args.add.as_deref(), args.remove.as_deref()) {
        (Some(title), _) => want(library, title, narrowing),
        (None, Some(title)) => unwant(library, title, narrowing),
        (None, None) => crate::wants(library),
    }
}

fn same(held: &str, asked: &str) -> bool {
    folded_name(held) == folded_name(asked)
}

fn agrees(held: Option<&str>, asked: Option<&str>) -> bool {
    asked.is_none_or(|asked| held.is_some_and(|held| same(held, asked)))
}

fn missing_matching(
    library: &Library,
    title: &str,
    narrowing: Narrowing<'_>,
) -> Result<Vec<MissingTrack>> {
    let found: Vec<MissingTrack> = library
        .missing_tracks(None, None)?
        .into_iter()
        .filter(|track| same(&track.title, title))
        .filter(|track| {
            narrowing.artist.is_none_or(|artist| {
                agrees(track.owner.as_deref(), Some(artist))
                    || agrees(track.artist.as_deref(), Some(artist))
            })
        })
        .filter(|track| agrees(Some(&track.album_title), narrowing.album))
        .collect();

    if found.is_empty() {
        return Err(Error::NothingNamed {
            kind: Named::MissingTrack,
            given: Given::new(title),
        });
    }
    Ok(found)
}

fn shown_of(title: &str, album: &str) -> String {
    format!("{title} on {album}")
}

pub fn want(library: &Library, title: &str, narrowing: Narrowing<'_>) -> Result<()> {
    let found = missing_matching(library, title, narrowing)?;
    let rows: Vec<ReleaseTrackId> = found.iter().map(|track| track.release_track).collect();

    library.want_release_tracks(&rows)?;
    for track in &found {
        said!("wanted {}", shown_of(&track.title, &track.album_title));
    }
    Ok(())
}

pub fn dismiss(library: &Library, title: &str, narrowing: Narrowing<'_>) -> Result<()> {
    let found = missing_matching(library, title, narrowing)?;

    for track in &found {
        library.dismiss_missing(track.release_track)?;
        said!("dismissed {}", shown_of(&track.title, &track.album_title));
    }
    said!(
        "{} dismissed, --bring-back lists them again",
        counted(found.len() as u64, "missing track", "missing tracks")
    );
    Ok(())
}

pub fn dismiss_release(library: &Library, title: &str, artist: Option<&str>) -> Result<()> {
    let found: Vec<UnheldRelease> = library
        .unheld_releases(None, None)?
        .into_iter()
        .filter(|release| same(&release.title, title))
        .filter(|release| agrees(Some(&release.artist_name), artist))
        .collect();

    if found.is_empty() {
        return Err(Error::NothingNamed {
            kind: Named::UnheldRelease,
            given: Given::new(title),
        });
    }
    for release in &found {
        library.dismiss_release(release.artist, &release.mbid)?;
        said!("dismissed {} by {}", release.title, release.artist_name);
    }
    said!(
        "{} dismissed, --bring-back lists them again",
        counted(found.len() as u64, "release", "releases")
    );
    Ok(())
}

pub fn unwant(library: &Library, title: &str, narrowing: Narrowing<'_>) -> Result<()> {
    let found: Vec<Want> = library
        .wants_as_they_stand()?
        .into_iter()
        .filter(|want| same(&want.title, title))
        .filter(|want| agrees(want.artist.as_deref(), narrowing.artist))
        .filter(|want| agrees(Some(&want.album_title), narrowing.album))
        .collect();

    if found.is_empty() {
        return Err(Error::NothingNamed {
            kind: Named::WantedTrack,
            given: Given::new(title),
        });
    }
    for want in &found {
        library.unwant(want.id)?;
        said!(
            "no longer wanted: {}",
            shown_of(&want.title, &want.album_title)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::Duration};

    use clap::Parser as _;
    use resonate_library::{AlbumQuery, Mbid, Medium, Release, ReleaseTrack};

    use super::*;
    use crate::{
        cli::{Cli, Sub},
        fixtures::{ALBUM, ARTIST, TITLE, scanned, scratch, tagged_wave},
    };

    const RELEASE: &str = "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f";

    fn wants_asked(arguments: &[&str]) -> WantsArgs {
        let cli = Cli::try_parse_from(
            ["resonate", "wants"]
                .into_iter()
                .chain(arguments.iter().copied()),
        )
        .expect("a command");
        let Some(Sub::Wants(args)) = cli.command else {
            unreachable!("the command named wants");
        };
        args
    }

    fn row(position: u32, title: &str) -> ReleaseTrack {
        ReleaseTrack {
            position,
            number: position.to_string(),
            title: title.to_owned(),
            artist: None,
            recording: None,
            track: None,
            length: Some(Duration::from_secs(u64::from(position))),
            isrcs: Vec::new(),
            links: Vec::new(),
        }
    }

    fn release_of(titles: &[&str]) -> Release {
        Release {
            id: Mbid::new(RELEASE).expect("a well-formed mbid"),
            group: None,
            title: "Orbits".to_owned(),
            credit: Vec::new(),
            date: None,
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: None,
            disambiguation: None,
            has_front_cover: false,
            links: Vec::new(),
            media: vec![Medium {
                position: 1,
                format: None,
                title: None,
                tracks: titles
                    .iter()
                    .zip(1..)
                    .map(|(title, position)| row(position, title))
                    .collect(),
            }],
        }
    }

    fn library_missing_two_tracks() -> (PathBuf, Library) {
        let root = scratch("wanting");
        fs::write(
            root.join("a.wav"),
            tagged_wave(&[(TITLE, "Held"), (ARTIST, "Ada"), (ALBUM, "Orbits")]),
        )
        .expect("a song");
        let library = scanned(&root);
        let album = library
            .albums(&AlbumQuery::default())
            .expect("the albums")
            .remove(0);
        library
            .land_release(album.id, &release_of(&["Held", "San Tropez", "Seamus"]))
            .expect("a release");
        library.rematch(album.id).expect("a match");
        (root, library)
    }

    fn titles_missing(library: &Library) -> Vec<String> {
        library
            .missing_tracks(None, None)
            .expect("the missing tracks")
            .into_iter()
            .map(|track| track.title)
            .collect()
    }

    fn titles_wanted(library: &Library) -> Vec<String> {
        library
            .wants_as_they_stand()
            .expect("the wants")
            .into_iter()
            .map(|want| want.title)
            .collect()
    }

    #[test]
    fn a_missing_track_is_wanted_by_title_and_wanted_no_longer() {
        let (root, library) = library_missing_two_tracks();

        run(&library, &wants_asked(&["--add", "san tropez"])).expect("a want");
        let wanted = titles_wanted(&library);
        run(
            &library,
            &wants_asked(&["--remove", "San Tropez", "--album", "Orbits"]),
        )
        .expect("an unwant");
        let after = titles_wanted(&library);
        let held = run(&library, &wants_asked(&["--add", "Held"]));
        let elsewhere = run(
            &library,
            &wants_asked(&["--add", "Seamus", "--artist", "Nobody"]),
        );
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert_eq!(wanted, ["San Tropez"]);
        assert!(after.is_empty());
        assert!(matches!(
            held,
            Err(Error::NothingNamed {
                kind: Named::MissingTrack,
                ..
            })
        ));
        assert!(matches!(elsewhere, Err(Error::NothingNamed { .. })));
    }

    #[test]
    fn a_dismissed_missing_track_leaves_the_listing_and_a_name_nothing_matches_is_refused() {
        let (root, library) = library_missing_two_tracks();

        dismiss(&library, "Seamus", Narrowing::default()).expect("a dismissal");
        let left = titles_missing(&library);
        let again = dismiss(&library, "Seamus", Narrowing::default());
        let release = dismiss_release(&library, "Orbits", None);
        let unwanted = unwant(&library, "San Tropez", Narrowing::default());
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert_eq!(left, ["San Tropez"]);
        assert!(matches!(again, Err(Error::NothingNamed { .. })));
        assert!(matches!(
            release,
            Err(Error::NothingNamed {
                kind: Named::UnheldRelease,
                ..
            })
        ));
        assert!(matches!(
            unwanted,
            Err(Error::NothingNamed {
                kind: Named::WantedTrack,
                ..
            })
        ));
    }
}
