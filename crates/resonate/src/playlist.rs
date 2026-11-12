use std::{
    num::NonZeroUsize,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

use resonate_codec::Sources;
use resonate_core::{MediaLocation, elapsed};
use resonate_library::{
    Cut, Direction, Kept, Library, Playlist, PlaylistName, PlaylistOrder, RowOrder, SavedQuery,
    Search, SortOrder, Upkeep,
};

use crate::{
    arguments::{local_path, names_a_sheet, sheet_cuts},
    cli::{Cli, PlaylistArgs, PlaylistOrderArg},
    config::Config,
    error::{Error, Result},
    followed, open_library,
    playing::{Opening, play_queue},
    playlists,
    table::Table,
    versions, words,
    words::columns,
};

pub(crate) fn list_playlists(
    library: &Library,
    order: PlaylistOrderArg,
    reverse: bool,
    named: Option<&str>,
) -> Result<()> {
    let order = PlaylistOrder::from(order);
    let reading = read_as(order.reads(), reverse);
    let listed = library.playlists(order, reading, named)?;

    let mut table = Table::new(vec![
        columns::playlist(),
        columns::fills(),
        columns::tracks(),
        columns::length(),
        columns::last_played(),
        columns::plays(),
        columns::pinned(),
        columns::from(),
    ]);
    for playlist in listed {
        table.push(vec![
            playlist.name.clone(),
            fills(&playlist),
            playlist.entries.to_string(),
            played(playlist.duration),
            ago(playlist.played),
            times(playlist.plays),
            ago(playlist.pinned),
            followed::came_from(playlist.source.as_ref()),
        ]);
    }

    said_on!("{}", table.render());
    Ok(())
}

fn times(plays: u32) -> String {
    match plays {
        0 => String::new(),
        plays => plays.to_string(),
    }
}

pub(crate) fn fills(playlist: &Playlist) -> String {
    let Some(query) = playlist.query.as_ref() else {
        return match playlist.kept {
            Some(kept) => words::main::a_list_kept_in(kept_order(kept)),
            None => words::main::a_list().to_owned(),
        };
    };

    match query.text.as_deref() {
        Some(text) => words::main::itself_with(format!("{text:?}")),
        None => words::main::itself().to_owned(),
    }
}

fn kept_order(kept: Kept) -> String {
    match kept.reading {
        Direction::Ascending => words::main::in_order(ordered_by(kept.order)),
        Direction::Descending => words::main::in_reverse_order(ordered_by(kept.order)),
    }
}

fn ordered_by(order: RowOrder) -> &'static str {
    match order {
        RowOrder::Album => words::main::by_album(),
        RowOrder::Artist => words::main::by_artist(),
        RowOrder::Title => words::main::by_title(),
        RowOrder::Length => words::main::by_length(),
        RowOrder::File => words::main::by_file_name(),
    }
}

pub(crate) fn played(duration: Option<Duration>) -> String {
    duration.map_or_else(String::new, elapsed::clock)
}

pub(crate) fn ago(played: Option<SystemTime>) -> String {
    played.map_or_else(String::new, |played| {
        elapsed::since(played, SystemTime::now())
    })
}

