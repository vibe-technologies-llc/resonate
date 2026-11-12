use std::{collections::BTreeMap, sync::Arc, time::Duration};

use gpui::{Context, Div, Stateful, Task, div, prelude::*};
use resonate_library::{
    Compacted, Library, Pruned, Released, VaultWeighing, VaultWeighingProgress, VaultWeighingStats,
};

use crate::{
    Notice, Pass, format,
    icons::Icon,
    toast,
    views::{
        kit,
        root::RootView,
        settings::{Armed, action, note},
    },
    words,
};

const WEIGHING_LOOKED_AT_EVERY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Tend {
    Check,
    Prune,
    Release,
    Compact,
}

impl Tend {
    pub(crate) const ALL: [Self; 4] = [Self::Check, Self::Prune, Self::Release, Self::Compact];

    fn name(self) -> &'static str {
        match self {
            Self::Check => words::settings_tending::check_name(),
            Self::Prune => words::settings_tending::prune_name(),
            Self::Release => words::settings_tending::release_name(),
            Self::Compact => words::settings_tending::compact_name(),
        }
    }

    fn under(self) -> &'static str {
        match self {
            Self::Check => words::settings_tending::check_under(),
            Self::Prune => words::settings_tending::prune_under(),
            Self::Release => words::settings_tending::release_under(),
            Self::Compact => words::settings_tending::compact_under(),
        }
    }

    pub(crate) const fn id(self, pass: Pass) -> &'static str {
        match (self, pass) {
            (Self::Check, Pass::Preview) => "check-the-vault",
            (Self::Check, Pass::Apply) => "mend-the-vault",
            (Self::Prune, Pass::Preview) => "count-what-prune-takes",
            (Self::Prune, Pass::Apply) => "prune-the-vault",
            (Self::Release, Pass::Preview) => "count-what-release-points-back",
            (Self::Release, Pass::Apply) => "release-from-the-vault",
            (Self::Compact, Pass::Preview) => "count-the-covers-to-compact",
            (Self::Compact, Pass::Apply) => "compact-the-vault-covers",
        }
    }

    fn label(self, pass: Pass, running: bool, armed: bool) -> &'static str {
        use words::settings_tending as said;

        match (self, pass, running, armed) {
            (Self::Check, Pass::Preview, false, _) => said::check(),
            (Self::Check, Pass::Preview, true, _) => said::checking(),
            (Self::Check, Pass::Apply, false, false) => said::mend(),
            (Self::Check, Pass::Apply, false, true) => said::press_again_to_mend(),
            (Self::Check, Pass::Apply, true, _) => said::mending(),
            (_, Pass::Preview, false, _) => said::count(),
            (_, Pass::Preview, true, _) => said::counting(),
            (Self::Prune, Pass::Apply, false, false) => said::take_away(),
            (Self::Prune, Pass::Apply, false, true) => said::press_again_to_take_away(),
            (Self::Prune, Pass::Apply, true, _) => said::taking_away(),
            (Self::Release, Pass::Apply, false, false) => said::release(),
            (Self::Release, Pass::Apply, false, true) => said::press_again_to_release(),
            (Self::Release, Pass::Apply, true, _) => said::releasing(),
            (Self::Compact, Pass::Apply, false, false) => said::compact(),
            (Self::Compact, Pass::Apply, false, true) => said::press_again_to_compact(),
            (Self::Compact, Pass::Apply, true, _) => said::compacting(),
        }
    }

    fn asking_again(self) -> &'static str {
        match self {
            Self::Check => words::settings_tending::ask_again_to_mend(),
            Self::Prune => words::settings_tending::ask_again_to_take_away(),
            Self::Release => words::settings_tending::ask_again_to_release(),
            Self::Compact => words::settings_tending::ask_again_to_compact(),
        }
    }

    fn doing(self, pass: Pass) -> &'static str {
        match (self, pass) {
            (Self::Check, Pass::Preview) => words::settings_tending::check_the_vault(),
            (Self::Check, Pass::Apply) => words::settings_tending::mend_the_vault(),
            (Self::Prune, _) => words::settings_tending::prune_the_vault(),
            (Self::Release, _) => words::settings_tending::release_the_tracks_from_the_vault(),
            (Self::Compact, _) => words::settings_tending::compact_the_vault_covers(),
        }
    }

    pub(crate) fn run(self, library: &Library, pass: Pass) -> resonate_library::Result<Tended> {
        let applies = pass.applies();

        Ok(match self {
            Self::Check => Tended::Weighed(library.weigh_the_vault(applies)?.join()?),
            Self::Prune if applies => Tended::Pruned(library.prune_the_vault()?),
            Self::Prune => Tended::Pruned(library.vault_prune_foretold()?),
            Self::Release if applies => Tended::Released(library.release_from_vault(&[])?),
            Self::Release => Tended::Released(library.vault_release_foretold(&[])?),
            Self::Compact if applies => Tended::Compacted(library.compact_the_vault_covers()?),
            Self::Compact => Tended::Compacted(library.vault_covers_compaction_foretold()?),
        })
    }
}

