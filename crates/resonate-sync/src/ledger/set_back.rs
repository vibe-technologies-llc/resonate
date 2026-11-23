use rusqlite::{Connection, params};

use super::Ledger;
use crate::{
    Error, Result, StoreOp,
    change::{Clock, STAMP_AHEAD_OF_THE_WALL_AT_MOST_NANOS, wall_nanos},
};

const STAMPED_TABLES: [&str; 9] = [
    "devices",
    "playlists",
    "queries",
    "favourites",
    "handoffs",
    "shares",
    "layouts",
    "answers",
    "deletions",
];

const WALL_TIMES_KEPT_HERE: [&str; 3] = [
    "UPDATE deletions SET heard = ?1 WHERE heard > ?1",
    "UPDATE refusals SET refused = ?1 WHERE refused > ?1",
    "UPDATE state SET clock = ?1, joined = min(joined, ?1) WHERE id = 1",
];

fn restamped_to_the_wall(connection: &Connection, wall: i64) -> Result<usize> {
    let bound = wall.saturating_add(STAMP_AHEAD_OF_THE_WALL_AT_MOST_NANOS);
    let mut restamped = 0;

    for table in STAMPED_TABLES {
        restamped += connection
            .execute(
                &format!("UPDATE {table} SET nanos = ?1, owed = 1 WHERE nanos > ?2"),
                params![wall, bound],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
    }
    for kept in WALL_TIMES_KEPT_HERE {
        connection
            .execute(kept, params![wall])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
    }
    Ok(restamped)
}

pub(super) fn kept_or_set_back(connection: &Connection, clock: Clock, wall: i64) -> Result<Clock> {
    if !clock.runs_too_far_ahead_of(wall) {
        return Ok(clock);
    }

    let restamped = restamped_to_the_wall(connection, wall)?;

    tracing::warn!(
        clock = clock.last(),
        wall,
        restamped,
        "the ledger's clock ran too far past the wall clock and was set back with the rows it stamped"
    );
    Ok(Clock::from_last(wall))
}

impl Ledger {
    pub(super) fn set_back_a_clock_run_ahead(&mut self) -> Result<()> {
        let wall = wall_nanos();

        if !self.clock.runs_too_far_ahead_of(wall) {
            return Ok(());
        }

        let clock = self.clock;
        let transaction = self.writing()?;

        let set_back = kept_or_set_back(&transaction, clock, wall)?;

        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        self.clock = set_back;
        Ok(())
    }
}
