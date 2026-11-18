use std::vec;

use crate::{
    BroughtIn, HeardElsewhere, HeardOnce, History, HistoryFrom, HistoryRead, Library,
    ListeningService, Result, heard_in_lines, line_of,
};

pub const LISTENS_READ_AT_ONCE: usize = 1_000;

pub const LINES_READ_AS: ListeningService = ListeningService::ListenBrainz;

pub trait KeptListens {
    fn history_from(&self, from: HistoryFrom, most: usize) -> Result<HistoryRead>;

    fn bring_in(&self, service: ListeningService, heard: &[HeardElsewhere]) -> Result<BroughtIn>;
}

impl KeptListens for Library {
    fn history_from(&self, from: HistoryFrom, most: usize) -> Result<HistoryRead> {
        Self::history_from(self, from, most)
    }

    fn bring_in(&self, service: ListeningService, heard: &[HeardElsewhere]) -> Result<BroughtIn> {
        Self::bring_in(self, service, heard)
    }
}

pub struct HistoryLines<'a> {
    kept: &'a dyn KeptListens,
    from: Option<HistoryFrom>,
    page: vec::IntoIter<HeardOnce>,
}

impl Iterator for HistoryLines<'_> {
    type Item = Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(heard) = self.page.next() {
                return Some(Ok(line_of(&heard)));
            }
            let from = self.from.take()?;
            match self.kept.history_from(from, LISTENS_READ_AT_ONCE) {
                Ok(read) => {
                    self.from = read.next;
                    self.page = read.heard.into_iter();
                }
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

pub fn history_lines(kept: &dyn KeptListens) -> HistoryLines<'_> {
    HistoryLines {
        kept,
        from: Some(HistoryFrom::START),
        page: Vec::new().into_iter(),
    }
}

pub fn bring_in_lines(
    kept: &dyn KeptListens,
    bytes: &[u8],
    so_far: &mut dyn FnMut(BroughtIn),
) -> Result<BroughtIn> {
    let heard = heard_in_lines(bytes)?;

    let mut brought = BroughtIn::default();
    for page in heard.chunks(LISTENS_READ_AT_ONCE) {
        brought = brought.and(kept.bring_in(LINES_READ_AS, page)?);
        so_far(brought);
    }
    Ok(brought)
}

pub fn bring_in_history(
    kept: &dyn KeptListens,
    history: &dyn History,
    so_far: &mut dyn FnMut(BroughtIn),
) -> Result<BroughtIn> {
    let service = history.service();

    let mut brought = BroughtIn::default();
    let mut from = None;
    loop {
        let page = history.page(from)?;
        brought = brought.and(kept.bring_in(service, &page.heard)?);
        so_far(brought);
        match page.next {
            Some(next) if Some(next) != from => from = Some(next),
            Some(_) | None => return Ok(brought),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        time::{Duration, UNIX_EPOCH},
    };

    use resonate_core::Recording;

    use super::*;
    use crate::HeardPage;

    #[derive(Default)]
    struct Kept {
        held: Vec<HeardOnce>,
        brought: RefCell<Vec<(ListeningService, usize)>>,
    }

    impl KeptListens for Kept {
        fn history_from(&self, from: HistoryFrom, most: usize) -> Result<HistoryRead> {
            let HistoryFrom::Held(after) = from else {
                return Ok(HistoryRead {
                    heard: Vec::new(),
                    next: None,
                });
            };
            let after = usize::try_from(after).expect("a place");
            let heard = self
                .held
                .iter()
                .skip(after)
                .take(most)
                .cloned()
                .collect::<Vec<_>>();
            let next = (after + heard.len() < self.held.len())
                .then(|| HistoryFrom::Held((after + heard.len()) as u64));
            Ok(HistoryRead { heard, next })
        }

        fn bring_in(
            &self,
            service: ListeningService,
            heard: &[HeardElsewhere],
        ) -> Result<BroughtIn> {
            self.brought.borrow_mut().push((service, heard.len()));
            Ok(BroughtIn {
                credited: heard.len() as u64,
                held_already: 0,
                unmatched: 0,
            })
        }
    }

    fn once(second: u64) -> HeardOnce {
        HeardOnce {
            began: UNIX_EPOCH + Duration::from_secs(second),
            heard: Duration::from_secs(1),
            elsewhere: false,
            billed: None,
            location: format!("file:///{second}.flac"),
        }
    }

    struct Paged(Vec<HeardPage>);

    impl History for Paged {
        fn service(&self) -> ListeningService {
            ListeningService::LastFm
        }

        fn page(&self, from: Option<u64>) -> Result<HeardPage> {
            let at = usize::try_from(from.unwrap_or(0)).expect("a page");
            Ok(self.0[at].clone())
        }
    }

    fn heard_at(second: u64) -> HeardElsewhere {
        HeardElsewhere {
            at: UNIX_EPOCH + Duration::from_secs(second),
            recording: Recording {
                mbid: None,
                title: "Words".to_owned(),
                artist: "Low".to_owned(),
                album: String::new(),
                length: None,
            },
        }
    }

    #[test]
    fn every_listen_kept_is_one_line_across_the_pages_it_is_read_in() {
        let kept = Kept {
            held: (0..2_500).map(once).collect(),
            ..Kept::default()
        };

        let lines = history_lines(&kept)
            .collect::<Result<Vec<_>>>()
            .expect("the lines");

        assert_eq!(lines.len(), 2_500);
        assert!(lines[2_499].starts_with("{\"listened_at\":2499,"));
    }

    #[test]
    fn a_services_pages_are_brought_in_until_it_names_no_next_or_the_same_one_again() {
        let kept = Kept::default();
        let history = Paged(vec![
            HeardPage {
                heard: vec![heard_at(1), heard_at(2)],
                next: Some(1),
            },
            HeardPage {
                heard: vec![heard_at(3)],
                next: Some(1),
            },
        ]);
        let mut told = Vec::new();

        let brought = bring_in_history(&kept, &history, &mut |so_far| told.push(so_far.credited))
            .expect("brought in");

        assert_eq!(brought.credited, 3);
        assert_eq!(told, [2, 3]);
        assert_eq!(
            *kept.brought.borrow(),
            [(ListeningService::LastFm, 2), (ListeningService::LastFm, 1)]
        );
    }

    #[test]
    fn a_file_is_brought_in_as_listenbrainz_a_thousand_listens_at_a_time() {
        let kept = Kept {
            held: (0..1_500).map(once).collect(),
            ..Kept::default()
        };
        let lines = history_lines(&kept)
            .map(|line| line.map(|line| format!("{line}\n")))
            .collect::<Result<String>>()
            .expect("the lines");
        let named = lines.replace(
            "\"track_metadata\":{",
            "\"track_metadata\":{\"artist_name\":\"Low\",\"track_name\":\"Words\",",
        );

        let brought = bring_in_lines(&kept, named.as_bytes(), &mut |_| {}).expect("brought in");

        assert_eq!(brought.credited, 1_500);
        assert_eq!(
            *kept.brought.borrow(),
            [(LINES_READ_AS, LISTENS_READ_AT_ONCE), (LINES_READ_AS, 500)]
        );
    }
}