#[derive(Debug)]
pub(crate) enum Tended {
    Weighed(VaultWeighing),
    Pruned(Pruned),
    Released(Released),
    Compacted(Compacted),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Told {
    pub(crate) text: String,
    pub(crate) acts: bool,
}

impl Tended {
    pub(crate) fn told(&self, pass: Pass) -> Told {
        match self {
            Self::Weighed(weighed) => weighing_told(weighed, pass),
            Self::Pruned(pruned) => pruning_told(*pruned, pass),
            Self::Released(released) => releasing_told(*released, pass),
            Self::Compacted(compacted) => compacting_told(*compacted, pass),
        }
    }
}

fn weighing_told(weighed: &VaultWeighing, pass: Pass) -> Told {
    if weighed.held_nothing() {
        return Told {
            text: words::settings_tending::nothing_to_check().to_owned(),
            acts: false,
        };
    }
    if weighed.cancelled {
        return Told {
            text: words::settings_tending::stopped_before_it_finished(checked_told(weighed)),
            acts: false,
        };
    }
    if pass.applies() {
        let mended = weighed.mended.unwrap_or_default();
        let repacked = weighed
            .reframed
            .as_ref()
            .map_or(0, |reframed| reframed.repacked);
        return Told {
            text: words::settings_tending::mended(
                mended.covers_let_go,
                mended.released.released,
                repacked,
                mended.released.stranded,
            ),
            acts: false,
        };
    }
    let mut parts = vec![checked_told(weighed)];
    if !weighed.unframed.is_empty() {
        parts.push(words::settings_tending::packed_as_one_frame(
            weighed.unframed.len(),
        ));
    }
    parts.push(words::settings_tending::nothing_changed().to_owned());
    Told {
        text: parts.join(" · "),
        acts: weighed.has_something_to_mend(),
    }
}

fn checked_told(weighed: &VaultWeighing) -> String {
    words::settings_tending::checked(
        weighed.covers,
        weighed.covers_held,
        weighed.covers_moved.len(),
        weighed.objects_held,
        weighed.objects_moved.len(),
        weighed.objects,
        weighed.unreached.len(),
    )
}

fn weighing_progress_told(stats: VaultWeighingStats) -> Option<String> {
    (stats.to_weigh > 0).then(|| {
        words::settings_tending::checked_so_far(stats.weighed.min(stats.to_weigh), stats.to_weigh)
    })
}

fn pruning_told(pruned: Pruned, pass: Pass) -> Told {
    let text = if pass.applies() {
        words::settings_tending::pruned(pruned.covers, pruned.left, pruned.objects, pruned.staged)
    } else {
        words::settings_tending::would_prune(
            pruned.covers,
            pruned.left,
            pruned.objects,
            pruned.staged,
        )
    };
    Told {
        text,
        acts: pruned.objects + pruned.covers + pruned.staged > 0,
    }
}

fn releasing_told(released: Released, pass: Pass) -> Told {
    let text = if pass.applies() {
        words::settings_tending::released(released.released, released.stranded)
    } else {
        words::settings_tending::would_release(released.released, released.stranded)
    };
    Told {
        text,
        acts: released.released > 0,
    }
}

fn compacting_told(compacted: Compacted, pass: Pass) -> Told {
    let now = format::bytes(compacted.now);
    let saved = format::bytes(compacted.was.saturating_sub(compacted.now));
    let was = format::bytes(compacted.was);
    let text = if pass.applies() {
        words::settings_tending::compacted(
            compacted.covers,
            compacted.kept,
            compacted.left,
            now,
            saved,
            was,
        )
    } else {
        words::settings_tending::would_compact(
            compacted.covers,
            compacted.kept,
            compacted.left,
            now,
            saved,
            was,
        )
    };
    Told {
        text,
        acts: compacted.covers > 0,
    }
}

pub(crate) struct Tending {
    running: Option<(Tend, Pass)>,
    counted: BTreeMap<Tend, Told>,
    weighing: Option<Arc<VaultWeighingProgress>>,
    task: Task<()>,
}

impl Default for Tending {
    fn default() -> Self {
        Self {
            running: None,
            counted: BTreeMap::new(),
            weighing: None,
            task: Task::ready(()),
        }
    }
}

impl Tending {
    pub(crate) const fn running(&self) -> Option<(Tend, Pass)> {
        self.running
    }

