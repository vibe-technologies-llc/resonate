use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use parking_lot::Mutex;

pub struct Searches {
    kept_for: Duration,
    bytes_at_most: usize,
    kept: Mutex<Kept>,
}

#[derive(Default)]
struct Kept {
    answers: HashMap<String, Remembered>,
    order: VecDeque<String>,
    bytes: usize,
}

struct Remembered {
    at: Instant,
    answer: Recalled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recalled {
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl Searches {
    pub fn new(kept_for: Duration, bytes_at_most: usize) -> Self {
        Self {
            kept_for,
            bytes_at_most,
            kept: Mutex::new(Kept::default()),
        }
    }

    pub fn recalled(&self, target: &str) -> Option<Recalled> {
        let mut kept = self.kept.lock();
        let remembered = kept.answers.get(target)?;
        if remembered.at.elapsed() < self.kept_for {
            return Some(remembered.answer.clone());
        }
        kept.forget(target);
        None
    }

    pub fn remember(&self, target: &str, content_type: Option<&str>, body: &[u8]) {
        if self.kept_for.is_zero() || body.len() > self.bytes_at_most {
            return;
        }
        let mut kept = self.kept.lock();
        kept.forget(target);
        while kept.bytes + body.len() > self.bytes_at_most {
            let Some(oldest) = kept.order.front().cloned() else {
                break;
            };
            kept.forget(&oldest);
        }
        kept.bytes += body.len();
        kept.order.push_back(target.to_owned());
        kept.answers.insert(
            target.to_owned(),
            Remembered {
                at: Instant::now(),
                answer: Recalled {
                    content_type: content_type.map(str::to_owned),
                    body: body.to_vec(),
                },
            },
        );
    }

    pub fn len(&self) -> usize {
        self.kept.lock().answers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Kept {
    fn forget(&mut self, target: &str) {
        if let Some(forgotten) = self.answers.remove(target) {
            self.bytes -= forgotten.answer.body.len();
            self.order.retain(|ordered| ordered != target);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;

    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);
    const JSON: &str = "application/json";

    fn body_of(searches: &Searches, target: &str) -> Option<Vec<u8>> {
        searches.recalled(target).map(|recalled| recalled.body)
    }

    #[test]
    fn an_answer_is_recalled_for_the_same_target_alone() {
        let searches = Searches::new(HOUR, 1024);

        searches.remember("/search/?i=A", Some(JSON), b"one");

        assert_eq!(
            searches.recalled("/search/?i=A"),
            Some(Recalled {
                content_type: Some(JSON.to_owned()),
                body: b"one".to_vec()
            })
        );
        assert_eq!(body_of(&searches, "/search/?i=B"), None);
    }

    #[test]
    fn an_answer_older_than_the_time_it_is_kept_for_is_forgotten() {
        let searches = Searches::new(Duration::from_millis(20), 1024);

        searches.remember("/search/?i=A", Some(JSON), b"one");
        sleep(Duration::from_millis(60));

        assert_eq!(body_of(&searches, "/search/?i=A"), None);
        assert!(searches.is_empty());
    }

    #[test]
    fn the_oldest_answers_go_first_when_the_bytes_are_spent() {
        let searches = Searches::new(HOUR, 10);

        searches.remember("a", None, b"1234");
        searches.remember("b", None, b"1234");
        searches.remember("c", None, b"1234");

        assert_eq!(body_of(&searches, "a"), None);
        assert!(body_of(&searches, "b").is_some());
        assert!(body_of(&searches, "c").is_some());
        assert_eq!(searches.len(), 2);
    }

    #[test]
    fn an_answer_remembered_again_replaces_itself_and_one_too_large_is_not_kept() {
        let searches = Searches::new(HOUR, 8);

        searches.remember("a", None, b"1234");
        searches.remember("a", None, b"5678");
        searches.remember("b", None, b"123456789");

        assert_eq!(body_of(&searches, "a"), Some(b"5678".to_vec()));
        assert_eq!(body_of(&searches, "b"), None);
        assert_eq!(searches.len(), 1);
    }

    #[test]
    fn nothing_is_kept_where_the_time_is_nil() {
        let searches = Searches::new(Duration::ZERO, 1024);

        searches.remember("a", None, b"1");

        assert!(searches.is_empty());
    }
}
