use std::{collections::BTreeMap, path::Path, sync::Arc};

use gpui::{AnyElement, Context, Div, Stateful, prelude::*};
use resonate_library::Copies;

use crate::{
    Planned,
    icons::Icon,
    views::{
        kit,
        root::RootView,
        settings::{Armed, Rows, action, note},
    },
    words,
};

const GROUPS_SHOWN: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identical<'a> {
    pub(super) kept: &'a Path,
    pub(super) copies: Vec<&'a Path>,
}

pub(super) fn grouped(copies: &Copies) -> Vec<Identical<'_>> {
    let mut by_song: BTreeMap<&Path, Vec<&Path>> = BTreeMap::new();
    for identical in &copies.identical {
        by_song
            .entry(identical.kept.as_path())
            .or_default()
            .push(identical.copy.as_path());
    }

    by_song
        .into_iter()
        .map(|(kept, mut copies)| {
            copies.sort_unstable();
            Identical { kept, copies }
        })
        .collect()
}

impl RootView {
    pub(super) fn identical_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);
        let busy = library.is_busy();
        let finding = library.is_finding_identical();
        let folding = library.is_folding_identical();
        let planned = library.previewed_identical();
        let found = library.identical_songs().filter(|_| planned.is_shown());
        let foldable = found
            .as_ref()
            .is_some_and(|copies| !copies.identical.is_empty());
        let armed = self.is_armed(&Armed::FoldTheCopies) && foldable;

        kit::rows()
            .child(
                kit::action_row()
                    .child(self.find_identical(busy, finding, cx))
                    .child(self.fold_identical(busy || !foldable, folding, armed, cx)),
            )
            .when(found.is_none() && !finding, |body| {
                body.child(note(unfound(planned)))
            })
            .when(armed, |body| {
                body.child(note(words::settings_identical::ask_again_to_fold()))
            })
            .when_some(found, |body, copies| {
                body.children(listed(&copies)).child(note(told(&copies)))
            })
            .note(words::settings_identical::note())
    }

    fn find_identical(
        &self,
        held_back: bool,
        finding: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "find-identical-songs",
            if finding {
                words::settings_identical::finding()
            } else {
                words::settings_identical::find()
            },
            Icon::Search,
            held_back,
            |this, _, cx| this.look_for_identical_songs(cx),
            self,
            cx,
        )
    }

    fn fold_identical(
        &self,
        held_back: bool,
        folding: bool,
        armed: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "fold-identical-songs",
            if folding {
                words::settings_identical::folding()
            } else if armed {
                words::settings_identical::press_again_to_fold()
            } else {
                words::settings_identical::fold()
            },
            Icon::Fold,
            held_back,
            move |this, _, cx| {
                if armed {
                    this.fold_the_identical_songs(cx);
                } else {
                    this.arm(Armed::FoldTheCopies);
                    cx.notify();
                }
            },
            self,
            cx,
        )
    }

    fn look_for_identical_songs(&mut self, cx: &mut Context<Self>) {
        self.lower(&Armed::FoldTheCopies);
        self.library
            .update(cx, |library, cx| library.find_identical_songs(cx));
        cx.notify();
    }

    fn fold_the_identical_songs(&mut self, cx: &mut Context<Self>) {
        self.lower(&Armed::FoldTheCopies);
        self.library
            .update(cx, |library, cx| library.fold_identical_songs(cx));
        cx.notify();
    }
}

fn unfound(planned: Planned) -> &'static str {
    match planned {
        Planned::Outdated => words::settings_identical::moved_under_the_search(),
        Planned::Not | Planned::Shown(_) => words::settings_identical::find_first(),
    }
}

