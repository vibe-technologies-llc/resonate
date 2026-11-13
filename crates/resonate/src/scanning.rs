use std::{num::NonZeroUsize, sync::Arc, thread};

use resonate_core::Switch;
use resonate_library::{
    EnrichOptions, EnrichStats, EnrichSummary, Library, LookupOp, Outcome, PassKind, ScanOptions,
};

use crate::{
    cli::ScanArgs,
    config::Config,
    error::{Error, Result},
    online,
    online::Asking,
    passes::{finished, none_failed, until_told},
    sinks::broken_down,
    words,
};

pub(crate) fn scan(library: &Library, config: &Config, wanted: &ScanArgs) -> Result<()> {
    let roots = &wanted.roots;
    if let Some(root) = roots.iter().find(|root| !root.is_dir()) {
        return Err(Error::MissingLibraryRoot { path: root.clone() });
    }

    let workers = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let handle = library.scan(scan_options(wanted, workers))?;

    let summary = until_told(handle)?;
    let stats = summary.stats;
    said!(
        "{}{}",
        words::main::scanned(
            stats.added,
            stats.discovered,
            stats.failed.total(),
            stats.moved,
            stats.removed,
            stats.updated,
        ),
        broken_down(stats.failed)
    );
    if summary.cancelled {
        said!("{}", words::main::cancelled());
    }
    finished(PassKind::Scan, summary.cancelled)?;
    let scanned = none_failed(PassKind::Scan, stats.failed.total());

    if config.is_on(Switch::EnrichAfterScan)
        && let Some(reference) = online::reference(config, Asking::InTheBackground)
    {
        let summary = until_told(library.enrich(
            reference,
            Arc::new(online::fingerprinters(
                config,
                Arc::new(library.sources()),
                &online::by_sound(config),
                Asking::InTheBackground,
            )),
            carrying_on(
                library,
                EnrichOptions {
                    studies: config.is_on(Switch::Study),
                    lyrics: config.is_on(Switch::FetchLyrics),
                    ..EnrichOptions::default()
                },
            )?,
        )?)?;
        said_on!("{}", enriched(&summary));
        finished(PassKind::Enrich, summary.cancelled)?;
    }
    scanned
}

fn scan_options(wanted: &ScanArgs, workers: NonZeroUsize) -> ScanOptions {
    ScanOptions {
        roots: wanted.roots.clone(),
        incremental: !wanted.full,
        follow_symlinks: wanted.follow_links,
        extract_cover_art: true,
        workers,
    }
}

pub(crate) fn enrich(library: &Library, config: &Config, options: EnrichOptions) -> Result<()> {
    let reference = online::reference_asked_for(config, Asking::InTheBackground)?;
    let options = carrying_on(library, options)?;
    let summary = until_told(library.enrich(
        reference,
        Arc::new(online::fingerprinters(
            config,
            Arc::new(library.sources()),
            &online::by_sound(config),
            Asking::InTheBackground,
        )),
        options,
    )?)?;
    said_on!("{}", enriched(&summary));
    finished(PassKind::Enrich, summary.cancelled)
}

fn carrying_on(library: &Library, options: EnrichOptions) -> Result<EnrichOptions> {
    if options.refresh {
        return Ok(options);
    }
    let Some(left) = library.unfinished_enrichment()? else {
        return Ok(options);
    };

    tracing::info!(
        began = ?left.began,
        refresh = left.refresh,
        "carrying on a lookup the last run left unfinished"
    );
    said!("{}", words::main::carrying_on());

    Ok(EnrichOptions {
        refresh: left.refresh,
        ..options
    })
}

fn enriched(summary: &EnrichSummary) -> String {
    let mut told = counted_by_section(&summary.stats);
    if let Some(op) = summary.stopped_by {
        told.push_str(&words::main::stopped_unreached(asked_for(op)));
        told.push('\n');
    }
    if summary.cancelled {
        told.push_str(words::main::cancelled());
        told.push('\n');
    }
    told
}

fn counted_by_section(stats: &EnrichStats) -> String {
    if stats.is_blank() {
        return format!("{}\n", words::main::nothing_asked());
    }
    let widest = Outcome::ALL
        .into_iter()
        .map(|outcome| stats.of(outcome).to_string().len())
        .max()
        .unwrap_or(1);

    let mut told = String::new();
    for (section, counted) in stats.told_by_section() {
        told.push_str(section.title());
        told.push('\n');
        for (outcome, count) in counted {
            told.push_str(&format!("  {count:>widest$}  {}\n", outcome.label()));
        }
    }
    told
}

