use resonate_core::{Switch, ZoneId};
use resonate_engine::{Keep, Keeping, Player, PlayerState, Queued};
use resonate_library::Library;
use resonate_mpris::{Running, Session};

use crate::config::Config;

pub(crate) fn resuming(config: &Config, library: Option<&Library>) -> bool {
    if config.is_on(Switch::Resume) {
        return true;
    }
    if let Some(library) = library
        && let Err(error) = library.forget_resumption()
    {
        tracing::warn!(%error, "what was kept of a queue could not be discarded");
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Moment {
    Playing,
    Leaving,
}

impl Moment {
    fn kept(self, keeping: &mut Keeping, state: &PlayerState, queued: &Queued) -> Option<Keep> {
        match self {
            Self::Playing => keeping.kept(state, queued),
            Self::Leaving => keeping.kept_on_leaving(state, queued),
        }
    }
}

pub(crate) struct KeptBesideTheWindow {
    keeping: Keeping,
    held_back: bool,
    a_window_runs: Box<dyn FnMut() -> bool>,
}

impl KeptBesideTheWindow {
    pub(crate) fn asking_the_session() -> Self {
        let session = Session::open().ok();

        Self::watching(move || {
            session
                .as_ref()
                .is_some_and(|session| matches!(Running::a_window_in(session), Ok(Some(_))))
        })
    }

    fn watching(a_window_runs: impl FnMut() -> bool + 'static) -> Self {
        Self {
            keeping: Keeping::default(),
            held_back: false,
            a_window_runs: Box::new(a_window_runs),
        }
    }

    pub(crate) fn keep(&mut self, player: &Player, library: Option<&Library>) {
        if let Some(library) = library {
            self.keep_into(library, Moment::Playing, &player.state(), &player.queued());
        }
    }

    pub(crate) fn keep_on_leaving(&mut self, player: &Player, library: Option<&Library>) {
        if let Some(library) = library {
            self.keep_into(library, Moment::Leaving, &player.state(), &player.queued());
        }
    }

    fn keep_into(
        &mut self,
        library: &Library,
        moment: Moment,
        state: &PlayerState,
        queued: &Queued,
    ) {
        let kept = match self.kept(moment, state, queued) {
            Some(Keep::Queue(resumption)) => library.keep_resumption(ZoneId::FIRST, &resumption),
            Some(Keep::Order(reordered)) => library.keep_order(ZoneId::FIRST, &reordered),
            Some(Keep::Place { row, at }) => library.keep_place(ZoneId::FIRST, row, at),
            None => return,
        };

        if let Err(error) = kept {
            tracing::warn!(%error, "the queue was not kept for the next run");
        }
    }

    fn kept(&mut self, moment: Moment, state: &PlayerState, queued: &Queued) -> Option<Keep> {
        let answer = moment.kept(&mut self.keeping, state, queued);
        let owed_as_it_leaves = self.held_back && moment == Moment::Leaving;

        if answer.is_none() && !owed_as_it_leaves {
            return None;
        }
        if (self.a_window_runs)() {
            self.held_back = true;
            return None;
        }
        if !std::mem::take(&mut self.held_back) {
            return answer;
        }

        self.keeping = Keeping::default();
        moment.kept(&mut self.keeping, state, queued)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use resonate_core::{MediaLocation, Resumption, TrackId};
    use resonate_engine::{QueueItem, stamp_of};

    use super::*;

    fn queue(names: &[&str]) -> Queued {
        let items: Vec<QueueItem> = names
            .iter()
            .enumerate()
            .map(|(number, name)| QueueItem {
                id: TrackId::new(number as u64 + 1).expect("track ids start at one"),
                location: MediaLocation::local(format!("/music/{name}.flac")),
                span: None,
            })
            .collect();

        Queued {
            stamp: stamp_of(&items),
            rows: items.into_iter().collect(),
            ..Queued::default()
        }
    }

    fn at_row(row: usize) -> PlayerState {
        PlayerState {
            queue_position: Some(row),
            ..PlayerState::default()
        }
    }

    fn kept_rows(library: &Library) -> Option<(Vec<MediaLocation>, usize)> {
        library
            .resumption_of(ZoneId::FIRST)
            .expect("the kept queue reads")
            .map(|Resumption { rows, row, .. }| {
                (rows.into_iter().map(|kept| kept.location).collect(), row)
            })
    }

    fn the_window(shown: bool) -> Rc<Cell<bool>> {
        Rc::new(Cell::new(shown))
    }

    fn beside(window: &Rc<Cell<bool>>) -> KeptBesideTheWindow {
        let window = Rc::clone(window);

        KeptBesideTheWindow::watching(move || window.get())
    }

    fn the_window_keeps(library: &Library, names: &[&str], row: usize) {
        let state = at_row(row);
        let queued = queue(names);

        library
            .keep_resumption(
                ZoneId::FIRST,
                &resonate_engine::resumption_of(&state, &queued),
            )
            .expect("the window keeps its queue");
    }

    #[test]
    fn a_play_beside_a_running_window_keeps_nothing_of_its_queue() {
        let library = Library::open_in_memory().expect("a catalog");
        let window = the_window(true);
        let mut keeper = beside(&window);

        the_window_keeps(&library, &["window-one", "window-two", "window-three"], 2);
        let windows = kept_rows(&library);
        let queued = queue(&["terminal-one", "terminal-two"]);

        keeper.keep_into(&library, Moment::Playing, &at_row(0), &queued);
        keeper.keep_into(&library, Moment::Playing, &at_row(1), &queued);
        keeper.keep_into(&library, Moment::Leaving, &at_row(1), &queued);

        assert_eq!(kept_rows(&library), windows);
    }

    #[test]
    fn a_play_with_no_window_running_keeps_its_queue_and_place() {
        let library = Library::open_in_memory().expect("a catalog");
        let window = the_window(false);
        let mut keeper = beside(&window);
        let queued = queue(&["one", "two", "three"]);

        keeper.keep_into(&library, Moment::Playing, &at_row(0), &queued);
        keeper.keep_into(&library, Moment::Playing, &at_row(1), &queued);
        keeper.keep_into(&library, Moment::Leaving, &at_row(2), &queued);

        let (rows, row) = kept_rows(&library).expect("the queue is kept");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], MediaLocation::local("/music/one.flac"));
        assert_eq!(row, 2);
    }

    #[test]
    fn a_window_opening_while_play_runs_stops_its_keeping_from_landing_on_the_windows_queue() {
        let library = Library::open_in_memory().expect("a catalog");
        let window = the_window(false);
        let mut keeper = beside(&window);
        let queued = queue(&["terminal-one", "terminal-two", "terminal-three"]);

        keeper.keep_into(&library, Moment::Playing, &at_row(0), &queued);
        window.set(true);
        the_window_keeps(&library, &["window-one", "window-two"], 0);
        let windows = kept_rows(&library);

        keeper.keep_into(&library, Moment::Playing, &at_row(2), &queued);
        keeper.keep_into(&library, Moment::Leaving, &at_row(2), &queued);

        assert_eq!(kept_rows(&library), windows);
    }

    #[test]
    fn a_window_gone_before_play_leaves_has_the_whole_queue_kept_again_not_just_its_place() {
        let library = Library::open_in_memory().expect("a catalog");
        let window = the_window(false);
        let mut keeper = beside(&window);
        let queued = queue(&["terminal-one", "terminal-two", "terminal-three"]);

        keeper.keep_into(&library, Moment::Playing, &at_row(0), &queued);
        window.set(true);
        keeper.keep_into(&library, Moment::Playing, &at_row(1), &queued);
        the_window_keeps(&library, &["window-one", "window-two"], 1);
        window.set(false);
        keeper.keep_into(&library, Moment::Leaving, &at_row(1), &queued);

        let (rows, row) = kept_rows(&library).expect("the queue is kept");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], MediaLocation::local("/music/terminal-one.flac"));
        assert_eq!(row, 1);
    }
}