    pub(crate) fn counted(&self, tend: Tend) -> Option<&Told> {
        self.counted.get(&tend)
    }

    pub(crate) fn weighing(&self) -> Option<&Arc<VaultWeighingProgress>> {
        self.weighing.as_ref()
    }
}

impl RootView {
    pub(super) fn tending_rows(&mut self, held_back: bool, cx: &mut Context<Self>) -> Vec<Div> {
        Tend::ALL
            .into_iter()
            .map(|tend| self.tending_row(tend, held_back, cx))
            .collect()
    }

    fn tending_row(&mut self, tend: Tend, held_back: bool, cx: &mut Context<Self>) -> Div {
        let held_back = held_back || self.tending.running().is_some();
        let counted = self.tending.counted(tend).cloned();
        let acts = counted.as_ref().is_some_and(|counted| counted.acts);
        let armed = self.is_armed(&Armed::TendTheVault(tend)) && acts;
        let running = self.tending.running();
        let previewing = running == Some((tend, Pass::Preview));
        let applying = running == Some((tend, Pass::Apply));
        let progress = (tend == Tend::Check && (previewing || applying))
            .then(|| self.tending.weighing().map(|weighing| weighing.snapshot()))
            .flatten()
            .and_then(weighing_progress_told);

        let preview = action(
            tend.id(Pass::Preview),
            tend.label(Pass::Preview, previewing, false),
            Icon::Search,
            held_back,
            move |this, _, cx| this.tend_to_the_vault(tend, Pass::Preview, cx),
            self,
            cx,
        );
        let stop =
            (tend == Tend::Check && (previewing || applying)).then(|| self.stop_tending_row(cx));
        let apply = action(
            tend.id(Pass::Apply),
            tend.label(Pass::Apply, applying, armed),
            Icon::Tidy,
            held_back || !acts,
            move |this, _, cx| {
                if armed {
                    this.tend_to_the_vault(tend, Pass::Apply, cx);
                } else {
                    this.arm(Armed::TendTheVault(tend));
                    cx.notify();
                }
            },
            self,
            cx,
        );

        div()
            .flex()
            .flex_col()
            .child(kit::row(
                tend.name(),
                tend.under(),
                kit::actions().children(stop).child(preview).child(apply),
            ))
            .when_some(progress, |body, progress| body.child(note(progress)))
            .when_some(counted, |body, counted| body.child(note(counted.text)))
            .when(armed, |body| body.child(note(tend.asking_again())))
    }

    fn stop_tending_row(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let stopping = self
            .tending
            .weighing()
            .is_some_and(|weighing| weighing.is_cancelled());

        action(
            "stop-checking-the-vault",
            if stopping {
                words::settings_tending::stopping()
            } else {
                words::settings_tending::stop()
            },
            Icon::Stop,
            stopping,
            |this, _, cx| this.stop_tending_the_vault(cx),
            self,
            cx,
        )
    }