fn listed(copies: &Arc<Copies>) -> Vec<AnyElement> {
    let groups = grouped(copies);
    let mut rows: Vec<AnyElement> = groups
        .iter()
        .take(GROUPS_SHOWN)
        .map(|group| group_row(group).into_any_element())
        .collect();
    if let Some(rest) = groups
        .len()
        .checked_sub(GROUPS_SHOWN)
        .filter(|rest| *rest > 0)
    {
        rows.push(note(words::settings_identical::and_more(rest)).into_any_element());
    }

    rows
}

fn group_row(group: &Identical<'_>) -> Div {
    let names: Vec<String> = group.copies.iter().map(|copy| named(copy)).collect();

    kit::named(
        named(group.kept),
        words::settings_identical::copies_of_it(group.copies.len(), names.join(", ")),
    )
}

fn named(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn told(copies: &Copies) -> String {
    let folding = copies.identical.len();
    if folding == 0 && copies.differing.is_empty() {
        return words::settings_identical::none_held_twice().to_owned();
    }

    words::settings_identical::would_fold(folding, grouped(copies).len(), copies.differing.len())
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use resonate_library::{Library, TrackQuery};

    use super::*;
    use crate::{
        driven::{Driven, Folder},
        views::settings::Page,
    };

    fn scanned(folder: &Folder) -> Arc<Library> {
        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
        Driven::scanned(&library, folder);
        library
    }

    fn titles_held(library: &Library) -> usize {
        library
            .tracks(&TrackQuery::default())
            .expect("the tracks read")
            .len()
    }

    #[test]
    fn copies_of_one_song_are_one_group_named_by_the_song_that_is_not_numbered() {
        let folder = Folder::new();
        folder.tone("Song.wav", 1);
        folder.tone("Song (1).wav", 1);
        folder.tone("Song (2).wav", 1);
        folder.tone("Song2.wav", 1);
        folder.tone("Other.wav", 2);
        folder.tone("Other (1).wav", 3);
        let library = scanned(&folder);

        let copies = library.identical_copies().expect("the copies are found");
        let groups = grouped(&copies);

        assert_eq!(groups.len(), 1);
        assert!(groups[0].kept.ends_with("Song.wav"));
        assert_eq!(groups[0].copies.len(), 3);
        assert!(groups[0].copies[0].ends_with("Song (1).wav"));
        assert!(groups[0].copies[1].ends_with("Song (2).wav"));
        assert!(groups[0].copies[2].ends_with("Song2.wav"));
        assert_eq!(copies.differing.len(), 1);
    }

    #[gpui::test]
    fn identical_songs_are_found_listed_and_folded_only_once_found_and_pressed_twice(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let song = folder.tone("Song.wav", 1);
        let copy = folder.tone("Song (1).wav", 1);
        let other = folder.tone("Other.wav", 2);
        let differing = folder.tone("Other (1).wav", 3);
        let library = scanned(&folder);
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.root.update(&mut driven.cx, |root, cx| {
            root.set_pane(crate::Pane::Settings, cx);
            root.open_the_settings_page(Page::Identical, cx);
        });
        driven.settle();

        let found = |driven: &mut Driven| {
            driven.read(|root, cx| root.library.read(cx).previewed_identical().is_shown())
        };
        assert!(!found(&mut driven));
        driven.click("fold-identical-songs");
        assert!(copy.exists(), "a fold nothing found took a file");

        driven.click("find-identical-songs");
        driven.until(|root, cx| root.library.read(cx).previewed_identical().is_shown());
        let listed = driven
            .read(|root, cx| root.library.read(cx).identical_songs())
            .expect("the copies are listed");
        assert_eq!(listed.identical.len(), 1);
        assert_eq!(listed.differing.len(), 1);
        assert!(copy.exists());

        driven.click("fold-identical-songs");
        assert!(copy.exists(), "one press folded");
        driven.click("fold-identical-songs");
        driven.until(|_, _| !copy.exists());
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 3);

        assert!(song.exists());
        assert!(other.exists());
        assert!(differing.exists());
        assert_eq!(titles_held(&library), 3);
        assert!(!found(&mut driven));
    }
}
