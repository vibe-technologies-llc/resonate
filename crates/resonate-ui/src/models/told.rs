use std::{sync::Arc, time::Duration};

use ahash::AHashSet;
use gpui::{AsyncApp, WeakEntity};
use resonate_core::{AlbumId, MediaLocation};
use resonate_library::{
    Covering, Deleted, Edit, EnrichSummary, GroupRelease, ImportSummary, Library, Maker,
    OrganiseSummary, PollSummary, Reference, ReleaseMatch, RetagSummary, ScanSummary, Shared,
    Track, Undoable,
};

use super::{
    LibraryModel, Notice, Prompted, RETRIES_ASKED_AT_LEAST, UNHEARD_RETRIES_DOUBLE_AT_MOST, Wanted,
};
use crate::{format, toast, words};

pub(super) fn put_back_as(put_back: &Undoable) -> String {
    made_by_saying(put_back.made_by, edit_put_back_as(put_back))
}

pub(super) fn done_again_as(done_again: &Undoable) -> String {
    made_by_saying(done_again.made_by, edit_done_again_as(done_again))
}

fn made_by_saying(made_by: Maker, saying: String) -> String {
    match made_by {
        Maker::ThisRun => saying,
        Maker::AModel => words::models::the_models_edit(saying),
    }
}

fn edit_put_back_as(put_back: &Undoable) -> String {
    let name = &put_back.name;

    match put_back.edit {
        Edit::Started => words::models::gone_again(name),
        Edit::Renamed => words::models::it_is_again(name),
        Edit::Revised => words::models::fills_itself_from_the_search_it_had(name),
        Edit::Discarded => words::models::put_back(name),
        Edit::Added | Edit::Copied | Edit::Imported => {
            words::models::took_back_out_what_went_in(name)
        }
        Edit::Removed | Edit::Dropped | Edit::Tidied | Edit::Folded => {
            words::models::put_back_in_what_went_out(name)
        }
        Edit::Moved | Edit::Ordered | Edit::Kept => {
            words::models::put_back_in_the_order_it_was_in(name)
        }
        Edit::Refreshed => words::models::put_back_the_rows_held_before_refreshed(name),
    }
}

fn edit_done_again_as(done_again: &Undoable) -> String {
    let name = &done_again.name;

    match done_again.edit {
        Edit::Started => words::models::started_again(name),
        Edit::Renamed => words::models::it_is_again(name),
        Edit::Revised => words::models::fills_itself_from_the_search_it_was_given(name),
        Edit::Discarded => words::models::gone_again(name),
        Edit::Added | Edit::Copied | Edit::Imported => {
            words::models::put_back_into_what_had_gone_in(name)
        }
        Edit::Removed | Edit::Dropped | Edit::Tidied | Edit::Folded => {
            words::models::took_back_out_what_had_gone_out(name)
        }
        Edit::Moved | Edit::Ordered | Edit::Kept => {
            words::models::put_back_in_the_order_that_edit_left(name)
        }
        Edit::Refreshed => words::models::refreshed_from_its_link_again(name),
    }
}

pub(super) async fn cover_what_was_wanted(
    this: &WeakEntity<LibraryModel>,
    cx: &mut AsyncApp,
    library: Arc<Library>,
    reference: Arc<dyn Reference>,
    covering: Covering,
) {
    cx.background_executor()
        .spawn(async move { library.cover_what_was_wanted(reference.as_ref(), &covering) })
        .await;
    let _ = this.update(cx, |this, cx| this.read(Wanted::Everything, cx));
}

pub(super) fn scanned(summary: &ScanSummary, prompted: Prompted) -> Option<Notice> {
    let stats = summary.stats;
    let failed = stats.failed.total();
    let tracks_done: [(_, fn(_) -> String); 4] = [
        (stats.added, words::models::tracks_added),
        (stats.updated, words::models::tracks_updated),
        (stats.moved, words::models::tracks_moved),
        (stats.removed, words::models::tracks_removed),
    ];
    let mut said: Vec<String> = tracks_done
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, done)| done(count))
        .collect();
    if failed > 0 {
        said.push(words::models::could_not_be_read(failed));
    }
    if said.is_empty() && prompted != Prompted::ByHand && !summary.cancelled {
        return None;
    }

    let mut told = if said.is_empty() {
        words::models::the_library_is_up_to_date().to_owned()
    } else {
        said.join(" · ")
    };
    if summary.cancelled {
        told.push_str(words::models::stopped_early());
    }
    Some(Notice::Noted(told))
}

