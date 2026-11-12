use resonate_core::ZoneId;
use resonate_engine::{Keep, Keeping, Player, PlayerState, Queued};
use resonate_library::Library;

use crate::config::Config;

pub(crate) fn resuming(config: &Config, library: Option<&Library>) -> bool {
    if config.resumes() {
        return true;
    }
    if let Some(library) = library
        && let Err(error) = library.forget_resumption()
    {
        tracing::warn!(%error, "what was kept of a queue could not be discarded");
    }
    false
}

pub(crate) fn keep_the_queue(player: &Player, library: Option<&Library>, keeping: &mut Keeping) {
    keep_the_queue_by(player, library, keeping, Keeping::kept);
}

pub(crate) fn keep_the_queue_on_leaving(
    player: &Player,
    library: Option<&Library>,
    keeping: &mut Keeping,
) {
    keep_the_queue_by(player, library, keeping, Keeping::kept_on_leaving);
}

fn keep_the_queue_by(
    player: &Player,
    library: Option<&Library>,
    keeping: &mut Keeping,
    keep: fn(&mut Keeping, &PlayerState, &Queued) -> Option<Keep>,
) {
    let Some(library) = library else {
        return;
    };
    let kept = match keep(keeping, &player.state(), &player.queued()) {
        Some(Keep::Queue(resumption)) => library.keep_resumption(ZoneId::FIRST, &resumption),
        Some(Keep::Order(reordered)) => library.keep_order(ZoneId::FIRST, &reordered),
        Some(Keep::Place { row, at }) => library.keep_place(ZoneId::FIRST, row, at),
        None => return,
    };

    if let Err(error) = kept {
        tracing::warn!(%error, "the queue was not kept for the next run");
    }
}