pub(crate) fn asked_for(op: LookupOp) -> &'static str {
    match op {
        LookupOp::Release => words::main::asked_release(),
        LookupOp::FindRelease => words::main::asked_find_release(),
        LookupOp::Recording => words::main::asked_recording(),
        LookupOp::Isrc => words::main::asked_isrc(),
        LookupOp::FindRecording => words::main::asked_find_recording(),
        LookupOp::ReleaseGroup => words::main::asked_release_group(),
        LookupOp::FindReleaseGroup => words::main::asked_find_release_group(),
        LookupOp::Artist => words::main::asked_artist(),
        LookupOp::FindArtist => words::main::asked_find_artist(),
        LookupOp::ReleaseGroupsOfArtist => words::main::asked_release_groups_of_artist(),
        LookupOp::ReleasesOfGroup => words::main::asked_releases_of_group(),
        LookupOp::ReleasesOfArtist => words::main::asked_releases_of_artist(),
        LookupOp::Cover => words::main::asked_cover(),
        LookupOp::Portrait => words::main::asked_portrait(),
        LookupOp::Lyrics => words::main::asked_lyrics(),
        LookupOp::Devices => words::main::asked_devices(),
        LookupOp::Correction => words::main::asked_correction(),
        LookupOp::Recognise => words::main::asked_recognise(),
        LookupOp::Submit => words::main::asked_submit(),
        LookupOp::Love => words::main::asked_love(),
        LookupOp::Token => words::main::asked_token(),
        LookupOp::StreamLink => words::main::asked_stream_link(),
        LookupOp::FollowLink => words::main::asked_follow_link(),
        LookupOp::ShareFile => words::main::asked_share_file(),
        LookupOp::LatestRelease => words::main::asked_latest_release(),
        LookupOp::SpotifyAccount => words::main::asked_spotify_account(),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;
    use crate::{
        cli::Sub,
        fixtures::{parsed, silent_wave},
    };

    #[test]
    fn a_pass_is_told_by_section_with_what_it_counted_and_nothing_it_did_not() {
        let summary = EnrichSummary {
            stats: EnrichStats {
                albums: 12,
                releases: 8,
                albums_unmatched: 3,
                by_isrc: 40,
                refused: 2,
                ..EnrichStats::default()
            },
            ..EnrichSummary::default()
        };

        assert_eq!(
            enriched(&summary),
            "Albums\n\
             \x20 12  asked about\n\
             \x20  8  matched to a release\n\
             \x20  3  with no strict match\n\
             Tracks\n\
             \x20 40  identified by their ISRC\n\
             Trouble\n\
             \x20  2  requests a service refused or answered unreadably\n"
        );
    }

    #[test]
    fn a_pass_that_asked_nothing_says_so_and_a_cancelled_one_says_that_too() {
        let summary = EnrichSummary {
            cancelled: true,
            ..EnrichSummary::default()
        };

        assert_eq!(enriched(&summary), "nothing was asked\ncancelled\n");
    }

    fn scan_of(arguments: &[&str]) -> ScanOptions {
        match parsed(arguments).command {
            Some(Sub::Scan(wanted)) => scan_options(&wanted, NonZeroUsize::MIN),
            other => panic!("{arguments:?} is not scan: {other:?}"),
        }
    }

    #[test]
    fn a_scan_reads_every_file_and_follows_links_only_where_asked() {
        let usual = scan_of(&["resonate", "scan", "/music"]);
        let asked = scan_of(&["resonate", "scan", "--full", "--follow-links", "/music"]);

        assert!(usual.incremental);
        assert!(!usual.follow_symlinks);
        assert_eq!(usual.roots, vec![PathBuf::from("/music")]);
        assert!(!asked.incremental);
        assert!(asked.follow_symlinks);
        assert_eq!(asked.roots, vec![PathBuf::from("/music")]);
    }

    #[test]
    fn a_pass_no_signal_interrupts_answers_its_summary_through_until_told() {
        let root = crate::fixtures::scratch("until-told");
        fs::write(root.join("a.wav"), silent_wave(1)).expect("a track");
        let library = Library::open_in_memory().expect("a catalog");
        library.add_root(&root).expect("a root");

        let summary = until_told(
            library
                .scan(crate::fixtures::scan_of(&root))
                .expect("a scan under way"),
        )
        .expect("a summary");
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert_eq!(summary.stats.added, 1);
        assert!(!summary.cancelled);
    }
}