pub(super) fn retried_after(unheard_in_a_row: u32) -> Duration {
    RETRIES_ASKED_AT_LEAST * 2_u32.pow(unheard_in_a_row.min(UNHEARD_RETRIES_DOUBLE_AT_MOST))
}

pub(super) fn relocations(summary: &OrganiseSummary) -> Vec<(MediaLocation, MediaLocation)> {
    summary
        .plan
        .moves
        .iter()
        .flat_map(|planned| planned.files())
        .map(|(from, to)| (MediaLocation::local(from), MediaLocation::local(to)))
        .collect()
}

pub(super) fn filed(summary: &OrganiseSummary) -> Notice {
    let stats = summary.stats;
    let mut told = match stats.moved {
        0 => words::models::no_file_needed_moving().to_owned(),
        moved => words::models::moved_files(moved),
    };
    let unmoved = stats.collided + stats.failed;
    if unmoved > 0 {
        told = [told, words::models::could_not_be_moved(unmoved)].join(" · ");
    }
    if summary.cancelled {
        told.push_str(words::models::stopped_early());
    }
    Notice::Done(told)
}

pub(super) fn retagged(summary: &RetagSummary) -> Notice {
    let mut told = match summary.stats.written {
        0 => words::models::no_file_needed_its_tags_written().to_owned(),
        written => words::models::wrote_the_tags_into(written),
    };
    if summary.cancelled {
        told.push_str(words::models::stopped_early());
    }
    Notice::Done(told)
}

pub(super) fn vaulted(summary: &ImportSummary) -> Notice {
    let stats = summary.stats;
    let mut told = match stats.vaulted {
        0 => words::models::the_vault_already_holds_every_track_it().to_owned(),
        vaulted => words::models::kept_in_the_vault(vaulted, format::bytes(stats.saved())),
    };
    if summary.cancelled {
        told.push_str(words::models::stopped_early());
    }
    Notice::Done(told)
}

pub(super) fn deletion_told(
    deleted: resonate_library::Result<Deleted>,
    title: &str,
    asked: usize,
) -> Notice {
    match (deleted, asked) {
        (Ok(deleted), 1) if deleted.kept > 0 => {
            Notice::Trouble(words::models::could_not_delete_the_song(title))
        }
        (Ok(_), 1) => Notice::Done(words::models::deleted_the_song(title)),
        (Ok(deleted), asked) if deleted.kept > 0 => Notice::Trouble(
            words::models::could_not_delete_some_songs(asked, deleted.kept),
        ),
        (Ok(_), asked) => Notice::Done(words::models::deleted_songs(asked)),
        (Err(error), asked) => {
            tracing::error!(%error, "tracks could not be deleted");
            toast::could_not(
                if asked == 1 {
                    words::models::delete_that_track()
                } else {
                    words::models::delete_those_tracks()
                },
                &error,
            )
        }
    }
}

pub(super) fn folding_told(folded: resonate_library::Result<Deleted>) -> Notice {
    match folded {
        Ok(deleted) if deleted.kept > 0 => Notice::Trouble(
            words::models::folded_identical_songs_some_kept(deleted.files, deleted.kept),
        ),
        Ok(deleted) => Notice::Done(words::models::folded_identical_songs(deleted.files)),
        Err(error) => {
            tracing::error!(%error, "identical songs could not be folded");
            toast::could_not(words::models::fold_the_identical_songs(), &error)
        }
    }
}