    pub(crate) fn stop_tending_the_vault(&mut self, cx: &mut Context<Self>) {
        if let Some(weighing) = self.tending.weighing() {
            weighing.cancel();
            cx.notify();
        }
    }

    fn weigh_the_vault(&mut self, pass: Pass, library: &Library, cx: &mut Context<Self>) {
        let handle = match library.weigh_the_vault(pass.applies()) {
            Ok(handle) => handle,
            Err(error) => {
                self.tended(Tend::Check, pass, Err(error), cx);
                return;
            }
        };

        self.tending.weighing = Some(Arc::clone(handle.progress()));
        self.tending.running = Some((Tend::Check, pass));
        self.tending.task = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor()
                    .timer(WEIGHING_LOOKED_AT_EVERY)
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
            let done = handle.join().map(Tended::Weighed);
            let _ = this.update(cx, |this, cx| this.tended(Tend::Check, pass, done, cx));
        });
        cx.notify();
    }

    pub(crate) fn tend_to_the_vault(&mut self, tend: Tend, pass: Pass, cx: &mut Context<Self>) {
        self.lower(&Armed::TendTheVault(tend));
        if self.tending.running.is_some() {
            return;
        }

        let library = self.library.read(cx).catalog();
        if tend == Tend::Check {
            self.weigh_the_vault(pass, &library, cx);
            return;
        }

        let done = cx
            .background_executor()
            .spawn(async move { tend.run(&library, pass) });
        self.tending.running = Some((tend, pass));
        self.tending.task = cx.spawn(async move |this, cx| {
            let done = done.await;
            let _ = this.update(cx, |this, cx| this.tended(tend, pass, done, cx));
        });
        cx.notify();
    }

    fn tended(
        &mut self,
        tend: Tend,
        pass: Pass,
        done: resonate_library::Result<Tended>,
        cx: &mut Context<Self>,
    ) {
        self.tending.running = None;
        self.tending.weighing = None;
        match done {
            Ok(done) if pass.applies() => {
                self.tending.counted.clear();
                toast::tell(Notice::Done(done.told(pass).text), cx);
                self.library.update(cx, |library, cx| library.reload(cx));
            }
            Ok(done) => {
                self.tending.counted.insert(tend, done.told(pass));
            }
            Err(error) => {
                tracing::error!(%error, "the vault could not be tended to");
                toast::tell(toast::could_not(tend.doing(pass), &error), cx);
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::TestAppContext;
    use resonate_library::{ImportOptions, Sources, Vault, VaultWeighingStats};

    use super::{super::pages::pressed, *};
    use crate::driven::{Driven, Folder};

    #[test]
    fn a_pass_over_nothing_acts_on_nothing_and_says_so() {
        let empty = Tended::Weighed(VaultWeighing::default());

        let told = empty.told(Pass::Preview);

        assert!(!told.acts);
        assert_eq!(told.text, words::settings_tending::nothing_to_check());
    }

    #[test]
    fn a_count_that_finds_something_offers_to_act_and_one_that_finds_nothing_does_not() {
        let nothing = Tended::Pruned(Pruned::default()).told(Pass::Preview);
        let something = Tended::Pruned(Pruned {
            objects: 2,
            ..Pruned::default()
        })
        .told(Pass::Preview);

        assert!(!nothing.acts);
        assert!(something.acts);
        assert!(something.text.starts_with("would take away objects 2"));
    }

    #[test]
    fn a_release_that_would_point_no_row_back_offers_nothing() {
        let stranded = Tended::Released(Released {
            released: 0,
            stranded: 3,
        })
        .told(Pass::Preview);
        let pointing = Tended::Released(Released {
            released: 1,
            stranded: 0,
        })
        .told(Pass::Preview);

        assert!(!stranded.acts);
        assert!(pointing.acts);
    }

    #[test]
    fn a_compaction_says_what_it_would_save() {
        let compacted = Compacted {
            covers: 2,
            was: 3 * 1024 * 1024,
            now: 1024 * 1024,
            kept: 0,
            left: 0,
        };

        let counted = compacting_told(compacted, Pass::Preview);
        let done = compacting_told(compacted, Pass::Apply);

        assert!(counted.acts);
        assert!(counted.text.starts_with("would swap covers 2"));
        assert!(done.text.starts_with("swapped covers 2"));
        assert!(done.text.contains(&format::bytes(2 * 1024 * 1024)));
    }

    #[test]
    fn every_action_of_the_vault_has_an_id_of_its_own() {
        let mut ids: Vec<_> = Tend::ALL
            .into_iter()
            .flat_map(|tend| [tend.id(Pass::Preview), tend.id(Pass::Apply)])
            .collect();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), 8);
    }

    const LONG_ENOUGH_TO_STOP_IN_SECONDS: u32 = 240;

    fn a_vault_holding_two_songs(
        cx: &mut TestAppContext,
    ) -> (Driven, Arc<Library>, Arc<Vault>, Folder, Folder) {
        let music = Folder::new();
        let held = Folder::new();
        music.tone("Echoes.wav", 1);
        music.tone("Time.wav", 2);
        let vault = Arc::new(Vault::make(held.path()).expect("a vault"));
        let library =
            Arc::new(Library::open_in_memory_with_vault(Arc::clone(&vault)).expect("a catalog"));
        Driven::scanned(&library, &music);
        let summary = library
            .import(
                Arc::new(Sources::local()),
                ImportOptions {
                    apply: true,
                    ..ImportOptions::default()
                },
            )
            .expect("an import starts")
            .join()
            .expect("an import ends");
        assert_eq!(summary.stats.vaulted, 2);

        let mut driven = Driven::open(cx, Arc::clone(&library));
        driven.click("tab-settings");
        driven.click("category-Library");
        (driven, library, vault, music, held)
    }

    fn counted(driven: &mut Driven, tend: Tend) -> Told {
        driven.until(|root, _| {
            root.tending.running().is_none() && root.tending.counted(tend).is_some()
        });
        driven.read(|root, _| root.tending.counted(tend).cloned().expect("a count"))
    }

    fn stands_in(library: &Library, title: &str) -> bool {
        let row = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the tracks")
            .into_iter()
            .find(|track| track.title == title)
            .expect("the track");
        library
            .stand_in()
            .stands_in(&row.location, row.span)
            .is_some()
    }

    #[gpui::test]
    fn checking_a_sound_vault_reads_it_back_and_offers_nothing_to_mend(cx: &mut TestAppContext) {
        let (mut driven, library, _vault, _music, _held) = a_vault_holding_two_songs(cx);

        pressed(&mut driven, "check-the-vault");
        let told = counted(&mut driven, Tend::Check);

        assert!(
            told.text
                .starts_with("checked 2 · read back 2 · did not read back 0")
        );
        assert!(!told.acts);
        assert!(
            library
                .vault_objects()
                .expect("objects")
                .iter()
                .all(|object| object.validated)
        );

        pressed(&mut driven, "mend-the-vault");
        pressed(&mut driven, "mend-the-vault");
        driven.settle();

        assert!(!driven.read(|root, _| root.is_armed(&Armed::TendTheVault(Tend::Check))));
        assert!(stands_in(&library, "Echoes") && stands_in(&library, "Time"));
    }

    fn a_vault_holding_two_long_songs(
        cx: &mut TestAppContext,
    ) -> (Driven, Arc<Library>, Folder, Folder) {
        let music = Folder::new();
        let held = Folder::new();
        music.tone("Echoes.wav", LONG_ENOUGH_TO_STOP_IN_SECONDS);
        music.tone("Time.wav", LONG_ENOUGH_TO_STOP_IN_SECONDS + 1);
        let vault = Arc::new(Vault::make(held.path()).expect("a vault"));
        let library =
            Arc::new(Library::open_in_memory_with_vault(Arc::clone(&vault)).expect("a catalog"));
        Driven::scanned(&library, &music);
        library
            .import(
                Arc::new(Sources::local()),
                ImportOptions {
                    apply: true,
                    ..ImportOptions::default()
                },
            )
            .expect("an import starts")
            .join()
            .expect("an import ends");

        let mut driven = Driven::open(cx, Arc::clone(&library));
        driven.click("tab-settings");
        driven.click("category-Library");
        (driven, library, music, held)
    }

    #[test]
    fn a_weighing_under_way_says_how_far_it_has_come_and_says_nothing_before_it_has_counted() {
        let begun = VaultWeighingStats {
            to_weigh: 0,
            weighed: 0,
        };
        let under_way = VaultWeighingStats {
            to_weigh: 12,
            weighed: 5,
        };

        assert_eq!(weighing_progress_told(begun), None);
        assert_eq!(
            weighing_progress_told(under_way),
            Some(words::settings_tending::checked_so_far(5, 12))
        );
        assert_eq!(
            words::settings_tending::checked_so_far(5, 12),
            "Checked 5 of 12"
        );
    }

    #[test]
    fn a_weighing_that_was_stopped_says_so_and_offers_nothing_to_mend() {
        let stopped = Tended::Weighed(VaultWeighing {
            objects: 3,
            objects_held: 1,
            objects_moved: vec!["broken".into()],
            cancelled: true,
            ..VaultWeighing::default()
        });

        let told = stopped.told(Pass::Preview);
        let mended = stopped.told(Pass::Apply);

        assert!(!told.acts);
        assert!(told.text.starts_with("checked 3 · read back 1"));
        assert!(told.text.ends_with("stopped before it finished"));
        assert!(mended.text.ends_with("stopped before it finished"));
    }

    #[gpui::test]
    fn a_check_shows_how_far_it_has_come_and_stop_ends_it_before_the_vault_is_read(
        cx: &mut TestAppContext,
    ) {
        let (mut driven, _library, _music, _held) = a_vault_holding_two_long_songs(cx);

        pressed(&mut driven, "check-the-vault");
        driven.until(|root, _| {
            root.tending
                .weighing()
                .is_some_and(|weighing| weighing.snapshot().to_weigh == 2)
        });
        let watched = driven.read(|root, _| root.tending.weighing().cloned().expect("a weighing"));
        assert_eq!(
            driven.read(|root, _| root.tending.running()),
            Some((Tend::Check, Pass::Preview))
        );
        assert!(!watched.is_cancelled());

        pressed(&mut driven, "stop-checking-the-vault");
        assert!(driven.read(|root, _| {
            root.tending
                .weighing()
                .is_some_and(|weighing| weighing.is_cancelled())
        }));
        let told = counted(&mut driven, Tend::Check);

        assert!(!told.acts);
        assert!(told.text.ends_with("stopped before it finished"));
        assert!(driven.read(|root, _| root.tending.weighing().is_none()));
        assert!(watched.snapshot().weighed < watched.snapshot().to_weigh);
    }

    #[gpui::test]
    fn mending_waits_for_a_second_press_and_points_a_broken_object_back_at_its_file(
        cx: &mut TestAppContext,
    ) {
        let (mut driven, library, _vault, _music, _held) = a_vault_holding_two_songs(cx);
        let broken = library.vault_objects().expect("objects").remove(0);
        std::fs::write(&broken.path, b"no longer what went in").expect("an object overwritten");

        pressed(&mut driven, "check-the-vault");
        let told = counted(&mut driven, Tend::Check);
        assert!(told.acts);
        assert!(told.text.contains("did not read back 1"));

        pressed(&mut driven, "mend-the-vault");
        assert!(driven.read(|root, _| root.is_armed(&Armed::TendTheVault(Tend::Check))));
        assert!(driven.read(|root, _| root.tending.running().is_none()));
        let standing = ["Echoes", "Time"].map(|title| stands_in(&library, title));
        assert_eq!(standing, [true, true]);

        pressed(&mut driven, "mend-the-vault");
        driven.until(|root, _| {
            root.tending.running().is_none() && root.tending.counted(Tend::Check).is_none()
        });

        let standing = ["Echoes", "Time"].map(|title| stands_in(&library, title));
        assert_eq!(standing.iter().filter(|stands| **stands).count(), 1);
        assert!(!driven.read(|root, _| root.is_armed(&Armed::TendTheVault(Tend::Check))));
    }

    #[gpui::test]
    fn releasing_counts_first_and_points_every_track_back_at_its_own_file_once_asked_twice(
        cx: &mut TestAppContext,
    ) {
        let (mut driven, library, _vault, _music, _held) = a_vault_holding_two_songs(cx);

        pressed(&mut driven, "release-from-the-vault");
        assert!(!driven.read(|root, _| root.is_armed(&Armed::TendTheVault(Tend::Release))));

        pressed(&mut driven, "count-what-release-points-back");
        let told = counted(&mut driven, Tend::Release);
        assert!(told.acts);
        assert!(told.text.starts_with("would release 2"));
        assert!(stands_in(&library, "Echoes"));

        pressed(&mut driven, "release-from-the-vault");
        assert!(driven.read(|root, _| root.is_armed(&Armed::TendTheVault(Tend::Release))));
        assert!(stands_in(&library, "Echoes"));

        pressed(&mut driven, "release-from-the-vault");
        driven.until(|root, _| {
            root.tending.running().is_none() && root.tending.counted(Tend::Release).is_none()
        });

        assert!(!stands_in(&library, "Echoes") && !stands_in(&library, "Time"));
    }

    #[gpui::test]
    fn pruning_takes_away_the_objects_a_release_left_and_nothing_before_it(
        cx: &mut TestAppContext,
    ) {
        let (mut driven, library, vault, _music, _held) = a_vault_holding_two_songs(cx);

        pressed(&mut driven, "count-what-prune-takes");
        let nothing = counted(&mut driven, Tend::Prune);
        assert!(!nothing.acts);

        library.release_from_vault(&[]).expect("a release");
        let objects = vault.holding().expect("a count").objects;
        pressed(&mut driven, "count-what-prune-takes");
        driven.until(|root, _| {
            root.tending
                .counted(Tend::Prune)
                .is_some_and(|told| told.acts)
        });
        assert_eq!(vault.holding().expect("a count").objects, objects);

        pressed(&mut driven, "prune-the-vault");
        pressed(&mut driven, "prune-the-vault");
        driven.until(|root, _| {
            root.tending.running().is_none() && root.tending.counted(Tend::Prune).is_none()
        });

        assert_eq!(vault.holding().expect("a count").objects, 0);
        assert_eq!(
            library.vault_prune_foretold().expect("a count"),
            Pruned::default()
        );
    }

    #[gpui::test]
    fn compacting_a_vault_with_no_covers_counts_nothing_and_offers_nothing(
        cx: &mut TestAppContext,
    ) {
        let (mut driven, _library, _vault, _music, _held) = a_vault_holding_two_songs(cx);

        pressed(&mut driven, "count-the-covers-to-compact");
        let told = counted(&mut driven, Tend::Compact);
        assert!(!told.acts);
        assert!(told.text.starts_with("would swap covers 0"));

        pressed(&mut driven, "compact-the-vault-covers");
        assert!(!driven.read(|root, _| root.is_armed(&Armed::TendTheVault(Tend::Compact))));
    }

    #[gpui::test]
    fn arming_one_action_of_the_vault_lowers_another(cx: &mut TestAppContext) {
        let (mut driven, library, _vault, _music, _held) = a_vault_holding_two_songs(cx);
        library.release_from_vault(&[]).expect("a release");

        pressed(&mut driven, "count-what-release-points-back");
        pressed(&mut driven, "count-what-prune-takes");
        driven.until(|root, _| {
            root.tending
                .counted(Tend::Prune)
                .is_some_and(|told| told.acts)
        });
        pressed(&mut driven, "prune-the-vault");
        assert!(driven.read(|root, _| root.is_armed(&Armed::TendTheVault(Tend::Prune))));

        pressed(&mut driven, "check-the-vault");
        driven.until(|root, _| root.tending.counted(Tend::Check).is_some());
        pressed(&mut driven, "count-what-release-points-back");
        driven.settle();

        assert!(driven.read(|root, _| root.tending.running().is_none()));
    }
}
