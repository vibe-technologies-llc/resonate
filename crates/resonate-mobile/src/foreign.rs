use std::panic::{self, AssertUnwindSafe};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Callback {
    OpenDocument,
    RewriteDocument,
    #[cfg(any(target_os = "android", test))]
    PreferBitPerfect,
    #[cfg(any(target_os = "android", test))]
    MixFreely,
    PlayerMoved,
    Logbook,
}

pub(crate) fn answered<T>(callback: Callback, call: impl FnOnce() -> T) -> Option<T> {
    let answer = panic::catch_unwind(AssertUnwindSafe(call)).ok();
    if answer.is_none() && callback != Callback::Logbook {
        tracing::warn!(
            ?callback,
            "the front end's callback failed and was answered for it"
        );
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callback_that_answers_is_answered_as_it_said() {
        assert_eq!(answered(Callback::OpenDocument, || 7), Some(7));
    }

    #[test]
    fn a_callback_that_fails_unwinds_no_further_than_the_guard() {
        let answer: Option<i32> = answered(Callback::OpenDocument, || panic!("a thrown exception"));

        assert_eq!(answer, None);
    }
}