pub(crate) fn playlist(cli: &Cli, config: &Config, wanted: &PlaylistArgs) -> Result<()> {
    let library = Arc::new(open_library(cli, config)?);
    let name = wanted.name.as_str();
    if let Some(text) = wanted.query.as_deref() {
        return save_query(&library, name, text, wanted);
    }
    if !wanted.add.is_empty() {
        return extend(&library, name, &wanted.add);
    }

    let found = library
        .playlist_named(name)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(name)))?;
    if let Some(into) = wanted.rename.as_deref() {
        library.rename_playlist(found.id, into)?;
        said!("{}", words::main::renamed(into, &found.name));
        return Ok(());
    }
    if wanted.discard {
        library.remove_playlist(found.id)?;
        said!("{}", discarded(&found));
        return Ok(());
    }
    if wanted.refresh {
        return followed::refresh(&library, config, &found);
    }
    if let Some(upkeep) = wanted.upkeep {
        return followed::set_upkeep(&library, &found, upkeep.into());
    }
    if wanted.forget_link {
        return followed::forget_link(&library, &found);
    }
    if wanted.pin || wanted.unpin {
        library.pin_playlist(found.id, wanted.pin)?;
        said!("{}", pinning(&found.name, wanted.pin));
        return Ok(());
    }
    if let Some(target) = wanted.export.as_deref() {
        let written = library.export_playlist(found.id, target)?;
        said!(
            "{}",
            words::main::exported(
                written.format.name(),
                &found.name,
                target.display().to_string(),
                written.rows,
            )
        );
        return Ok(());
    }
    if let Some(target) = wanted.into.as_deref() {
        return copy_into(&library, &found, target, wanted.matching.as_deref());
    }
    if let Some(order) = wanted.order {
        let kept = Kept {
            order: order.into(),
            reading: read_as(Direction::Ascending, wanted.reverse),
        };
        if wanted.keep {
            let moved = library.keep_playlist_in_order(found.id, Some(kept))?;
            said!("{}", now_kept(moved, &found.name, kept));
            return Ok(());
        }
        let moved = library.sort_playlist(found.id, kept.order, kept.reading)?;

        said!("{}", reordered(moved, &found.name));
        return Ok(());
    }
    if wanted.by_hand {
        library.keep_playlist_in_order(found.id, None)?;
        said!("{}", words::main::back_in_hand(&found.name));
        return Ok(());
    }
    if wanted.tidy {
        said!("{}", tidied(library.prune_playlist(found.id)?, &found.name));
        return Ok(());
    }
    if wanted.fold {
        said!("{}", folded(library.fold_doubles(found.id)?, &found.name));
        return Ok(());
    }
    if let Some(text) = wanted.matching.as_deref().filter(|_| wanted.drop) {
        let dropped = library.remove_matching(found.id, text)?;
        said!("{}", taken_out(dropped, &found.name, text));
        return Ok(());
    }

    let matching = wanted.matching.as_deref();
    let entries = library.playlist_entries(found.id, matching)?;
    if entries.is_empty() {
        said!("{}", words::main::holds_nothing(&found.name));
        return Ok(());
    }

    let items = versions::played_as_chosen(Some(&library), playlists::queue_items(&entries));
    let whole = matching.is_none();
    play_queue(
        cli,
        config,
        items,
        Some(library),
        Opening {
            playing: whole.then_some(found.id),
            ..Opening::default()
        },
        &wanted.transport,
    )
}

fn copy_into(
    library: &Library,
    found: &Playlist,
    target: &str,
    matching: Option<&str>,
) -> Result<()> {
    let into = match library.playlist_named(target)? {
        Some(held) => held.id,
        None => library.create_playlist(target)?,
    };
    let copied = library.copy_playlist(found.id, into, matching)?;
    let held = library
        .playlist(into)?
        .map_or(copied as u32, |playlist| playlist.entries);

    said!("{}", words::main::copied(copied, held, &found.name, target));
    Ok(())
}

const fn read_as(reads: Direction, reversed: bool) -> Direction {
    if reversed { reads.flipped() } else { reads }
}

pub(crate) fn save_query(
    library: &Library,
    name: &str,
    text: &str,
    wanted: &PlaylistArgs,
) -> Result<()> {
    let sort = SortOrder::from(wanted.sort);
    let query = SavedQuery {
        text: (!text.trim().is_empty()).then(|| text.trim().to_owned()),
        sort,
        reading: read_as(sort.reads(), wanted.reverse),
        limit: wanted.limit.map(NonZeroUsize::get),
    };

    let (id, revised) = match library.playlist_named(name)? {
        Some(found) => {
            library.revise_query(found.id, &found.name, &query)?;
            (found.id, Some(found.name))
        }
        None => (library.save_query(name, &query)?, None),
    };
    let held = library.playlist(id)?.map_or(0, |found| found.entries);
    let matched = matching(query.text.as_deref());

    said!(
        "{}",
        match revised {
            Some(revised) => words::main::query_revised(held, matched, revised),
            None => words::main::query_saved(held, matched, name),
        }
    );
    if let Some(reads) = understood(query.text.as_deref()) {
        said!("{}", words::main::reads_as(reads));
    }
    Ok(())
}

pub(crate) fn matching(text: Option<&str>) -> String {
    match text {
        Some(text) => format!(" {}", words::main::whatever_matches(format!("{text:?}"))),
        None => format!(" {}", words::main::the_whole_library()),
    }
}

fn understood(text: Option<&str>) -> Option<String> {
    let reads = Search::read(text?).reads();

    (!reads.is_empty()).then(|| reads.join(" · "))
}

