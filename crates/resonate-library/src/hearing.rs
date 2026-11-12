use std::{cell::Cell, result};

use crate::{Error, Fruitless, LookupOp, REFUSALS_THAT_END_A_PASS, Result, Wording};

#[derive(Debug, PartialEq, Eq)]
pub enum Reply<T> {
    Answered(T),
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefusedBefore(u32);

#[derive(Debug, Default)]
pub struct Refusing {
    refusals: Cell<u32>,
    in_a_row: Cell<u32>,
}

impl Refusing {
    pub fn heard<T>(&self, answered: Result<T>, counted: impl FnOnce()) -> Result<Reply<T>> {
        match answered {
            Ok(value) => {
                self.in_a_row.set(0);
                Ok(Reply::Answered(value))
            }
            Err(Error::Refused { op, status }) => {
                tracing::warn!(?op, status, "the reference refused a lookup");
                self.refused(op, counted)?;
                Ok(Reply::Refused)
            }
            Err(Error::Unreadable { op }) => {
                tracing::warn!(
                    ?op,
                    "the reference answered with something this build cannot read"
                );
                self.refused(op, counted)?;
                Ok(Reply::Refused)
            }
            Err(Error::TooLarge { op, limit }) => {
                tracing::warn!(
                    ?op,
                    limit,
                    "the reference answered with a document too large to read, which asking again would not change"
                );
                Ok(Reply::Refused)
            }
            Err(other) => Err(other),
        }
    }

    fn refused(&self, op: LookupOp, counted: impl FnOnce()) -> Result<()> {
        self.refusals.set(self.refusals.get().saturating_add(1));
        counted();
        let refusals = self.in_a_row.get().saturating_add(1);
        self.in_a_row.set(refusals);
        if refusals >= REFUSALS_THAT_END_A_PASS {
            return Err(Error::RefusedInARow { op, refusals });
        }
        Ok(())
    }

    pub fn since(&self) -> RefusedBefore {
        RefusedBefore(self.refusals.get())
    }

    pub fn fruitlessly(&self, since: RefusedBefore) -> Fruitless {
        if self.refusals.get() > since.0 {
            Fruitless::Refused
        } else {
            Fruitless::Missed
        }
    }
}

pub fn either_way<T, W, E>(
    mut ask: impl FnMut(Wording) -> result::Result<Reply<Vec<T>>, E>,
    mut weigh: impl FnMut(Vec<T>) -> result::Result<W, E>,
    landed: impl Fn(&W) -> bool,
    phrase_missed: impl FnOnce(),
) -> result::Result<Reply<W>, E> {
    let phrased = match ask(Wording::Phrase)? {
        Reply::Answered(found) => weigh(found)?,
        Reply::Refused => return Ok(Reply::Refused),
    };
    if landed(&phrased) {
        return Ok(Reply::Answered(phrased));
    }

    phrase_missed();
    match ask(Wording::Words)? {
        Reply::Answered(found) => weigh(found).map(Reply::Answered),
        Reply::Refused => Ok(Reply::Refused),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{Refusing, Reply, either_way};
    use crate::{Error, Fruitless, LookupOp, REFUSALS_THAT_END_A_PASS, Wording};

    fn refused() -> crate::Result<()> {
        Err(Error::Refused {
            op: LookupOp::Release,
            status: 503,
        })
    }

    #[test]
    fn a_run_of_refusals_ends_the_pass_and_an_answer_between_them_starts_the_run_again() {
        let refusing = Refusing::default();
        let counted = Cell::new(0);

        for _ in 1..REFUSALS_THAT_END_A_PASS {
            assert_eq!(
                refusing
                    .heard(refused(), || counted.set(counted.get() + 1))
                    .unwrap(),
                Reply::Refused
            );
        }
        assert_eq!(refusing.heard(Ok(()), || {}).unwrap(), Reply::Answered(()));
        for _ in 1..REFUSALS_THAT_END_A_PASS {
            refusing.heard(refused(), || {}).unwrap();
        }

        assert_eq!(counted.get(), REFUSALS_THAT_END_A_PASS - 1);
        assert!(matches!(
            refusing.heard(refused(), || {}),
            Err(Error::RefusedInARow { .. })
        ));
    }

    #[test]
    fn a_document_too_large_is_refused_without_counting_toward_the_run() {
        let refusing = Refusing::default();
        let since = refusing.since();

        let heard = refusing
            .heard(
                Err::<(), _>(Error::TooLarge {
                    op: LookupOp::Release,
                    limit: 1,
                }),
                || panic!("a document too large is no refusal counted"),
            )
            .unwrap();

        assert_eq!(heard, Reply::Refused);
        assert_eq!(refusing.fruitlessly(since), Fruitless::Missed);
    }

    #[test]
    fn an_ask_is_fruitless_by_refusal_only_where_a_refusal_came_after_it_began() {
        let refusing = Refusing::default();
        refusing.heard(refused(), || {}).unwrap();
        let since = refusing.since();

        assert_eq!(refusing.fruitlessly(since), Fruitless::Missed);
        refusing.heard(refused(), || {}).unwrap();
        assert_eq!(refusing.fruitlessly(since), Fruitless::Refused);
    }

    #[test]
    fn the_words_are_asked_only_where_the_phrase_landed_nothing() {
        let asked = Cell::new(Vec::new());
        let ask = |wording| {
            let mut so_far = asked.take();
            so_far.push(wording);
            asked.set(so_far);
            Ok::<_, ()>(Reply::Answered(vec![wording == Wording::Words]))
        };

        let landed = either_way(ask, |found| Ok(found[0]), |landed| *landed, || {});

        assert_eq!(landed, Ok(Reply::Answered(true)));
        assert_eq!(asked.take(), vec![Wording::Phrase, Wording::Words]);
    }

    #[test]
    fn a_refused_phrase_asks_no_words() {
        let words_asked = Cell::new(false);

        let heard = either_way(
            |wording| {
                words_asked.set(words_asked.get() || wording == Wording::Words);
                Ok::<Reply<Vec<()>>, ()>(Reply::Refused)
            },
            |_| Ok(()),
            |()| false,
            || {},
        );

        assert_eq!(heard, Ok(Reply::Refused));
        assert!(!words_asked.get());
    }
}