pub(super) fn polled(summary: &PollSummary, prompted: Prompted) -> Option<Notice> {
    let stats = summary.stats;
    if stats.kept == 0 && prompted != Prompted::ByHand && !summary.cancelled {
        return None;
    }

    let mut told = match stats.kept {
        0 => words::models::nothing_in_the_inbox_filled_a_want().to_owned(),
        kept => words::models::the_inbox_filled(kept),
    };
    if stats.unkept > 0 {
        told = [told, words::models::could_not_be_kept(stats.unkept)].join(" · ");
    }
    if summary.cancelled {
        told.push_str(words::models::stopped_early());
    }
    Some(Notice::Done(told))
}

pub(super) fn looked_up(summary: &EnrichSummary, prompted: Prompted) -> Option<Notice> {
    let stats = summary.stats;
    if prompted == Prompted::OnItsOwn && summary.stopped_by.is_none() {
        return None;
    }
    let mut parts = vec![words::models::albums_and_artists_answered(
        stats.releases,
        stats.artists,
    )];
    if stats.lyrics > 0 {
        parts.push(words::models::lyrics_for_tracks(stats.lyrics));
    }
    if stats.songs > 0 {
        parts.push(words::models::songs_of_albums_not_held_learnt(stats.songs));
    }
    let mut said = parts.join(" · ");
    if summary.cancelled {
        said.push_str(words::models::stopped_early());
    }

    Some(match summary.stopped_by {
        Some(op) => Notice::Trouble(words::models::musicbrainz_unreached(said, op.asked_for())),
        None => Notice::Noted(said),
    })
}

pub(super) struct Sharing {
    pub(super) link: Option<String>,
    pub(super) notice: Notice,
}

pub(super) fn sharing_told(read: Vec<resonate_library::Result<Option<Shared>>>) -> Sharing {
    let asked = read.len();
    let mut links = Vec::new();
    let mut said = None;
    let mut failure = None;
    let mut absent = 0;

    for shared in read {
        match shared {
            Err(error) => {
                tracing::error!(%error, "a track could not be shared");
                failure.get_or_insert(error);
            }
            Ok(None) => absent += 1,
            Ok(Some(shared)) => {
                if let Some(link) = shared.written() {
                    said.get_or_insert_with(|| on_the_clipboard(&shared));
                    links.push(link);
                }
            }
        }
    }

    let nothing = |notice| Sharing { link: None, notice };

    if links.is_empty() {
        return nothing(match (failure, asked, absent) {
            (Some(error), 1, _) => toast::could_not(words::models::share_that_track(), &error),
            (Some(error), _, _) => toast::could_not(words::models::share_those_tracks(), &error),
            (None, 1, 1) => Notice::Noted(words::models::nothing_to_share().to_owned()),
            (None, 1, _) => Notice::Noted(words::models::no_link_to_share().to_owned()),
            (None, _, _) => Notice::Noted(words::models::no_link_to_share_any().to_owned()),
        });
    }

    let notice = match (asked, links.len()) {
        (1, _) => Notice::Done(said.unwrap_or_default()),
        (asked, held) if held == asked => Notice::Done(words::models::links_on_the_clipboard(held)),
        (asked, held) => Notice::Done(words::models::some_links_on_the_clipboard(asked, held)),
    };

    Sharing {
        link: Some(links.join("\n")),
        notice,
    }
}

pub(super) fn on_the_clipboard(shared: &Shared) -> String {
    match shared.artist.as_deref() {
        Some(artist) => words::models::song_by_on_the_clipboard(artist, &shared.title),
        None => words::models::song_on_the_clipboard(&shared.title),
    }
}

pub(super) fn named_with_its_credit(found: &ReleaseMatch) -> String {
    let credit = found.credited_as();
    if credit.trim().is_empty() {
        found.title.clone()
    } else {
        format!("{} · {credit}", found.title)
    }
}

pub(super) fn in_the_order_they_came_out(mut releases: Vec<GroupRelease>) -> Arc<[GroupRelease]> {
    releases.sort_by(|one, other| {
        (one.date.is_none(), one.date.as_deref())
            .cmp(&(other.date.is_none(), other.date.as_deref()))
    });
    releases.into()
}

pub(super) fn albums_of<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> Vec<AlbumId> {
    let mut held = AHashSet::new();
    tracks
        .into_iter()
        .filter_map(|track| track.album_id)
        .filter(|album| held.insert(*album))
        .collect()
}