pub(crate) fn extend(library: &Library, name: &str, paths: &[PathBuf]) -> Result<()> {
    let sources = Sources::local();
    let mut wanted = Vec::with_capacity(paths.len());
    for path in paths.iter().map(|path| local_path(path)) {
        let location =
            MediaLocation::local(
                path.canonicalize()
                    .map_err(|source| Error::UnresolvedFile {
                        path: path.clone(),
                        source,
                    })?,
            );
        match location.as_path().filter(|_| names_a_sheet(&location)) {
            Some(sheet) => wanted.extend(sheet_cuts(&sources, sheet).into_iter().map(
                |(location, span)| Cut {
                    location,
                    span: Some(span),
                },
            )),
            None => wanted.push(Cut::whole(location)),
        }
    }

    let (id, added) = match library.playlist_named(name)? {
        Some(found) => (found.id, library.add_to_playlist(found.id, &wanted)?),
        None => (library.start_playlist(name, &wanted)?, wanted.len()),
    };
    let held = library
        .playlist(id)?
        .map_or(added as u32, |playlist| playlist.entries);

    said!("{}", words::main::added(added, held, name));
    Ok(())
}

pub(crate) fn import(
    library: &Library,
    config: &Config,
    files: &[PathBuf],
    name: Option<&str>,
    upkeep: Upkeep,
) -> Result<()> {
    if name.is_some() && files.len() > 1 {
        return Err(Error::NameForOnePlaylistOnly { given: files.len() });
    }

    for file in files {
        if let Some(profile) = followed::profile_named_by(file) {
            followed::follow_profile(library, config, &profile, upkeep)?;
            continue;
        }
        if let Some(link) = followed::link_named_by(file) {
            followed::import(library, config, &link, name, upkeep)?;
            continue;
        }
        if upkeep == Upkeep::InStep {
            return Err(Error::KeptInStepOnlyWithALink { file: file.clone() });
        }
        let read = library.import_playlist(file, name)?;
        let held = library
            .playlist(read.id)?
            .map_or(read.added as u32, |playlist| playlist.entries);

        said!(
            "{}{}{}{}{}",
            words::main::imported(
                read.added,
                read.encoding.name(),
                read.format.name(),
                held,
                &read.name,
            ),
            already_held(read.already),
            passed_over(read.elsewhere),
            not_on_disk(read.missing),
            fell_short(read.short)
        );
    }
    Ok(())
}

pub(crate) fn reordered(moved: usize, name: &str) -> String {
    match moved {
        0 => words::main::already_in_order(name),
        moved => words::main::reordered(moved, name),
    }
}

fn now_kept(moved: usize, name: &str, kept: Kept) -> String {
    let order = kept_order(kept);
    match moved {
        0 => words::main::kept_already(name, order),
        moved => words::main::now_kept(moved, name, order),
    }
}

fn tidied(dropped: usize, name: &str) -> String {
    match dropped {
        0 => words::main::all_there(name),
        dropped => words::main::tidied(dropped, name),
    }
}

pub(crate) fn folded(dropped: usize, name: &str) -> String {
    match dropped {
        0 => words::main::none_doubled(name),
        dropped => words::main::folded_doubles(dropped, name),
    }
}

fn pinning(name: &str, pinned: bool) -> String {
    if pinned {
        words::main::pinned(name)
    } else {
        words::main::unpinned(name)
    }
}

pub(crate) fn discarded(found: &Playlist) -> String {
    let name = &found.name;
    if found.query.is_some() {
        return words::main::discarded_search(name);
    }

    match found.entries {
        0 => words::main::discarded_empty(name),
        held => words::main::discarded(held, name),
    }
}

pub(crate) fn taken_out(dropped: usize, name: &str, text: &str) -> String {
    match dropped {
        0 => words::main::none_match(name, format!("{text:?}")),
        dropped => words::main::took_out(dropped, name, format!("{text:?}")),
    }
}

pub(crate) fn already_held(already: usize) -> String {
    match already {
        0 => String::new(),
        already => format!(" | {}", words::main::already_in_it(already)),
    }
}

pub(crate) fn passed_over(elsewhere: usize) -> String {
    match elsewhere {
        0 => String::new(),
        passed => format!(" | {}", words::main::passed_over(passed)),
    }
}

fn fell_short(short: usize) -> String {
    match short {
        0 => String::new(),
        short => format!(" | {}", words::main::fell_short(short)),
    }
}

