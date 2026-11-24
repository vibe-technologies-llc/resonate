use resonate_core::{Answered, AnswersMark};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::{AnswerSlot, Ledger};
use crate::{
    DeviceId, Error, Result, StoreOp,
    catalog::Catalog,
    change::Clock,
    wire::{encoded_answer, encoded_recording},
};

pub(super) const ANSWERS_OF_ONE_INSTANT_ARE_TOLD_APART: &str = "
ALTER TABLE state ADD COLUMN answers_after INTEGER;
";

const ANSWERS_LOOKED_AT_ONCE: usize = 500;
const ANSWER_PAGES_A_LOOK: usize = 8;

impl Ledger {
    pub(super) fn look_at_answers(&mut self, catalog: &dyn Catalog) -> Result<()> {
        if !catalog.takes_answers() {
            return Ok(());
        }

        let mut mark = self.answers_mark()?;

        for _ in 0..ANSWER_PAGES_A_LOOK {
            let answered = catalog
                .answers_after(mark, ANSWERS_LOOKED_AT_ONCE)
                .map_err(Error::Catalog)?;
            let Some(last) = answered
                .iter()
                .max_by_key(|answered| (answered.at, answered.id))
            else {
                break;
            };
            let whole_page = answered.len() >= ANSWERS_LOOKED_AT_ONCE;
            let moved_on = mark.lists(last);

            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|source| Error::store(StoreOp::Write, source))?;

            for answered in answered.iter().filter(|answered| answered.answer.travels()) {
                keep_answer_learnt_here(&transaction, &mut self.clock, self.device, answered)?;
            }
            mark = if whole_page {
                AnswersMark::past(last)
            } else {
                AnswersMark::from_the_instant_of(last)
            };
            keep_answers_mark(&transaction, mark)?;
            transaction
                .commit()
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            if !whole_page || !moved_on {
                break;
            }
        }
        Ok(())
    }

    fn answers_mark(&self) -> Result<AnswersMark> {
        self.connection
            .query_row(
                "SELECT answers_mark, answers_after FROM state WHERE id = 1",
                [],
                |row| {
                    Ok(AnswersMark {
                        at: row.get(0)?,
                        after: row.get(1)?,
                    })
                },
            )
            .map_err(|source| Error::store(StoreOp::Read, source))
    }
}

fn keep_answer_learnt_here(
    transaction: &Transaction<'_>,
    clock: &mut Clock,
    device: DeviceId,
    answered: &Answered,
) -> Result<()> {
    let Answered {
        recording, answer, ..
    } = answered;

    let slot = AnswerSlot::of(recording, answer);
    let encoded = encoded_answer(answer);
    let held: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT answer FROM answers WHERE key = ?1 AND kind = ?2 AND file = ?3",
            params![slot.key.as_str(), slot.kind, slot.file],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Read, source))?;

    if held.as_deref() == Some(encoded.as_slice()) {
        return Ok(());
    }

    let stamp = clock.now(device);

    transaction
            .execute(
                "INSERT INTO answers (key, kind, file, recording, answer, here, nanos, stamped_by, owed, named)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, 1, ?8)
                 ON CONFLICT (key, kind, file) DO UPDATE SET recording = ?4, answer = ?5, here = 1,
                     nanos = ?6, stamped_by = ?7, owed = 1, named = ?8",
                params![
                    slot.key.as_str(),
                    slot.kind,
                    slot.file,
                    encoded_recording(recording),
                    encoded,
                    stamp.nanos,
                    device.0.as_slice(),
                    recording.named_key().as_str()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
    Ok(())
}

fn keep_answers_mark(transaction: &Transaction<'_>, mark: AnswersMark) -> Result<()> {
    transaction
        .execute(
            "UPDATE state SET answers_mark = ?1, answers_after = ?2 WHERE id = 1",
            params![mark.at, mark.after],
        )
        .map_err(|source| Error::store(StoreOp::Write, source))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use resonate_core::{Answer, Recording, SharedLyrics};
    use rusqlite::Connection;

    use super::*;
    use crate::ledger::MIGRATIONS;

    const AN_INSTANT: i64 = 1_700_000_000_000_000_000;

    fn learnt(at: i64, id: i64) -> Answered {
        Answered {
            recording: Recording {
                mbid: None,
                title: "Echoes".into(),
                artist: "Pink Floyd".into(),
                album: "Meddle".into(),
                length: None,
            },
            at,
            id,
            answer: Answer::Lyrics(SharedLyrics {
                text: "Overhead the albatross".into(),
                synced: false,
            }),
        }
    }

    #[test]
    fn a_mark_kept_before_answers_of_one_instant_were_told_apart_lists_that_instant_from_its_first_answer()
     {
        let connection = Connection::open_in_memory().expect("a connection");
        let before = MIGRATIONS
            .iter()
            .position(|step| *step == ANSWERS_OF_ONE_INSTANT_ARE_TOLD_APART)
            .expect("the step is in the chain");

        for step in &MIGRATIONS[..before] {
            connection
                .execute_batch(step)
                .expect("the ledger of before");
        }
        connection
            .execute(
                "INSERT INTO state (id, device, answers_mark) VALUES (1, ?1, ?2)",
                params![[3_u8; 16].as_slice(), AN_INSTANT],
            )
            .expect("a state row with a mark");
        connection
            .pragma_update(
                None,
                "user_version",
                u32::try_from(before).expect("a count"),
            )
            .expect("stamped");

        let mut ledger = Ledger::over(connection).expect("carried forward");

        let kept = ledger.answers_mark().expect("a mark");
        let first_of_that_instant = learnt(AN_INSTANT, i64::MIN);
        let an_instant_before = learnt(AN_INSTANT - 1, i64::MAX);
        let next = AnswersMark::past(&first_of_that_instant);
        let transaction = ledger
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("a transaction");

        keep_answers_mark(&transaction, next).expect("kept");
        transaction.commit().expect("committed");

        let kept_again = ledger.answers_mark().expect("a mark");

        assert_eq!(
            kept,
            AnswersMark {
                at: AN_INSTANT,
                after: None
            }
        );
        assert!(kept.lists(&first_of_that_instant));
        assert!(!kept.lists(&an_instant_before));
        assert_eq!(kept_again, next);
        assert!(!kept_again.lists(&first_of_that_instant));
        assert!(kept_again.lists(&learnt(AN_INSTANT, i64::MIN + 1)));
    }
}
