use rusqlite::params;

use super::Ledger;
use crate::{Error, Result, StoreOp, wire::Incarnation};

pub(super) const A_SPACE_IS_KNOWN_BY_ITS_INCARNATION: &str = "
ALTER TABLE state ADD COLUMN incarnation INTEGER;
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Welcomed {
    Continued,
    MadeAnew,
}

impl Ledger {
    pub fn incarnation(&self) -> Result<Option<Incarnation>> {
        let kept: Option<i64> = self
            .connection
            .query_row("SELECT incarnation FROM state WHERE id = 1", [], |row| {
                row.get(0)
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        Ok(kept.map(|kept| Incarnation(kept.cast_unsigned())))
    }

    pub fn welcomed(&mut self, head: u64, told: Option<Incarnation>) -> Result<Welcomed> {
        let cursor = self.cursor()?;
        let kept = self.incarnation()?;

        let another_incarnation = matches!((kept, told), (Some(kept), Some(told)) if kept != told);
        let welcomed = if head < cursor || another_incarnation {
            Welcomed::MadeAnew
        } else {
            Welcomed::Continued
        };
        let learnt = told.filter(|told| kept != Some(*told));

        if welcomed == Welcomed::Continued && learnt.is_none() {
            return Ok(welcomed);
        }

        let transaction = self.writing()?;

        if welcomed == Welcomed::MadeAnew {
            Self::made_anew(&transaction)?;
        }
        if let Some(learnt) = learnt {
            transaction
                .execute(
                    "UPDATE state SET incarnation = ?1 WHERE id = 1",
                    params![learnt.0.cast_signed()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(welcomed)
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::{DeviceId, SpaceId, ledger::MIGRATIONS};

    const SPACE: [u8; 16] = [5; 16];
    const CURSOR_OF_BEFORE: u64 = 40;

    #[test]
    fn a_ledger_of_before_incarnations_keeps_its_cursor_and_learns_the_first_incarnation_it_is_told()
     {
        let connection = Connection::open_in_memory().expect("a connection");
        let before = MIGRATIONS
            .iter()
            .position(|step| *step == A_SPACE_IS_KNOWN_BY_ITS_INCARNATION)
            .expect("the step is in the chain");

        for step in &MIGRATIONS[..before] {
            connection
                .execute_batch(step)
                .expect("the ledger of before");
        }
        connection
            .execute(
                "INSERT INTO state (id, device, space, cursor) VALUES (1, ?1, ?2, ?3)",
                params![
                    [3_u8; 16].as_slice(),
                    SPACE.as_slice(),
                    CURSOR_OF_BEFORE.cast_signed()
                ],
            )
            .expect("a state row");
        connection
            .execute(
                "INSERT INTO devices (device, name, platform, nanos, stamped_by)
                 VALUES (X'03030303030303030303030303030303', 'Desk', 0, 1, X'03030303030303030303030303030303')",
                [],
            )
            .expect("a device sent before");
        connection
            .pragma_update(
                None,
                "user_version",
                u32::try_from(before).expect("a count"),
            )
            .expect("stamped");

        let mut ledger = Ledger::over(connection).expect("carried forward");

        let kept_before = ledger.incarnation().expect("read");
        let entered_again = ledger.enter_space(SpaceId(SPACE)).expect("the same space");
        let first = ledger
            .welcomed(CURSOR_OF_BEFORE, Some(Incarnation(9)))
            .expect("welcomed");
        let learnt = ledger.incarnation().expect("read");
        let cursor = ledger.cursor().expect("the cursor");
        let owed = ledger.owed_count().expect("owed");

        assert_eq!(ledger.device(), DeviceId([3; 16]));
        assert_eq!(kept_before, None);
        assert!(!entered_again);
        assert_eq!(first, Welcomed::Continued);
        assert_eq!(learnt, Some(Incarnation(9)));
        assert_eq!(cursor, CURSOR_OF_BEFORE);
        assert_eq!(owed, 0);
    }
}