fn not_on_disk(missing: usize) -> String {
    match missing {
        0 => String::new(),
        missing => format!(" | {}", words::main::not_on_disk(missing)),
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use clap::Parser as _;

    use super::*;
    use crate::{
        analyse,
        arguments::from_here,
        cli::{RepeatArg, Sub, TransportArgs},
        fixtures::parsed,
    };

    #[test]
    fn the_terminal_tells_a_time_as_the_window_does_in_years_and_with_hours_on_a_long_clock() {
        let long_ago = SystemTime::now() - Duration::from_secs(400 * 24 * 3_600);

        assert_eq!(ago(Some(long_ago)), "1 year ago");
        assert_eq!(ago(None), "");
        assert_eq!(played(Some(Duration::from_secs(4_503))), "1:15:03");
        assert_eq!(played(None), "");
    }

    #[test]
    fn a_profile_imported_for_a_device_is_a_command_the_grammar_takes() {
        let parsed = Cli::try_parse_from([
            "resonate",
            "eq",
            "--import",
            "profile.txt",
            "--for",
            "alsa_output.usb",
        ]);

        let Ok(Cli {
            command: Some(Sub::Eq(wanted)),
            ..
        }) = parsed
        else {
            panic!("--import beside --for was refused: {parsed:?}");
        };
        assert_eq!(wanted.r#for.as_deref(), Some("alsa_output.usb"));
    }

    #[test]
    fn a_flag_that_would_do_nothing_or_more_than_it_says_is_refused() {
        let parsed = |arguments: &[&str]| Cli::try_parse_from(arguments.iter().copied()).is_ok();

        assert!(!parsed(&["resonate", "playlist", "Road", "--reverse"]));
        assert!(!parsed(&[
            "resonate",
            "playlist",
            "Road",
            "--export",
            "a.m3u",
            "--reverse"
        ]));
        assert!(parsed(&[
            "resonate",
            "playlist",
            "Road",
            "--order",
            "title",
            "--reverse"
        ]));
        assert!(parsed(&[
            "resonate",
            "playlist",
            "Road",
            "--query",
            "floyd",
            "--reverse"
        ]));
        assert!(!parsed(&[
            "resonate",
            "eq",
            "--off",
            "--for",
            "alsa_output.usb"
        ]));
        assert!(!parsed(&[
            "resonate",
            "eq",
            "--on",
            "--for",
            "alsa_output.usb"
        ]));
        assert!(!parsed(&[
            "resonate",
            "eq",
            "--suggest",
            "--for",
            "alsa_output.usb"
        ]));
        assert!(parsed(&["resonate", "eq", "--off"]));
        assert!(parsed(&["resonate", "eq", "--on"]));
    }

    #[test]
    fn a_sheet_added_to_a_playlist_is_added_as_the_rows_it_cuts() {
        let folder = env::temp_dir().join(format!("resonate-extend-{}", process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        fs::write(folder.join("whole.wav"), analyse::tests::silent_wave()).expect("a wave file");
        let sheet = folder.join("sheet.cue");
        fs::write(
            &sheet,
            "FILE \"whole.wav\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 \
             AUDIO\n    INDEX 01 00:00:30\n",
        )
        .expect("a sheet");
        let library = Library::open(&folder.join("library.db")).expect("a catalog");

        extend(&library, "Mixtape", &[sheet]).expect("the sheet is added");

        let playlist = library
            .playlist_named("Mixtape")
            .expect("a read")
            .expect("the playlist was started");
        let cuts = library.playlist_cuts(playlist.id).expect("a read");
        assert_eq!(cuts.len(), 2, "a sheet was stored as one row");
        assert!(cuts.iter().all(|cut| cut.span.is_some()
            && cut.location.as_path() == Some(from_here(&folder.join("whole.wav")).as_path())));

        drop(library);
        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    fn playlist_transport_of(arguments: &[&str]) -> TransportArgs {
        match parsed(arguments).command {
            Some(Sub::Playlist(wanted)) => wanted.transport,
            other => panic!("{arguments:?} is not playlist: {other:?}"),
        }
    }

    #[test]
    fn a_playlist_played_takes_the_transport_and_one_edited_refuses_it() {
        let set = playlist_transport_of(&[
            "resonate",
            "playlist",
            "Evening",
            "--matching",
            "floyd",
            "--shuffle",
            "--repeat",
            "queue",
            "--volume",
            "25",
        ]);
        let refused = |arguments: &[&str]| Cli::try_parse_from(arguments.iter().copied()).is_err();

        assert!(set.shuffle);
        assert_eq!(set.repeat, Some(RepeatArg::Queue));
        assert_eq!(set.volume, Some(25));
        assert!(refused(&[
            "resonate",
            "playlist",
            "Evening",
            "--tidy",
            "--shuffle"
        ]));
        assert!(refused(&[
            "resonate", "playlist", "Evening", "--pin", "--volume", "10"
        ]));
        assert!(refused(&[
            "resonate",
            "playlist",
            "Evening",
            "--matching",
            "x",
            "--drop",
            "--repeat",
            "track"
        ]));
        assert!(!refused(&["resonate", "playlist", "Evening", "--shuffle"]));
    }
}
